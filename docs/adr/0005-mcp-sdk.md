# ADR-0005 — MCP integration: official Rust SDK (`rmcp`)

**Status:** Accepted · **Decision:** D5

## Context
Masters's tools are delivered as **MCP servers** so it inherits the open extension ecosystem shared by Goose and
Claude. Goose currently uses an internal MCP implementation and is migrating toward the official SDK.

## Decision
Adopt the **official Model Context Protocol Rust SDK (`rmcp`)** from the start, used both to **host** external
MCP servers (as a client) and to **implement** Masters's built-in servers (Files, Knowledge, Study, Memory,
Web).

## Alternatives considered
- **Custom MCP implementation** (Goose's legacy path) — full control, but reimplements protocol details and
  incurs the migration debt Goose is now paying down.

## Consequences
- (+) Standards-compliant interop with the existing MCP server ecosystem; less protocol code to own; built-ins
  and externals share one transport/handshake; uniform permission/audit gating regardless of server origin.
- (−) Coupled to the SDK's release cadence and API.
- Revisit if: the SDK lacks a needed capability that can't be contributed upstream in time.
