//! **`route_brief`** — the master router's ranking (Phase 4b, FR-40; docs/04 §2.7, docs/09 §3).
//!
//! Pure, read-only ranking: score a team's masters against a brief and return them ranked, top
//! first. The router *recommends* — it executes nothing (orchestration lives in the server/Core).
//! This is a deterministic **lexical** scorer (term overlap over the master's name/summary/persona/
//! allowed-skills); an embedding-based ranker is a documented later upgrade, like vector recall
//! elsewhere. Keeping it pure here makes it trivially testable and reusable by both the HTTP route
//! and a future `route_brief` MCP tool.

use super::Master;

/// One master's ranking against a brief.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedMaster {
    pub slug: String,
    pub name: String,
    pub score: f32,
}

/// Split text into lowercased alphanumeric terms of length ≥ 3 (drops noise like "a"/"the"/"of").
fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Rank `masters` (each `(slug, Master)`) against `brief`, best match first.
///
/// Score = the number of distinct brief terms that appear in the master's text, with **name** and
/// **summary** matches weighted above persona/skills (identity + description drive routing, per
/// docs/09 §2). Ties keep input order (stable sort), so an explicit member order is respected.
pub fn rank(brief: &str, masters: &[(String, Master)]) -> Vec<RankedMaster> {
    let brief_terms: Vec<String> = {
        let mut t = terms(brief);
        t.sort();
        t.dedup();
        t
    };

    let mut ranked: Vec<RankedMaster> = masters
        .iter()
        .map(|(slug, e)| {
            let strong: std::collections::HashSet<String> =
                terms(&format!("{} {}", e.name, e.summary))
                    .into_iter()
                    .collect();
            let weak: std::collections::HashSet<String> =
                terms(&format!("{} {}", e.persona, e.allowed_skills.join(" ")))
                    .into_iter()
                    .collect();
            let score: f32 = brief_terms
                .iter()
                .map(|t| {
                    if strong.contains(t) {
                        2.0
                    } else if weak.contains(t) {
                        1.0
                    } else {
                        0.0
                    }
                })
                .sum();
            RankedMaster {
                slug: slug.clone(),
                name: e.name.clone(),
                score,
            }
        })
        .collect();

    // Stable sort by descending score (ties keep input/member order).
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

/// The selected master's slug: the top-ranked with a non-zero score, else the `coordinator` (the
/// *(no mention) → coordinator* rule, docs/09 §4). Returns an empty string only if neither exists.
pub fn select(ranked: &[RankedMaster], coordinator: &str) -> String {
    match ranked.first() {
        Some(top) if top.score > 0.0 => top.slug.clone(),
        _ => coordinator.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master(name: &str, summary: &str, persona: &str) -> Master {
        Master {
            name: name.into(),
            summary: summary.into(),
            persona: persona.into(),
            default_model: String::new(),
            allowed_skills: vec![],
            allowed_tools: vec![],
            output_contract: String::new(),
            origin: String::new(),
            body: String::new(),
            backend: crate::masters::BACKEND_INTERNAL.into(),
            acp: None,
        }
    }

    fn members() -> Vec<(String, Master)> {
        vec![
            (
                "architect".into(),
                master(
                    "Backend Architect",
                    "Designs API and database schema decisions.",
                    "A senior backend engineer.",
                ),
            ),
            (
                "writer".into(),
                master(
                    "Copy Writer",
                    "Drafts marketing prose and blog posts.",
                    "A punchy copywriter.",
                ),
            ),
        ]
    }

    #[test]
    fn ranks_relevant_master_first() {
        let ranked = rank("design the API database schema", &members());
        assert_eq!(ranked[0].slug, "architect");
        assert!(ranked[0].score > ranked[1].score);
        assert_eq!(select(&ranked, "writer"), "architect");
    }

    #[test]
    fn falls_back_to_coordinator_on_no_match() {
        let ranked = rank("xyzzy plugh", &members());
        assert!(ranked.iter().all(|r| r.score == 0.0));
        assert_eq!(select(&ranked, "writer"), "writer");
    }
}
