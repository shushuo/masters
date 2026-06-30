//! Chunking — split extracted blocks into overlapping, citable chunks (docs/05 §3).
//!
//! Chunks respect paragraph boundaries and carry their block's `location`. Oversized
//! paragraphs are hard-split with character overlap so retrieval stays granular.

use super::extract::ExtractedBlock;

/// A retrieval unit: text + location + a sequential ordinal across the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub ordinal: usize,
    pub text: String,
    pub location: Option<String>,
}

/// Default chunk sizing (characters). Personal-scale, citation-friendly.
pub const TARGET_CHARS: usize = 1000;
pub const OVERLAP_CHARS: usize = 150;

/// Chunk a document's extracted blocks. Paragraphs are packed up to `target`; a single
/// paragraph longer than `target` is windowed with `overlap`.
pub fn chunk_blocks(blocks: &[ExtractedBlock], target: usize, overlap: usize) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut ordinal = 0usize;

    for block in blocks {
        let location = block.location.clone();
        let paragraphs: Vec<&str> = block
            .text
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();

        let mut buf = String::new();
        let push = |text: &str, ordinal: &mut usize, out: &mut Vec<Chunk>| {
            let text = text.trim();
            if !text.is_empty() {
                out.push(Chunk {
                    ordinal: *ordinal,
                    text: text.to_string(),
                    location: location.clone(),
                });
                *ordinal += 1;
            }
        };

        for para in paragraphs {
            if para.len() > target {
                // Flush whatever is buffered, then window the long paragraph.
                if !buf.is_empty() {
                    push(&buf, &mut ordinal, &mut out);
                    buf.clear();
                }
                for window in window_text(para, target, overlap) {
                    push(&window, &mut ordinal, &mut out);
                }
            } else if buf.len() + para.len() + 2 > target && !buf.is_empty() {
                push(&buf, &mut ordinal, &mut out);
                buf.clear();
                buf.push_str(para);
            } else {
                if !buf.is_empty() {
                    buf.push_str("\n\n");
                }
                buf.push_str(para);
            }
        }
        if !buf.is_empty() {
            push(&buf, &mut ordinal, &mut out);
        }
    }
    out
}

/// Split `text` into windows of ~`target` chars with `overlap`, on char boundaries.
fn window_text(text: &str, target: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= target {
        return vec![text.to_string()];
    }
    let step = target.saturating_sub(overlap).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + target).min(chars.len());
        out.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str, loc: Option<&str>) -> ExtractedBlock {
        ExtractedBlock {
            text: text.to_string(),
            location: loc.map(str::to_string),
        }
    }

    #[test]
    fn small_block_is_one_chunk_with_location() {
        let chunks = chunk_blocks(
            &[block("hello world", Some("heading: Intro"))],
            TARGET_CHARS,
            OVERLAP_CHARS,
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ordinal, 0);
        assert_eq!(chunks[0].location.as_deref(), Some("heading: Intro"));
    }

    #[test]
    fn long_paragraph_windows_with_overlap_and_ordinals() {
        let long = "x".repeat(2500);
        let chunks = chunk_blocks(&[block(&long, None)], 1000, 150);
        assert!(
            chunks.len() >= 3,
            "expected multiple windows, got {}",
            chunks.len()
        );
        // Ordinals are sequential.
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.ordinal, i);
        }
    }

    #[test]
    fn paragraphs_pack_up_to_target() {
        let text = "para one.\n\npara two.\n\npara three.";
        let chunks = chunk_blocks(&[block(text, None)], 1000, 150);
        assert_eq!(chunks.len(), 1, "small paragraphs pack into one chunk");
        assert!(chunks[0].text.contains("para three"));
    }
}
