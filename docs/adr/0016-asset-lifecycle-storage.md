# ADR-0016 — Asset lifecycle storage (progressive accumulation)

**Status:** Accepted · **Decision:** D16

## Context
The cold-start reality of the investing vertical ([docs/11](../11-investment-agent.md) §2): new
users do **not** hand over their holdings — they probe with "how is stock XX?" and only disclose
themselves gradually as trust builds. An earlier design (docs/11 v2) required holdings up front
for a "portfolio checkup" and was cancelled for exactly this reason: it demanded the most
sensitive data before the product had proven any value. Personal investment data therefore needs
a storage model that **accumulates progressively from conversation** — and the product's second
touchpoint ("the XX you asked about reported earnings") depends on remembering what the user has
merely *asked about*, not only what they own.

## Decision
1. **One spine, not two features.** A single `assets` table carries each instrument through a
   lifecycle — `watching → holding → sold` (asking *is* the entry into `watching`) — instead of
   separate "watchlist" and "portfolio ledger" features. `positions` / `txns` / `accounts` hang
   off it; the watch view and the (later-unlocked) portfolio view are two states of the same list.
2. **DB-owned structured data via a gated built-in server.** Like Study
   ([ADR-0007](./0007-layered-memory-prompt.md)'s stated exception face), assets/positions/txns
   are relational, computed-over data — an rmcp `AssetsServer` ([ADR-0005](./0005-mcp-sdk.md))
   owns them behind gated tools. The **risk profile stays file-backed** (`RISK_PROFILE.md`,
   user-editable truth per ADR-0007), itself built progressively from conversation rather than an
   upfront questionnaire.
3. **Silent-but-revocable tracking.** The answer flow calls `track_asset`, recording a
   **point-in-time snapshot** — price, date, and the *reason* extracted from the conversation —
   surfaced as a light "now watching, one-click remove" notice rather than a confirmation step.
   It is Write-classified but low-sensitivity (local, reversible, no side effects beyond the local
   DB) and always audited. Position/transaction writes remain **approval-gated**, with
   domain-rendered approval cards ("record: bought 200 × XX @ ¥xx"), never raw JSON.
4. **Progressive holdings, never bulk demands.** "I bought XX" in conversation → a light proposal
   → a one-sentence recording. The product never asks for a full portfolio; bulk import
   (CSV / screenshot, [ADR-0018](./0018-provider-vision.md)) is a later convenience for users
   already convinced. Portfolio-level capability (FinCalc concentration/allocation, the allocation
   planner expert, a possible checkup revival) **unlocks** as holdings organically reach critical
   mass.
5. **Privacy boundary.** Asset detail never leaves the device (NFR-INV-3). An optional
   **redaction mode** strips absolute amounts from cloud-bound model context (weights and ratios
   only); holdings-heavy analysis can be pinned to a local-model expert
   ([ADR-0013](./0013-per-master-model.md)). Snapshot-derived hypothetical returns ("the XX you
   asked about in March is +12%") surface **only in coached quarterly reviews** — never in daily
   UI — to avoid feeding FOMO (docs/11 D10).

## Alternatives considered
- **Separate watchlist + ledger tables/features** — they are the same object at different trust
  stages; separating them forces a later migration exactly when users start converting watches
  into holdings. Rejected.
- **File-backed Markdown ledger** (Memory/Skills-style) — positions and transactions are
  relational and constantly computed over (returns, concentration, drift); hand-editable text is
  the wrong truth store, as Study already established for review state. Rejected.
- **Upfront import at onboarding** — reverses the trust-before-data order that killed the checkup
  wedge; kept only as an optional later path. Rejected as the entry design.
- **Confirmation prompt for every track** — control-theatre for a reversible local write; the
  friction kills the silent second-touchpoint loop. Rejected in favor of notice + undo.

## Consequences
- (+) Zero-friction accumulation aligned with the user's trust curve; the second touchpoint works
  from the first question; unified queries make the portfolio-level unlock a state transition, not
  a new feature; every write is audited and the sensitive ones approval-gated.
- (−) Every consumer must tolerate **partial data** (watching-only assets, positions with unknown
  cost); the Write-but-low-sensitivity classification of `track_asset` must stay narrowly scoped
  lest it become a precedent for silently writing anything; lifecycle states add invariants
  (e.g. `sold` requires a prior `holding`) the server must enforce.
- **Deferred:** multi-account reconciliation/dedupe; any broker synchronization (a standing
  non-goal — the ledger is a record, never an order channel, per docs/11 §7); the checkup's
  revival as an unlock for mature ledgers.
