//! Side-effect classification, the default policy inputs, and standing permissions (docs/06 §2).

use std::collections::HashSet;
use std::path::PathBuf;

use getmasters_proto::SideEffect;
use serde_json::Value;

/// Classify a tool name into its side-effect class.
///
/// Phase 1a uses a name heuristic: `web.*` is network; otherwise the last path segment
/// decides (`read`/`list`/`search`/… → read, `delete`/`forget` → destructive, `send` → send,
/// everything else → write, the conservative default). The Files server co-locates an
/// authoritative `tool_classes()` map that the registry can later use to override this.
pub fn classify(tool: &str) -> SideEffect {
    if tool.starts_with("web.") {
        return SideEffect::Network;
    }
    let seg = tool.rsplit('.').next().unwrap_or(tool);
    match seg {
        "read" | "list" | "search" | "recall" | "recall_skill" | "status" | "answer"
        | "list_skills" | "list_masters" | "route_brief" | "start_review" | "list_decks"
        | "review_stats" => SideEffect::Read,
        // `forget` is a curation edit of a memory file (revert-eligible), not a destructive
        // delete of user data (docs/04 §2.4); only `files.delete` is destructive.
        "delete" => SideEffect::Destructive,
        "fetch" => SideEffect::Network,
        "send" => SideEffect::Send,
        // `remember`/`forget`/`create_skill` write the agent's managed memory/skill files.
        _ => SideEffect::Write,
    }
}

/// The file paths a file tool touches, paired with whether write access is required.
pub fn paths_for(tool: &str, args: &Value) -> Vec<(String, bool)> {
    let seg = tool.rsplit('.').next().unwrap_or(tool);
    let get = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    match seg {
        "read" | "list" | "search" => get("path").map(|p| vec![(p, false)]).unwrap_or_default(),
        // knowledge.ingest reads source files (write access not required) but is classified Write.
        "ingest" => get("path").map(|p| vec![(p, false)]).unwrap_or_default(),
        "create" | "edit" | "delete" => get("path").map(|p| vec![(p, true)]).unwrap_or_default(),
        "move" | "rename" => {
            let mut v = Vec::new();
            if let Some(f) = get("from") {
                v.push((f, false));
            }
            if let Some(t) = get("to") {
                v.push((t, true));
            }
            v
        }
        _ => Vec::new(),
    }
}

/// In-memory standing permissions for a session (persistence deferred to Phase 1b's
/// `permissions` table — this is the seam).
#[derive(Default)]
pub struct StandingPerms {
    always_tools: HashSet<String>,
    folders: Vec<PathBuf>,
}

impl StandingPerms {
    /// Whether a tool call over `resolved` paths is already covered by a standing permission.
    pub fn allows(&self, tool: &str, resolved: &[PathBuf]) -> bool {
        if self.always_tools.contains(tool) {
            return true;
        }
        !resolved.is_empty()
            && resolved
                .iter()
                .all(|p| self.folders.iter().any(|f| p.starts_with(f)))
    }

    /// Grant "always allow this tool".
    pub fn grant_tool(&mut self, tool: &str) {
        self.always_tools.insert(tool.to_string());
    }

    /// Grant "allow this folder" for the parent directories of the resolved paths.
    pub fn grant_folders(&mut self, resolved: &[PathBuf]) {
        for p in resolved {
            if let Some(parent) = p.parent() {
                self.folders.push(parent.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classification() {
        assert_eq!(classify("files.read"), SideEffect::Read);
        assert_eq!(classify("files.create"), SideEffect::Write);
        assert_eq!(classify("files.delete"), SideEffect::Destructive);
        assert_eq!(classify("web.search"), SideEffect::Network);
        assert_eq!(classify("web.fetch"), SideEffect::Network);
        // Knowledge tools.
        assert_eq!(classify("knowledge.ingest"), SideEffect::Write);
        assert_eq!(classify("knowledge.search"), SideEffect::Read);
        assert_eq!(classify("knowledge.status"), SideEffect::Read);
        // Memory tools (ADR-0007): writes prompt, recall auto-allows, forget is a Write edit.
        assert_eq!(classify("memory.remember"), SideEffect::Write);
        assert_eq!(classify("memory.recall"), SideEffect::Read);
        assert_eq!(classify("memory.forget"), SideEffect::Write);
        // Skills tools (ADR-0006): recall_skill/list_skills are reads, create_skill writes.
        assert_eq!(classify("skills.create_skill"), SideEffect::Write);
        assert_eq!(classify("skills.recall_skill"), SideEffect::Read);
        assert_eq!(classify("skills.list_skills"), SideEffect::Read);
        // Study tools (Phase 3a): saving/grading write, review/listing read.
        assert_eq!(classify("study.save_flashcards"), SideEffect::Write);
        assert_eq!(classify("study.grade_card"), SideEffect::Write);
        assert_eq!(classify("study.start_review"), SideEffect::Read);
        assert_eq!(classify("study.list_decks"), SideEffect::Read);
        assert_eq!(classify("study.review_stats"), SideEffect::Read);
        assert_eq!(classify("study.create_study_plan"), SideEffect::Write);
        // External MCP connector tools (Phase 4d): unknown verbs fall through to Write, so a
        // third-party server's tools are gated (require approval) by default.
        assert_eq!(classify("notion.create_page"), SideEffect::Write);
        assert_eq!(classify("filesystem.write_file"), SideEffect::Write);
        // Conventionally-named destructive/read verbs are still recognized across any prefix.
        assert_eq!(classify("notion.delete"), SideEffect::Destructive);
    }

    #[test]
    fn memory_and_skill_tools_need_no_grant_paths() {
        // These act inside the managed project data dir, not a user-granted folder.
        assert!(paths_for("memory.remember", &json!({"title":"x","content":"y"})).is_empty());
        assert!(paths_for("skills.create_skill", &json!({"name":"x"})).is_empty());
        assert!(paths_for("memory.recall", &json!({"query":"x"})).is_empty());
    }

    #[test]
    fn path_extraction() {
        assert_eq!(
            paths_for("files.create", &json!({"path":"a.txt"})),
            vec![("a.txt".to_string(), true)]
        );
        assert_eq!(
            paths_for("files.move", &json!({"from":"a","to":"b"})),
            vec![("a".to_string(), false), ("b".to_string(), true)]
        );
    }

    #[test]
    fn standing_always_tool_collapses_prompt() {
        let mut s = StandingPerms::default();
        assert!(!s.allows("files.create", &[]));
        s.grant_tool("files.create");
        assert!(s.allows("files.create", &[]));
    }

    #[test]
    fn standing_folder_covers_contained_paths() {
        let mut s = StandingPerms::default();
        s.grant_folders(&[PathBuf::from("/work/a.txt")]);
        assert!(s.allows("files.edit", &[PathBuf::from("/work/b.txt")]));
        assert!(!s.allows("files.edit", &[PathBuf::from("/other/c.txt")]));
    }
}
