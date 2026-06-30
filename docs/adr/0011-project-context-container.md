# ADR-0011 — Project as a context container

**Status:** Accepted · **Decision:** D11

## Context
[FR-7](../01-product-requirements.md) currently defines a **Project** thinly: "one or more
folders + instructions + memory + linked sources, resumable across app restarts." Meanwhile
Masters has independently grown every other ingredient a project needs — folder grants
([06](../06-security-privacy.md)), a per-project Knowledge/RAG index ([05](../05-data-storage-rag.md)),
layered file-backed memory ([ADR-0007](./0007-layered-memory-prompt.md)), Skills
([ADR-0006](./0006-skills-procedural-memory.md)), and external MCP connectors
([04](../04-extensions-mcp.md)). But these are configured and recalled *separately*; nothing
unifies them under the Project, and modular prompt assembly ([ADR-0007 #4](./0007-layered-memory-prompt.md))
has no stated rule for ranking project-scoped knowledge above global.

WorkBuddy — the closest single-user-adjacent analogue to Masters — models the Project as a
**two-layer Project→Task context hub**: a Project bundles its instructions, connectors,
masters, skills, and knowledge base, and **auto-injects them into every task created under it**,
with project-scoped items ranked first. That is precisely the missing unification. This is an
*evolution* of FR-7, not a new primitive: Masters already has all the pieces.

## Decision
A **Project is the context container**. It bundles, as one coherent unit:
**{instructions (`INSTRUCTIONS.md`), folder grants, knowledge base, memory
(`MEMORY.md`/`USER.md`), skills, masters / master-team ([ADR-0010](./0010-master-team-orchestration.md)),
MCP connectors}**.

1. **Auto-injection** — every session ("task") under a Project inherits the bundle through
   modular prompt assembly ([ADR-0007 #4](./0007-layered-memory-prompt.md)); the user does not
   re-attach context per task.
2. **Project-first ranking** — in assembly and recall, **project-scoped items rank above
   global** ones (a project skill/master/memory wins over its global namesake). This *extends*
   ADR-0007's assembly ordering with an explicit precedence rule.
3. **Project-scoped connectors** — which built-in/external MCP servers are active is part of the
   bundle (a new `project_connectors` projection, [05 §2](../05-data-storage-rag.md)), so a
   Project can enable Notion/Calendar without turning them on globally.
4. **Project templates** — a Project can be created from a **portable template** that pre-seeds
   the bundle (instructions + recommended masters/team + skill set + connectors), the same way
   Skills and Recipes are portable ([ADR-0006](./0006-skills-procedural-memory.md),
   [04 §4](../04-extensions-mcp.md)).

This **extends** FR-7 and ADR-0007 (exactly as [ADR-0007](./0007-layered-memory-prompt.md) framed
itself as extending [ADR-0004](./0004-vector-store.md)); it does not reverse them. The Project
stays **single-user** — WorkBuddy's collaboration/sharing/handoff layers are out of scope
([00 non-goals](../00-overview.md), [ADR-0009](./0009-outbound-delivery-surfaces.md)).

## Alternatives considered
- **Keep FR-7 thin; configure each element separately** — simplest and already partly built, but
  loses the "one brief inherits the whole working context" value, leaves project-vs-global ranking
  undefined, and contradicts the unifying product story.
- **Introduce a separate "workspace" object distinct from Project** — redundant: the Project
  already *is* the workspace; a second object would only duplicate state and confuse the model.
- **Global-only masters/skills/connectors (no project scoping)** — less to model, but forfeits
  relevance ranking and per-project specialization, the main point of a context hub.

## Consequences
- (+) A coherent context hub: tasks start fully equipped; templates make onboarding fast; reuses
  every existing store (grants, RAG, memory, skills, masters) with no new trust surface.
- (+) Project-first ranking gives sharper recall and lets a Project override global defaults.
- (−) Must define **precedence/conflict rules** in prompt assembly (project vs. global) and a
  **bundle/template serialization format**; the Project screen gains more configuration surface.
- Maps to roadmap: container auto-injection + project-first ranking land with **Phase 2**
  (Projects/RAG/memory); **templates** and the masters/team element land in **Phase 3**
  ([08](../08-roadmap.md)). Revisit if: per-element configuration proves preferable to a bundled
  container, or template serialization drifts from the Recipe/Skill portability format.

## Amendment — standalone (global) masters + quick chat (Masters sidebar)

Masters may now also exist **globally**, independent of any project: a top-level **Masters**
sidebar manages them as files under `<data_home>/masters/` (the `global_masters` index table),
so a user can author and browse masters — including a curated **built-in template gallery**
("system masters") — without first creating a project. This **extends** (does not reverse) the
rule above that a Project *bundles* its masters: project-scoped masters still take precedence on
slug collision, and a session under a project still injects the project's masters first.

Because running a master needs an agent/grant/tool context (which lives on a Project), the system
keeps a lazily-created **default project** as the run context for **quick chat** — start an
interactive chat with one master (1:1) or several at once, reusing the existing group-chat
machinery (an ephemeral team under the default project; a single master = a one-member team). A
user-**starred default master** (a `settings` row) coordinates when no master is explicitly
addressed. Master *loading* falls back project→global, so a global master is addressable by the
router, mentions, teams, group chat, and bundles unchanged.
