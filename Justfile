# Masters task runner. Mirrors Goose's `just`-driven workflow (docs/03, docs/08).
#
# NOTE: `just` is a convenience only — every recipe maps to a raw cargo/pnpm command you can
# run directly (which is how CI / a headless container verifies the build). Install just with
# `cargo install just` if you want the shortcuts.

set shell := ["bash", "-uc"]

# List available recipes.
default:
    @just --list

# --- Rust ------------------------------------------------------------------

# Build the whole workspace (excludes the Tauri app — see root Cargo.toml).
build:
    cargo build --workspace

# Run all Rust tests (unit + integration). Live Anthropic tests stay #[ignore]d.
# (`--workspace` enables getmasters-core's `testing` feature via the server's dev-dependency.)
test:
    cargo test --workspace

# Lean-core gate: the core crate's own tests need the `testing` feature for the offline fake.
test-core:
    cargo test -p getmasters-core --features testing

# Run the live Anthropic provider test (needs ANTHROPIC_API_KEY + network).
test-live:
    GETMASTERS_RUN_LIVE=1 cargo test -p getmasters-core --test anthropic_live -- --ignored

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run the daemon (mock provider unless ANTHROPIC_API_KEY is set).
run-server:
    cargo run -p getmasters-server --bin getmastersd

# In-process end-to-end smoke test through the agent loop (mock; no key, no daemon).
ping prompt="Hello from getmasters ping":
    cargo run -p getmasters-cli -- ping "{{prompt}}"

# --- API contract ----------------------------------------------------------

# Emit the daemon's OpenAPI spec, then regenerate the desktop's typed client from it.
# This is the single source of truth for the client⇄daemon DTOs (docs/02 §4).
gen-openapi:
    cargo run -p getmasters-server --bin gen_openapi -- ui/desktop/src/api/openapi.json
    cd ui/desktop && pnpm openapi-typescript src/api/openapi.json -o src/api/schema.ts

# --- Desktop (ui/desktop) --------------------------------------------------

ui-install:
    cd ui/desktop && pnpm install

# Front-end bundle only (Vite/tsc) — works headless, no native Tauri build.
ui-build:
    cd ui/desktop && pnpm build

# Full desktop app in dev mode — requires webkit/GTK + a display (NOT a headless container).
run-ui:
    cd ui/desktop && pnpm tauri dev
