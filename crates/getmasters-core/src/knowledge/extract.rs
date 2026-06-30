//! Text extraction — pluggable per file type. Phase 2a ships plain-text/Markdown; PDF/DOCX
//! register here in a later phase without touching the pipeline.

use std::path::Path;

/// A unit of extracted text with an optional location hint (for citations).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedBlock {
    pub text: String,
    /// e.g. `"heading: Introduction"` — surfaced in citations.
    pub location: Option<String>,
}

/// Extracts text blocks from a file of a supported type.
pub trait Extractor: Send + Sync {
    fn supports(&self, ext: &str) -> bool;
    fn extract(&self, path: &Path) -> Result<Vec<ExtractedBlock>, String>;
}

/// Plain text + Markdown. For Markdown, sections are split on ATX headings (`#`), and each
/// section's heading becomes its blocks' `location`.
pub struct PlainTextExtractor;

impl Extractor for PlainTextExtractor {
    fn supports(&self, ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "md" | "markdown" | "txt" | "text" | "rst"
        )
    }

    fn extract(&self, path: &Path) -> Result<Vec<ExtractedBlock>, String> {
        let raw =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
            .unwrap_or(false);

        if !is_md {
            let text = raw.trim().to_string();
            return Ok(if text.is_empty() {
                Vec::new()
            } else {
                vec![ExtractedBlock {
                    text,
                    location: None,
                }]
            });
        }

        // Markdown: accumulate text under the most recent heading.
        let mut blocks = Vec::new();
        let mut heading: Option<String> = None;
        let mut buf = String::new();
        let flush =
            |buf: &mut String, heading: &Option<String>, blocks: &mut Vec<ExtractedBlock>| {
                let text = buf.trim().to_string();
                if !text.is_empty() {
                    blocks.push(ExtractedBlock {
                        text,
                        location: heading.as_ref().map(|h| format!("heading: {h}")),
                    });
                }
                buf.clear();
            };
        for line in raw.lines() {
            if let Some(title) = line.trim_start().strip_prefix('#') {
                flush(&mut buf, &heading, &mut blocks);
                heading = Some(title.trim_start_matches('#').trim().to_string());
            } else {
                buf.push_str(line);
                buf.push('\n');
            }
        }
        flush(&mut buf, &heading, &mut blocks);
        Ok(blocks)
    }
}

/// PDF text extraction (feature `pdf`). Each page becomes one block located as `page: N`.
///
/// Best-effort only: text PDFs extract cleanly, but scanned/image PDFs yield nothing (there is no
/// OCR) and complex column/table layouts can reorder text. That is acceptable for RAG (chunks are
/// fuzzy-matched) — an ingest that yields zero chunks signals an image-only PDF.
#[cfg(feature = "pdf")]
pub struct PdfExtractor;

#[cfg(feature = "pdf")]
impl Extractor for PdfExtractor {
    fn supports(&self, ext: &str) -> bool {
        ext.eq_ignore_ascii_case("pdf")
    }

    fn extract(&self, path: &Path) -> Result<Vec<ExtractedBlock>, String> {
        let pages = pdf_extract::extract_text_by_pages(path)
            .map_err(|e| format!("pdf extract {}: {e}", path.display()))?;
        let mut blocks = Vec::new();
        for (i, page) in pages.into_iter().enumerate() {
            let text = page.trim().to_string();
            if !text.is_empty() {
                blocks.push(ExtractedBlock {
                    text,
                    location: Some(format!("page: {}", i + 1)),
                });
            }
        }
        Ok(blocks)
    }
}

/// DOCX text extraction (feature `docx`). Reads `word/document.xml` from the OOXML zip and streams
/// `<w:t>` runs, treating `<w:p>` as paragraph boundaries and heading-styled paragraphs
/// (`<w:pStyle w:val="Heading…">`) as the running `location` — mirroring `PlainTextExtractor`.
#[cfg(feature = "docx")]
pub struct DocxExtractor;

#[cfg(feature = "docx")]
impl Extractor for DocxExtractor {
    fn supports(&self, ext: &str) -> bool {
        ext.eq_ignore_ascii_case("docx")
    }

    fn extract(&self, path: &Path) -> Result<Vec<ExtractedBlock>, String> {
        use std::io::Read;
        let file =
            std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| format!("docx not a zip {}: {e}", path.display()))?;
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .map_err(|e| format!("docx missing word/document.xml: {e}"))?
            .read_to_string(&mut xml)
            .map_err(|e| format!("docx read: {e}"))?;
        Ok(parse_docx_xml(&xml))
    }
}

/// Parse a WordprocessingML `document.xml` body into blocks (heading-aware, like the Markdown path).
#[cfg(feature = "docx")]
fn parse_docx_xml(xml: &str) -> Vec<ExtractedBlock> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut blocks = Vec::new();
    let mut running_heading: Option<String> = None;
    let mut buf = String::new(); // body accumulated under the running heading
    let mut para = String::new(); // current paragraph text
    let mut in_text = false;
    let mut para_is_heading = false;

    let flush = |buf: &mut String, heading: &Option<String>, blocks: &mut Vec<ExtractedBlock>| {
        let text = buf.trim().to_string();
        if !text.is_empty() {
            blocks.push(ExtractedBlock {
                text,
                location: heading.as_ref().map(|h| format!("heading: {h}")),
            });
        }
        buf.clear();
    };

    // `<w:pStyle w:val="Heading1"/>` marks a heading paragraph (Start or Empty form).
    let is_heading_style = |e: &quick_xml::events::BytesStart| -> bool {
        e.name().as_ref() == b"w:pStyle"
            && e.try_get_attribute("w:val")
                .ok()
                .flatten()
                .and_then(|a| {
                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .ok()
                        .map(|v| v.starts_with("Heading"))
                })
                .unwrap_or(false)
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:p" => {
                    para.clear();
                    para_is_heading = false;
                }
                b"w:t" => in_text = true,
                b"w:pStyle" if is_heading_style(&e) => para_is_heading = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:pStyle" if is_heading_style(&e) => para_is_heading = true,
                b"w:br" => para.push('\n'),
                b"w:tab" => para.push('\t'),
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(t) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                        para.push_str(&t);
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => {
                    let p = para.trim().to_string();
                    if para_is_heading && !p.is_empty() {
                        flush(&mut buf, &running_heading, &mut blocks);
                        running_heading = Some(p);
                    } else if !p.is_empty() {
                        buf.push_str(&p);
                        buf.push('\n');
                    }
                    para.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    flush(&mut buf, &running_heading, &mut blocks);
    blocks
}

/// A set of extractors tried in order by file extension.
pub struct ExtractorRegistry {
    extractors: Vec<Box<dyn Extractor>>,
}

impl ExtractorRegistry {
    /// Plain text + Markdown always; PDF/DOCX when their features are enabled (Phase 2c).
    pub fn default_set() -> Self {
        let mut extractors: Vec<Box<dyn Extractor>> = vec![Box::new(PlainTextExtractor)];
        #[cfg(feature = "pdf")]
        extractors.push(Box::new(PdfExtractor));
        #[cfg(feature = "docx")]
        extractors.push(Box::new(DocxExtractor));
        Self { extractors }
    }

    pub fn supports(&self, ext: &str) -> bool {
        self.extractors.iter().any(|e| e.supports(ext))
    }

    /// Extract a file, or `None` if no registered extractor supports its extension.
    pub fn extract(&self, path: &Path) -> Option<Result<Vec<ExtractedBlock>, String>> {
        let ext = path.extension().and_then(|e| e.to_str())?;
        self.extractors
            .iter()
            .find(|e| e.supports(ext))
            .map(|e| e.extract(path))
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::default_set()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_splits_on_headings() {
        let dir = std::env::temp_dir().join(format!("getmasters-extract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("doc.md");
        std::fs::write(&f, "# Intro\nhello world\n\n## Details\nmore text").unwrap();

        let blocks = PlainTextExtractor.extract(&f).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].location.as_deref(), Some("heading: Intro"));
        assert!(blocks[0].text.contains("hello world"));
        assert_eq!(blocks[1].location.as_deref(), Some("heading: Details"));
    }

    #[test]
    fn registry_supports_text() {
        let r = ExtractorRegistry::default_set();
        assert!(r.supports("md"));
        assert!(r.supports("txt"));
        // PDF/DOCX support is feature-gated.
        #[cfg(not(feature = "pdf"))]
        assert!(!r.supports("pdf"));
        #[cfg(feature = "pdf")]
        assert!(r.supports("pdf"));
        #[cfg(feature = "docx")]
        assert!(r.supports("docx"));
    }

    /// A minimal single-page PDF with one text line, assembled with correct byte offsets so the
    /// xref table is valid regardless of content length.
    #[cfg(feature = "pdf")]
    fn minimal_pdf(text: &str) -> Vec<u8> {
        let objs: [String; 5] = [
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .into(),
            {
                let stream = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
                format!(
                    "<< /Length {} >>\nstream\n{stream}\nendstream",
                    stream.len()
                )
            },
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objs.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        let xref_off = pdf.len();
        pdf.push_str(&format!("xref\n0 {}\n", objs.len() + 1));
        pdf.push_str("0000000000 65535 f \n");
        for off in &offsets {
            pdf.push_str(&format!("{off:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_off}\n%%EOF",
            objs.len() + 1
        ));
        pdf.into_bytes()
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn pdf_extracts_text_with_page_location() {
        let dir = std::env::temp_dir().join(format!("getmasters-pdf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("doc.pdf");
        std::fs::write(&f, minimal_pdf("Hello Masters")).unwrap();

        let blocks = PdfExtractor.extract(&f).unwrap();
        assert!(!blocks.is_empty(), "expected at least one page block");
        assert_eq!(blocks[0].location.as_deref(), Some("page: 1"));
        assert!(
            blocks[0].text.contains("Hello"),
            "extracted text: {:?}",
            blocks[0].text
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Build a minimal `.docx` (a zip with `word/document.xml`) for the extractor test.
    #[cfg(feature = "docx")]
    fn write_docx(path: &std::path::Path, document_xml: &str) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[cfg(feature = "docx")]
    #[test]
    fn docx_extracts_paragraphs_with_heading_location() {
        let dir = std::env::temp_dir().join(format!("getmasters-docx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("doc.docx");
        let xml = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Introduction</w:t></w:r></w:p>
  <w:p><w:r><w:t>Rust is a systems language.</w:t></w:r></w:p>
</w:body>
</w:document>"#;
        write_docx(&f, xml);

        let blocks = DocxExtractor.extract(&f).unwrap();
        assert_eq!(blocks.len(), 1, "blocks: {blocks:?}");
        assert_eq!(blocks[0].location.as_deref(), Some("heading: Introduction"));
        assert!(blocks[0].text.contains("systems language"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
