# ADR-0004 — RAG vector store: SQLite + sqlite-vec

**Status:** Accepted · **Decision:** D4

## Context
Masters's grounding feature (doc 05) needs to embed and search over a single user's documents — typically
hundreds to low-thousands of files. Masters already uses SQLite for sessions/projects/memory/audit.

## Decision
Store vector embeddings in the **same SQLite database** using the **`sqlite-vec`** extension (a `vec0` virtual
table keyed to `chunks.id`). No separate vector service.

## Alternatives considered
- **LanceDB** — excellent for large/embedded vector workloads; heavier than needed at personal scale and a
  second store to manage.
- **Qdrant / pgvector** — require running a separate server/database; violates the zero-infra, local-first goal.

## Consequences
- (+) Zero external infra; one file (`getmasters.db`) holds everything and is trivial to back up; transactional
  consistency between chunks and vectors; sub-second search at personal scale.
- (−) Not optimized for very large corpora or heavy ANN tuning.
- **Upgrade path:** the `Knowledge` service abstracts the vector layer, so swapping to **LanceDB** for large
  libraries changes only the implementation, not the interface (see roadmap Phase 4).
- Revisit if: a user's library outgrows comfortable in-SQLite search latency.
