# Masters desktop (src-tauri)

The Tauri 2 shell. It owns the `getmastersd` daemon lifecycle: spawns it as a sidecar, parses the
`GETMASTERSD_READY {json}` stdout handshake, and emits a `daemon-ready` event with `{ port, token }`
to the webview (`src/main.rs`).

## Not built in CI / headless containers

This crate is **excluded from the root Cargo workspace** on purpose — building it needs
webkit/GTK and a display. `cargo build --workspace` therefore never touches it. Build and run
it only on a real desktop (macOS/Windows/Linux-with-display).

## Local desktop build steps

1. Build the daemon and stage it as the sidecar binary (Tauri expects a target-triple suffix):

   ```bash
   cargo build -p getmasters-server --release
   TRIPLE=$(rustc -Vv | sed -n 's/host: //p')
   mkdir -p ui/desktop/src-tauri/binaries
   cp target/release/getmastersd "ui/desktop/src-tauri/binaries/getmastersd-$TRIPLE"
   ```

2. Generate the app icons once (creates `icons/`):

   ```bash
   cd ui/desktop && pnpm tauri icon path/to/logo.png
   ```

3. Run or bundle:

   ```bash
   cd ui/desktop && pnpm install && pnpm tauri dev      # dev window
   cd ui/desktop && pnpm tauri build                    # installer
   ```

## Browser-only front-end dev (headless-friendly)

The front-end can be developed without Tauri against a manually-started daemon:

```bash
cargo run -p getmasters-server --bin getmastersd      # note the GETMASTERSD_READY port + token
cd ui/desktop
VITE_GETMASTERS_PORT=<port> VITE_GETMASTERS_TOKEN=<token> pnpm dev
```
