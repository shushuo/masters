# 03 — Tech Stack

This document records *what* technologies Masters uses and *why*. The five foundational **stack** choices (D1–D5)
have standalone [ADRs](./adr/) with fuller context and consequences; the later product/architecture decisions
(D6–D9, Hermes-informed: Skills, file-backed memory, isolation, delivery; and D10–D13, WorkBuddy-informed:
Master Teams, Project-as-context-container, multi-master conversation, per-master model) live in the same ADR
folder. Of those, the one with a direct stack consequence is **D13** — per-master model selection rides on the
same `Provider` trait (see the LLM section below).

## Summary table

| Layer | Choice | Key alternatives considered |
|---|---|---|
| Backend language | **Rust** (Cargo workspace) | TypeScript/Node, Go |
| Async runtime | **Tokio** | async-std |
| Daemon HTTP/WS | **axum** + tower | actix-web, warp |
| API contract | **OpenAPI** + generated TS client | tRPC, hand-written client |
| Desktop shell | **Tauri 2** | Electron, native (egui/SwiftUI) |
| Frontend | **React + TypeScript + Vite** | Svelte, SolidJS |
| Styling/UI kit | **Tailwind CSS + shadcn/ui** | Mantine, MUI |
| LLM access | **Claude-first** via provider trait | Multi-provider parity, single-vendor lock-in |
| Embeddings | Provider embeddings or local model | OpenAI embeddings only |
| Storage | **SQLite** (via `sqlx`/`rusqlite`) | Postgres (overkill), files only |
| Vector search | **`sqlite-vec`** | LanceDB, Qdrant, pgvector |
| Extensions | **MCP** via official **`rmcp`** SDK | Custom plugin protocol |
| Secrets | **OS keychain** (keyring crate) | Encrypted config file |
| Build/tasks | **Cargo + Just + pnpm** | Make, npm |
| Packaging | **Tauri bundler** (msi/dmg/AppImage) | electron-builder |

## Rationale by area

### Backend: Rust + Tokio + axum (D1)
Following Goose, the core agent logic lives in Rust for performance, a single static binary, and a clean split
between a reusable **core crate**, a **daemon**, and an optional **CLI**. axum (built on tower/hyper) gives
ergonomic HTTP + WebSocket handlers that fit the streaming agent loop. Tokio powers concurrent MCP subprocess
management and provider streaming.

> Trade-off: Rust is slower to write than TypeScript for a solo builder. Accepted because the daemon/CLI/desktop
> split and MCP Rust SDK are worth it, and the heaviest UI work happens in TypeScript anyway. See
> [ADR-0001](./adr/0001-backend-language.md).

### Desktop: Tauri 2 + React + TypeScript (D2)
Goose uses Electron; Masters chooses **Tauri 2** instead. Tauri uses the OS WebView (no bundled Chromium), so
installers and idle memory are dramatically smaller — important for a personal always-available app (NFR-3).
Tauri's backend is Rust, so it shares one toolchain with the core. The renderer is still a standard React +
TypeScript SPA (Vite, Tailwind, shadcn/ui), so UI development is conventional.

> Trade-off: WebView rendering differs slightly per OS vs. Electron's uniform Chromium. Accepted for a
> content-light, native-feeling app. See [ADR-0002](./adr/0002-desktop-shell.md).

### API contract: OpenAPI + generated client
The daemon publishes an OpenAPI description; the desktop's typed client is generated from it (mirroring Goose's
`just generate-openapi` step). One source of truth keeps DTOs synchronized and prevents UI/daemon drift.

### LLM: Claude-first behind a provider trait (D3)
Masters is "Claude Cowork–like," so **Anthropic Claude** is the default and best-supported provider
(`claude-opus-4-8` for heavy reasoning, a Sonnet tier for fast/cheap turns). But all model access goes through a
`Provider` trait (`chat`, `stream`, `embed`), so OpenAI, Ollama, and local models can be added without touching
agent code. This keeps the MVP simple while preserving Goose-style provider agnosticism as a growth path.
Building on the same trait, **individual masters can be assigned different models/providers** (a heterogeneous
mix within one session), declared in each persona — still Claude-first by default
([ADR-0013](./adr/0013-per-master-model.md)).

> See [ADR-0003](./adr/0003-llm-providers.md).

### Storage + RAG: SQLite + sqlite-vec (D4)
A single embedded SQLite database holds sessions, messages, projects, memory, audit log, **and** vector
embeddings via the `sqlite-vec` extension. Zero external services, one file to back up, trivial to ship in a
desktop app. This covers the personal-scale corpus comfortably; if a user's library outgrows it, LanceDB is the
documented upgrade path.

> See [ADR-0004](./adr/0004-vector-store.md) and [05 — Data/Storage/RAG](./05-data-storage-rag.md).

### Extensions: MCP via `rmcp` (D5)
Masters speaks the **Model Context Protocol** so it inherits the open extension ecosystem (the same protocol
Goose and Claude use). Masters adopts the **official Rust SDK (`rmcp`)** from day one — both to *host* external
MCP servers and to *implement* its own built-in servers — avoiding the internal-implementation debt Goose is
migrating away from.

> See [ADR-0005](./adr/0005-mcp-sdk.md) and [04 — Extensions/MCP](./04-extensions-mcp.md).

### Secrets & build
API keys live in the **OS keychain** (via the `keyring` crate), never in plaintext config (NFR-6). Build/task
automation uses **Just** (Rust side) and **pnpm** (UI), with **Cargo** for compilation — matching Goose's
tooling so the workflow is familiar.

## Version/target notes

- **Rust** stable, edition 2021+.
- **Tauri 2.x**, **React 18+**, **Vite 5+**, **TypeScript 5+**.
- **SQLite** bundled; `sqlite-vec` loaded as an extension.
- **Targets:** Windows (msi), macOS (dmg, universal), Linux (AppImage/deb).
- Confirm current crate/library versions with Context7 / upstream docs at implementation time (this doc fixes
  *choices*, not pinned versions).
