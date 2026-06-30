# ADR-0001 — Backend language & architecture: Rust core + daemon

**Status:** Accepted · **Decision:** D1

## Context
Masters needs a long-running local backend that drives the agent loop, hosts MCP servers, manages provider
streaming, and persists state — exposed to a desktop UI (and optionally a CLI). Goose, our architectural
reference, uses a Rust core (`goose`) with a daemon (`goosed`) and a CLI sharing the same crate.

## Decision
Implement the backend in **Rust as a Cargo workspace** with a clean split:
`getmasters-core` (agent logic, reusable as a library) → `getmasters-server`/`getmastersd` (daemon) and an optional
`getmasters-cli`, plus `getmasters-mcp` (built-in servers) and `getmasters-proto` (shared DTOs).

## Alternatives considered
- **TypeScript/Node backend** — fastest for a solo builder and one language across the stack, but gives up the
  performance, single-binary distribution, and the mature Rust MCP SDK; weaker fit for supervising subprocesses
  and heavy concurrent I/O.
- **Go** — good concurrency and binaries, but smaller MCP/agent ecosystem and a second language alongside the
  Tauri (Rust) shell.

## Consequences
- (+) One toolchain shared with the Tauri desktop backend; reusable core for CLI; strong concurrency for
  streaming + MCP subprocess management; single static binary.
- (−) Slower to write than TypeScript; steeper contributor ramp.
- Revisit if: the project pivots to a TS-only stack or contributor velocity in Rust proves limiting.
