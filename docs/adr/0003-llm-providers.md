# ADR-0003 — LLM providers: Claude-first, pluggable

**Status:** Accepted · **Decision:** D3

## Context
Masters is explicitly "Claude Cowork–like." Goose is provider-agnostic across 15+ backends from day one, which
adds breadth but also surface area (model registry, per-provider quirks, auth/retry matrices).

## Decision
Make **Anthropic Claude the default and best-supported provider** (`claude-opus-4-8` for heavy reasoning, a
Sonnet tier for fast/cheap turns), but route all model access through a **`Provider` trait** (`chat`, `stream`,
`embed`). Additional providers (OpenAI, Ollama/local) are added later without changing agent code.

## Alternatives considered
- **Multi-provider parity from day one** (Goose-style) — maximal flexibility, but more to build/test before the
  core experience is proven.
- **Single-vendor lock-in (Anthropic only)** — simplest, but forecloses a fully-local mode and user choice.

## Consequences
- (+) Simple, high-quality MVP aligned with the product's identity; clean seam for growth; enables a
  fully-local mode later (Ollama + local embeddings) for privacy (doc 06). The same trait later lets
  **individual masters run on different models/providers** ([ADR-0013](./0013-per-master-model.md)).
- (−) Non-Claude users wait for later phases.
- Revisit if: demand for other providers is strong enough to pull multi-provider work earlier in the roadmap.
