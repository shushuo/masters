# ADR-0006 — Skills: self-improving procedural memory

**Status:** Accepted · **Decision:** D6

## Context
Masters already has two ways to capture "how to do something": **Recipes** (human-authored, declarative YAML
workflows — [04 §4](../04-extensions-mcp.md)) and **Memory** (declarative facts/preferences — [04 §2.4](../04-extensions-mcp.md)).
Neither captures *learned procedure*: after the agent figures out a multi-step task ("how this user's lecture
slides are structured," "the exact dedupe rules that worked on Downloads"), that knowledge is lost at the end of
the session and re-derived from scratch next time. Hermes Agent's standout idea is **skills** — procedural
memory the agent writes for itself, improves on use, and can share. Masters's study/work routines are exactly the
repetitive, learnable procedures this targets.

## Decision
Add **Skills** as a first-class concept: a skill is an agent-authored, improvable, portable record of *how to
perform a recurring task* — its step-by-step procedure, known pitfalls, and verification steps. Skills are
stored as **plain Markdown files on disk** (per-project and global), indexed in `getmasters.db` for recall, and
managed by a built-in **Skills** MCP server (`create_skill`, `improve_skill`, `recall_skill`,
`list_skills`). The agent recalls relevant skills into context at task start and, after a successful complex
task, is nudged to capture/refine a skill (see [ADR-0007](./0007-layered-memory-prompt.md) for the curation
mechanism). Skills are **portable** (an open, documented file format; importable/exportable, optionally via a
community hub) — but an imported skill is *instructions, not trusted code*: every side-effecting step it drives
still routes through **Permission & Audit** ([04 §5](../04-extensions-mcp.md), [06](../06-security-privacy.md)).

**Skills vs Recipes vs Memory:** Recipes are *human-written and explicitly parameterized* (deterministic,
scheduleable); Memory is *declarative state* (facts/preferences); Skills are *agent-learned procedure*. A mature
skill can be "promoted" into a Recipe when the user wants determinism/scheduling.

## Alternatives considered
- **Reuse Recipes for everything** — but Recipes are human-authored and static; they don't self-improve or get
  written by the agent from experience, which is the whole point.
- **Store procedures inside Memory rows** — conflates declarative facts with multi-step procedures, and opaque
  DB rows aren't user-inspectable/editable (counter to local-first transparency, see ADR-0007).
- **No procedural memory (status quo)** — simplest, but forfeits the compounding-capability advantage and keeps
  re-deriving known workflows.

## Consequences
- (+) Compounding capability: recurring study/work tasks get faster and more reliable over time; transparent,
  user-editable skill files fit the local-first ethos; clean bridge to Recipes for determinism.
- (+) Portability lets users share/import procedures without sharing data.
- (−) New surface to design (capture heuristics, improvement loop, dedup of near-duplicate skills) and a new
  trust boundary for imported skills (mitigated: skills never bypass permission gating).
- Maps to roadmap **Phase 2** (with Projects/Memory/RAG). Revisit if: skills and recipes converge enough that one
  abstraction suffices.
