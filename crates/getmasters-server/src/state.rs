//! Shared application state and the handler error type.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::extensions::{ExtensionManager, ExternalConnector};
use getmasters_core::knowledge::{build_index, Embedder};
use getmasters_core::permission::{ApprovalRegistry, GrantSet};
use getmasters_core::secrets::{MemoryStore, SecretStore};
use getmasters_core::CoreError;
use getmasters_proto::ErrorDto;

/// Every built-in MCP server Masters plans to host (mirrors `getmasters_mcp::BUILTIN_SERVERS`); the
/// extensions UI lists all of them, greying out the not-yet-implemented placeholders (FR-19).
pub const ALL_BUILTIN_SERVERS: &[&str] = &[
    "files",
    "knowledge",
    "memory",
    "skills",
    "study",
    "masters",
    "web",
];

/// Built-in MCP servers actually implemented today. These are the toggleable extensions (FR-19);
/// the rest are placeholders.
pub const IMPLEMENTED_SERVERS: &[&str] = &["files", "knowledge", "memory", "skills", "study"];

/// `settings` key holding the system default project id (backs quick chat; see
/// [`AppState::ensure_default_project`]).
pub const DEFAULT_PROJECT_KEY: &str = "default_project_id";

/// `settings` key holding the user-starred default (global) master slug — the master quick chat
/// uses when none is explicitly picked.
pub const DEFAULT_MASTER_KEY: &str = "default_master_slug";

/// State shared across all handlers (cheaply cloneable).
#[derive(Clone)]
pub struct AppState {
    /// Base no-tools agent — used for project-less sessions and as the template for project agents.
    pub agent: AgentService,
    /// Per-launch bearer token the desktop must present on every non-public request.
    pub token: String,
    /// Daemon crate version (surfaced via `/health`).
    pub version: &'static str,
    /// Resolves approval decisions for in-flight runs (present when the agent wires approvals).
    pub approvals: Option<Arc<ApprovalRegistry>>,
    /// Where API keys are stored (keychain on a desktop; memory in tests/headless).
    pub secrets: Arc<dyn SecretStore>,
    /// Effective config, for resolving the per-project embedder.
    pub cfg: Config,
    /// SMTP transport for outbound email delivery (Phase 3e, FR-27). The live daemon uses the real
    /// `lettre` transport; tests inject a capturing fake.
    pub email: Arc<dyn crate::delivery::EmailTransport>,
    /// Lazily-built, per-project agents (files + knowledge enabled), keyed by project id.
    session_agents: Arc<Mutex<HashMap<String, AgentService>>>,
}

impl AppState {
    pub fn new(agent: AgentService, token: String) -> Self {
        let approvals = agent.approval_registry();
        Self {
            agent,
            token,
            version: env!("CARGO_PKG_VERSION"),
            approvals,
            secrets: Arc::new(MemoryStore::new()),
            cfg: Config::default(),
            email: crate::delivery::default_transport(),
            session_agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Use a specific secret store (the daemon supplies the OS keychain).
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Use a specific email transport (tests inject a capturing fake).
    pub fn with_email_transport(mut self, email: Arc<dyn crate::delivery::EmailTransport>) -> Self {
        self.email = email;
        self
    }

    /// Provide the effective config (used to resolve the per-project embedder).
    pub fn with_config(mut self, cfg: Config) -> Self {
        self.cfg = cfg;
        self
    }

    /// Build (and cache) a project's agent: the base agent + a Files+Knowledge extension
    /// manager scoped to the project's grants and documents (ADR-0011 context container).
    pub async fn project_agent(&self, project_id: &str) -> Result<AgentService, String> {
        if let Some(a) = self.session_agents.lock().unwrap().get(project_id) {
            return Ok(a.clone());
        }
        let store = self.agent.store().clone();
        let grants = Arc::new(GrantSet::new(
            store
                .list_folder_grants(Some(project_id))
                .map_err(|e| e.to_string())?,
        ));
        let embedder = Arc::new(Embedder::resolve(&self.cfg, &store));
        let index = build_index(store.clone(), embedder.dim());
        let project_dir = self.project_dir(project_id);
        // Host every implemented built-in the project hasn't explicitly disabled (FR-19;
        // absent = enabled).
        let disabled = store
            .disabled_extensions(project_id)
            .map_err(|e| e.to_string())?;
        let enabled: std::collections::HashSet<String> = IMPLEMENTED_SERVERS
            .iter()
            .filter(|s| !disabled.contains(**s))
            .map(|s| s.to_string())
            .collect();
        // External MCP connectors the project has enabled (Phase 4d) — spawned over stdio alongside
        // the built-ins; their tools route through the same gate.
        let connectors: Vec<ExternalConnector> = store
            .list_connectors(project_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|c| c.enabled)
            .map(|c| ExternalConnector {
                name: c.name,
                command: c.command,
                args: c.args,
                env: c.env,
            })
            .collect();
        let mgr = ExtensionManager::with_project(
            project_id,
            grants.clone(),
            store.clone(),
            embedder,
            index,
            project_dir.clone(),
            &enabled,
            &connectors,
        )
        .await?;
        let agent = self
            .agent
            .clone()
            .with_extensions(Arc::new(mgr), grants)
            .with_project_dir(project_dir);
        self.session_agents
            .lock()
            .unwrap()
            .insert(project_id.to_string(), agent.clone());
        Ok(agent)
    }

    /// The data-home base directory (the DB's parent), under which `projects/` and the standalone
    /// `masters/` dir live. Falls back to `.` when the DB has no parent (in-memory test stores).
    pub fn data_base(&self) -> std::path::PathBuf {
        self.cfg
            .db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    /// The per-project data directory (file-backed Memory + Skills live here), derived from the
    /// DB location: `{db_parent}/projects/{project_id}/`.
    pub fn project_dir(&self, project_id: &str) -> std::path::PathBuf {
        self.data_base().join("projects").join(project_id)
    }

    /// The standalone (project-less) master store, rooted at `{db_parent}/masters/`. Masters here
    /// exist independent of any project — managed from the Masters sidebar (extends ADR-0011).
    pub fn global_master_store(&self) -> getmasters_core::masters::MasterStore {
        getmasters_core::masters::MasterStore::global(self.data_base(), self.agent.store().clone())
    }

    /// The system **default project** that backs quick chat — masters need an agent/grant/tool
    /// context to run, which lives on a project. Returns the persisted default project id, lazily
    /// creating a "Default" project the first time it's needed and saving its id in `settings`.
    pub fn ensure_default_project(&self) -> Result<String, String> {
        let store = self.agent.store();
        if let Ok(Some(id)) = store.get_setting(DEFAULT_PROJECT_KEY) {
            // Honor the saved default only if it still exists (it may have been deleted).
            if store.get_project(&id).is_ok() {
                return Ok(id);
            }
        }
        let id = store
            .create_project("Default", None)
            .map_err(|e| e.to_string())?;
        store
            .set_setting(DEFAULT_PROJECT_KEY, &id)
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Drop a project's cached agent (after its grants/instructions change).
    pub fn invalidate_project(&self, project_id: &str) {
        self.session_agents.lock().unwrap().remove(project_id);
    }

    /// The agent to run a session with: a project agent (tools+knowledge) when the session is
    /// under a project, otherwise the base no-tools agent.
    pub async fn agent_for_session(&self, session_id: &str) -> AgentService {
        let pid = self
            .agent
            .store()
            .get_session(session_id)
            .ok()
            .and_then(|s| s.project_id);
        match pid {
            Some(pid) => self.project_agent(&pid).await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, project = %pid, "failed to build project agent; using base agent");
                self.agent.clone()
            }),
            None => self.agent.clone(),
        }
    }
}

/// Uniform handler error → `(status, ErrorDto)` response.
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorDto::new(self.message))).into_response()
    }
}

impl From<CoreError> for AppError {
    fn from(e: CoreError) -> Self {
        let status = match e {
            CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        AppError::new(status, e.to_string())
    }
}
