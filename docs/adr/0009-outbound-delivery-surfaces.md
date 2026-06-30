# ADR-0009 — Outbound delivery & notification surfaces

**Status:** Accepted · **Decision:** D9

## Context
Masters's **Scheduler** ([08 Phase 3](../08-roadmap.md), [04 §4](../04-extensions-mcp.md)) runs recurring routines
like "every Monday, summarize my /Inbox." But a digest is only useful if it *reaches the user*. v1 fires routines
only while the app is open and writes output to a file; there is no notion of *delivery*. Hermes Agent's gateway
delivers agent output across 20+ messaging platforms. Masters's non-goals ([00](../00-overview.md)) preclude
multi-user messaging bots, a hosted backend, and mobile apps — so adopting that gateway wholesale is out. But a
**local-first, single-user, read-only** slice of the idea is valuable: let scheduled output be *delivered* via
**local OS notifications** and an **optional outbound email digest**, without changing the agent's trust or
deployment model.

## Decision
Add an **outbound delivery** capability for routine output, with two channels in scope:
1. **Local OS notifications** (via Tauri) — fully on-device, no privacy boundary crossed.
2. **Optional email digest** — user-configured SMTP/provider; **off by default**; classified as a **`send`**
   side-effect, so it is approval-gated and audited like any external side-effect, and its privacy boundary
   (what content leaves the device) is shown ([06 §5](../06-security-privacy.md)).

Delivery is **one-way/outbound only** (Masters does not become a chat-bot inbound surface), preserves the
**loopback + per-launch token** daemon model ([02 §4](../02-architecture.md)), and remains **single-user**. This
**relaxes** the [00 non-goals](../00-overview.md) wording from "no notifications/output beyond the desktop" to
"optional, read-only, outbound notification/delivery surfaces" — recorded here so docs stay consistent.

## Alternatives considered
- **Full messaging gateway (Hermes-style, Telegram/Slack/etc.)** — rejected: implies inbound control surfaces,
  multi-user/bot tokens, and effectively a hosted/always-on backend — all explicit non-goals.
- **File-output only (status quo)** — simplest and fully local, but routines silently produce files the user may
  never notice; weak product value for automation.
- **Always-on background delivery service** — deferred to a future opt-in service ([06 §3](../06-security-privacy.md),
  [08 Phase 4](../08-roadmap.md)) with its own security review; v1 delivers only while the app runs.

## Consequences
- (+) Automation becomes genuinely useful (output reaches the user) while staying local-first and single-user;
  email reuses the existing `send` permission/audit class — no new bypass.
- (−) Email introduces an outbound network/privacy path that must be explicit, gated, and redaction-aware;
  notifications depend on OS integration.
- Maps to roadmap **Phase 3** (with the Scheduler); background/always-on delivery is **Phase 4** opt-in. Revisit
  if: demand for richer inbound/interactive surfaces challenges the single-user, local-first stance (would need a
  new ADR).
