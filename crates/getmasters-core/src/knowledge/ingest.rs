//! Ingestion pipeline (docs/05 §3): enumerate supported files within a grant → `content_hash`
//! skip unchanged → extract → chunk → embed → store chunks + vector index + FTS.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::permission::GrantSet;
use crate::store::Store;

use super::chunk::{chunk_blocks, OVERLAP_CHARS, TARGET_CHARS};
use super::extract::ExtractorRegistry;
use super::vector::VectorIndex;
use super::Embedder;

/// Counts from one ingest run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestResult {
    pub indexed: usize,
    pub skipped: usize,
    pub chunks: usize,
}

/// A stable (non-cryptographic) content hash for change detection.
fn content_hash(bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn mime_for(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => "text/markdown",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "text/plain",
    }
}

/// Ingest a file or folder (within a grant) into the project's knowledge index.
#[allow(clippy::too_many_arguments)]
pub async fn ingest_path(
    store: &Store,
    project_id: &str,
    grants: &GrantSet,
    embedder: &Embedder,
    index: &dyn VectorIndex,
    registry: &ExtractorRegistry,
    path: &str,
) -> Result<IngestResult, String> {
    // Ingest reads source files → require read access within a grant.
    let root = grants.resolve(path, false)?;

    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root);
    } else {
        for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if registry.supports(ext) {
                        files.push(entry.path().to_path_buf());
                    }
                }
            }
        }
    }

    let mut result = IngestResult::default();
    for file in files {
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !registry.supports(ext) {
            continue;
        }
        let bytes = std::fs::read(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        let hash = content_hash(&bytes);
        let path_str = file.to_string_lossy().into_owned();

        if let Some((_, existing)) = store
            .get_document_by_path(project_id, &path_str)
            .map_err(|e| e.to_string())?
        {
            if existing == hash {
                result.skipped += 1;
                continue;
            }
        }

        let doc_id = store
            .upsert_document(project_id, &path_str, &hash, Some(mime_for(ext)))
            .map_err(|e| e.to_string())?;
        store
            .delete_document_chunks(&doc_id)
            .map_err(|e| e.to_string())?; // clear old chunks for re-index

        let blocks = match registry.extract(&file) {
            Some(Ok(b)) => b,
            Some(Err(e)) => return Err(e),
            None => continue,
        };
        let chunks = chunk_blocks(&blocks, TARGET_CHARS, OVERLAP_CHARS);
        if chunks.is_empty() {
            result.indexed += 1;
            continue;
        }

        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let embeddings = embedder.embed(texts).await.map_err(|e| e.to_string())?;
        if embeddings.len() != chunks.len() {
            return Err("embedding count mismatch".to_string());
        }

        for (chunk, emb) in chunks.iter().zip(embeddings) {
            let chunk_id = store
                .insert_chunk(
                    &doc_id,
                    project_id,
                    chunk.ordinal as i64,
                    &chunk.text,
                    chunk.location.as_deref(),
                )
                .map_err(|e| e.to_string())?;
            index.add(&chunk_id, &emb)?;
            store
                .fts_index(&chunk_id, project_id, &chunk.text)
                .map_err(|e| e.to_string())?;
            result.chunks += 1;
        }
        result.indexed += 1;
    }
    Ok(result)
}
