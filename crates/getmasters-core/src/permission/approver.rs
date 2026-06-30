//! Approval channel: how a "prompt" decision is resolved.
//!
//! - [`AutoApprover`] always allows — used by the CLI and tests so the gated loop runs headless.
//! - [`ChannelApprover`] emits the request to the UI (via a caller-supplied closure) and awaits
//!   the user's decision through an [`ApprovalRegistry`]. The registry tolerates a decision
//!   arriving **before** the wait is registered (e.g. a fast client), so there is no race.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use getmasters_proto::SideEffect;
use tokio::sync::oneshot;

/// A pending request for the user to approve a side-effecting tool call.
#[derive(Clone, Debug)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool: String,
    pub summary: String,
    pub classes: Vec<SideEffect>,
}

/// The user's answer to an [`ApprovalRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Allow this one call.
    Allow,
    /// Allow and remember the folder.
    AllowFolder,
    /// Allow and always allow this tool.
    AlwaysTool,
    /// Reject this call.
    Deny,
}

impl ApprovalDecision {
    /// Parse the wire form (`ClientCommand::ApprovalDecision.decision`).
    pub fn from_wire(s: &str) -> Self {
        match s {
            "allow" => ApprovalDecision::Allow,
            "allow_folder" => ApprovalDecision::AllowFolder,
            "always_tool" => ApprovalDecision::AlwaysTool,
            _ => ApprovalDecision::Deny,
        }
    }
}

/// Resolves a "prompt" decision into an [`ApprovalDecision`].
#[async_trait]
pub trait Approver: Send + Sync {
    async fn decide(&self, req: ApprovalRequest) -> ApprovalDecision;
}

/// Always allows — for the CLI and headless tests.
#[derive(Clone, Default)]
pub struct AutoApprover;

#[async_trait]
impl Approver for AutoApprover {
    async fn decide(&self, _req: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Allow
    }
}

/// Shared rendezvous between the agent run (which waits) and the WS handler (which resolves).
/// Handles either arrival order via an `early` map.
#[derive(Default)]
pub struct ApprovalRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    waiters: HashMap<String, oneshot::Sender<ApprovalDecision>>,
    early: HashMap<String, ApprovalDecision>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a pending request (called by the WS handler on `ApprovalDecision`).
    pub fn resolve(&self, request_id: &str, decision: ApprovalDecision) {
        let mut g = self.inner.lock().expect("approval registry poisoned");
        if let Some(tx) = g.waiters.remove(request_id) {
            let _ = tx.send(decision);
        } else {
            g.early.insert(request_id.to_string(), decision);
        }
    }

    /// Wait for a decision (called by the agent run). A dropped sender resolves to `Deny`,
    /// so a disconnected client or a Stop never wedges the run.
    pub async fn wait(&self, request_id: &str) -> ApprovalDecision {
        let rx = {
            let mut g = self.inner.lock().expect("approval registry poisoned");
            if let Some(d) = g.early.remove(request_id) {
                return d;
            }
            let (tx, rx) = oneshot::channel();
            g.waiters.insert(request_id.to_string(), tx);
            rx
        };
        rx.await.unwrap_or(ApprovalDecision::Deny)
    }
}

/// Emits an approval request to the UI and awaits the decision via an [`ApprovalRegistry`].
#[derive(Clone)]
pub struct ChannelApprover {
    registry: Arc<ApprovalRegistry>,
    emit: Arc<dyn Fn(ApprovalRequest) + Send + Sync>,
}

impl ChannelApprover {
    pub fn new(
        registry: Arc<ApprovalRegistry>,
        emit: Arc<dyn Fn(ApprovalRequest) + Send + Sync>,
    ) -> Self {
        Self { registry, emit }
    }
}

#[async_trait]
impl Approver for ChannelApprover {
    async fn decide(&self, req: ApprovalRequest) -> ApprovalDecision {
        let request_id = req.request_id.clone();
        (self.emit)(req); // surface to the UI (the agent's event stream)
        self.registry.wait(&request_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auto_approver_allows() {
        let a = AutoApprover;
        let req = ApprovalRequest {
            request_id: "r1".into(),
            tool: "files.create".into(),
            summary: "create a.txt".into(),
            classes: vec![SideEffect::Write],
        };
        assert_eq!(a.decide(req).await, ApprovalDecision::Allow);
    }

    #[tokio::test]
    async fn channel_approver_resolves_via_registry() {
        let registry = Arc::new(ApprovalRegistry::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let e2 = emitted.clone();
        let approver = ChannelApprover::new(
            registry.clone(),
            Arc::new(move |req: ApprovalRequest| e2.lock().unwrap().push(req.request_id)),
        );
        let req = ApprovalRequest {
            request_id: "r9".into(),
            tool: "files.create".into(),
            summary: "x".into(),
            classes: vec![SideEffect::Write],
        };
        // The decision is already available (e.g. a fast client); decide() emits then returns it.
        registry.resolve("r9", ApprovalDecision::AlwaysTool);
        assert_eq!(approver.decide(req).await, ApprovalDecision::AlwaysTool);
        assert_eq!(emitted.lock().unwrap().as_slice(), &["r9".to_string()]);
    }

    #[tokio::test]
    async fn early_decision_is_not_lost() {
        let registry = ApprovalRegistry::new();
        registry.resolve("r1", ApprovalDecision::Allow); // arrives first
        assert_eq!(registry.wait("r1").await, ApprovalDecision::Allow);
    }
}
