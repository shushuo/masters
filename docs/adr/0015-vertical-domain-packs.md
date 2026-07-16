# ADR-0015 — Vertical domain packs (the investing pivot pattern)

**Status:** Accepted · **Decision:** D15

## Context
Masters pivots from a general study/work agent into a **vertical product**: 《大师》 (Masters for
Investing), a local-first personal investment research & discipline workbench for the Chinese
market ([docs/11](../11-investment-agent.md), product decisions D1–D13). The foundation built under
ADR-0001..0014 is deliberately domain-neutral — Projects, Masters/Teams/group chat, Knowledge/RAG,
Memory/Skills/Study, Recipes+Scheduler+Delivery, MCP connectors, Permission & Audit. The question
this ADR settles: **how does a general foundation become a vertical product without forking the
architecture** — and in a way that could be repeated for a future vertical?

## Decision
1. **Verticalization = four layers on an unchanged foundation.**
   - **L1 — general core (untouched):** the crates and invariants of ADR-0001..0014. No investing
     types enter `getmasters-core`; the lean-core discipline (Study/Recipes precedents) holds.
   - **L2 — domain data & compute as built-in MCP servers** ([ADR-0005](./0005-mcp-sdk.md)):
     `AssetsServer` ([ADR-0016](./0016-asset-lifecycle-storage.md)), `MarketDataServer`
     ([ADR-0017](./0017-market-data-supply.md)), `FinCalcServer` (pure, clock-free functions —
     the `study::sm2` precedent), later `JournalServer`. DB-owned structured domain data behind
     **gated** rmcp tools, toggleable per project (FR-19), audited like every built-in.
   - **L3 — domain content packs, distributed via the existing catalog sync:** the expert-team
     bundle ([ADR-0010](./0010-master-team-orchestration.md) bundles), process Skills, routine
     Recipes, study decks, and the daily master-quote cards. Content iterates **without an app
     release**; system packs install as `origin:"system"` and never overwrite user content.
   - **L4 — vertical UI:** research cards, the watch/assets view, briefings, journal — thin views
     over the generated OpenAPI client, built on the docs/10 design system.
2. **The product identity pivots fully; the architecture does not.** Branding, onboarding, and
   default views are investing (docs/11 D2/D12); the general capabilities remain as substrate.
   Vertical logic lives only in L2 servers + L3 content + L4 views.
3. **Compliance is content + fixed UI, not scattered code.** The three-layer compliance language
   system (declaration / boundary / fallback — docs/11 §7) is enforced through personas and output
   contracts (L3) plus fixed UI surfaces (disclaimers, timestamps, provenance drawers — L4). The
   numeric discipline — *the LLM never mental-maths money* — is enforced by persona contract plus
   deterministic L2 tools, and by UI that exposes each figure's computation basis.
4. **The pattern is the template.** Foundation + domain servers + content packs + vertical UI is
   the repeatable recipe for any future vertical; docs/11 is the reference instance.

## Alternatives considered
- **Fork the repo per vertical** — permanent divergence of the loop/gate/UI foundations; every
  hardening fix lands N times. Rejected.
- **Put domain logic in `getmasters-core`** — breaks the lean-core rule that has kept every domain
  (YAML, cron, SMTP, ACP) out of core; investing math and schemas fit the established
  server/MCP-crate seams. Rejected.
- **Prompt-only verticalization** (personas + skills, no domain servers) — cannot guarantee
  correct numbers, structured accumulation, or provenance; violates "never a wrong figure".
  Rejected — this is the v1-of-docs/11 mistake the checkup/no-code claim already exposed.
- **Third-party marketplace only** (users assemble their own vertical from connectors) — the MVP
  aha requires first-party quality and zero-config data; a marketplace complements, not replaces.
  Rejected as the primary path.

## Consequences
- (+) No fork: one codebase serves the vertical; core fixes benefit everything. Content-speed
  iteration on personas/compliance language via catalog. The next vertical starts at L2, not L0.
- (−) First-party cloud surfaces (catalog, cross-section snapshot — ADR-0017) become
  product-critical operations. Compliance now depends on **content discipline**, so system-pack
  changes need a review step before publishing. The vertical UI adds surface the bundle-only
  verification must keep covering.
- **Deferred:** extraction of a formal "vertical template"; a second vertical to validate
  repeatability; marketplace/community packs beyond the system catalog.
