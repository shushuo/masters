//! **Recipes** (Phase 3c, FR-16) — human-authored, parameterized automations (docs/04 §4).
//!
//! A recipe is a YAML file (`recipes/<name>.yaml` under the project data dir) with a `prompt` that
//! seeds the agent loop; `{{key}}` placeholders are substituted from run-time params (falling back
//! to declared defaults, plus a built-in `{{date}}`). Like Skills, the file is the source of truth;
//! the `recipes` table indexes it for listing. The serde model + YAML live here (not in the lean
//! core, which stays YAML-free); [`crate::routes::recipes`] wires the HTTP surface + the run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use getmasters_core::skills::slugify;
use getmasters_core::store::Store;
use getmasters_proto::RecipeDto;

/// Subdirectory (under the project data dir) holding recipe files.
pub const RECIPES_DIR: &str = "recipes";

/// Parse a recipe from its YAML representation.
pub fn parse(yaml: &str) -> Result<RecipeDto, String> {
    serde_yaml_ng::from_str(yaml).map_err(|e| format!("invalid recipe YAML: {e}"))
}

/// Render a recipe back to YAML.
pub fn render(recipe: &RecipeDto) -> Result<String, String> {
    serde_yaml_ng::to_string(recipe).map_err(|e| e.to_string())
}

/// Substitute `{{key}}` placeholders in the recipe prompt: supplied `params` win, then each
/// parameter's declared `default`, then the built-in `{{date}}` (today, UTC `YYYY-MM-DD`).
/// Unknown placeholders are left literal.
pub fn substitute(recipe: &RecipeDto, params: &HashMap<String, String>) -> String {
    let mut out = recipe.prompt.replace("{{date}}", &today_utc());
    for p in &recipe.parameters {
        let val = params
            .get(&p.key)
            .cloned()
            .or_else(|| p.default.clone())
            .unwrap_or_default();
        out = out.replace(&format!("{{{{{}}}}}", p.key), &val);
    }
    // Allow supplied params that weren't declared (e.g. ad-hoc overrides).
    for (k, v) in params {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

/// File-backed recipes for one project: owns the `recipes/` dir and keeps the DB index in sync.
#[derive(Clone)]
pub struct RecipeStore {
    project_dir: PathBuf,
    project_id: String,
    store: Store,
}

impl RecipeStore {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            project_dir,
            project_id: project_id.into(),
            store,
        }
    }

    fn dir(&self) -> PathBuf {
        self.project_dir.join(RECIPES_DIR)
    }

    /// Create/overwrite a recipe: slugify its name, write `recipes/<slug>.yaml`, index it. Returns
    /// the stored recipe (with its canonical, slugified `name`).
    pub fn save(&self, recipe: &RecipeDto) -> Result<RecipeDto, String> {
        let slug = slugify(&recipe.name);
        let mut stored = recipe.clone();
        stored.name = slug.clone();
        let yaml = render(&stored)?;
        std::fs::create_dir_all(self.dir()).map_err(|e| e.to_string())?;
        let file = format!("{RECIPES_DIR}/{slug}.yaml");
        std::fs::write(self.dir().join(format!("{slug}.yaml")), yaml).map_err(|e| e.to_string())?;
        self.store
            .upsert_recipe(
                &self.project_id,
                &slug,
                &stored.title,
                &stored.description,
                &file,
            )
            .map_err(|e| e.to_string())?;
        Ok(stored)
    }

    /// Load a recipe by name (resolved through the same slug), or `None` if absent.
    pub fn load(&self, name: &str) -> Result<Option<RecipeDto>, String> {
        let slug = slugify(name);
        let path = self.dir().join(format!("{slug}.yaml"));
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(Some(parse(&text)?))
    }
}

/// Run an already-loaded recipe: substitute params, open a `recipe:<name>` session, and drive the
/// project agent to completion with approvals cleared (headless auto-approval, grant-bounded +
/// audited). Shared by the HTTP "run now" handler and the Scheduler (Phase 3d).
pub async fn run_loaded(
    state: &crate::state::AppState,
    project_id: &str,
    recipe: &RecipeDto,
    params: &HashMap<String, String>,
) -> Result<getmasters_proto::RecipeRunResult, String> {
    let prompt = substitute(recipe, params);
    let session = state
        .agent
        .store()
        .create_session(Some(project_id), Some(&format!("recipe:{}", recipe.name)))
        .map_err(|e| e.to_string())?;
    let agent = state.project_agent(project_id).await?.without_approval();
    let message = agent.complete_turn(&session.id, &prompt).await?;
    Ok(getmasters_proto::RecipeRunResult {
        session_id: session.id,
        message,
    })
}

/// Today's date in UTC as `YYYY-MM-DD`, for the built-in `{{date}}` placeholder (no date-lib dep).
pub fn today_utc() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(ms / 86_400_000);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert a count of days since the Unix epoch to a civil `(year, month, day)`
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m as u32, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
name: weekly-inbox-digest
title: Weekly Inbox Digest
description: Summarize new files in the Inbox folder into a dated brief.
parameters:
  - key: inbox
    description: Folder to scan
    default: "~/Inbox"
prompt: |
  List files added to {{inbox}} in the last 7 days, then write
  "{{inbox}}/digests/digest-{{date}}.md".
extensions: [files, knowledge]
"#;

    #[test]
    fn parses_the_documented_recipe() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.name, "weekly-inbox-digest");
        assert_eq!(r.title, "Weekly Inbox Digest");
        assert_eq!(r.parameters.len(), 1);
        assert_eq!(r.parameters[0].key, "inbox");
        assert_eq!(r.parameters[0].default.as_deref(), Some("~/Inbox"));
        assert_eq!(r.extensions, vec!["files", "knowledge"]);
    }

    #[test]
    fn substitutes_params_default_and_date() {
        let r = parse(SAMPLE).unwrap();

        // Supplied value wins over the default.
        let mut params = HashMap::new();
        params.insert("inbox".to_string(), "/work/In".to_string());
        let out = substitute(&r, &params);
        assert!(out.contains("/work/In"));
        assert!(!out.contains("{{inbox}}"));

        // Falling back to the declared default when no value is supplied.
        let out_default = substitute(&r, &HashMap::new());
        assert!(out_default.contains("~/Inbox"));

        // The built-in {{date}} is replaced with an ISO date.
        assert!(!out.contains("{{date}}"));
        let date_line = out.lines().find(|l| l.contains("digest-")).unwrap();
        assert!(
            date_line.contains("digest-20") || date_line.contains("digest-19"),
            "date not substituted: {date_line}"
        );
    }

    #[test]
    fn render_parse_round_trips() {
        let r = parse(SAMPLE).unwrap();
        let r2 = parse(&render(&r).unwrap()).unwrap();
        assert_eq!(r.name, r2.name);
        assert_eq!(r.prompt, r2.prompt);
        assert_eq!(r.parameters[0].key, r2.parameters[0].key);
    }

    #[test]
    fn civil_date_known_epoch() {
        // 2021-11-14 was day 18945 since the epoch.
        assert_eq!(civil_from_days(18_945), (2021, 11, 14));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}
