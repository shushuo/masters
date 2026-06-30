# 00 — Overview

## Vision

**Masters is a local-first agentic desktop companion that helps one person learn and get personal work done.**

You give Masters a goal and access to a folder. It reads your materials, plans the steps, uses tools (file
operations, knowledge retrieval, web fetch, study utilities), and produces a *finished deliverable* — a sorted
folder, a synthesized draft, a set of flashcards, a study plan — while asking for your approval before anything
consequential. It is the difference between an assistant that *tells you how* and an agent that *does it with
you*.

It deliberately borrows three things:

- From **Claude Cowork** — the *product philosophy*: agentic, file-touching, outcome-oriented, persistent
  Projects with memory, and human-in-the-loop control.
- From **Goose** — the *engineering blueprint*: a Rust core with a clean CLI/daemon/desktop split, a
  provider-agnostic LLM layer, an open MCP extension ecosystem, and recipes + a scheduler.
- From **Hermes Agent** ([NousResearch](https://github.com/NousResearch/hermes-agent)) — the *learning loop*:
  **self-improving Skills** (procedural memory the agent writes and refines), **layered, file-backed memory**
  the user can read and edit, and a defense-in-depth posture (least-privilege mode, isolation). Adapted to
  Masters's local-first, single-user focus — its 20+ messaging-platform gateway and remote/serverless execution
  are explicitly *not* borrowed (see [ADR-0009](./adr/0009-outbound-delivery-surfaces.md)).

It also borrows, from **WorkBuddy** (a desktop office agent), a structural pattern: the **Project as a context
container** and an **Master Team** (personas-over-Skills routed across parallel sub-agents). Masters adopts only
the *single-user slice* — WorkBuddy's multi-user collaboration, task sharing/handoff, and inbound messaging
control are **not** borrowed (see [ADR-0010](./adr/0010-master-team-orchestration.md),
[ADR-0011](./adr/0011-project-context-container.md), [09](./09-projects-masters.md)).

Masters's novelty is **focus**: where Cowork is broad knowledge work and Goose is a coding agent, Masters's
built-in toolset and UX are tuned for the two activities a single person does most at a desk — **studying** and
**personal work**.

## Who it's for

### Persona A — "The learner"
A student or self-directed learner with a folder full of PDFs, lecture slides, papers, and scattered notes.

> *"I have 40 PDFs for this course. Help me understand them, quiz me, and tell me what to review before the
> exam."*

Wants: grounded summaries and Q&A **over their own materials** (with citations, not hallucinations),
auto-generated flashcards, a spaced-repetition review schedule, and a study plan that adapts to a deadline.

### Persona B — "The knowledge worker"
An individual professional drowning in files and repetitive document chores.

> *"Sort my Downloads folder, merge these three reports into one brief, and every Monday summarize what landed
> in my Inbox folder."*

Wants: file organization (rename/sort/dedupe), multi-source synthesis into a draft, and **recurring routines**
that run on a schedule.

Most real users are *both* at different moments of the day, which is why a single app serves them.

## Value proposition

1. **It finishes tasks, not sentences.** Outcome-oriented agent loop with real file side effects.
2. **It's grounded in *your* materials.** Local RAG over your documents → answers cite your sources.
3. **It's local-first and private.** Your files and embeddings stay on your machine; only prompts/context go to
   the chosen model provider, and that boundary is explicit and auditable.
4. **You stay in control.** Folder-scoped permissions and per-action approvals for anything that writes,
   deletes, or sends.
5. **It's extensible.** Anything expressible as an MCP server (Notion, Calendar, a custom tool) plugs in.
6. **It remembers.** Projects persist files, instructions, and memory across sessions.

## How Masters compares

| Dimension | Claude Cowork | Goose | Hermes Agent | **Masters** |
|---|---|---|---|---|
| Primary job | General knowledge work | Software engineering | Always-on personal assistant | **Studying + personal work** |
| Target user | Knowledge worker | Developer | Self-hoster / power user | **Individual learner / worker** |
| Deployment | Desktop, cloud-coupled | Local (desktop + CLI + API) | Self-hosted (laptop → VPS → serverless) | **Local-first desktop (+ optional CLI)** |
| LLM providers | Anthropic only | 15+ providers | Model-agnostic (300+) | **Claude-first, pluggable** |
| Extension model | Connectors (curated) | MCP (open, 70+) | Skills + tools | **MCP (open) + study/work built-ins** |
| Persistence | Projects + memory | SQLite sessions | Layered file memory + skills + SQLite FTS | **Projects + file-backed memory + Skills + sessions (SQLite)** |
| Procedural learning | — | — | **Self-improving Skills** | **Self-improving Skills (gated)** |
| Automation | — | Recipes + scheduler | Cron + multi-platform delivery | **Recipes + scheduler + outbound notify/email** |
| User surfaces | Desktop | Desktop + CLI | 20+ messaging platforms + desktop | **Desktop (+ optional CLI), single-user** |
| Grounding on local docs | Yes (file access) | Via dev tools | Via tools | **First-class RAG with citations** |
| Study features | — | — | — | **Flashcards, spaced repetition, study plans** |
| Open source | No | Yes (Apache-2.0) | Yes | **Yes (intended Apache-2.0)** |

## Design principles

- **Local-first.** The default assumption is that data never leaves the device except the model context the
  user can see.
- **Human-in-the-loop by default.** Reads can be auto-approved per folder; writes/deletes/sends require
  explicit consent until the user grants standing permission.
- **Grounded over generative.** When materials exist, answer from them and cite; flag when answering from model
  knowledge instead.
- **Learn and compound.** Procedures the agent figures out are captured as **Skills** and refined over time, so
  recurring study/work tasks get faster and more reliable ([ADR-0006](./adr/0006-skills-procedural-memory.md)).
- **Transparent, editable memory.** Durable memory and instructions live as **files the user can read and edit**,
  not opaque rows ([ADR-0007](./adr/0007-layered-memory-prompt.md)).
- **Borrow proven architecture.** Don't reinvent Goose's structure or Hermes' learning loop; adapt them.
- **One person, deeply served.** No multi-tenant complexity in v1 — optimize the solo experience.

## Scope boundaries (non-goals for v1)

- No team/multi-user collaboration or shared cloud workspaces. This includes **no multi-user master
  collaboration, task sharing/handoff, or member approval** (WorkBuddy-style) — Masters and Master Teams are
  single-user ([ADR-0010](./adr/0010-master-team-orchestration.md)).
- No hosted backend — the daemon runs on the user's machine.
- Not a coding IDE (though the Files extension can touch code, code-authoring UX is out of scope).
- No mobile apps. Output may reach the user through **optional, read-only outbound surfaces** — local OS
  notifications and an opt-in email digest for scheduled routines — but Masters is not an inbound chat-bot and
  adds no messaging-platform gateway ([ADR-0009](./adr/0009-outbound-delivery-surfaces.md)).
- No remote/serverless code-execution backends — execution stays local; isolation is for *sandboxing*, not
  distribution ([ADR-0008](./adr/0008-agent-isolation-parallelism.md)).
- No model training/fine-tuning.

See [01 — Product Requirements](./01-product-requirements.md) for the detailed feature breakdown.
