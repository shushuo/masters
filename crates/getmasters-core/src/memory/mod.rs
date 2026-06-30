//! File-backed, layered Memory (ADR-0007, FR-31/32).
//!
//! Durable memory lives in **Markdown files** the user can read and edit — `MEMORY.md` (facts,
//! decisions, project context) and `USER.md` (the stable user profile). The SQLite `memories`
//! table is only an FTS **index** over those files: the files are the source of truth, and every
//! mutation re-indexes its file so the two never drift (the same "delete-by-source then
//! re-insert" discipline the Knowledge ingest uses).
//!
//! The [`MemoryServer`] (see [`server`]) exposes `remember`/`recall`/`forget` as gated MCP tools;
//! [`load_memory_context`] surfaces the durable context for prompt auto-injection (FR-37).

pub mod server;

use std::path::PathBuf;

use crate::error::Result;
use crate::store::Store;

pub use server::MemoryServer;

/// Backing file for facts/decisions/project context.
pub const FACTS_FILE: &str = "MEMORY.md";
/// Backing file for the stable user profile.
pub const USER_FILE: &str = "USER.md";

/// How many `MEMORY.md` facts to surface in the auto-injected prompt block.
const MAX_INJECTED_FACTS: usize = 10;

/// Where a remembered item is filed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `MEMORY.md` — facts, decisions, project context (the default).
    Fact,
    /// `USER.md` — stable profile of the user.
    User,
}

impl Scope {
    /// Parse the wire string; anything but `"user"` files as a fact (the safe default).
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("user") | Some("profile") => Scope::User,
            _ => Scope::Fact,
        }
    }

    /// The backing file name.
    pub fn file(&self) -> &'static str {
        match self {
            Scope::Fact => FACTS_FILE,
            Scope::User => USER_FILE,
        }
    }

    /// The `memories.kind` discriminator persisted alongside the index row.
    pub fn kind(&self) -> &'static str {
        match self {
            Scope::Fact => "fact",
            Scope::User => "user",
        }
    }

    /// The H1 title rendered at the top of the file.
    fn heading(&self) -> &'static str {
        match self {
            Scope::Fact => "Memory",
            Scope::User => "User Profile",
        }
    }
}

/// One `##` section of a memory file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub body: String,
}

/// Parse a memory file into its `##` sections (content before the first `##` is ignored).
pub fn parse_sections(content: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    for line in content.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(s) = current.take() {
                sections.push(finish(s));
            }
            current = Some(Section {
                title: title.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(s) = current.as_mut() {
            s.body.push_str(line);
            s.body.push('\n');
        }
    }
    if let Some(s) = current.take() {
        sections.push(finish(s));
    }
    sections
}

fn finish(mut s: Section) -> Section {
    s.body = s.body.trim().to_string();
    s
}

/// Render sections back into a stable Markdown document.
pub fn render(heading: &str, sections: &[Section]) -> String {
    let mut out = format!("# {heading}\n");
    for s in sections {
        out.push_str(&format!("\n## {}\n{}\n", s.title, s.body));
    }
    out
}

/// Insert a section or replace the existing one with the same title (case-insensitive match).
pub fn upsert_section(sections: &mut Vec<Section>, title: &str, body: &str) {
    let body = body.trim().to_string();
    if let Some(s) = sections
        .iter_mut()
        .find(|s| s.title.eq_ignore_ascii_case(title))
    {
        s.body = body;
    } else {
        sections.push(Section {
            title: title.trim().to_string(),
            body,
        });
    }
}

/// Remove a section by title (case-insensitive); returns whether one was removed.
pub fn remove_section(sections: &mut Vec<Section>, title: &str) -> bool {
    let before = sections.len();
    sections.retain(|s| !s.title.eq_ignore_ascii_case(title));
    sections.len() != before
}

/// File-backed memory for one project: owns the project data dir and keeps the DB index in sync.
#[derive(Clone)]
pub struct MemoryStore {
    project_dir: PathBuf,
    project_id: String,
    store: Store,
}

impl MemoryStore {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            project_dir,
            project_id: project_id.into(),
            store,
        }
    }

    fn path(&self, scope: Scope) -> PathBuf {
        self.project_dir.join(scope.file())
    }

    fn read_sections(&self, scope: Scope) -> Vec<Section> {
        std::fs::read_to_string(self.path(scope))
            .map(|c| parse_sections(&c))
            .unwrap_or_default()
    }

    /// Persist `sections` to the scope's file and re-index it. Files are truth; the index follows.
    fn write_sections(&self, scope: Scope, sections: &[Section]) -> Result<()> {
        std::fs::create_dir_all(&self.project_dir).map_err(crate::CoreError::from)?;
        std::fs::write(self.path(scope), render(scope.heading(), sections))
            .map_err(crate::CoreError::from)?;
        let rows: Vec<(String, String)> = sections
            .iter()
            .map(|s| (s.title.clone(), s.body.clone()))
            .collect();
        self.store
            .replace_memories_for_file(&self.project_id, scope.file(), scope.kind(), &rows)
    }

    /// Remember a fact (or profile item): upsert the section in its file and re-index.
    pub fn remember(&self, title: &str, content: &str, scope: Scope) -> Result<String> {
        let mut sections = self.read_sections(scope);
        upsert_section(&mut sections, title, content);
        self.write_sections(scope, &sections)?;
        Ok(format!("remembered \"{title}\" in {}", scope.file()))
    }

    /// Forget a section by title; scans both files, returns whether anything was removed.
    pub fn forget(&self, title: &str) -> Result<bool> {
        let mut removed = false;
        for scope in [Scope::Fact, Scope::User] {
            let mut sections = self.read_sections(scope);
            if remove_section(&mut sections, title) {
                self.write_sections(scope, &sections)?;
                removed = true;
            }
        }
        Ok(removed)
    }

    /// FTS recall over the project's memories.
    pub fn recall(&self, query: &str, k: usize) -> Result<Vec<crate::store::MemoryRow>> {
        self.store.search_memories(&self.project_id, query, k)
    }
}

/// Build the auto-injected durable-context block for a project: the `USER.md` profile first, then
/// the most recent `MEMORY.md` facts. `None` when the project has no memories. The prompt module
/// frames this with the "treat as authoritative" preamble (FR-37).
pub fn load_memory_context(store: &Store, project_id: &str) -> Option<String> {
    let mems = store.list_memories(project_id).ok()?;
    if mems.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    for m in mems.iter().filter(|m| m.kind == "user") {
        lines.push(format!("- {}: {}", m.title, oneline(&m.body)));
    }
    for m in mems
        .iter()
        .filter(|m| m.kind == "fact")
        .take(MAX_INJECTED_FACTS)
    {
        lines.push(format!("- {}: {}", m.title, oneline(&m.body)));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn oneline(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_round_trip_through_render() {
        let doc = "# Memory\n\n## Deadline\nThe thesis is due in March.\n\n## Tooling\nRust and SQLite.\n";
        let sections = parse_sections(doc);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Deadline");
        assert_eq!(sections[0].body, "The thesis is due in March.");
        // Re-render then re-parse yields the same sections (stable round-trip).
        let rendered = render("Memory", &sections);
        assert_eq!(parse_sections(&rendered), sections);
    }

    #[test]
    fn upsert_replaces_existing_title() {
        let mut sections = vec![Section {
            title: "Tooling".into(),
            body: "Rust".into(),
        }];
        upsert_section(&mut sections, "tooling", "Rust and SQLite");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].body, "Rust and SQLite");
        upsert_section(&mut sections, "New", "x");
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn remember_recall_forget_keep_file_and_index_in_sync() {
        let dir = std::env::temp_dir().join(format!("getmasters-mem-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let mem = MemoryStore::new(dir.clone(), pid.clone(), store.clone());

        mem.remember("Deadline", "The thesis is due in March.", Scope::Fact)
            .unwrap();
        mem.remember("Name", "The user is Kai.", Scope::User)
            .unwrap();

        // Both files exist on disk (files are truth).
        assert!(dir.join("MEMORY.md").exists());
        assert!(dir.join("USER.md").exists());

        // Index recall works.
        let hits = mem.recall("thesis deadline", 5).unwrap();
        assert_eq!(hits[0].title, "Deadline");

        // Injected context puts the user profile first.
        let block = load_memory_context(&store, &pid).unwrap();
        assert!(block.contains("Name"));
        assert!(block.contains("Deadline"));

        // Forget removes from the file and the index.
        assert!(mem.forget("Deadline").unwrap());
        assert!(mem.recall("thesis", 5).unwrap().is_empty());
        let on_disk = std::fs::read_to_string(dir.join("MEMORY.md")).unwrap();
        assert!(!on_disk.contains("Deadline"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
