//! **Built-in master templates** ("system masters") — a curated, read-only gallery the user can
//! browse from the Masters sidebar and clone into their own (global) collection with one click.
//!
//! These are plain [`MasterDto`]s served by `GET /masters/templates` (a global, project-less route
//! like `/acp/harnesses`). They carry `origin: "builtin"` and the default internal backend; the
//! desktop "Use template" action POSTs one (re-tagged `imported`) to `POST /masters`.

use getmasters_proto::MasterDto;

/// The provider-qualified default model every template ships with (the app's headline Claude tier).
const DEFAULT_MODEL: &str = "anthropic:claude-opus-4-8";

fn template(
    name: &str,
    summary: &str,
    persona: &str,
    allowed_tools: &[&str],
    output_contract: &str,
    body: &str,
) -> MasterDto {
    MasterDto {
        // Slug is derived server-side on save; leave empty here.
        slug: String::new(),
        name: name.to_string(),
        summary: summary.to_string(),
        persona: persona.to_string(),
        default_model: DEFAULT_MODEL.to_string(),
        allowed_skills: Vec::new(),
        allowed_tools: allowed_tools.iter().map(|s| s.to_string()).collect(),
        output_contract: output_contract.to_string(),
        origin: "builtin".to_string(),
        body: body.to_string(),
        backend: "internal".to_string(),
        acp_command: String::new(),
        acp_args: Vec::new(),
        acp_env: Vec::new(),
    }
}

/// The curated built-in master gallery.
pub fn builtin() -> Vec<MasterDto> {
    vec![
        template(
            "Backend Architect",
            "Designs service architecture and reviews API / data-model decisions.",
            "A senior backend engineer; favors simple, testable designs and flags risk early.",
            &["files.read", "knowledge.search"],
            "A decision note: options, trade-offs, and a clear recommendation.",
            "State assumptions first. Prefer boring, proven technology. Call out failure modes and \
             the cheapest way to de-risk them before proposing a design.",
        ),
        template(
            "Copy Writer",
            "Writes and edits crisp, on-brand product and marketing copy.",
            "A versatile copywriter with a sharp ear for tone; cuts filler and leads with the benefit.",
            &["files.read"],
            "Polished copy plus a one-line note on the tone and audience you wrote for.",
            "Lead with the reader's benefit. Keep sentences short. Offer two or three variants when \
             the ask is open-ended, and flag anything that needs a fact-check.",
        ),
        template(
            "Researcher",
            "Gathers, synthesizes, and cites information from the project's knowledge base.",
            "A meticulous research analyst; separates evidence from inference and always cites sources.",
            &["files.read", "knowledge.search"],
            "A short briefing: key findings, supporting citations, and open questions.",
            "Distinguish what the sources say from your own inference. Quote sparingly and cite \
             every claim. End with what you could not determine and what would resolve it.",
        ),
        template(
            "Tutor",
            "Explains concepts and builds study materials at the learner's level.",
            "A patient teacher who checks understanding and adapts explanations to the learner.",
            &["files.read", "knowledge.search"],
            "A clear explanation, a worked example, and a check-for-understanding question.",
            "Start from what the learner already knows. Use one concrete example before \
             generalizing. Finish with a question that tests the idea, not recall.",
        ),
        template(
            "Code Reviewer",
            "Reviews diffs for correctness, clarity, and risk.",
            "A pragmatic reviewer focused on real defects and maintainability, not style nitpicks.",
            &["files.read"],
            "Findings ordered by severity, each with the file, the risk, and a concrete fix.",
            "Prioritize correctness and security over style. For each finding give a concrete \
             failure scenario and the smallest fix. Note when something is fine as-is.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_well_formed() {
        let all = builtin();
        assert!(all.len() >= 4);
        for m in &all {
            assert!(!m.name.is_empty());
            assert!(!m.persona.is_empty());
            assert_eq!(m.origin, "builtin");
            assert_eq!(m.backend, "internal");
        }
    }
}
