# ADR-0007 — Layered file-backed memory + modular prompt assembly

**Status:** Accepted · **Decision:** D7

## Context
[ADR-0004](./0004-vector-store.md) keeps all state in a single `getmasters.db`. The illustrative schema
([05 §2](../05-data-storage-rag.md)) models project memory as one flat `memories` table (key/value/source). Two
problems surface from studying Hermes Agent: (1) opaque DB rows are not something a user can *read, edit, or
diff* — which is in tension with Masters's local-first "you own your data" principle ([00](../00-overview.md)); and
(2) a single undifferentiated memory blob conflates durable **facts** about a project with **user preferences**
and grows without bound. Hermes addresses both with **layered, file-backed memory** (`MEMORY.md` for facts,
`USER.md` for preferences), a periodic **curation "nudge"** that decides what is worth persisting, and
**modular prompt assembly** where the system prompt is composed from editable sources rather than baked into code.

## Decision
1. **Layered memory** — distinguish memory by kind: **facts** (project knowledge), **preferences** (durable user
   choices), and **procedural** (Skills, [ADR-0006](./0006-skills-procedural-memory.md)). The `memories` schema
   gains a `kind` discriminator.
2. **File-backed, transparent** — durable memory and instructions live as **user-editable Markdown** in the
   project (`INSTRUCTIONS.md`, `MEMORY.md`, `USER.md`) as the **source of truth**; `getmasters.db` holds the
   **index/FTS/embeddings** derived from them. Edits to the files re-index; the UI offers view/edit
   ([07](../07-ux-flows.md)). This *extends*, not contradicts, ADR-0004: one DB still holds all derived/indexed
   state and remains the single backup-plus-keychain story; the human-readable truth additionally lives in files
   the user already owns.
3. **Curation nudge** — the agent autonomously proposes what to persist/forget (and when to capture/refine a
   Skill), avoiding memory bloat; consequential writes still surface through normal approval.
4. **Modular prompt assembly** — the Agent Core composes each turn's system prompt from modular sources
   (persona/defaults + project `INSTRUCTIONS.md` + relevant memory layers + recalled Skills + RAG context), so
   customization is *editing files*, not changing code.

## Alternatives considered
- **Keep memory DB-only (status quo)** — simplest and one store, but opaque and unbounded; loses the
  transparency/editability that suits a local-first personal app.
- **Files-only, no DB index** — maximally transparent but loses fast FTS/semantic recall at scale; reconciled by
  making files the truth and the DB the derived index.
- **Single flat memory (no layers)** — less to model, but mixes facts/preferences/procedures and degrades recall
  relevance and curation.

## Consequences
- (+) Transparent, portable, diff-able memory aligned with local-first; better recall via typed layers + hybrid
  FTS/vector ([05 §3](../05-data-storage-rag.md)); code-free customization via editable prompt sources.
- (−) Must keep files and DB index in sync (define on-change re-index + conflict handling) and design the
  curation heuristics; slightly more moving parts than a single table.
- Maps to roadmap **Phase 2** (memory/Projects), with modular prompt assembly landing as early as **Phase 1**.
  Revisit if: file/DB sync proves fragile or users prefer an opaque-but-simpler store.
