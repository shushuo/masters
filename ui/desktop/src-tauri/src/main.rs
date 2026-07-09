// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Masters desktop entry point.
//!
//! The desktop owns the `getmastersd` lifecycle (docs/02 §5): it spawns the daemon as a Tauri
//! sidecar, reads its `GETMASTERSD_READY {json}` handshake from stdout, and emits a
//! `daemon-ready` event carrying `{ port, token }` to the webview. The renderer then
//! health-checks and connects. Daemon logs arrive on stderr and are forwarded to the
//! console for debugging.

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

/// Payload of the `daemon-ready` event (mirrors the daemon's handshake JSON).
#[derive(Clone, Serialize, serde::Deserialize)]
struct DaemonReady {
    port: u16,
    token: String,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // In-app auto-update (checked from the renderer) + relaunch after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Spawn the bundled `getmastersd` sidecar (configured in tauri.conf.json `externalBin`).
            let sidecar = app
                .shell()
                .sidecar("getmastersd")
                .expect("getmastersd sidecar must be configured");
            let (mut rx, _child) = sidecar.spawn().expect("failed to spawn getmastersd");

            // Read the daemon's output. Moving `_child` into the task keeps it alive for the
            // app's lifetime; when the app exits the runtime is torn down and the child stops.
            tauri::async_runtime::spawn(async move {
                let _child = _child;
                while let Some(event) = rx.recv().await {
                    match event {
                        CommandEvent::Stdout(bytes) => {
                            let line = String::from_utf8_lossy(&bytes);
                            if let Some(rest) = line.trim().strip_prefix("GETMASTERSD_READY ") {
                                match serde_json::from_str::<DaemonReady>(rest) {
                                    Ok(ready) => {
                                        let _ = handle.emit("daemon-ready", ready);
                                    }
                                    Err(e) => eprintln!("bad GETMASTERSD_READY handshake: {e}"),
                                }
                            }
                        }
                        CommandEvent::Stderr(bytes) => {
                            eprint!("[getmastersd] {}", String::from_utf8_lossy(&bytes));
                        }
                        CommandEvent::Terminated(payload) => {
                            eprintln!("[getmastersd] terminated: {payload:?}");
                            break;
                        }
                        _ => {}
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Masters desktop");
}
