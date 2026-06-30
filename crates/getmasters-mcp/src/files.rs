//! The built-in **Files** MCP server (docs/04 §2.1), implemented with rmcp (ADR-0005).
//!
//! Tools act strictly within the session's folder grants. Path containment here is a
//! **backstop** (defense in depth): the authoritative permission decision is made by the
//! Core gate before the Extension Manager ever calls a tool. Tools never panic — an
//! out-of-grant or IO failure is returned as a tool error result.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use getmasters_proto::{FolderGrant, SideEffect};

/// Side-effect class for each Files tool — the authoritative map Core's classifier mirrors.
pub fn tool_classes() -> &'static [(&'static str, SideEffect)] {
    &[
        ("read", SideEffect::Read),
        ("list", SideEffect::Read),
        ("search", SideEffect::Read),
        ("create", SideEffect::Write),
        ("edit", SideEffect::Write),
        ("move", SideEffect::Write),
        ("rename", SideEffect::Write),
        ("delete", SideEffect::Destructive),
    ]
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReadParams {
    /// Path to a UTF-8 text file inside a granted folder.
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    /// Directory to list, inside a granted folder.
    pub path: String,
    /// Recurse into subdirectories.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Directory to search under, inside a granted folder.
    pub path: String,
    /// Substring to find in file contents.
    pub query: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateParams {
    /// Path of the file to create, inside a granted folder.
    pub path: String,
    /// File contents.
    pub content: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EditParams {
    /// Path of the file to edit, inside a granted folder.
    pub path: String,
    /// Exact text to replace.
    pub find: String,
    /// Replacement text.
    pub replace: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct MoveParams {
    /// Source path (inside a granted folder).
    pub from: String,
    /// Destination path (inside a granted folder, writable).
    pub to: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteParams {
    /// Path of the file to delete, inside a granted folder.
    pub path: String,
}

/// The Files MCP server, scoped to a set of folder grants.
#[derive(Clone)]
pub struct FilesServer {
    grants: Arc<Vec<FolderGrant>>,
    // Populated by `#[tool_router]`; consumed by the generated `#[tool_handler]` dispatch.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl FilesServer {
    pub fn new(grants: Vec<FolderGrant>) -> Self {
        Self {
            grants: Arc::new(grants),
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve a path within a grant (backstop). Mirrors `getmasters-core`'s `GrantSet`.
    fn resolve(&self, path: &str, need_write: bool) -> Result<PathBuf, String> {
        if self.grants.is_empty() {
            return Err("no folder grants configured".into());
        }
        let target = canonical_target(Path::new(path))
            .map_err(|e| format!("cannot resolve '{path}': {e}"))?;
        for g in self.grants.iter() {
            let Ok(root) = Path::new(&g.path).canonicalize() else {
                continue;
            };
            if target.starts_with(&root) {
                if need_write && !g.access.allows_write() {
                    return Err(format!("'{path}' is inside a read-only grant"));
                }
                return Ok(target);
            }
        }
        Err(format!("'{path}' is outside any granted folder"))
    }
}

/// Build an error tool-result (so the model sees the failure instead of the run aborting).
fn tool_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.into())])
}

fn tool_ok(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(msg.into())])
}

#[tool_router]
impl FilesServer {
    #[tool(description = "Read a UTF-8 text file within a granted folder")]
    async fn read(
        &self,
        Parameters(p): Parameters<ReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.resolve(&p.path, false) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        Ok(match std::fs::read_to_string(&path) {
            Ok(text) => tool_ok(text),
            Err(e) => tool_error(format!("read failed: {e}")),
        })
    }

    #[tool(description = "List files within a granted folder")]
    async fn list(
        &self,
        Parameters(p): Parameters<ListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = match self.resolve(&p.path, false) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        let max_depth = if p.recursive { usize::MAX } else { 1 };
        let mut entries = Vec::new();
        for e in walkdir::WalkDir::new(&root)
            .max_depth(max_depth)
            .into_iter()
            .flatten()
        {
            if e.file_type().is_file() {
                entries.push(e.path().display().to_string());
            }
        }
        Ok(tool_ok(serde_json::to_string(&entries).unwrap_or_default()))
    }

    #[tool(description = "Search file contents for a substring within a granted folder")]
    async fn search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = match self.resolve(&p.path, false) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        let mut hits = Vec::new();
        for e in walkdir::WalkDir::new(&root).into_iter().flatten() {
            if e.file_type().is_file() {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    if text.contains(&p.query) {
                        hits.push(e.path().display().to_string());
                    }
                }
            }
        }
        Ok(tool_ok(serde_json::to_string(&hits).unwrap_or_default()))
    }

    #[tool(description = "Create a new file with the given contents within a granted folder")]
    async fn create(
        &self,
        Parameters(p): Parameters<CreateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.resolve(&p.path, true) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        Ok(match std::fs::write(&path, &p.content) {
            Ok(()) => tool_ok(format!("created {}", path.display())),
            Err(e) => tool_error(format!("create failed: {e}")),
        })
    }

    #[tool(description = "Replace the first occurrence of `find` with `replace` in a file")]
    async fn edit(
        &self,
        Parameters(p): Parameters<EditParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.resolve(&p.path, true) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return Ok(tool_error(format!("edit failed (read): {e}"))),
        };
        if !text.contains(&p.find) {
            return Ok(tool_error("edit failed: `find` text not present"));
        }
        let updated = text.replacen(&p.find, &p.replace, 1);
        Ok(match std::fs::write(&path, updated) {
            Ok(()) => tool_ok(format!("edited {}", path.display())),
            Err(e) => tool_error(format!("edit failed (write): {e}")),
        })
    }

    #[tool(description = "Move a file from one path to another within granted folders")]
    async fn r#move(
        &self,
        Parameters(p): Parameters<MoveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_move(&p.from, &p.to)
    }

    #[tool(description = "Rename a file (alias of move) within granted folders")]
    async fn rename(
        &self,
        Parameters(p): Parameters<MoveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_move(&p.from, &p.to)
    }

    #[tool(description = "Delete a file within a granted folder")]
    async fn delete(
        &self,
        Parameters(p): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.resolve(&p.path, true) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        // Phase 1a removes the file; soft-delete to trash is a 1b refinement.
        Ok(match std::fs::remove_file(&path) {
            Ok(()) => tool_ok(format!("deleted {}", path.display())),
            Err(e) => tool_error(format!("delete failed: {e}")),
        })
    }
}

impl FilesServer {
    fn do_move(&self, from: &str, to: &str) -> Result<CallToolResult, ErrorData> {
        let src = match self.resolve(from, false) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        let dst = match self.resolve(to, true) {
            Ok(p) => p,
            Err(e) => return Ok(tool_error(e)),
        };
        Ok(match std::fs::rename(&src, &dst) {
            Ok(()) => tool_ok(format!("moved {} -> {}", src.display(), dst.display())),
            Err(e) => tool_error(format!("move failed: {e}")),
        })
    }
}

#[tool_handler]
impl ServerHandler for FilesServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters built-in Files server: act on the user's files within granted folders.".into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

/// Canonicalize a path that may not exist yet (file about to be created).
fn canonical_target(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    Ok(parent
        .canonicalize()?
        .join(path.file_name().unwrap_or_default()))
}
