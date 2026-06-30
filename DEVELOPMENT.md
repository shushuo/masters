# Development — Phases 0 + 1a

Phase 0 (Foundations) stood up the buildable skeleton; **Phase 1a** added tool-calling, the
built-in Files server, Permission & Audit, and OpenAI-compatible providers (see
[`docs/08-roadmap.md`](docs/08-roadmap.md)). Together: a Rust Cargo workspace, the `getmastersd`
daemon (loopback HTTP + WebSocket), a `Provider` abstraction with Anthropic, OpenAI-compatible,
and offline-mock backends, a content-block tool-calling agent loop gated by Permission & Audit,
a bundled-SQLite store, an OpenAPI-derived TypeScript client, and a Tauri 2 desktop shell.

## Layout

```
crates/
  getmasters-proto    wire DTOs + WS envelopes + permission leaf types (source of truth for the TS client)
  getmasters-core     Provider trait (Anthropic/OpenAI/Mock), store, agent tool loop,
                  permission/ (gate + audit + approver), extensions/ (rmcp client host), prompt/
  getmasters-server   getmastersd — axum daemon (HTTP + WS approvals), bearer auth, OpenAPI; + gen_openapi
  getmasters-mcp      built-in MCP servers — the rmcp Files server (Knowledge/Study/… follow)
  getmasters-cli      getmasters — `getmasters ping` (chat) and `getmasters agent --grant` (gated tool loop)
ui/desktop        Tauri 2 + React + TS shell (src-tauri is NOT a workspace member)
```

## Toolchain

- Rust stable (pinned to 1.94 via `rust-toolchain.toml`).
- Node 20+ / pnpm 10 for the desktop front-end.
- [`just`](https://github.com/casey/just) optional — every recipe in the `Justfile` maps to a
  raw `cargo`/`pnpm` command you can run directly.

## Build & test (headless-friendly)

```bash
cargo build --workspace          # compiles everything except the Tauri app
cargo test  --workspace          # 18 tests incl. the HTTP+WS E2E over the mock provider
cargo clippy --workspace --all-targets
cargo fmt --all

cargo run -p getmasters-cli -- ping "hello"            # chat turn over the mock (no key, no daemon)
cargo run -p getmasters-cli -- agent --grant ./scratch # gated tool loop: creates a file + audit log
```

### Run the daemon by hand

```bash
cargo run -p getmasters-server --bin getmastersd
# → prints:  GETMASTERSD_READY {"port":<n>,"token":"<t>"}
curl http://127.0.0.1:<n>/health                                   # public
curl -X POST http://127.0.0.1:<n>/sessions -H "Authorization: Bearer <t>" \
     -H 'content-type: application/json' -d '{"title":"demo"}'
```

The default provider is the **mock** (no key needed). To use a real provider:

```bash
# Claude (default family)
export ANTHROPIC_API_KEY=sk-...      # auto-switches to Anthropic

# OpenAI or any OpenAI-compatible endpoint (Groq, Together, OpenRouter, local Ollama)
export GETMASTERS_PROVIDER=openai
export OPENAI_API_KEY=sk-...
export OPENAI_BASE_URL=https://api.openai.com   # or e.g. http://localhost:11434 for Ollama (keyless)
```

Other env vars: `GETMASTERS_MODEL` (default `claude-opus-4-8`).

**Data home / DB path.** The daemon roots all on-disk state in a data home, resolved with this
precedence (highest first):

1. `GETMASTERS_DB_PATH` — an explicit DB file path (what the test suite uses; e.g. a tempfile).
2. `GETMASTERS_HOME` — the data-home directory; the DB is `$GETMASTERS_HOME/getmasters.db`.
3. **`~/.getmasters`** — the default; DB at `~/.getmasters/getmasters.db`, projects under `~/.getmasters/projects/`.

The home directory is created on first run (`getmasters_server::home::resolve_db_path`). When running the
daemon repeatedly from a checkout, set `GETMASTERS_DB_PATH=./getmasters.db` (or `GETMASTERS_HOME=./.getmasters`) to keep
state inside the repo instead of your real home.

Masters (Phase 3) select a model per persona using a **provider-qualified** id resolved by
`provider::resolve_provider` — `anthropic:claude-opus-4-8`, `openai:gpt-…`, `groq:…`,
`ollama:llama3`, etc.

### Live Anthropic test (opt-in)

```bash
GETMASTERS_RUN_LIVE=1 ANTHROPIC_API_KEY=sk-... \
  cargo test -p getmasters-core --test anthropic_live -- --ignored
```

## API contract

`getmasters-proto` is the single source of truth. Regenerate the desktop's typed client after
changing any DTO:

```bash
cargo run -p getmasters-server --bin gen_openapi -- ui/desktop/src/api/openapi.json
cd ui/desktop && pnpm openapi-typescript src/api/openapi.json -o src/api/schema.ts
# or: just gen-openapi
```

## Desktop front-end

```bash
cd ui/desktop && pnpm install && pnpm build      # typecheck + bundle (headless OK)
```

The full Tauri window (`pnpm tauri dev`) needs webkit/GTK + a display and is **not** buildable
in a headless container — see [`ui/desktop/src-tauri/README.md`](ui/desktop/src-tauri/README.md)
for the desktop build steps (staging the `getmastersd` sidecar, generating icons).

### Headless web dev path (WSL / browser)

When there's no desktop (e.g. WSL), run the app as a plain web page instead of the Tauri shell:

```bash
make dev          # or: ./scripts/dev-web.sh
```

This builds + starts `getmastersd`, captures its handshake (port + per-launch token), keeps DB
state in the repo (`GETMASTERS_DB_PATH=./getmasters.db`), enables dev CORS
(`GETMASTERS_DEV_CORS=1`), then launches the Vite dev server (`--host`) wired to the daemon via
`VITE_GETMASTERS_PORT`/`VITE_GETMASTERS_TOKEN`. Open the printed URL in a browser on the host;
Ctrl-C stops both.

### Marketing landing page

The marketing site lives in the separate **`masters-cloud`** repository (`apps/web`, a Next.js +
Tailwind app), not in this repo. It is not part of this workspace.

## Phase 1a — tools, permissions, multi-provider

- **Tool-calling.** Messages are content blocks (`Text`/`ToolUse`/`ToolResult`); the agent loop
  streams the model, gates each requested tool, dispatches it, feeds the result back, and
  re-calls — bounded by `MAX_TOOL_ITERATIONS`. Providers accumulate tool-call JSON internally
  and emit one `ToolUse`, so the loop is provider-identical.
- **Files server.** `getmasters-mcp` hosts an rmcp `FilesServer` (read/list/search/create/edit/
  move/rename/delete) **in-process** over `tokio::io::duplex`; `getmasters-core::extensions` is the
  rmcp client host (ADR-0005). Tools are scoped to folder grants.
- **Permission & Audit** (`getmasters-core::permission`). The gate classifies each call, enforces
  grants + the docs/06 default policy matrix, resolves prompts via an `Approver`
  (`AutoApprover` for CLI/tests, `ChannelApprover` for the WS round-trip), and records every
  call in `audit_log`. The gate runs in **Core**, before any tool dispatch — never bypassed.
- The mock provider supports a tool trigger (`[[tool:files.create|path=…|content=…]]`) so the
  whole gated loop is testable with no key.

## Phase 1b — secrets, settings, revert, Blank Slate, approval UI

- **Secrets** (`getmasters-core::secrets`). API keys live in the OS keychain (`keyring`), never in
  `getmasters.db`/config. A `MemoryStore` is the test + headless fallback (no Secret Service). The
  daemon probes the keychain and falls back with a logged warning.
- **Settings** (migration 0003). Non-secret settings (provider/model/base URL) persist in a
  `settings` table; `Config::resolve` layers DB settings over env over defaults, and keys over
  the secret store. Endpoints: `GET/PUT /settings`, `PUT /settings/secret`,
  `DELETE /settings/secret/{name}` (keys are write-only — only presence is reported).
- **Revert** (migration 0004, `getmasters-core::revision`). Before a write/destructive file op the
  agent captures the pre-image; on success it logs a `file_revisions` row. `POST
  /sessions/{id}/revert` undoes the last op (delete a created file, restore prior content, move
  back). A `diff_summary` helper surfaces an edit preview.
- **Blank Slate / least-privilege** (ADR-0008). `AgentService::blank_slate(true)` boots with no
  tools enabled and no standing permissions (every side-effect re-prompts); capability is
  granted via `with_enabled_tools`. The gate denies un-enabled tools outright.
- **Desktop UI.** A Settings screen (provider/model/keys) and an in-chat **approval dialog**
  (consumes `ApprovalRequest`, replies `ApprovalDecision`) plus a Revert button. The front-end
  type-checks and bundles headless; the live Tauri window still defers to a real desktop.

Settings: `GETMASTERS_PROVIDER`/`OPENAI_BASE_URL` env still work; the UI/keychain are the
persistent path. Provider/model changes apply on the next daemon launch (live swap is later).

## Phase 2a — grounded knowledge + Projects

- **Knowledge/RAG** (`getmasters-core::knowledge`, migration 0005). Ingest `.md/.txt` (pluggable
  `Extractor` seam) → chunk (overlap + heading/line `location`) → embed → index. A
  `VectorIndex` trait backs **brute-force cosine** (default; no C compiler) and, behind the
  `sqlite-vec` feature, the real **`vec0`** KNN backend (ADR-0004) with a runtime probe + fallback.
  Hybrid retrieval (vector + FTS5, reciprocal-rank fusion) with project-first ranking.
- **Knowledge server** — an rmcp `KnowledgeServer` (in core) with `ingest`/`search`/`status`
  tools, project-scoped. No `answer` tool: the Agent Core composes the cited answer via a
  grounding prompt section, so the single permission/audit path is preserved.
- **Embeddings** — `OpenAiProvider::embed` (`/v1/embeddings`) + a project-level `Embedder`
  resolved from the `embedding_model` setting (default `openai:text-embedding-3-small`), with
  the mock (dim-8) fallback for keyless dev/CI.
- **Projects as context containers** (ADR-0011) — project CRUD (`/projects…`), folder grants,
  and a per-session agent cache: a session under a project automatically gets its grants +
  Knowledge + instructions, with project-scoped results ranked above global.
- Try it headless: `cargo run -p getmasters-cli -- knowledge --grant ./notes --ask "…"` (mock
  embedder). Build the real `vec0` backend with `cargo build --features sqlite-vec`.

## Phase 2b — file-backed Memory + Skills

- **Memory** (`getmasters-core::memory`, migration 0006). Durable memory lives in **Markdown files**
  the user can edit — `MEMORY.md` (facts/decisions) + `USER.md` (profile); the `memories` table is
  only an FTS index over them (kind `fact`/`user`). An rmcp `MemoryServer` exposes `remember`
  (write), `recall` (read), `forget` (write); every write re-indexes its file (delete-by-source
  then re-insert), so the index never drifts.
- **Skills** (`getmasters-core::skills`). Agent-authored procedures saved as `skills/<slug>.md` with
  frontmatter; the `skills` table indexes them. An rmcp `SkillsServer` exposes `create_skill`
  (write), `recall_skill` (read), `list_skills` (read).
- **Per-project data dir** — `{db_parent}/projects/{project_id}/` roots the memory/skills files,
  derived from the DB path and passed to `ExtensionManager::with_project(..., project_dir)`. The
  project agent hosts Files + Knowledge + Memory + Skills together.
- **Auto-injection + curation** (FR-37). `PromptAssembler::assemble` now takes a `PromptContext`
  carrying the recalled `USER.md`/`MEMORY.md` block + available-skill summaries + a curation nudge
  (when memory/skills tools are present); project instructions still rank last (ADR-0011).
- **Permission fixes** — `recall_skill`/`list_skills`/`recall` classify as Read (auto-allowed);
  `forget` is a Write curation edit (revert-eligible), not Destructive (docs/04 §2.4).
- **Daemon + UI** — `GET /projects/{id}/memories` + `/skills` (read-only `MemoryDto`/`SkillDto`);
  desktop `Memory.tsx` + `Skills.tsx` (bundle-only).
- Try it headless: `cargo run -p getmasters-cli -- learn --grant ./scratch` drives
  `remember`→`recall` and `create_skill`→`recall_skill` over the mock provider (reads auto, writes
  approved).

## Phase 2c — v1 close-out (PDF/DOCX ingest · FR-19 · management UIs)

- **PDF/DOCX extraction** (`getmasters-core::knowledge::extract`, **feature-gated**). `PdfExtractor`
  (pure-Rust `pdf-extract`, per-page `location: "page: N"`) and `DocxExtractor` (hand-rolled `zip`
  + `quick-xml` over `word/document.xml`, heading-aware) register in `ExtractorRegistry::default_set`
  behind `#[cfg(feature = "pdf")]` / `#[cfg(feature = "docx")]`. The library default stays lean and
  headless (`cargo test -p getmasters-core`); the **daemon + CLI enable both** so the product ingests
  real documents. All deps are pure-Rust (no C). Caveat: PDF extraction is best-effort — image-only
  PDFs yield no text (no OCR).
- **FR-19 — per-project extensions toggle** (migration 0007 `project_extensions`; absent row =
  enabled, so existing projects keep every server). `ExtensionManager::with_project(..., enabled)`
  hosts only the chosen built-ins — a non-hosted server contributes no tools (not-hosting *is* the
  enforcement). `GET /projects/{id}/extensions` + `PUT /projects/{id}/extensions/{name}` (with
  `invalidate_project`). Store: `set_project_extension`/`list_project_extensions`/
  `disabled_extensions`.
- **Knowledge read endpoint** — `GET /projects/{id}/knowledge` (`KnowledgeStatusDto` = counts +
  indexed `DocumentDto` paths via `store.list_documents`). Ingest stays a gated chat/agent action.
- **Desktop management UIs** — `MastersClient` gains project/knowledge/extension methods; a
  `Projects` picker + `ProjectDetail` (tabs Instructions/Knowledge/Memory/Skills/Extensions) finally
  supply the `projectId` the Memory/Skills views need. `App.tsx` nav is `chat | projects | settings`.
- Try it headless: ingest a PDF/DOCX folder via `cargo run -p getmasters-cli -- knowledge --grant <dir>
  --ask "…"`; the FR-19 e2e (`extensions_toggle.rs`) disables `memory` and confirms its tools vanish
  from the next session while files/knowledge keep working.

## Not yet built (Phase 3+)

Study tools (flashcards/SM-2), Recipes + Scheduler, outbound delivery, external MCP servers,
Masters/Teams, OCR for image PDFs, and vector recall for memory/skills. The agent loop and prompt
assembler mark the seams; see the roadmap.
