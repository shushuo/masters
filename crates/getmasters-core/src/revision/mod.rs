//! File revisions — capture the pre-image of a write/destructive file op so it can be
//! **reverted** (docs/06: surprising changes must be reversible). This is the backend for
//! diff-preview/revert; the desktop renders the diff and offers undo.
//!
//! The pre-image is captured in Core (which holds the grants), *before* the tool runs, and
//! committed only if the tool succeeds. Revert walks the log newest-first.

use uuid::Uuid;

use crate::permission::GrantSet;
use crate::store::{RevisionRow, Store};

/// A pre-image captured before a tool runs; committed on success.
pub struct PendingRevision {
    tool: String,
    op: String,
    path: String,
    prior_content: Option<String>,
    existed: bool,
    move_from: Option<String>,
}

/// Capture the pre-image for a write/destructive file tool, resolving paths through `grants`.
/// Returns `None` for read tools / non-file tools (nothing to revert).
pub fn capture(
    grants: &GrantSet,
    tool: &str,
    input: &serde_json::Value,
) -> Option<PendingRevision> {
    let seg = tool.rsplit('.').next().unwrap_or(tool);
    let get = |k: &str| input.get(k).and_then(|v| v.as_str());

    match seg {
        "create" | "edit" | "delete" => {
            let raw = get("path")?;
            let resolved = grants.resolve(raw, true).ok()?;
            let existed = resolved.exists();
            let prior_content = if existed {
                std::fs::read_to_string(&resolved).ok()
            } else {
                None
            };
            Some(PendingRevision {
                tool: tool.to_string(),
                op: seg.to_string(),
                path: resolved.to_string_lossy().into_owned(),
                prior_content,
                existed,
                move_from: None,
            })
        }
        "move" | "rename" => {
            let from = grants.resolve(get("from")?, false).ok()?;
            let to = grants.resolve(get("to")?, true).ok()?;
            Some(PendingRevision {
                tool: tool.to_string(),
                op: "move".to_string(),
                path: to.to_string_lossy().into_owned(),
                prior_content: None,
                existed: to.exists(),
                move_from: Some(from.to_string_lossy().into_owned()),
            })
        }
        _ => None,
    }
}

/// Persist a captured revision (call only after the tool succeeded).
pub fn commit(store: &Store, session_id: Option<&str>, pending: PendingRevision) {
    let row = RevisionRow {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.map(str::to_string),
        tool: pending.tool,
        op: pending.op,
        path: pending.path,
        prior_content: pending.prior_content,
        existed: pending.existed,
        move_from: pending.move_from,
    };
    if let Err(e) = store.insert_revision(&row) {
        tracing::warn!(error = %e, "failed to record file revision");
    }
}

/// Revert the most recent file op for a session. Returns a human summary or an error.
pub fn revert_last(store: &Store, session_id: &str) -> Result<String, String> {
    let row = store
        .last_revision(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "nothing to revert".to_string())?;

    let result = match row.op.as_str() {
        "create" => std::fs::remove_file(&row.path)
            .map(|_| format!("reverted create: deleted {}", row.path))
            .map_err(|e| format!("revert failed: {e}")),
        "edit" | "delete" => {
            let content = row.prior_content.clone().unwrap_or_default();
            std::fs::write(&row.path, content)
                .map(|_| format!("reverted {}: restored {}", row.op, row.path))
                .map_err(|e| format!("revert failed: {e}"))
        }
        "move" => {
            let from = row.move_from.clone().ok_or("missing original path")?;
            std::fs::rename(&row.path, &from)
                .map(|_| format!("reverted move: {} -> {}", row.path, from))
                .map_err(|e| format!("revert failed: {e}"))
        }
        other => Err(format!("cannot revert op '{other}'")),
    }?;

    // Consume the revision so a repeated revert walks further back.
    let _ = store.delete_revision(&row.id);
    Ok(result)
}

/// Minimal line-diff counts `(added, removed)` between an optional pre-image and the next
/// content — the raw signal behind [`diff_summary`] and the approval-prompt [`FilePreview`].
pub fn diff_counts(prior: Option<&str>, next: &str) -> (u32, u32) {
    let prior_lines: Vec<&str> = prior.map(|p| p.lines().collect()).unwrap_or_default();
    let next_lines: Vec<&str> = next.lines().collect();
    let removed = prior_lines
        .iter()
        .filter(|l| !next_lines.contains(l))
        .count() as u32;
    let added = next_lines
        .iter()
        .filter(|l| !prior_lines.contains(l))
        .count() as u32;
    (added, removed)
}

/// A minimal line-diff summary (added/removed counts) for surfacing an edit preview.
pub fn diff_summary(prior: Option<&str>, next: &str) -> String {
    let (added, removed) = diff_counts(prior, next);
    format!("+{added} -{removed} lines")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts() {
        assert_eq!(diff_summary(Some("a\nb"), "a\nc"), "+1 -1 lines");
        assert_eq!(diff_summary(None, "a\nb"), "+2 -0 lines");
    }
}
