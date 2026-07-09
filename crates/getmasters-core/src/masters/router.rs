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

/// Whether a char is CJK (the ranges that matter for Chinese text; kana/hangul excluded — they
/// still match via their own runs' bigrams if added later).
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // Extension A
        | '\u{F900}'..='\u{FAFF}' // Compatibility Ideographs
    )
}

/// Split text into match terms: lowercased ASCII-alphanumeric runs of length ≥ 3 (drops noise
/// like "a"/"the"/"of"), plus **character bigrams** over each CJK run (a single-char run yields
/// the char itself) — CJK has no whitespace word boundaries, so bigram overlap is the standard
/// lexical signal.
fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut ascii_run = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_ascii = |run: &mut String, out: &mut Vec<String>| {
        if run.len() >= 3 {
            out.push(run.to_ascii_lowercase());
        }
        run.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, out: &mut Vec<String>| {
        match run.len() {
            0 => {}
            1 => out.push(run[0].to_string()),
            _ => {
                for pair in run.windows(2) {
                    out.push(pair.iter().collect());
                }
            }
        }
        run.clear();
    };

    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk_run, &mut out);
            ascii_run.push(c);
        } else if is_cjk(c) {
            flush_ascii(&mut ascii_run, &mut out);
            cjk_run.push(c);
        } else {
            flush_ascii(&mut ascii_run, &mut out);
            flush_cjk(&mut cjk_run, &mut out);
        }
    }
    flush_ascii(&mut ascii_run, &mut out);
    flush_cjk(&mut cjk_run, &mut out);
    out
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

    #[test]
    fn cjk_terms_are_bigrams() {
        let t = terms("设计数据库");
        assert!(t.contains(&"设计".to_string()));
        assert!(t.contains(&"数据".to_string()));
        assert!(t.contains(&"据库".to_string()));
        // Mixed text still yields the ASCII word.
        let mixed = terms("设计 API 架构");
        assert!(mixed.contains(&"api".to_string()));
        assert!(mixed.contains(&"架构".to_string()));
    }

    #[test]
    fn ranks_chinese_brief_against_chinese_masters() {
        let members = vec![
            (
                "architect".into(),
                master(
                    "后端架构师",
                    "负责数据库设计和接口方案。",
                    "资深后端工程师。",
                ),
            ),
            (
                "writer".into(),
                master("文案写手", "撰写营销文案和博客。", "文风活泼的写手。"),
            ),
        ];
        let ranked = rank("帮我设计数据库表结构", &members);
        assert_eq!(ranked[0].slug, "architect");
        assert!(ranked[0].score > ranked[1].score);
        assert_eq!(select(&ranked, "writer"), "architect");
    }
}
