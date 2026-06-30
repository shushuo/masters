# Masters task runner (Makefile). Mirrors the Justfile / docs/03 / docs/08 workflow.
#
# Every target maps to a raw cargo/pnpm command you can also run by hand. The Justfile
# remains for `just` users; this Makefile is the `make`-based equivalent.

SHELL := bash
.DEFAULT_GOAL := help

UI_DIR := ui/desktop

.PHONY: help build test test-core test-live fmt clippy check run-server ping \
        gen-openapi ui-install ui-build run-ui dev

help: ## List available targets.
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

# --- Rust ------------------------------------------------------------------

build: ## Build the whole workspace (excludes the Tauri app).
	cargo build --workspace

test: ## Run all Rust tests (unit + integration).
	cargo test --workspace

test-core: ## Lean-core gate: core tests need the `testing` feature for the offline fake.
	cargo test -p getmasters-core --features testing

test-live: ## Run the live Anthropic provider test (needs ANTHROPIC_API_KEY + network).
	GETMASTERS_RUN_LIVE=1 cargo test -p getmasters-core --test anthropic_live -- --ignored

fmt: ## Format all Rust code.
	cargo fmt --all

clippy: ## Lint the workspace, denying warnings.
	cargo clippy --workspace --all-targets -- -D warnings

check: fmt clippy test ## Format, lint, and test (pre-commit sweep).

run-server: ## Run the daemon (mock provider unless ANTHROPIC_API_KEY is set).
	cargo run -p getmasters-server --bin getmastersd

PROMPT ?= Hello from getmasters ping
ping: ## In-process agent-loop smoke test over the mock (no key, no daemon). Override with PROMPT=...
	cargo run -p getmasters-cli -- ping "$(PROMPT)"

# --- API contract ----------------------------------------------------------

gen-openapi: ## Emit the daemon OpenAPI spec, then regenerate the desktop's typed client.
	cargo run -p getmasters-server --bin gen_openapi -- $(UI_DIR)/src/api/openapi.json
	cd $(UI_DIR) && pnpm openapi-typescript src/api/openapi.json -o src/api/schema.ts

# --- Desktop (ui/desktop) --------------------------------------------------

ui-install: ## Install the desktop front-end dependencies.
	cd $(UI_DIR) && pnpm install

ui-build: ## Front-end bundle only (tsc + Vite) — works headless.
	cd $(UI_DIR) && pnpm build

run-ui: ## Full desktop app in dev mode — requires webkit/GTK + a display (NOT headless).
	cd $(UI_DIR) && pnpm tauri dev

dev: ## Headless web dev path (WSL/browser): daemon + Vite wired together.
	./scripts/dev-web.sh
