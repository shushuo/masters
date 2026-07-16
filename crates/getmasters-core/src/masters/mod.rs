//! **Masters** — persona-over-Skill role descriptors (ADR-0010/0013, FR-39/46; docs/09 §2).
//!
//! A Master is a *thin role descriptor* layered on the Skills system: editable Markdown
//! `masters/<slug>.md` (YAML-ish frontmatter + a Markdown body) that sets a **persona** (voice +
//! stance), a provider-qualified **`default_model`** (ADR-0013), and a least-privilege
//! **`allowed_tools`** subset (NFR-9), plus `summary`/`allowed_skills`/`output_contract`/`origin`.
//! Like Skills, the file is the source of truth and the SQLite `masters` table indexes it for
//! listing; the same hand-rolled frontmatter reader is reused, so the lean core needs no YAML dep.
//!
//! This slice (Phase 4a) covers the single-master primitive: define/edit masters + run a brief
//! through one master on its own persona + model + tools. Multi-master teams, the router, group
//! chat, and agent-authored "learned" masters are deferred (ADR-0010/0012).

pub mod mentions;
pub mod router;

use std::path::PathBuf;

use crate::error::Result;
use crate::skills::slugify;
use crate::store::{MasterRow, Store};

/// Subdirectory (under the project data dir) that holds master files.
pub const MASTERS_DIR: &str = "masters";

/// The internal persona-over-model backend (the default for every master).
pub const BACKEND_INTERNAL: &str = "internal";
/// The external-ACP backend: this master is a pre-installed ACP-compatible coding CLI that Masters
/// drives as a subprocess (Phase 4i, ADR-0014).
pub const BACKEND_ACP: &str = "acp";

/// Launch config for an external ACP coding agent (Phase 4i, ADR-0014). Mirrors a connector's shape:
/// the executable to spawn + args + the *only* environment it receives (credential stripping, ADR-0008).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AcpLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// A master's parsed contents (frontmatter + body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Master {
    pub name: String,
    pub summary: String,
    /// System-prompt fragment: voice, masterise, stance.
    pub persona: String,
    /// Provider-qualified model id (`anthropic:claude-…`, `openai:gpt-…`, `ollama:…`); persona-fixed.
    pub default_model: String,
    /// Skills this master may use (stored + surfaced; enforcement deferred).
    pub allowed_skills: Vec<String>,
    /// Least-privilege tool allow-list (namespaced, e.g. `files.read`); empty = all project tools.
    pub allowed_tools: Vec<String>,
    /// Expected deliverable shape (e.g. "a decision note: options, trade-offs, recommendation").
    pub output_contract: String,
    /// `builtin` | `learned` | `imported` (trust provenance).
    pub origin: String,
    /// Extended persona instructions (Markdown body after the frontmatter fence).
    pub body: String,
    /// Execution backend (Phase 4i, ADR-0014): [`BACKEND_INTERNAL`] (default) or [`BACKEND_ACP`].
    pub backend: String,
    /// ACP launch config — `Some` only when `backend == "acp"`. The external agent owns its own
    /// prompt + model + tool loop, so `persona`/`default_model` go unused for the ACP backend.
    pub acp: Option<AcpLaunch>,
}

impl Master {
    /// The full persona text injected for a run: the one-line persona plus any extended body.
    pub fn persona_block(&self) -> String {
        match (self.persona.trim(), self.body.trim()) {
            ("", body) => body.to_string(),
            (persona, "") => persona.to_string(),
            (persona, body) => format!("{persona}\n\n{body}"),
        }
    }

    /// Whether this master is an external ACP coding agent (vs the internal persona-over-model loop).
    pub fn is_acp(&self) -> bool {
        self.backend == BACKEND_ACP
    }
}

fn join_list(items: &[String]) -> String {
    items.join(", ")
}

fn split_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Render `KEY=VALUE` env pairs joined by `; ` (avoids the comma that `split_list` splits on, and
/// keeps the value intact since we only split on the first `=`).
fn join_env(env: &[(String, String)]) -> String {
    env.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Parse a `KEY=VALUE; KEY=VALUE` env line back into pairs (split on the first `=` only).
fn split_env(value: &str) -> Vec<(String, String)> {
    value
        .split(';')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let (k, v) = item.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Render a master into its `masters/<slug>.md` representation (frontmatter + body). The `backend`
/// line is always emitted; the `acp_*` lines only when this is an ACP master (Phase 4i).
pub fn render(master: &Master) -> String {
    let backend = if master.backend.is_empty() {
        BACKEND_INTERNAL
    } else {
        &master.backend
    };
    let mut front = format!(
        "name: {}\nsummary: {}\npersona: {}\ndefault_model: {}\nallowed_skills: {}\nallowed_tools: {}\noutput_contract: {}\norigin: {}\nbackend: {}",
        master.name,
        master.summary,
        master.persona,
        master.default_model,
        join_list(&master.allowed_skills),
        join_list(&master.allowed_tools),
        master.output_contract,
        master.origin,
        backend,
    );
    if let Some(acp) = &master.acp {
        front.push_str(&format!(
            "\nacp_command: {}\nacp_args: {}\nacp_env: {}",
            acp.command,
            join_list(&acp.args),
            join_env(&acp.env),
        ));
    }
    format!("---\n{}\n---\n\n{}\n", front, master.body.trim())
}

/// Parse an `masters/<slug>.md` document. Missing frontmatter keys default to empty; everything
/// after the closing `---` fence is the body. Falls back to treating the whole text as the body.
pub fn parse(content: &str) -> Master {
    let mut e = Master {
        name: String::new(),
        summary: String::new(),
        persona: String::new(),
        default_model: String::new(),
        allowed_skills: Vec::new(),
        allowed_tools: Vec::new(),
        output_contract: String::new(),
        origin: String::new(),
        body: String::new(),
        backend: BACKEND_INTERNAL.to_string(),
        acp: None,
    };
    // ACP launch fields are collected separately and folded into `e.acp` only for the ACP backend.
    let mut acp_command = String::new();
    let mut acp_args: Vec<String> = Vec::new();
    let mut acp_env: Vec<(String, String)> = Vec::new();

    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            for line in front.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("name:") {
                    e.name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("summary:") {
                    e.summary = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("persona:") {
                    e.persona = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("default_model:") {
                    e.default_model = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("allowed_skills:") {
                    e.allowed_skills = split_list(v);
                } else if let Some(v) = line.strip_prefix("allowed_tools:") {
                    e.allowed_tools = split_list(v);
                } else if let Some(v) = line.strip_prefix("output_contract:") {
                    e.output_contract = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("origin:") {
                    e.origin = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("backend:") {
                    e.backend = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("acp_command:") {
                    acp_command = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("acp_args:") {
                    acp_args = split_list(v);
                } else if let Some(v) = line.strip_prefix("acp_env:") {
                    acp_env = split_env(v);
                }
            }
            let after = &rest[end + "\n---".len()..];
            e.body = after.trim_start_matches('\n').trim().to_string();
            finalize_acp(&mut e, acp_command, acp_args, acp_env);
            return e;
        }
    }
    // No usable frontmatter — treat the whole thing as the body.
    e.body = content.trim().to_string();
    e
}

/// Fold the parsed `acp_*` fields into `e.acp` when this is an ACP master with a command; otherwise
/// normalize an empty/absent backend to [`BACKEND_INTERNAL`].
fn finalize_acp(e: &mut Master, command: String, args: Vec<String>, env: Vec<(String, String)>) {
    if e.backend.is_empty() {
        e.backend = BACKEND_INTERNAL.to_string();
    }
    if e.backend == BACKEND_ACP && !command.is_empty() {
        e.acp = Some(AcpLaunch { command, args, env });
    }
}

/// Whether a [`MasterStore`] owns a single project's masters or the standalone (global) ones. The
/// only difference is the on-disk root and which DB index methods it calls — file format, parsing,
/// and slugs are identical (so a master is portable between the two).
#[derive(Clone)]
enum Scope {
    /// Masters under a project data dir, indexed by `(project_id, slug)`.
    Project(String),
    /// Standalone masters under `<data_home>/masters/`, indexed by `slug` alone.
    Global,
}

/// File-backed masters: owns a `masters/` dir and keeps the DB index in sync. Either project-scoped
/// (`new`) or standalone/global (`global`).
#[derive(Clone)]
pub struct MasterStore {
    /// Root that contains the `masters/` subdir (a project data dir, or the data home for global).
    base_dir: PathBuf,
    scope: Scope,
    store: Store,
}

impl MasterStore {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            base_dir: project_dir,
            scope: Scope::Project(project_id.into()),
            store,
        }
    }

    /// A standalone (project-less) master store rooted at `data_home` (files live under
    /// `<data_home>/masters/`, indexed in the `global_masters` table by slug).
    pub fn global(data_home: PathBuf, store: Store) -> Self {
        Self {
            base_dir: data_home,
            scope: Scope::Global,
            store,
        }
    }

    fn dir(&self) -> PathBuf {
        self.base_dir.join(MASTERS_DIR)
    }

    /// Create (or overwrite) a master: write `masters/<slug>.md` and upsert its index row. Returns
    /// the canonical slug.
    pub fn create(&self, master: &Master) -> Result<String> {
        self.create_with_slug(&slugify(&master.name), master)
    }

    /// Create (or overwrite) a master under an explicit slug. The seam for seeded system
    /// masters whose display name is non-ASCII (e.g. 首席顾问) but whose slug must stay a
    /// stable ASCII handle (`chief`) for @-mentions, team membership, and file names.
    pub fn create_with_slug(&self, slug: &str, master: &Master) -> Result<String> {
        let slug = slugify(slug);
        let dir = self.dir();
        std::fs::create_dir_all(&dir)?;
        let file = format!("{MASTERS_DIR}/{slug}.md");
        std::fs::write(dir.join(format!("{slug}.md")), render(master))?;
        let backend = if master.backend.is_empty() {
            BACKEND_INTERNAL
        } else {
            &master.backend
        };
        match &self.scope {
            Scope::Project(project_id) => self.store.upsert_master(
                project_id,
                &slug,
                &master.name,
                &master.summary,
                &master.default_model,
                &file,
                backend,
            )?,
            Scope::Global => self.store.upsert_global_master(
                &slug,
                &master.name,
                &master.summary,
                &master.default_model,
                &file,
                backend,
            )?,
        }
        Ok(slug)
    }

    /// Load a master by slug (the file is the source of truth), or `None` if absent.
    pub fn load(&self, slug: &str) -> Result<Option<Master>> {
        let slug = slugify(slug);
        let path = self.dir().join(format!("{slug}.md"));
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(parse(&std::fs::read_to_string(path)?)))
    }

    /// All masters in scope (index metadata, for listing).
    pub fn list(&self) -> Result<Vec<MasterRow>> {
        match &self.scope {
            Scope::Project(project_id) => self.store.list_masters(project_id),
            Scope::Global => self.store.list_global_masters(),
        }
    }

    /// Delete a master: remove its file and index row.
    pub fn delete(&self, slug: &str) -> Result<()> {
        let slug = slugify(slug);
        let path = self.dir().join(format!("{slug}.md"));
        std::fs::remove_file(&path).ok();
        match &self.scope {
            Scope::Project(project_id) => self.store.delete_master(project_id, &slug),
            Scope::Global => self.store.delete_global_master(&slug),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Master {
        Master {
            name: "Backend Architect".into(),
            summary: "Designs service architecture and reviews API/data-model decisions.".into(),
            persona:
                "A senior backend engineer; favors simple, testable designs; flags risk early."
                    .into(),
            default_model: "anthropic:claude-opus-4-8".into(),
            allowed_skills: vec!["api-review".into(), "schema-design".into()],
            allowed_tools: vec!["files.read".into(), "knowledge.search".into()],
            output_contract: "A decision note: options, trade-offs, recommendation.".into(),
            origin: "learned".into(),
            body: "Always state assumptions first.".into(),
            backend: BACKEND_INTERNAL.into(),
            acp: None,
        }
    }

    fn sample_acp() -> Master {
        Master {
            name: "Claude Code".into(),
            summary: "External ACP coding agent for hands-on edits.".into(),
            persona: String::new(),
            default_model: String::new(),
            allowed_skills: Vec::new(),
            allowed_tools: vec!["files.read".into(), "files.create".into()],
            output_contract: String::new(),
            origin: "imported".into(),
            body: String::new(),
            backend: BACKEND_ACP.into(),
            acp: Some(AcpLaunch {
                command: "claude-code-acp".into(),
                args: vec!["--stdio".into()],
                env: vec![("ANTHROPIC_API_KEY".into(), "sk-test".into())],
            }),
        }
    }

    #[test]
    fn frontmatter_round_trips() {
        let e = sample();
        assert_eq!(parse(&render(&e)), e);
    }

    #[test]
    fn acp_frontmatter_round_trips() {
        let e = sample_acp();
        let parsed = parse(&render(&e));
        assert!(parsed.is_acp());
        assert_eq!(parsed, e);
    }

    #[test]
    fn legacy_master_without_backend_key_defaults_to_internal() {
        // A pre-4i master file has no `backend:` line — it must parse as the internal backend.
        let legacy = "---\nname: Old\nsummary: s\npersona: p\ndefault_model: m\n---\n\nbody";
        let e = parse(legacy);
        assert_eq!(e.backend, BACKEND_INTERNAL);
        assert!(!e.is_acp());
        assert!(e.acp.is_none());
    }

    #[test]
    fn persona_block_combines_persona_and_body() {
        let e = sample();
        let block = e.persona_block();
        assert!(block.contains("senior backend engineer"));
        assert!(block.contains("state assumptions first"));
    }

    #[test]
    fn create_list_load_delete_keep_file_and_index_in_sync() {
        let dir = std::env::temp_dir().join(format!("getmasters-master-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let masters = MasterStore::new(dir.clone(), pid.clone(), store.clone());

        let slug = masters.create(&sample()).unwrap();
        assert_eq!(slug, "backend-architect");
        assert!(dir.join(format!("{MASTERS_DIR}/{slug}.md")).exists());

        // Index reflects the listing metadata.
        let rows = masters.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Backend Architect");
        assert_eq!(rows[0].default_model, "anthropic:claude-opus-4-8");

        // The file is the source of truth for the full master.
        let loaded = masters.load(&slug).unwrap().unwrap();
        assert_eq!(loaded.allowed_tools, vec!["files.read", "knowledge.search"]);

        // Re-create with the same name overwrites in place (idempotent slug).
        let mut updated = sample();
        updated.summary = "Updated".into();
        masters.create(&updated).unwrap();
        assert_eq!(masters.list().unwrap().len(), 1);

        // Delete removes both the file and the index row.
        masters.delete(&slug).unwrap();
        assert!(masters.list().unwrap().is_empty());
        assert!(!dir.join(format!("{MASTERS_DIR}/{slug}.md")).exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn global_store_is_project_independent() {
        let dir = std::env::temp_dir().join(format!("getmasters-global-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let masters = MasterStore::global(dir.clone(), store.clone());

        // No project needed: create/list/load/delete all work standalone.
        let slug = masters.create(&sample()).unwrap();
        assert_eq!(slug, "backend-architect");
        assert!(dir.join(format!("{MASTERS_DIR}/{slug}.md")).exists());

        let rows = masters.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Backend Architect");

        let loaded = masters.load(&slug).unwrap().unwrap();
        assert_eq!(loaded.allowed_tools, vec!["files.read", "knowledge.search"]);

        masters.delete(&slug).unwrap();
        assert!(masters.list().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
