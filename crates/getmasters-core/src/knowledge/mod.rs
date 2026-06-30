//! Knowledge / RAG (Phase 2a, ADR-0004): ingest the user's documents → chunk → embed →
//! index, then retrieve cited context for grounded answers.
//!
//! Vector search runs in the same `getmasters.db` via [`vector::VectorIndex`] — a brute-force
//! cosine index (always available) or the sqlite-vec `vec0` backend (feature `sqlite-vec`).

pub mod chunk;
pub mod extract;
pub mod ingest;
pub mod server;
pub mod vector;

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::provider::{resolve_provider, Provider, ProviderError};
use crate::store::Store;

pub use ingest::{ingest_path, IngestResult};
pub use server::KnowledgeServer;
pub use vector::{register_vec_extension, BruteForceIndex, VectorIndex};

/// A retrieved, citable chunk.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Hit {
    pub path: String,
    pub location: Option<String>,
    pub text: String,
    pub score: f32,
}

/// Hybrid retrieval: vector top-k + FTS top-k fused by reciprocal-rank fusion, with
/// project-scoped hits ranked above global (ADR-0011). Robust when one signal is weak (e.g.
/// the mock embedder), because the FTS ranking still discriminates.
pub async fn search(
    store: &Store,
    project_id: &str,
    embedder: &Embedder,
    index: &dyn VectorIndex,
    query: &str,
    k: usize,
) -> Result<Vec<Hit>, String> {
    const RRF_K: f32 = 60.0;
    let pool = (k * 4).max(8);

    let qvec = embedder.embed_one(query).await.map_err(|e| e.to_string())?;
    let vec_hits = index.search(project_id, &qvec, pool)?;
    let fts_hits = store
        .fts_search(project_id, query, pool)
        .map_err(|e| e.to_string())?;

    let mut fused: HashMap<String, f32> = HashMap::new();
    for (rank, (id, _)) in vec_hits.iter().enumerate() {
        *fused.entry(id.clone()).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
    }
    for (rank, (id, _)) in fts_hits.iter().enumerate() {
        *fused.entry(id.clone()).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
    }
    if fused.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<String> = fused.keys().cloned().collect();
    let rows = store.get_chunks_by_ids(&ids).map_err(|e| e.to_string())?;
    let mut hits: Vec<Hit> = rows
        .into_iter()
        .map(|r| {
            let base = fused.get(&r.id).copied().unwrap_or(0.0);
            // Project-first: a project hit always outranks a global one.
            let boost = if r.project_id == project_id { 1.0 } else { 0.0 };
            Hit {
                path: r.path,
                location: r.location,
                text: r.text,
                score: base + boost,
            }
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(k);
    Ok(hits)
}

/// Default embedding model (provider-qualified). Requires an OpenAI key (or a local base) in
/// production; with no usable embedding provider, `embed` fails with a clear `Auth` error.
pub const DEFAULT_EMBEDDING_MODEL: &str = "openai:text-embedding-3-small";

/// The embedding model used by Knowledge, resolved independently of the chat provider
/// (ADR-0013: embeddings are project-level, not per-master).
#[derive(Clone)]
pub struct Embedder {
    provider: Arc<dyn Provider>,
    dim: usize,
}

impl Embedder {
    /// Resolve from the `embedding_model` setting (default [`DEFAULT_EMBEDDING_MODEL`]).
    /// In production a missing embedding key surfaces as an `Auth` error at `embed` time. Under
    /// the `testing` feature it falls back to the mock embedder so headless ingest/search works
    /// with no credentials.
    pub fn resolve(cfg: &Config, store: &Store) -> Self {
        let qualified = store
            .get_setting("embedding_model")
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());
        let (provider, model) = resolve_provider(cfg, &qualified);

        #[cfg(feature = "testing")]
        {
            let needs_key_we_lack = provider.name() == "openai"
                && cfg.openai_api_key.is_none()
                && !crate::config::is_local_base(&cfg.openai_base());
            if needs_key_we_lack {
                return Self {
                    provider: Arc::new(crate::provider::MockProvider::new()),
                    dim: dim_for("mock", ""),
                };
            }
        }

        let dim = dim_for(provider.name(), &model);
        Self { provider, dim }
    }

    /// Build directly from a provider (tests).
    pub fn from_provider(provider: Arc<dyn Provider>, dim: usize) -> Self {
        Self { provider, dim }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider.name()
    }

    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.provider.embed(texts).await
    }

    /// Embed a single string.
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        let mut v = self.embed(vec![text.to_string()]).await?;
        v.pop()
            .ok_or_else(|| ProviderError::Decode("empty embedding".into()))
    }
}

/// Build the best available vector index for `dim`: the sqlite-vec `vec0` backend when the
/// feature is on and the extension loads, otherwise the brute-force cosine index.
pub fn build_index(store: Store, dim: usize) -> Arc<dyn VectorIndex> {
    #[cfg(feature = "sqlite-vec")]
    {
        if let Some(v) = vector::Vec0Index::probe(store.clone(), dim) {
            tracing::info!("knowledge: using sqlite-vec (vec0) backend");
            return Arc::new(v);
        }
        tracing::warn!("knowledge: sqlite-vec unavailable; using brute-force backend");
    }
    Arc::new(BruteForceIndex::new(store, dim))
}

/// Known embedding dimensions by provider/model (with a sensible default).
fn dim_for(provider: &str, model: &str) -> usize {
    match provider {
        "mock" => 8,
        "openai" => {
            if model.contains("3-large") {
                3072
            } else {
                1536 // text-embedding-3-small / ada-002
            }
        }
        _ => 1536,
    }
}
