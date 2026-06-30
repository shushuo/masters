//! Permission & Audit — the single gate every side-effecting tool call passes through
//! before execution (docs/06; the cross-cutting invariant in CLAUDE.md).
//!
//! `PermissionGate::authorize` classifies a call, enforces folder grants, applies the default
//! policy matrix (read-in-grant auto-allows; write/destructive/network/send prompt unless a
//! standing permission covers them), resolves prompts through an [`Approver`], records the
//! outcome in the audit log, and returns a verdict. It lives in **Core**, never in an MCP
//! server, so no tool can bypass it.

pub mod approver;
pub mod audit;
pub mod grant;
pub mod policy;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use getmasters_proto::SideEffect;
use serde_json::Value;
use uuid::Uuid;

pub use approver::{
    ApprovalDecision, ApprovalRegistry, ApprovalRequest, Approver, AutoApprover, ChannelApprover,
};
pub use grant::GrantSet;

use crate::store::{AuditEntry, Store};
use policy::{classify, paths_for, StandingPerms};

/// The verdict of a permission check.
#[derive(Debug)]
pub enum Authorized {
    Allowed,
    Denied(String),
}

/// Gates and audits tool calls for one agent run.
pub struct PermissionGate {
    grants: Arc<GrantSet>,
    approver: Arc<dyn Approver>,
    standing: Mutex<StandingPerms>,
    store: Store,
    session_id: Option<String>,
    /// When `Some`, only these tools may run (Blank Slate / least-privilege).
    enabled_tools: Option<Arc<HashSet<String>>>,
    /// When true, standing permissions are not consulted or persisted (every side-effect
    /// re-prompts) — the Blank Slate posture (docs/06, ADR-0008).
    no_standing: bool,
}

impl PermissionGate {
    pub fn new(
        grants: Arc<GrantSet>,
        approver: Arc<dyn Approver>,
        store: Store,
        session_id: Option<String>,
    ) -> Self {
        Self {
            grants,
            approver,
            standing: Mutex::new(StandingPerms::default()),
            store,
            session_id,
            enabled_tools: None,
            no_standing: false,
        }
    }

    /// Apply a least-privilege posture: restrict to `enabled_tools` (if `Some`) and, when
    /// `no_standing`, re-prompt every side-effecting call (no standing permissions).
    pub fn least_privilege(
        mut self,
        enabled_tools: Option<Arc<HashSet<String>>>,
        no_standing: bool,
    ) -> Self {
        self.enabled_tools = enabled_tools;
        self.no_standing = no_standing;
        self
    }

    /// Authorize a tool call, recording the outcome in the audit log.
    pub async fn authorize(&self, tool: &str, args: &Value) -> Authorized {
        // Blank Slate: a tool that has not been enabled cannot run at all.
        if let Some(enabled) = &self.enabled_tools {
            if !enabled.contains(tool) {
                return self.deny(
                    tool,
                    args,
                    "tool not enabled (Blank Slate / least-privilege)",
                );
            }
        }

        let class = classify(tool);

        // File-accessing tools (Files + knowledge.ingest) must resolve every touched path
        // within a grant of sufficient access.
        let mut resolved: Vec<PathBuf> = Vec::new();
        if tool.starts_with("files.") || tool == "knowledge.ingest" {
            let needed = paths_for(tool, args);
            if needed.is_empty() {
                return self.deny(tool, args, "missing path argument");
            }
            for (path, need_write) in &needed {
                match self.grants.resolve(path, *need_write) {
                    Ok(p) => resolved.push(p),
                    Err(reason) => return self.deny(tool, args, &reason),
                }
            }
        }

        // Reads inside a grant auto-allow; other classes prompt unless standing-approved.
        if class == SideEffect::Read {
            return self.allow(tool, args, "auto");
        }

        if !self.no_standing && self.standing.lock().unwrap().allows(tool, &resolved) {
            return self.allow(tool, args, "approved");
        }

        let request = ApprovalRequest {
            request_id: Uuid::new_v4().to_string(),
            tool: tool.to_string(),
            summary: summary(tool, args),
            classes: vec![class],
        };
        match self.approver.decide(request).await {
            ApprovalDecision::Deny => self.deny(tool, args, "denied by user"),
            ApprovalDecision::Allow => self.allow(tool, args, "approved"),
            // In Blank Slate mode, "always"/"folder" grants are not persisted (allow once only).
            ApprovalDecision::AllowFolder => {
                if !self.no_standing {
                    self.standing.lock().unwrap().grant_folders(&resolved);
                }
                self.allow(tool, args, "approved")
            }
            ApprovalDecision::AlwaysTool => {
                if !self.no_standing {
                    self.standing.lock().unwrap().grant_tool(tool);
                }
                self.allow(tool, args, "approved")
            }
        }
    }

    fn allow(&self, tool: &str, args: &Value, decision: &str) -> Authorized {
        self.record(tool, args, decision, None);
        Authorized::Allowed
    }

    fn deny(&self, tool: &str, args: &Value, reason: &str) -> Authorized {
        self.record(tool, args, "denied", Some(reason));
        Authorized::Denied(reason.to_string())
    }

    fn record(&self, tool: &str, args: &Value, decision: &str, result_summary: Option<&str>) {
        let entry = AuditEntry {
            session_id: self.session_id.clone(),
            tool: tool.to_string(),
            args: Some(audit::redact_args(args)),
            decision: decision.to_string(),
            result_summary: result_summary.map(str::to_string),
        };
        if let Err(e) = self.store.insert_audit(&entry) {
            tracing::warn!(error = %e, "failed to write audit log");
        }
    }
}

/// A short human summary of a tool call for approval prompts / events.
fn summary(tool: &str, args: &Value) -> String {
    let path = args
        .get("path")
        .or_else(|| args.get("to"))
        .and_then(Value::as_str);
    match path {
        Some(p) => format!("{tool} {p}"),
        None => tool.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use getmasters_proto::{FolderAccess, FolderGrant};
    use serde_json::json;

    fn grant_dir() -> (PathBuf, GrantSet) {
        let dir = std::env::temp_dir().join(format!("getmasters-gate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let gs = GrantSet::new(vec![FolderGrant {
            id: "g".into(),
            project_id: None,
            path: dir.to_string_lossy().into_owned(),
            access: FolderAccess::ReadWrite,
            created_at: 0,
        }]);
        (dir, gs)
    }

    #[tokio::test]
    async fn write_in_grant_is_allowed_with_auto_approver() {
        let (dir, gs) = grant_dir();
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let gate = PermissionGate::new(
            Arc::new(gs),
            Arc::new(AutoApprover),
            store.clone(),
            Some(session.id.clone()),
        );
        let path = dir.join("a.txt");
        let v = gate
            .authorize("files.create", &json!({ "path": path.to_str().unwrap() }))
            .await;
        assert!(matches!(v, Authorized::Allowed));
        let audit = store.list_audit(&session.id).unwrap();
        assert_eq!(audit[0].0, "files.create");
        assert_eq!(audit[0].1, "approved");
    }

    #[tokio::test]
    async fn write_outside_grant_is_denied_and_audited() {
        let (_dir, gs) = grant_dir();
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let gate = PermissionGate::new(
            Arc::new(gs),
            Arc::new(AutoApprover),
            store.clone(),
            Some(session.id.clone()),
        );
        let v = gate
            .authorize("files.create", &json!({ "path": "/etc/passwd" }))
            .await;
        assert!(matches!(v, Authorized::Denied(_)));
        assert_eq!(store.list_audit(&session.id).unwrap()[0].1, "denied");
    }

    #[tokio::test]
    async fn read_in_grant_auto_allows() {
        let (dir, gs) = grant_dir();
        let f = dir.join("r.txt");
        std::fs::write(&f, "hi").unwrap();
        let store = Store::open_in_memory().unwrap();
        let gate = PermissionGate::new(Arc::new(gs), Arc::new(AutoApprover), store, None);
        let v = gate
            .authorize("files.read", &json!({ "path": f.to_str().unwrap() }))
            .await;
        assert!(matches!(v, Authorized::Allowed));
    }
}
