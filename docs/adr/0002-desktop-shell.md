# ADR-0002 — Desktop shell: Tauri 2

**Status:** Accepted · **Decision:** D2

## Context
Masters is a personal, always-available desktop app (NFR-3 favors a light footprint). Goose uses **Electron**.
The frontend will be a React + TypeScript SPA regardless of shell.

## Decision
Use **Tauri 2** as the desktop shell, with a React + TypeScript + Vite renderer (Tailwind + shadcn/ui).

## Alternatives considered
- **Electron** (Goose's choice) — uniform bundled Chromium and the largest ecosystem, but heavy install size
  and idle RAM for a single-user companion app.
- **Fully native** (egui / SwiftUI / WinUI) — best performance, but per-platform UI work and slower iteration
  for a content-rich app.

## Consequences
- (+) Much smaller installers and idle memory (OS WebView, no bundled Chromium); Rust backend shares the
  workspace toolchain (ADR-0001); native pickers/keychain/notifications.
- (−) Rendering varies slightly by OS WebView vs. Electron's uniform Chromium; some Electron-only libraries
  unavailable.
- Mitigation: content-light UI; test on all three platforms' WebViews.
- Revisit if: a hard dependency requires Chromium-uniform behavior, or WebView fragmentation causes real pain.
