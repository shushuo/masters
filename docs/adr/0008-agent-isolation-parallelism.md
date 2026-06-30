# ADR-0008 — Agent isolation, least-privilege & parallel subagents

**Status:** Accepted · **Decision:** D8

## Context
[06 §8](../06-security-privacy.md) leaves three security questions open: the exact default policy matrix per
side-effect class, sandboxing of external content/servers, and handling of untrusted inputs. Masters runs
**external MCP servers** as subprocesses ([04 §3](../04-extensions-mcp.md)) and ingests untrusted documents, so
its trust surface is real. Separately, some Masters tasks are embarrassingly parallel (ingesting a folder of
PDFs, scanning several granted folders) but the v1 agent loop is single-threaded. Hermes Agent informs both:
**defense-in-depth** (container isolation, command-approval policies, credential stripping from child
processes, a least-privilege "Blank Slate" mode) and **isolated subagents** for parallel workstreams. Masters
deliberately is **not** a code-execution/dev agent ([00 non-goals](../00-overview.md)), so it borrows the
*isolation and least-privilege* posture without adopting Hermes' multi-backend remote/serverless execution.

## Decision
1. **Least-privilege "Blank Slate" mode** — a session/profile can boot with a minimal default tool set and no
   standing permissions; capabilities are granted as needed. This becomes the conservative default posture for
   sensitive work and complements folder grants.
2. **Concrete default policy matrix** — publish the per-side-effect-class default (what auto-allows out of the
   box: reads inside a grant may auto-allow; `write`/`destructive`/`network`/`send` always prompt until standing
   permission is granted), closing the [06 §8](../06-security-privacy.md) open question.
3. **External-MCP isolation** — external servers run with **credential stripping** (inherit only env Masters
   injects, secrets resolved from keychain at spawn) and OS-level sandboxing where available; their declared
   tools + side-effect classes are shown before enabling.
4. **Parallel subagents** — the Core may spawn **isolated subagents** for parallel sub-tasks; each subagent's
   tool calls still pass through the *same* Permission & Audit path (no aggregation that bypasses gating), and
   results merge back into the parent run.

We explicitly **do not** adopt remote/serverless execution backends (Docker/SSH/Daytona/Modal) or running the
agent's own code in containers — out of scope for a local, non-coding personal app.

## Alternatives considered
- **Leave §8 open / status quo** — fastest, but a file-and-network agent needs a stated, testable security
  posture from the MVP to be trustworthy.
- **Full Hermes-style multi-backend execution** — powerful, but contradicts local-first/non-coding non-goals and
  adds large surface area for little personal-use benefit.
- **No subagents** — simpler loop, but leaves easy parallelism (bulk ingest) on the table.

## Consequences
- (+) A stated, default-deny-leaning policy matrix and least-privilege mode make the MVP defensibly safe;
  subagents speed up bulk work without weakening gating.
- (−) Sandboxing varies by OS (best-effort on some platforms); subagents add orchestration/merge complexity and
  must be rate/permission-bounded.
- Maps to roadmap: policy matrix + Blank Slate land with **Phase 1** permissions; sandbox hardening in **Phase 4**;
  parallel subagents in **Phase 3/4**. Revisit if: a hard requirement for isolated code execution emerges.
