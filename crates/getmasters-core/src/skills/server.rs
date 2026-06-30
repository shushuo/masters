//! The built-in **Skills** MCP server (docs/04 §2.5, ADR-0006). Lives in `getmasters-core` (needs the
//! `Store` index + the project data dir). Project-scoped (ADR-0011).
//!
//! Tools: `create_skill` (write), `recall_skill` (read), `list_skills` (read). Files are truth;
//! creating re-indexes the file. The Core permission gate runs before any of these dispatch.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use std::path::PathBuf;

use crate::store::Store;

use super::SkillStore;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateSkillParams {
    /// A short, descriptive name (also the basis for the skill's slug).
    pub name: String,
    /// One-line summary of what the skill does (shown in the prompt's skill list).
    pub summary: String,
    /// The procedure itself — the Markdown steps to follow.
    pub steps: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RecallSkillParams {
    /// What kind of procedure to look for.
    pub query: String,
    /// Max results (default 5).
    #[serde(default)]
    pub k: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListSkillsParams {}

/// A recalled skill (full body, so the agent can follow it).
#[derive(serde::Serialize)]
struct RecalledSkill {
    slug: String,
    name: String,
    summary: String,
    steps: String,
}

/// A listed skill (name + summary only).
#[derive(serde::Serialize)]
struct ListedSkill {
    slug: String,
    name: String,
    summary: String,
}

/// Skills server scoped to one project.
#[derive(Clone)]
pub struct SkillsServer {
    skills: SkillStore,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl SkillsServer {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            skills: SkillStore::new(project_dir, project_id, store),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this). Note `recall_skill` and
    /// `list_skills` are reads — only `create_skill` mutates.
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[
            ("create_skill", Write),
            ("recall_skill", Read),
            ("list_skills", Read),
        ]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

#[tool_router]
impl SkillsServer {
    #[tool(description = "Author a reusable procedure (skill) so it can be recalled later")]
    async fn create_skill(
        &self,
        Parameters(p): Parameters<CreateSkillParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.skills.create(&p.name, &p.summary, &p.steps) {
            Ok((slug, file)) => ok(format!("created skill '{slug}' ({file})")),
            Err(e) => err(format!("create_skill failed: {e}")),
        })
    }

    #[tool(description = "Recall a saved skill (its full steps) relevant to a query")]
    async fn recall_skill(
        &self,
        Parameters(p): Parameters<RecallSkillParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let k = p.k.unwrap_or(5);
        Ok(match self.skills.recall(&p.query, k) {
            Ok(rows) => {
                let hits: Vec<RecalledSkill> = rows
                    .into_iter()
                    .map(|s| RecalledSkill {
                        slug: s.slug,
                        name: s.name,
                        summary: s.summary,
                        steps: s.body,
                    })
                    .collect();
                ok(serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("recall_skill failed: {e}")),
        })
    }

    #[tool(description = "List the saved skills available in this project")]
    async fn list_skills(
        &self,
        Parameters(_): Parameters<ListSkillsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.skills.list() {
            Ok(rows) => {
                let listed: Vec<ListedSkill> = rows
                    .into_iter()
                    .map(|s| ListedSkill {
                        slug: s.slug,
                        name: s.name,
                        summary: s.summary,
                    })
                    .collect();
                ok(serde_json::to_string(&listed).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("list_skills failed: {e}")),
        })
    }
}

#[tool_handler]
impl ServerHandler for SkillsServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters Skills server: author and recall reusable procedures (skills/<slug>.md)."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
