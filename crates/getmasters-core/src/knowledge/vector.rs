//! Vector index abstraction over the shared SQLite store.
//!
//! [`BruteForceIndex`] (always available) stores embeddings as little-endian f32 BLOBs in
//! `chunk_embeddings` and ranks by cosine in Rust — sub-second at personal scale (docs/05 §4).
//! [`Vec0Index`] (feature `sqlite-vec`) uses the sqlite-vec `vec0` virtual table for KNN.

use crate::store::Store;

/// A pluggable vector index. Scores are higher = better (cosine similarity).
pub trait VectorIndex: Send + Sync {
    /// The embedding dimension this index was created for.
    fn dim(&self) -> usize;
    /// Add (or replace) a chunk's embedding.
    fn add(&self, chunk_id: &str, embedding: &[f32]) -> Result<(), String>;
    /// Top-k nearest chunks within `project_id` (plus global), best first.
    fn search(
        &self,
        project_id: &str,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<(String, f32)>, String>;
    /// The backend name for `status` (`"brute_force"` | `"vec0"`).
    fn backend(&self) -> &'static str;
}

/// Pack an f32 slice into a little-endian byte buffer (no extra deps).
pub fn pack_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Unpack a little-endian f32 byte buffer.
pub fn unpack_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity (0 when either vector has zero norm).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Brute-force cosine index over the `chunk_embeddings` table.
#[derive(Clone)]
pub struct BruteForceIndex {
    store: Store,
    dim: usize,
}

impl BruteForceIndex {
    pub fn new(store: Store, dim: usize) -> Self {
        Self { store, dim }
    }
}

impl VectorIndex for BruteForceIndex {
    fn dim(&self) -> usize {
        self.dim
    }

    fn add(&self, chunk_id: &str, embedding: &[f32]) -> Result<(), String> {
        if embedding.len() != self.dim {
            return Err(format!(
                "embedding dim {} != index dim {}",
                embedding.len(),
                self.dim
            ));
        }
        self.store
            .embeddings_add(chunk_id, &pack_f32(embedding), self.dim)
            .map_err(|e| e.to_string())
    }

    fn search(
        &self,
        project_id: &str,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<(String, f32)>, String> {
        if query.len() != self.dim {
            return Err(format!(
                "query dim {} != index dim {}",
                query.len(),
                self.dim
            ));
        }
        let rows = self
            .store
            .embeddings_for_project(project_id)
            .map_err(|e| e.to_string())?;
        let mut scored: Vec<(String, f32)> = rows
            .into_iter()
            .map(|(chunk_id, bytes)| (chunk_id, cosine(query, &unpack_f32(&bytes))))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        Ok(scored)
    }

    fn backend(&self) -> &'static str {
        "brute_force"
    }
}

/// sqlite-vec `vec0` backend (feature `sqlite-vec`). Stores `project_id` as a metadata column
/// so KNN can be project-scoped, with cosine distance to match [`BruteForceIndex`].
#[cfg(feature = "sqlite-vec")]
#[derive(Clone)]
pub struct Vec0Index {
    store: Store,
    dim: usize,
}

#[cfg(feature = "sqlite-vec")]
impl Vec0Index {
    /// Try to create/verify the `chunk_vectors` vec0 table at `dim`. Returns `None` if the
    /// extension isn't loaded (caller falls back to brute-force).
    pub fn probe(store: Store, dim: usize) -> Option<Self> {
        let sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vectors USING vec0(
                 chunk_id TEXT PRIMARY KEY,
                 project_id TEXT,
                 embedding float[{dim}] distance_metric=cosine
             )"
        );
        let ok = store.with_conn(|c| c.execute_batch(&sql)).is_ok();
        ok.then_some(Self { store, dim })
    }
}

#[cfg(feature = "sqlite-vec")]
impl VectorIndex for Vec0Index {
    fn dim(&self) -> usize {
        self.dim
    }

    fn add(&self, chunk_id: &str, embedding: &[f32]) -> Result<(), String> {
        if embedding.len() != self.dim {
            return Err(format!(
                "embedding dim {} != index dim {}",
                embedding.len(),
                self.dim
            ));
        }
        // The chunk's project_id is needed for the metadata column.
        let project_id = self
            .store
            .with_conn(|c| {
                c.query_row(
                    "SELECT project_id FROM chunks WHERE id = ?1",
                    [chunk_id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            })
            .unwrap_or_default();
        let blob = pack_f32(embedding);
        self.store
            .with_conn(|c| {
                c.execute("DELETE FROM chunk_vectors WHERE chunk_id = ?1", [chunk_id])?;
                c.execute(
                    "INSERT INTO chunk_vectors (chunk_id, project_id, embedding) VALUES (?1, ?2, ?3)",
                    rusqlite::params![chunk_id, project_id, blob],
                )
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn search(
        &self,
        project_id: &str,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<(String, f32)>, String> {
        if query.len() != self.dim {
            return Err(format!(
                "query dim {} != index dim {}",
                query.len(),
                self.dim
            ));
        }
        let blob = pack_f32(query);
        self.store
            .with_conn(|c| {
                let mut stmt = c.prepare(
                    "SELECT chunk_id, distance FROM chunk_vectors
                     WHERE embedding MATCH ?1 AND k = ?2
                       AND (project_id = ?3 OR project_id = 'global')
                     ORDER BY distance",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![blob, k as i64, project_id], |r| {
                        let id: String = r.get(0)?;
                        let dist: f64 = r.get(1)?;
                        // cosine distance → similarity score (higher = better).
                        Ok((id, 1.0 - dist as f32))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .map_err(|e: rusqlite::Error| e.to_string())
    }

    fn backend(&self) -> &'static str {
        "vec0"
    }
}

/// Register the sqlite-vec extension on all future connections. No-op without the feature.
/// MUST run before any `Connection::open` (called from `Store::init`).
#[cfg(feature = "sqlite-vec")]
pub fn register_vec_extension() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // `sqlite3_auto_extension` wants the extension's init fn pointer; transmute the
        // untyped symbol into the expected signature. The explicit target type satisfies
        // clippy's `missing_transmute_annotations` and keeps the call short enough that
        // rustfmt formats it identically across toolchain patch versions.
        type VecInit = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::os::raw::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::os::raw::c_int;
        let init: VecInit = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

/// No-op when the `sqlite-vec` feature is disabled (brute-force only).
#[cfg(not(feature = "sqlite-vec"))]
pub fn register_vec_extension() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip() {
        let v = vec![0.5f32, -1.0, 2.25];
        assert_eq!(unpack_f32(&pack_f32(&v)), v);
    }

    #[test]
    fn cosine_ranks_aligned_vectors_higher() {
        let q = [1.0, 0.0, 0.0];
        assert!(cosine(&q, &[1.0, 0.0, 0.0]) > cosine(&q, &[0.0, 1.0, 0.0]));
        assert_eq!(cosine(&q, &[0.0, 0.0, 0.0]), 0.0);
    }
}

#[cfg(all(test, feature = "sqlite-vec"))]
mod vec0_spike {
    use super::*;

    #[test]
    fn vec0_loads_and_does_knn() {
        register_vec_extension();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE v USING vec0(emb float[4]);")
            .expect("vec0 virtual table should be creatable when the extension loads");
        conn.execute(
            "INSERT INTO v(rowid, emb) VALUES (1, ?1)",
            rusqlite::params![pack_f32(&[1.0, 0.0, 0.0, 0.0])],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO v(rowid, emb) VALUES (2, ?1)",
            rusqlite::params![pack_f32(&[0.0, 1.0, 0.0, 0.0])],
        )
        .unwrap();
        let best: i64 = conn
            .query_row(
                "SELECT rowid FROM v WHERE emb MATCH ?1 ORDER BY distance LIMIT 1",
                rusqlite::params![pack_f32(&[0.9, 0.1, 0.0, 0.0])],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(best, 1);
    }

    #[test]
    fn vec0_index_add_and_search() {
        let store = Store::open_in_memory().unwrap();
        let project = store.create_project("p", None).unwrap();
        let doc = store
            .upsert_document(&project, "/a.md", "h", Some("text/markdown"))
            .unwrap();
        let c1 = store
            .insert_chunk(&doc, &project, 0, "alpha", None)
            .unwrap();
        let c2 = store.insert_chunk(&doc, &project, 1, "beta", None).unwrap();

        let idx = Vec0Index::probe(store, 4).expect("vec0 available under feature");
        idx.add(&c1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        idx.add(&c2, &[0.0, 1.0, 0.0, 0.0]).unwrap();

        let hits = idx.search(&project, &[0.9, 0.1, 0.0, 0.0], 2).unwrap();
        assert_eq!(hits[0].0, c1, "nearest chunk should be c1");
        assert_eq!(idx.backend(), "vec0");
    }
}
