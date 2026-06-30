//! Modular prompt assembly (FR-37, ADR-0007).
//!
//! The system prompt is composed from a small set of ordered, editable sources. This is the seam
//! that replaces inline system-prompt logic in the agent loop: tool guidance, RAG grounding,
//! auto-injected **Memory** + **Skills**, and (later) master personas all plug in here, with
//! project-scoped sources ranked above global (ADR-0011). The growing positional argument list is
//! replaced by [`PromptContext`] so new sources don't keep changing every call site.

/// Base persona/defaults (the first, lowest-priority section).
const BASE: &str = "You are Masters, a careful local-first study and work assistant. \
Be concise and accurate. Ground answers in the user's materials when available.";

/// Tool-use guidance, included only when tools are present.
const TOOL_PREAMBLE: &str = "You can call tools to act on the user's files. \
Side-effecting actions (writes, deletes) are gated by the user's approval and are audited — \
prefer the smallest action that accomplishes the task, and explain what you did.";

/// Grounding/citation guidance, included only when knowledge tools are present (ADR-0011).
const GROUNDING: &str = "Before answering questions about the user's materials, call \
knowledge.search and cite every claim with its source as (path · location). If \
knowledge.search returns nothing relevant, say so explicitly and label any answer as not \
grounded in the user's documents. Prefer grounded answers over generated ones.";

/// Curation nudge, included when memory/skills tools are present (ADR-0006/0007).
const CURATION: &str = "When the user states a durable fact, preference, or decision, propose \
calling memory.remember; when you work out a reusable procedure, propose skills.create_skill. \
Do not remember secrets or transient detail.";

/// Ordered inputs to [`PromptAssembler::assemble`]. Project-scoped fields rank above global ones,
/// and `project_instructions` is always emitted last so it takes precedence (ADR-0011).
#[derive(Default)]
pub struct PromptContext<'a> {
    /// The acting master's persona, if the turn runs as a master (ADR-0010/0013). Emitted high —
    /// right after the base prompt — so it frames the whole turn's voice/role.
    pub persona: Option<&'a str>,
    /// The owning project's instructions, if any (highest precedence — emitted last).
    pub project_instructions: Option<&'a str>,
    /// Whether any tools are advertised this turn.
    pub tools_present: bool,
    /// Whether `knowledge.search` is among the advertised tools.
    pub knowledge_present: bool,
    /// Whether memory/skills curation tools are advertised this turn.
    pub curation_present: bool,
    /// Auto-injected durable memory (USER.md profile + recent MEMORY.md facts), if any.
    pub memory_block: Option<String>,
    /// Auto-injected available-skill summaries (`name — summary`), if any.
    pub skill_summaries: Vec<(String, String)>,
}

/// Assembles the system prompt from ordered sources.
pub struct PromptAssembler;

impl PromptAssembler {
    pub fn assemble(ctx: &PromptContext) -> Option<String> {
        let mut sections: Vec<String> = vec![BASE.to_string()];
        // Master persona frames the turn's role/voice right after the base prompt (ADR-0010).
        if let Some(persona) = ctx.persona.map(str::trim).filter(|p| !p.is_empty()) {
            sections.push(format!(
                "You are acting as the following master:\n{persona}"
            ));
        }
        if ctx.tools_present {
            sections.push(TOOL_PREAMBLE.to_string());
        }
        if ctx.knowledge_present {
            sections.push(GROUNDING.to_string());
        }
        if ctx.curation_present {
            sections.push(CURATION.to_string());
        }
        // Auto-injected durable memory (FR-37): authoritative durable context.
        if let Some(mem) = ctx
            .memory_block
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            sections.push(format!(
                "Durable context the user established (treat as authoritative unless \
                 contradicted):\n{mem}"
            ));
        }
        // Available skills: name — summary; the agent recalls the full steps on demand.
        if !ctx.skill_summaries.is_empty() {
            let list = ctx
                .skill_summaries
                .iter()
                .map(|(name, summary)| format!("- {name} — {summary}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!(
                "Available skills (call skills.recall_skill for the steps before improvising):\n{list}"
            ));
        }
        // Project instructions stay last so they take precedence (project-first, ADR-0011).
        if let Some(instr) = ctx
            .project_instructions
            .map(str::trim)
            .filter(|i| !i.is_empty())
        {
            sections.push(instr.to_string());
        }
        Some(sections.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PromptContext<'static> {
        PromptContext::default()
    }

    #[test]
    fn tool_preamble_only_with_tools() {
        let without = PromptAssembler::assemble(&ctx()).unwrap();
        assert!(!without.contains("call tools"));
        let with = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(with.contains("call tools"));
    }

    #[test]
    fn grounding_only_with_knowledge() {
        let without = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(!without.contains("knowledge.search"));
        let with = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            knowledge_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(with.contains("knowledge.search"));
    }

    #[test]
    fn persona_injected_high_when_present() {
        let without = PromptAssembler::assemble(&ctx()).unwrap();
        assert!(!without.contains("acting as the following master"));
        let with = PromptAssembler::assemble(&PromptContext {
            persona: Some("A terse backend architect."),
            tools_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(with.contains("acting as the following master"));
        assert!(with.contains("terse backend architect"));
        // Persona is emitted before the tool guidance (frames the turn).
        assert!(with.find("backend architect").unwrap() < with.find("call tools").unwrap());
    }

    #[test]
    fn curation_only_with_curation_tools() {
        let without = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(!without.contains("memory.remember"));
        let with = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            curation_present: true,
            ..ctx()
        })
        .unwrap();
        assert!(with.contains("memory.remember"));
        assert!(with.contains("skills.create_skill"));
    }

    #[test]
    fn memory_and_skills_inject_when_present() {
        let p = PromptAssembler::assemble(&PromptContext {
            tools_present: true,
            memory_block: Some("- Name: Kai".into()),
            skill_summaries: vec![("Summarize a PDF".into(), "bullet notes".into())],
            ..ctx()
        })
        .unwrap();
        assert!(p.contains("Durable context"));
        assert!(p.contains("Name: Kai"));
        assert!(p.contains("Available skills"));
        assert!(p.contains("Summarize a PDF — bullet notes"));
    }

    #[test]
    fn project_instructions_come_last() {
        let p = PromptAssembler::assemble(&PromptContext {
            project_instructions: Some("Project rule: cite sources."),
            tools_present: true,
            knowledge_present: true,
            memory_block: Some("- Name: Kai".into()),
            ..ctx()
        })
        .unwrap();
        let base_idx = p.find("You are Masters").unwrap();
        let mem_idx = p.find("Durable context").unwrap();
        let instr_idx = p.find("Project rule").unwrap();
        assert!(base_idx < mem_idx);
        assert!(mem_idx < instr_idx);
    }
}
