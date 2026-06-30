# ADR-0012 — Multi-master conversation model

**Status:** Accepted · **Decision:** D12

## Context
[ADR-0010](./0010-master-team-orchestration.md) routes a brief to masters run as gated subagents,
but it says nothing about how multiple masters **communicate across turns**. The schema reflects
this gap: `messages` carries only `role` (`user`/`assistant`/`tool`) with no author, `sessions`
have no participant list, and there is no addressing or declarative multi-master sequencing beyond
`master_team_members.stage` ([05 §2](../05-data-storage-rag.md)). The user's three requirements —
**@-mention** one/several/all masters; every master sees the **full attributed group transcript**;
**declarative workflow orchestration** — need a stated conversation model.

This is a **single-user UI metaphor**: the only human author is the user, and "participants" are
personas the user controls — not collaborators. WorkBuddy's multi-user collaboration, sharing,
and handoff stay dropped ([ADR-0009](./0009-outbound-delivery-surfaces.md), [00](../00-overview.md)).
This ADR *extends* ADR-0010 the same way [ADR-0011](./0011-project-context-container.md) extended
[ADR-0007](./0007-layered-memory-prompt.md).

## Decision
1. **Session = single-user group chat.** Participants are the user + the masters attached to the
   session's team (new `session_participants`, [05 §2](../05-data-storage-rag.md)). Messages become
   **author-attributed** (`messages.author` / `author_master_id` / `addressed_to`); `role` stays
   for provider protocol compatibility.
2. **Shared read context, isolated gated execution.** When a master responds, modular prompt
   assembly ([ADR-0007 #4](./0007-layered-memory-prompt.md)) injects the **full speaker-labelled
   transcript** plus that master's persona (project-first ranking, [ADR-0011](./0011-project-context-container.md));
   its tool calls still run in an **isolated subagent under per-action Permission & Audit**
   ([ADR-0008](./0008-agent-isolation-parallelism.md)). Shared *reading* never merges *execution* —
   this principle reconciles the group-chat model with subagent isolation.
3. **Addressing.** `@master` (one or several) or `@all`/`@team` (everyone) selects responders; an
   **unaddressed** message is answered by the team's **coordinator master** (new
   `master_teams.coordinator_master_id`), which may delegate — with the master router
   ([FR-40](../01-product-requirements.md), `route_brief` [04 §2.7](../04-extensions-mcp.md))
   available as explicit routing assist. Multiple/all addressed → respond in parallel unless a
   workflow orders them. Mentions resolve against `session_participants`.
4. **Turn-taking / loop-safety.** Masters do **not** auto-reply to each other ad-hoc. A master
   speaks only when (a) @-addressed by the user, (b) ordered by a workflow step, or (c) selected
   by the coordinator/router. Optional bounded master↔master rounds sit behind a `max_rounds` cap.
   The user holds the floor; **Stop ([FR-3](../01-product-requirements.md)) halts the whole group.**
5. **Declarative Master Workflows.** A recipe-style YAML of ordered master steps posting attributed
   messages into the group; each step's output feeds the next (extends ADR-0010 sequential
   chaining). v1 is **linear + simple branching**, not a free DAG. Workflows are file-backed,
   runnable on demand or scheduled, and **promotable to a Recipe** ([04 §4](../04-extensions-mcp.md)).

## Alternatives considered
- **Keep flat role-only messages (no authorship)** — simplest, but cannot attribute who said what,
  which breaks @-mention, per-author audit, and the shared-transcript requirement.
- **Masters auto-reply freely (full autonomous multi-agent chat)** — emergent, but unbounded
  provider cost and real loop risk; contradicts the least-privilege/cost-bounded posture. Rejected
  in favor of a user-held floor + bounded `max_rounds`.
- **A full workflow DAG engine now** — overkill: Recipes already cover determinism; start with
  linear + simple branching and promote anything heavier to a Recipe.
- **Reuse Recipes for everything instead of an in-chat group model** — Recipes are
  deterministic/headless; the group chat is interactive and coordinator/router-driven. Keep both,
  bridged by promotion.

## Consequences
- (+) Natural multi-specialist conversation; author attribution sharpens the audit log (it records
  the authoring master); reuses subagents, prompt assembly, and the router — **no new trust
  bypass**; cost bounded by mention-scoping + parallelism caps.
- (−) The transcript grows, so long sessions need **context-window management** (summarization /
  RAG recall over old turns); `@all` fan-out multiplies provider calls (mitigated by
  mention-scoping + bounded parallelism + coordinator default); the schema gains authorship plus
  two tables; loop-safety must be enforced in **Core**, not by prompt wording alone.
- Maps to roadmap **Phase 3** (builds on masters/router from ADR-0010 and parallel subagents from
  [ADR-0008](./0008-agent-isolation-parallelism.md)). Revisit if: autonomous master↔master turns
  prove valuable enough to default-on, or the workflow model outgrows linear+branching and needs a
  real DAG.
