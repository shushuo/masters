//! **@-mention addressing** for multi-master group chat (Phase 4c, FR-43; docs/09 §4).
//!
//! Pure resolution of who a user message addresses, against a team's participants:
//! - `@all` / `@team` → every participant.
//! - `@name` / `@slug` (one or many) → the matched participants (by slug or slugified name).
//! - no valid mention → the team's **coordinator** (the *(no mention) → coordinator* rule).
//!
//! Resolution is deterministic and order-preserving (mentions in first-seen order; `@all` yields the
//! participant order). It executes nothing — the server dispatches the resolved masters.

use crate::skills::slugify;

/// Resolve the masters a `text` addresses. `participants` is `(slug, name)` for each team member;
/// `coordinator` is the fallback slug. Returns the addressed slugs (never empty when a coordinator
/// is set; empty only if there are no participants and no coordinator).
pub fn resolve(text: &str, participants: &[(String, String)], coordinator: &str) -> Vec<String> {
    let tokens = mention_tokens(text);

    // `@all` / `@team` → everyone, in participant order.
    if tokens.iter().any(|(_, t)| t == "all" || t == "team") {
        let everyone: Vec<String> = participants.iter().map(|(slug, _)| slug.clone()).collect();
        if !everyone.is_empty() {
            return everyone;
        }
    }

    // Collect participant hits with their text position, from two passes:
    // 1. mention tokens matched against a slug or slugified name;
    // 2. a direct `@slug` / `@name` substring scan — CJK names have no whitespace boundary
    //    ("@张三你好" tokenizes past the name), so scan for each participant verbatim.
    // Positions merge the passes into first-occurrence order.
    let mut hits: Vec<(usize, String)> = Vec::new();
    for (pos, token) in &tokens {
        let token_slug = slugify(token);
        if let Some((slug, _)) = participants
            .iter()
            .find(|(slug, name)| *slug == token_slug || slugify(name) == token_slug)
        {
            hits.push((*pos, slug.clone()));
        }
    }
    for (slug, name) in participants {
        for needle in [slug.as_str(), name.as_str()] {
            if needle.is_empty() {
                continue;
            }
            if let Some(pos) = text.find(&format!("@{needle}")) {
                hits.push((pos, slug.clone()));
            }
        }
    }
    hits.sort_by_key(|(pos, _)| *pos);
    let mut addressed: Vec<String> = Vec::new();
    for (_, slug) in hits {
        if !addressed.contains(&slug) {
            addressed.push(slug);
        }
    }

    if addressed.is_empty() {
        // No valid mention → the coordinator answers (may be empty if none is set).
        if coordinator.is_empty() {
            Vec::new()
        } else {
            vec![coordinator.to_string()]
        }
    } else {
        addressed
    }
}

/// Resolve **follow-up** mentions in a master's reply (Phase 4f, bounded turn-taking): the explicit
/// masters a reply addresses for the next round. Unlike [`resolve`], there is **no coordinator
/// fallback** — an unmentioned reply ends the thread — and the reply's own author (`self_slug`) is
/// excluded so a master can't re-trigger itself. `@all`/`@team` → everyone but `self_slug`.
pub fn followups(text: &str, participants: &[(String, String)], self_slug: &str) -> Vec<String> {
    // Reuse the explicit-mention semantics: an empty coordinator yields "mentions only, else empty".
    resolve(text, participants, "")
        .into_iter()
        .filter(|slug| slug != self_slug)
        .collect()
}

/// Extract the raw `@token` strings from `text` with their byte position (lowercased, without
/// the `@`). A token is the run of Unicode letters/digits/`-`/`_` after `@`, so trailing
/// punctuation self-delimits (e.g. "@architect," / "@all." / "@张三,").
fn mention_tokens(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut iter = text.char_indices().peekable();
    while let Some((pos, c)) = iter.next() {
        if c != '@' {
            continue;
        }
        let mut tok = String::new();
        while let Some(&(_, nc)) = iter.peek() {
            if nc.is_alphanumeric() || nc == '-' || nc == '_' {
                tok.extend(nc.to_lowercase());
                iter.next();
            } else {
                break;
            }
        }
        if !tok.is_empty() {
            out.push((pos, tok));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Vec<(String, String)> {
        vec![
            ("backend-architect".into(), "Backend Architect".into()),
            ("copy-writer".into(), "Copy Writer".into()),
        ]
    }

    #[test]
    fn single_mention_addresses_one() {
        assert_eq!(
            resolve("@backend-architect design it", &parts(), "copy-writer"),
            vec!["backend-architect"]
        );
    }

    #[test]
    fn mention_matches_slugified_name() {
        // "@Backend Architect" can't be one token, but "@backend-architect" and name-slug match.
        assert_eq!(
            resolve("hey @copy-writer, draft", &parts(), "backend-architect"),
            vec!["copy-writer"]
        );
    }

    #[test]
    fn multiple_mentions_address_each_deduped() {
        assert_eq!(
            resolve(
                "@copy-writer @backend-architect @copy-writer go",
                &parts(),
                "x"
            ),
            vec!["copy-writer", "backend-architect"]
        );
    }

    #[test]
    fn all_addresses_everyone() {
        assert_eq!(
            resolve("@all please review", &parts(), "copy-writer"),
            vec!["backend-architect", "copy-writer"]
        );
    }

    #[test]
    fn no_mention_falls_back_to_coordinator() {
        assert_eq!(
            resolve("just chatting", &parts(), "copy-writer"),
            vec!["copy-writer"]
        );
    }

    #[test]
    fn unknown_mention_falls_back_to_coordinator() {
        assert_eq!(
            resolve("@nobody hello", &parts(), "backend-architect"),
            vec!["backend-architect"]
        );
    }

    #[test]
    fn followups_address_explicit_other_masters() {
        // An architect reply that hands off to the copy-writer triggers a follow-up round.
        assert_eq!(
            followups(
                "Done. @copy-writer please name it.",
                &parts(),
                "backend-architect"
            ),
            vec!["copy-writer"]
        );
    }

    #[test]
    fn followups_have_no_coordinator_fallback() {
        // No mention → the thread ends (no coordinator loop, unlike `resolve`).
        assert!(followups("Looks good to me.", &parts(), "backend-architect").is_empty());
    }

    fn cn_parts() -> Vec<(String, String)> {
        vec![
            ("张三".into(), "张三".into()),
            ("产品经理".into(), "产品经理".into()),
        ]
    }

    #[test]
    fn chinese_mention_with_space_resolves() {
        assert_eq!(
            resolve("@张三 你怎么看?", &cn_parts(), "产品经理"),
            vec!["张三"]
        );
    }

    #[test]
    fn chinese_mention_without_boundary_resolves_via_substring() {
        // No whitespace after the name — the token pass overshoots ("张三你怎么看"),
        // the substring pass still finds "@张三".
        assert_eq!(
            resolve("@张三你怎么看?", &cn_parts(), "产品经理"),
            vec!["张三"]
        );
    }

    #[test]
    fn chinese_mentions_keep_occurrence_order_mixed_with_ascii() {
        let mixed = vec![
            ("zhang-san".into(), "张三".into()),
            ("copy-writer".into(), "Copy Writer".into()),
        ];
        assert_eq!(
            resolve("@张三 先来,然后 @copy-writer 收尾", &mixed, "x"),
            vec!["zhang-san", "copy-writer"]
        );
    }

    #[test]
    fn chinese_no_mention_falls_back_to_coordinator() {
        assert_eq!(
            resolve("随便聊聊", &cn_parts(), "产品经理"),
            vec!["产品经理"]
        );
    }

    #[test]
    fn followups_exclude_self() {
        // A master mentioning itself can't re-trigger its own next round.
        assert!(followups(
            "@backend-architect keep going",
            &parts(),
            "backend-architect"
        )
        .is_empty());
        // …but @all still pulls in the others, minus self.
        assert_eq!(
            followups("@all thoughts?", &parts(), "backend-architect"),
            vec!["copy-writer"]
        );
    }
}
