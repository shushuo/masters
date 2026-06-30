# ADR-0010 — Master Team & multi-agent orchestration

**Status:** Accepted · **Decision:** D10

## Context
Masters already has the two primitives a multi-agent feature needs: **Skills**
([ADR-0006](./0006-skills-procedural-memory.md)) — agent-learned, portable procedure stored as
editable Markdown — and **parallel subagents** ([ADR-0008 #4](./0008-agent-isolation-parallelism.md))
— isolated sub-runs whose tool calls pass through the *same* Permission & Audit path and merge
back into the parent. What it lacks is (1) a **persona/role abstraction** so the agent can adopt
a specialist viewpoint with a constrained toolset, and (2) a **router/orchestrator** that maps a
single brief onto the right specialists.

WorkBuddy's headline pattern — *"1 brief, 100+ masters"* — supplies the shape: an **master** is
a persona layered on a Skill (a "senior backend engineer," a "slide designer"), a **master
router (总路由器)** decomposes a brief and dispatches it to specialists, and those specialists run
in **parallel** (division of labor) and/or **sequentially** (staged workflow:
trigger→collection→processing→output). Masters's non-goals ([00](../00-overview.md),
[ADR-0009](./0009-outbound-delivery-surfaces.md)) preclude WorkBuddy's *multi-user* collaboration
and inbound control — those are dropped; the single-user orchestration core is kept.

## Decision
1. **Masters = persona-over-Skill** — a master is a *thin role descriptor* layered on the
   Skills system ([ADR-0006](./0006-skills-procedural-memory.md)): **persona/system-prompt,
   allowed skills + tools, default model, output contract (format)**. It is stored as
   **editable Markdown** (`masters/*.md`, YAML frontmatter + body) indexed in `getmasters.db`,
   **mirroring Skills exactly** ([05 §2](../05-data-storage-rag.md)). No separate heavyweight
   agent abstraction is introduced.
2. **Master Team = masters + a master router** — a Team groups masters and carries a **router
   config**; the router maps a brief → master(s)/team (**auto-select**, with **manual override**
   always available as a fallback).
3. **Orchestration reuses parallel subagents** — the **Core** runs each selected master as an
   isolated subagent ([ADR-0008 #4](./0008-agent-isolation-parallelism.md)): **fan-out** for
   division of labor and **sequential chaining** for staged workflows. **Every master
   subagent's tool calls pass through the same Permission & Audit gating — no bypass, no
   aggregation that skips approval** ([06 §3](../06-security-privacy.md)). Routing *recommendation*
   is exposed as a read-only MCP tool ([04 §2.7](../04-extensions-mcp.md)); orchestration
   *execution* lives in Core, so the trust boundary is unambiguous.
4. **Portable bundles + Recipe bridge** — masters and teams are portable/installable bundles; a
   proven team workflow can be **promoted to a Recipe** for determinism/scheduling, mirroring the
   Skill→Recipe promotion ([ADR-0006](./0006-skills-procedural-memory.md), [04 §4](../04-extensions-mcp.md)).
   An imported master is **instructions, not trusted code** — origin (`builtin`/`learned`/`imported`)
   is shown and every step it drives still routes through Permission & Audit.

We explicitly **do not** adopt WorkBuddy's multi-user collaboration, task sharing/handoff,
member approval, or inbound messaging surfaces — out of scope for a single-user, outbound-only
app ([00](../00-overview.md), [ADR-0009](./0009-outbound-delivery-surfaces.md)).

## Alternatives considered
- **Masters as standalone heavyweight agents** (own loops, own tool stacks) — duplicates Skills
  and subagents, adds large surface area, and contradicts "borrow proven architecture"
  ([00](../00-overview.md)). The thin persona-over-Skill keeps one agent core.
- **No router; user always picks the master manually** — simpler, but forfeits the "1 brief"
  value. We keep manual pick as the fallback rather than the only mode.
- **A separate orchestration engine instead of reusing subagents** — needless second mechanism;
  contradicts [ADR-0008](./0008-agent-isolation-parallelism.md) and risks a gating bypass.
- **Full WorkBuddy multi-agent collaboration (sharing/handoff/approval)** — violates the
  single-user non-goals.

## Consequences
- (+) Domain specialization and brief-decomposition with **compounding reuse of Skills**;
  orchestration is "just" gated subagents, so it introduces **no new trust bypass**; portable
  team bundles plus the Recipe bridge for determinism.
- (−) Router quality/cost to design (brief→master classification); parallel master fan-out
  **multiplies provider calls** (cost/latency) and adds merge complexity; a new trust boundary
  for imported masters (mitigated: never bypasses gating).
- Maps to roadmap **Phase 3** — it builds on Skills (Phase 2) and parallel subagents (Phase 3/4,
  [ADR-0008](./0008-agent-isolation-parallelism.md)); manual single-master use can appear
  earlier ([08](../08-roadmap.md)). Revisit if: masters and skills converge enough to merge into
  one abstraction, or the router adds no measurable value over manual selection.
