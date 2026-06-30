//! Emit the daemon's OpenAPI description to a file (or stdout).
//!
//! Usage: `cargo run -p getmasters-server --bin gen_openapi -- [out_path]`
//! Default out_path: `ui/desktop/src/api/openapi.json`. This is the source the desktop's
//! TypeScript client is generated from (`openapi-typescript`).

use std::path::PathBuf;

use getmasters_server::ApiDoc;
use utoipa::OpenApi;

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ui/desktop/src/api/openapi.json"));

    let json = ApiDoc::openapi().to_pretty_json()?;

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, json)?;
    eprintln!("wrote OpenAPI spec to {}", out.display());
    Ok(())
}
