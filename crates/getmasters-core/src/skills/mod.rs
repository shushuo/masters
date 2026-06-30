//! Self-improving procedural memory — **Skills** (ADR-0006, FR-29/30).
//!
//! A Skill is an agent-authored, portable how-to note saved as `skills/<slug>.md` with YAML-ish
//! frontmatter (`name`, `summary`, `tags`) over a Markdown body of steps. The files are the
//! source of truth; the SQLite `skills` table is an FTS index over them. The agent captures a
//! reusable procedure once (`create_skill`) and **recalls** it later (`recall_skill`), with
//! available skills auto-summarized into the prompt ([`load_skill_summaries`], FR-37).
//!
//! Frontmatter is parsed by a tiny hand-rolled reader (delimited by `---` fences) so the crate
//! needs no `serde_yaml` dependency — only `name`/`summary`/`tags` keys are recognized.

pub mod server;

use std::path::PathBuf;

use crate::error::Result;
use crate::store::{SkillRow, Store};

pub use server::SkillsServer;

/// Subdirectory (under the project data dir) that holds skill files.
pub const SKILLS_DIR: &str = "skills";

/// How many skill summaries to surface in the auto-injected prompt block.
const MAX_INJECTED_SKILLS: usize = 12;

/// A skill's parsed contents (frontmatter + body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub summary: String,
    pub tags: Vec<String>,
    /// The Markdown body (the steps).
    pub body: String,
}

/// Derive a filesystem-safe slug from a skill name (lowercase, non-alphanumeric → `-`).
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "skill".to_string()
    } else {
        slug
    }
}

/// Render a skill into its `skills/<slug>.md` representation (frontmatter + body).
pub fn render(skill: &Skill) -> String {
    let tags = skill.tags.join(", ");
    format!(
        "---\nname: {}\nsummary: {}\ntags: {}\n---\n\n{}\n",
        skill.name,
        skill.summary,
        tags,
        skill.body.trim()
    )
}

/// Parse a `skills/<slug>.md` document. Missing frontmatter keys default to empty; everything
/// after the closing `---` fence is the body. Falls back to treating the whole text as the body.
pub fn parse(content: &str) -> Skill {
    let mut name = String::new();
    let mut summary = String::new();
    let mut tags: Vec<String> = Vec::new();

    let rest = content.strip_prefix("---");
    if let Some(rest) = rest {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            for line in front.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("summary:") {
                    summary = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("tags:") {
                    tags = v
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
            // Body is everything after the closing fence line.
            let after = &rest[end + "\n---".len()..];
            let body = after.trim_start_matches('\n').trim().to_string();
            return Skill {
                name,
                summary,
                tags,
                body,
            };
        }
    }
    // No usable frontmatter — treat the whole thing as the body.
    Skill {
        name,
        summary,
        tags,
        body: content.trim().to_string(),
    }
}

/// File-backed skills for one project: owns the `skills/` dir and keeps the DB index in sync.
#[derive(Clone)]
pub struct SkillStore {
    project_dir: PathBuf,
    project_id: String,
    store: Store,
}

impl SkillStore {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            project_dir,
            project_id: project_id.into(),
            store,
        }
    }

    fn skills_dir(&self) -> PathBuf {
        self.project_dir.join(SKILLS_DIR)
    }

    /// Author (or overwrite) a skill: write `skills/<slug>.md` and upsert its index row.
    pub fn create(&self, name: &str, summary: &str, steps: &str) -> Result<(String, String)> {
        let slug = slugify(name);
        let skill = Skill {
            name: name.trim().to_string(),
            summary: summary.trim().to_string(),
            tags: Vec::new(),
            body: steps.trim().to_string(),
        };
        let dir = self.skills_dir();
        std::fs::create_dir_all(&dir)?;
        let file = format!("{SKILLS_DIR}/{slug}.md");
        std::fs::write(dir.join(format!("{slug}.md")), render(&skill))?;
        self.store.upsert_skill(
            &self.project_id,
            &slug,
            &skill.name,
            &skill.summary,
            &skill.body,
            &file,
        )?;
        Ok((slug, file))
    }

    /// FTS recall over the project's skills.
    pub fn recall(&self, query: &str, k: usize) -> Result<Vec<SkillRow>> {
        self.store.search_skills(&self.project_id, query, k)
    }

    /// All skills for the project.
    pub fn list(&self) -> Result<Vec<SkillRow>> {
        self.store.list_skills(&self.project_id)
    }
}

/// A name+summary pair for prompt auto-injection.
#[derive(Clone, Debug)]
pub struct SkillSummary {
    pub name: String,
    pub summary: String,
}

/// Build the auto-injected "available skills" list for a project (name — summary), or `None` when
/// the project has no skills. The prompt module frames this with the "call recall_skill" preamble.
pub fn load_skill_summaries(store: &Store, project_id: &str) -> Option<Vec<SkillSummary>> {
    let rows = store.list_skills(project_id).ok()?;
    if rows.is_empty() {
        return None;
    }
    Some(
        rows.into_iter()
            .take(MAX_INJECTED_SKILLS)
            .map(|s| SkillSummary {
                name: s.name,
                summary: s.summary,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_handles_spaces_and_symbols() {
        assert_eq!(slugify("Summarize a PDF!"), "summarize-a-pdf");
        assert_eq!(slugify("  Weird___Name  "), "weird-name");
        assert_eq!(slugify("***"), "skill");
    }

    #[test]
    fn frontmatter_round_trips() {
        let skill = Skill {
            name: "Summarize a PDF".into(),
            summary: "Turn a PDF into bullet notes".into(),
            tags: vec!["study".into(), "pdf".into()],
            body: "1. read\n2. outline\n3. write".into(),
        };
        let parsed = parse(&render(&skill));
        assert_eq!(parsed, skill);
    }

    #[test]
    fn create_recall_keep_file_and_index_in_sync() {
        let dir = std::env::temp_dir().join(format!("getmasters-skill-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let skills = SkillStore::new(dir.clone(), pid.clone(), store.clone());

        let (slug, file) = skills
            .create(
                "Summarize a PDF",
                "Turn a PDF into bullet notes",
                "1. read\n2. outline\n3. write",
            )
            .unwrap();
        assert_eq!(slug, "summarize-a-pdf");
        assert!(dir.join(&file).exists());

        let hits = skills.recall("summarize pdf", 5).unwrap();
        assert_eq!(hits[0].slug, "summarize-a-pdf");

        let summaries = load_skill_summaries(&store, &pid).unwrap();
        assert_eq!(summaries[0].name, "Summarize a PDF");

        // Re-create with the same name overwrites in place (idempotent slug).
        skills
            .create("Summarize a PDF", "Updated", "1. read")
            .unwrap();
        assert_eq!(skills.list().unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
