# ADR-0018 — Provider vision (screenshot-based holdings ingestion)

**Status:** Accepted (implementation deferred to P2) · **Decision:** D18

## Context
Chinese users' holdings live in apps that offer no usable export — Alipay/fund platforms, broker
apps, bank wealth pages ([docs/11](../11-investment-agent.md) §2). Conversational entry is the MVP
path ([ADR-0016](./0016-asset-lifecycle-storage.md)), but the natural **bulk** input is a
screenshot of a holdings page. The `Provider` trait ([ADR-0003](./0003-llm-providers.md)) is
text-only (`chat`/`stream`/`embed`), so screenshot understanding needs a capability extension —
and holdings screenshots are among the most sensitive data the product will ever touch, so the
privacy route matters as much as the capability.

## Decision
1. **Vision enters through the `Provider` trait, not beside it.** Message content blocks gain an
   image variant and providers expose a vision capability flag; Anthropic and OpenAI-compatible
   vision models are the first backends, and an Ollama-compatible local VLM preserves the
   per-master privacy boundary ([ADR-0013](./0013-per-master-model.md)) for users who want
   screenshots processed on-device. All model access stays behind the trait (ADR-0003 invariant).
2. **Extraction is a proposal, never a write.** The pipeline is: screenshot → vision model →
   structured candidate rows (instrument, quantity/amount, cost if visible) → a **diff-preview
   the user confirms** → gated `AssetsServer` writes (ADR-0016's approval posture, rendered as
   domain approval cards). A recognition error is therefore catchable before it becomes ledger
   state — consistent with "never a wrong figure" (NFR-INV-1).
3. **Privacy handling for the image itself.** The flow states plainly **which provider will see
   the image** before upload; offers the local-VLM route when configured; and does **not** persist
   the image beyond extraction — screenshots enter neither the transcript store nor the knowledge
   index by default. The extracted rows follow the ADR-0016 boundary (local DB, redaction mode for
   cloud contexts).
4. **Scope: image input for structured extraction only.** No image generation; no change to the
   Knowledge pipeline (OCR for image PDFs remains its own deferred feature); and **no chart
   reading** — feeding price-chart screenshots to a VLM invites exactly the predictive
   technical-analysis output the compliance boundary (docs/11 §7) forbids, so it is explicitly
   out of scope rather than merely unbuilt.

## Alternatives considered
- **Classical OCR (tesseract-class) as the primary extractor** — holdings pages are dense,
  app-specific layouts where field *relationships* matter (which number is cost vs market value);
  VLMs handle this, plain OCR does not. Kept only as a possible pre-processing aid. Rejected as
  primary.
- **A separate vision service outside the `Provider` trait** — duplicates auth/config/retry
  plumbing and breaks the single model-access seam of ADR-0003. Rejected.
- **Cloud-side extraction service** (upload to masters-cloud, extract there) — centralizes the
  most sensitive images on our infrastructure; contradicts the local-first privacy story that
  justifies the product. Rejected.
- **Skip vision entirely (CSV only)** — leaves the dominant real-world input path unserved;
  CSV import remains the fallback for the minority with exportable statements. Rejected as the
  end state.

## Consequences
- (+) The lowest-friction bulk input for the core market becomes possible without breaking the
  trust chain (proposal → preview → gated write); the trait extension is reusable by future
  image-bearing features; local-VLM routing keeps a fully on-device path available.
- (−) Trait churn across all provider backends (capability detection, error mapping); vision
  calls are the most expensive per-interaction model cost in the product; a new privacy surface
  that must be communicated clearly at the moment of use; recognition quality varies by app
  layout and needs a correction-friendly preview UI.
- **Deferred:** the whole implementation (P2 — after the ask-and-track loop and progressive
  ledger prove out); local VLM quality validation for CN app layouts; client-side crop/redact
  tooling before upload; CSV/statement import (ships alongside as the structured-file fallback).
