# ADR-0013 — Per-master model selection

**Status:** Accepted · **Decision:** D13

## Context
[ADR-0003](./0003-llm-providers.md) routes all model access through a pluggable `Provider` trait
(`chat`/`stream`/`embed`), Claude-first but provider-agnostic. [ADR-0010](./0010-master-team-orchestration.md)
runs each master as an isolated, gated subagent, and the master persona already carries a
`default_model` field ([09 §2](../09-projects-masters.md), [05 §2](../05-data-storage-rag.md)) — but
that field is under-specified: there is no decision on how wide the model choice is, whether
providers can be mixed within one session, whether the model can be overridden at runtime, or what
the privacy consequences are when different masters in the same group chat run on different models.
Specialist roles benefit from different models — a cheap/fast triage master versus a heavy-reasoning
architect — so per-master model selection should be a first-class, stated decision.

## Decision
1. **Provider-qualified, persona-declared model.** Each master's persona declares a
   **provider-qualified `default_model`** (e.g. `anthropic:claude-opus-4-8`, `openai:gpt-…`,
   `ollama:llama3`). The value names both the provider and the model.
2. **Any configured provider.** A master may use any model from any **configured** provider via the
   `Provider` trait ([ADR-0003](./0003-llm-providers.md)) — Claude tiers, OpenAI, or a local Ollama
   model. **Heterogeneous models run side by side** in one team/group chat.
3. **Dispatch reuses existing subagents.** The orchestrator already spawns one isolated subagent per
   master ([ADR-0010](./0010-master-team-orchestration.md), [ADR-0008](./0008-agent-isolation-parallelism.md));
   it simply dispatches each subagent's provider calls to that master's model. **No new execution
   path and no change to Permission & Audit gating.**
4. **Persona-fixed (no runtime override).** The model is fixed by the persona file; to change it,
   edit the persona. This keeps behavior predictable and the persona the single source of truth
   (consistent with file-backed transparency, [ADR-0007](./0007-layered-memory-prompt.md)).
5. **Per-master privacy boundary + fallback.** A cloud-model master sends *its* turn's context to
   that provider; a **local-model master keeps its context on-device**. Because a group chat shares
   one transcript, the same content may reach **multiple providers** (one per responding master) —
   the UI surfaces each master's provider/boundary ([06 §5](../06-security-privacy.md)). If an
   master's provider/model is unconfigured or unavailable, the orchestrator **falls back to the
   default provider with a visible notice**. Embeddings remain **project-level** (RAG), not
   per-master.

This **extends** ADR-0003 (per-master use of the pluggable trait) and ADR-0010 (the per-master
field becomes a fully-specified capability); it does not reverse either — Masters stays Claude-first
by default.

## Alternatives considered
- **Single global model for all masters (status quo intent)** — simplest, but forfeits role-by-model
  specialization (cheap triage vs. heavy reasoning) and the local-model privacy option.
- **Claude tiers only (Opus/Sonnet/Haiku)** — simpler and one privacy boundary, but no local/cheap
  or cross-provider option; rejected in favor of any-configured-provider since the trait already
  supports it.
- **Runtime model override (per run/turn)** — more flexible cost/quality tuning, but less
  predictable and a larger UI surface; deferred (the persona stays the source of truth). Revisit if
  demand appears.

## Consequences
- (+) Cost/quality tuning per role; local-model masters for privacy-sensitive roles; reuses the
  `Provider` trait and the existing one-subagent-per-master execution — **no new trust bypass**.
- (−) A group chat may **fan the shared transcript to multiple providers** (a wider/forked privacy
  surface that must be shown); provider availability/fallback must be handled; mixed-model output may
  vary in voice/quality across masters.
- Maps to roadmap **Phase 3** for Claude-tier-per-master (Claude is configured then); full
  cross-provider masters land once additional providers ship (**Phase 4**, [08](../08-roadmap.md)).
  Revisit if: a runtime override becomes necessary, or per-master embeddings are ever needed.
