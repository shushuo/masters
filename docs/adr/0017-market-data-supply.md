# ADR-0017 — Market data supply (hybrid client-direct + cloud snapshot)

**Status:** Accepted · **Decision:** D17

## Context
The vertical's entry experience — a research card answering "how is XX?" — needs market data
**at D0, zero-config** ([docs/11](../11-investment-agent.md) §3): daily quotes, NAVs, fundamentals,
filings, and a market-wide hot list. Three constraints collide: the local-first posture
([00](../00-overview.md)) resists a hard cloud dependency; CN market-data licensing is strict for
realtime redistribution; and a free tier cannot carry a commercial data bill. One decision already
made the problem tractable: the **slow-investing stance** (docs/11 D6) needs only end-of-day data —
every required class falls into the two lightest licensing tiers (statutory disclosure; delayed/EOD
public republication). This ADR settles who fetches what, from where, and how correctness is kept.

## Decision
1. **Hybrid supply, split by data shape.**
   - **Per-asset data → client-direct.** The desktop fetches quotes/NAVs/filings/fund profiles for
     the user's assets straight from public channels — each user acts as an individual reading
     public information (a browser-equivalent posture). Per-user volume is trivial (5–20 assets,
     daily).
   - **Market-wide cross-sections → cloud, once.** Hot lists and the weekly curated bulletin are
     computed by one daily/weekly cloud batch job and published as **static JSON on a CDN** — one
     artifact for all users, marginal cost ≈ zero. The human-reviewed "three things this week"
     bulletin (docs/11 §3.4, D13) ships inside this snapshot, making it a **single auditable
     compliance surface**.
   - **Cloud proxy = fallback only.** Used when client-direct fails; the cloud never becomes a
     hard runtime dependency (degradation, not gate).
2. **Source posture: statutory disclosure first, public republication second** (docs/11 D11).
   Filings and reports from the statutory disclosure channel (cninfo); fund NAVs from their
   mandated disclosure channels; EOD quotes from public republication channels. **No commercial
   redistribution licensing at this stage** (a post-scale subscription upgrade path);
   ToS-restricted APIs (e.g. Tushare) are available only as user-configured connectors
   ([ADR-0005](./0005-mcp-sdk.md) / Phase 4d) under the user's own account and terms. **Realtime
   and intraday data are permanently out of scope** (D6).
3. **Adapters are catalog-updated content, not baked code.** Fetch adapters are versioned
   artifacts distributed through the existing **catalog sync**, so an upstream page change is
   fixed by pushing the catalog — no app release. `MarketDataServer` owns the market-neutral
   unified schema (quotes/NAV/fundamentals/filings/calendar) and a local `price_cache` carrying
   **provenance**: source, fetched-at timestamp, validation status.
4. **Correctness over availability ("never a wrong figure", NFR-INV-1/2).** Key figures
   (close/NAV) are fetched from **two independent sources and cross-validated**; a match is
   cached and marked verified; a mismatch is flagged *disputed* and the UI degrades explicitly
   (⚠ on the field) rather than silently picking one. Every displayed value carries "data as of
   T"; a missing source yields an explicit lock/downgrade, never a fabricated or model-guessed
   number.
5. **Graceful absence.** With no data source reachable, the product still functions (assets,
   journal, RAG research, learning); data-dependent card fields lock with an explanation. Market
   data is an enhancer, not a boot precondition — deliberately unlike the provider requirement.

## Alternatives considered
- **Full cloud proxy** — best cache efficiency, but concentrates redistribution liability and the
  entire data bill on the free tier, and makes the cloud a hard dependency. Rejected.
- **Pure client-only** — market-wide cross-sections would be re-fetched per client (wasteful,
  slow, and hammering upstreams), and there would be no reviewed-bulletin channel. Rejected.
- **Commercial data purchase now** — quality and stability, at tens of thousands of ¥/year
  against a free tier with no revenue; premature. Deferred to the subscription tier at scale.
- **Bundling a Python AKShare runtime** — heavyweight desktop dependency for a handful of
  endpoints; AKShare serves as a development-time reference instead. The concrete adapter form
  (in-tree Rust vs catalog-distributed connector binary) is an implementation-time choice.

## Consequences
- (+) Free tier at near-zero marginal cost; the lightest available licensing posture; local-first
  preserved (cloud degrades, never gates); one reviewed artifact covers the compliance-sensitive
  editorial surface; upstream breakage is a content push away from fixed.
- (−) Adapter maintenance becomes an operational duty with an implicit freshness SLA; the UI must
  honestly render *verified / disputed / stale / locked* data states; formal legal review of the
  "EOD public republication" and "disclosure aggregation" boundaries is required before launch;
  double-fetching costs a little latency on first lookup.
- **Deferred:** the formal compliance opinion; adapter engineering selection; commercial upstream
  + hosted rich data for the subscription tier; overseas-market adapters (the schema is already
  market-neutral per docs/11 D1).
