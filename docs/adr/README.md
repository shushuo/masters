# Architecture Decision Records

Each ADR captures one foundational choice: the context, the decision, and its consequences. D1–D5 are the
original foundational set; D6–D9 add the design refinements borrowed from [Hermes Agent](https://github.com/NousResearch/hermes-agent)
(adapted to Masters's local-first, single-user focus); D10–D12 add the **Master-Team**,
**Project-as-context-container**, and **multi-master conversation** concepts adapted from
[WorkBuddy](https://www.workbuddy.cn/) (single-user slice; D12 extends D10 with the group-chat communication
model); D13 adds **per-master model selection** (extends D3 + D10); D14 adds **external ACP master agents** —
a coding-harness master backend (extends D10, reuses D8's gate); D15–D18 add the **investing-vertical**
decisions from the [docs/11](../11-investment-agent.md) pivot — the vertical-domain-pack pattern, asset
lifecycle storage, hybrid market-data supply, and provider vision. Status values: `Accepted` (chosen
default) — revisit if the listed assumptions change.

| ADR | Decision | Status |
|-----|----------|--------|
| [0001](./0001-backend-language.md) | Backend language & architecture — **Rust core + daemon** | Accepted |
| [0002](./0002-desktop-shell.md) | Desktop shell — **Tauri 2** | Accepted |
| [0003](./0003-llm-providers.md) | LLM providers — **Claude-first, pluggable** | Accepted |
| [0004](./0004-vector-store.md) | RAG vector store — **SQLite + sqlite-vec** | Accepted |
| [0005](./0005-mcp-sdk.md) | MCP integration — **official Rust SDK (`rmcp`)** | Accepted |
| [0006](./0006-skills-procedural-memory.md) | Skills — **self-improving procedural memory** | Accepted |
| [0007](./0007-layered-memory-prompt.md) | Memory — **layered, file-backed + modular prompt assembly** | Accepted |
| [0008](./0008-agent-isolation-parallelism.md) | Agent — **isolation, least-privilege & parallel subagents** | Accepted |
| [0009](./0009-outbound-delivery-surfaces.md) | Delivery — **outbound notification/email surfaces** | Accepted |
| [0010](./0010-master-team-orchestration.md) | Master Team — **personas-over-Skills + master router + gated orchestration** | Accepted |
| [0011](./0011-project-context-container.md) | Project — **context container (bundle auto-injected, project-first ranking)** | Accepted |
| [0012](./0012-multi-master-conversation.md) | Multi-master conversation — **single-user group chat: @-addressing, shared attributed transcript, bounded turn-taking, declarative workflows** | Accepted |
| [0013](./0013-per-master-model.md) | Per-master model — **each master runs on its own provider-qualified model (any configured provider); persona-fixed; per-master privacy boundary** | Accepted |
| [0014](./0014-external-acp-master-agents.md) | External ACP master agents — **a coding-harness backend: drive a pre-installed ACP CLI (Claude Code/Codex/OpenCode/Gemini) as a first-class master; fs/permission callbacks routed through the gate** | Accepted |
| [0015](./0015-vertical-domain-packs.md) | Vertical domain packs — **verticalization = domain MCP servers + catalog content packs + vertical UI on the unchanged general foundation; compliance as content; repeatable pattern** | Accepted |
| [0016](./0016-asset-lifecycle-storage.md) | Asset lifecycle storage — **one asset spine (watching→holding→sold), DB-owned behind a gated server; silent-but-revocable tracking with snapshots; progressive accumulation; details never leave device** | Accepted |
| [0017](./0017-market-data-supply.md) | Market data supply — **client-direct public EOD data via catalog-updated adapters + one daily cloud cross-section snapshot on CDN + proxy as fallback; disclosure-first sourcing; dual-source validation; no realtime** | Accepted |
| [0018](./0018-provider-vision.md) | Provider vision — **image input through the Provider trait for screenshot holdings extraction; proposal→preview→gated write; local-VLM privacy route; no chart reading** | Accepted (deferred to P2) |
