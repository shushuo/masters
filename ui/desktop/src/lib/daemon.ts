// Daemon connection discovery.
//
// In the packaged app the Tauri Rust side spawns `getmastersd`, parses its `GETMASTERSD_READY`
// handshake, and emits a `daemon-ready` event carrying { port, token }. This module waits
// for that event. When running the front-end in a plain browser (e.g. `vite dev` without
// Tauri), it falls back to `VITE_GETMASTERS_PORT` / `VITE_GETMASTERS_TOKEN` so the UI is still
// developable against a manually-started daemon.

import type { DaemonConn } from "../api/client";

/** True when running inside a Tauri webview. */
function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function fromTauriEvent(): Promise<DaemonConn> {
  const { listen } = await import("@tauri-apps/api/event");
  return new Promise<DaemonConn>((resolve) => {
    const unlisten = listen<DaemonConn>("daemon-ready", (event) => {
      resolve(event.payload);
      unlisten.then((fn) => fn());
    });
  });
}

function fromEnv(): DaemonConn | null {
  const port = Number(import.meta.env.VITE_GETMASTERS_PORT);
  const token = import.meta.env.VITE_GETMASTERS_TOKEN as string | undefined;
  if (port && token) return { port, token };
  return null;
}

/** Resolve the daemon connection (Tauri event in-app, env vars in browser dev). */
export async function resolveDaemon(): Promise<DaemonConn> {
  if (inTauri()) return fromTauriEvent();
  const env = fromEnv();
  if (env) return env;
  throw new Error(
    "no daemon connection: run inside Tauri, or set VITE_GETMASTERS_PORT / VITE_GETMASTERS_TOKEN for browser dev",
  );
}
