//! **Study** — flashcards + spaced repetition (Phase 3a, FR-13/14).
//!
//! Unlike Memory and Skills (which are file-backed Markdown the user hand-edits), a flashcard's
//! review state is structured SM-2 scheduling data — ease factor, interval, repetitions, due date.
//! So the SQLite `decks`/`cards` tables are the source of truth here (like `documents`/`chunks`),
//! not an index over files. Generation still follows the Skills pattern: the agent (LLM) authors the
//! cards from retrieved material and persists them through a gated tool ([`server::StudyServer`]);
//! the [`sm2`] module owns the (clock-free, pure) scheduling math.

pub mod server;
pub mod sm2;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Result;
use crate::store::{CardRow, DeckRow, DeckStatRow, Store, StudyPlanRow};

pub use server::StudyServer;
pub use sm2::Schedule;

/// Current wall-clock in epoch milliseconds (the one impure edge; [`sm2`] stays clock-free).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A card the agent generated, ready to persist.
pub struct NewCard {
    pub front: String,
    pub back: String,
    /// `"qa"` | `"cloze"` (defaults to `"qa"` when unset/unknown).
    pub kind: String,
}

/// Project-scoped study state: decks of flashcards and their SM-2 review schedule.
#[derive(Clone)]
pub struct StudyStore {
    project_id: String,
    store: Store,
}

impl StudyStore {
    pub fn new(project_id: impl Into<String>, store: Store) -> Self {
        Self {
            project_id: project_id.into(),
            store,
        }
    }

    /// Persist generated cards into a deck (created on demand). Returns the deck id and how many
    /// cards were added.
    pub fn save_flashcards(&self, deck_name: &str, cards: &[NewCard]) -> Result<(String, usize)> {
        let deck_id = self.store.upsert_deck(&self.project_id, deck_name, None)?;
        for c in cards {
            let kind = match c.kind.as_str() {
                "cloze" => "cloze",
                _ => "qa",
            };
            self.store
                .add_card(&deck_id, &self.project_id, &c.front, &c.back, kind)?;
        }
        Ok((deck_id, cards.len()))
    }

    /// All decks with card + due counts (due = `due_at <= now`).
    pub fn list_decks(&self) -> Result<Vec<DeckRow>> {
        self.store.list_decks(&self.project_id, now_ms())
    }

    /// Up to `k` cards due for review now, optionally within one deck (resolved by name).
    pub fn due_cards(&self, deck_name: Option<&str>, k: usize) -> Result<Vec<CardRow>> {
        let deck_id = match deck_name {
            Some(name) => Some(self.store.upsert_deck(&self.project_id, name, None)?),
            None => None,
        };
        self.store
            .due_cards(&self.project_id, deck_id.as_deref(), now_ms(), k)
    }

    /// Grade a reviewed card (`quality` 0–5), advancing its SM-2 schedule. Returns the card's back
    /// (the answer) and its new due time, or `None` if the card id is unknown.
    pub fn grade_card(&self, card_id: &str, quality: u8) -> Result<Option<(String, i64)>> {
        let Some(card) = self.store.get_card(card_id)? else {
            return Ok(None);
        };
        let prev = Schedule {
            ease_factor: card.ease_factor,
            interval_days: card.interval_days,
            repetitions: card.repetitions,
            lapses: card.lapses,
            due_at: card.due_at,
        };
        let next = sm2::schedule(prev, quality, now_ms());
        self.store.update_card_schedule(
            card_id,
            next.ease_factor,
            next.interval_days,
            next.repetitions,
            next.lapses,
            next.due_at,
        )?;
        Ok(Some((card.back, next.due_at)))
    }

    /// Per-deck review aggregates — the weak-area signal an adaptive plan should prioritize (FR-15).
    pub fn review_stats(&self) -> Result<Vec<DeckStatRow>> {
        self.store.deck_stats(&self.project_id, now_ms())
    }

    /// Persist (or replace) the project's adaptive study plan. `deadline_days` is the number of days
    /// from now until the target deadline; the agent authors `body` (a day-by-day plan).
    pub fn create_study_plan(&self, title: &str, deadline_days: i64, body: &str) -> Result<()> {
        let deadline_at = now_ms() + deadline_days.max(0) * sm2::DAY_MS;
        self.store
            .upsert_study_plan(&self.project_id, title, deadline_at, body)
    }

    /// The project's active study plan, if any.
    pub fn study_plan(&self) -> Result<Option<StudyPlanRow>> {
        self.store.get_study_plan(&self.project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_review_grade_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let study = StudyStore::new(pid.clone(), store.clone());

        let (_deck, n) = study
            .save_flashcards(
                "Chapter 3",
                &[
                    NewCard {
                        front: "Capital of France?".into(),
                        back: "Paris".into(),
                        kind: "qa".into(),
                    },
                    NewCard {
                        front: "H2O is ___".into(),
                        back: "water".into(),
                        kind: "cloze".into(),
                    },
                ],
            )
            .unwrap();
        assert_eq!(n, 2);

        // Both new cards are due immediately.
        let decks = study.list_decks().unwrap();
        assert_eq!(decks.len(), 1);
        assert_eq!(decks[0].card_count, 2);
        assert_eq!(decks[0].due_count, 2);

        let due = study.due_cards(None, 10).unwrap();
        assert_eq!(due.len(), 2);

        // Grade one card well — it should advance out of the due set.
        let card_id = due[0].id.clone();
        let (back, next_due) = study.grade_card(&card_id, 5).unwrap().unwrap();
        assert!(!back.is_empty());
        assert!(next_due > now_ms());

        let decks = study.list_decks().unwrap();
        assert_eq!(decks[0].due_count, 1, "graded card should no longer be due");

        // Unknown card id is a clean None, not an error.
        assert!(study.grade_card("nope", 5).unwrap().is_none());
    }

    #[test]
    fn study_plan_roundtrips_and_stats_track_weakness() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let study = StudyStore::new(pid, store);

        // No plan yet.
        assert!(study.study_plan().unwrap().is_none());

        study
            .save_flashcards(
                "Weak",
                &[NewCard {
                    front: "q".into(),
                    back: "a".into(),
                    kind: "qa".into(),
                }],
            )
            .unwrap();

        // Stats start with a neutral ease and no lapses.
        let stats0 = study.review_stats().unwrap();
        assert_eq!(stats0.len(), 1);
        assert_eq!(stats0[0].lapses, 0);
        assert!((stats0[0].avg_ease - 2.5).abs() < 1e-9);

        // Grade the only card badly → a lapse accrues and ease drops (a weak deck). The lapse also
        // reschedules the card a day out, so it's no longer in the due set.
        let due = study.due_cards(Some("Weak"), 10).unwrap();
        study.grade_card(&due[0].id, 0).unwrap();
        let stats1 = study.review_stats().unwrap();
        assert_eq!(stats1[0].lapses, 1);
        assert!(stats1[0].avg_ease < 2.5, "ease should drop on a lapse");
        assert_eq!(
            stats1[0].due, 0,
            "the lapsed card is rescheduled out of the due set"
        );

        // Author a plan; it round-trips with a future deadline and replaces on re-create.
        study
            .create_study_plan("Exam prep", 10, "Day 1: review Weak deck")
            .unwrap();
        let plan = study.study_plan().unwrap().unwrap();
        assert_eq!(plan.title, "Exam prep");
        assert!(plan.deadline_at > now_ms());
        assert!(plan.body.contains("Weak"));

        study
            .create_study_plan("Exam prep v2", 5, "Day 1: cram")
            .unwrap();
        let plan = study.study_plan().unwrap().unwrap();
        assert_eq!(plan.title, "Exam prep v2", "regeneration replaces the plan");
    }

    #[test]
    fn deck_is_reused_by_name() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let study = StudyStore::new(pid, store);
        study
            .save_flashcards(
                "Deck",
                &[NewCard {
                    front: "a".into(),
                    back: "b".into(),
                    kind: "qa".into(),
                }],
            )
            .unwrap();
        study
            .save_flashcards(
                "Deck",
                &[NewCard {
                    front: "c".into(),
                    back: "d".into(),
                    kind: "qa".into(),
                }],
            )
            .unwrap();
        let decks = study.list_decks().unwrap();
        assert_eq!(decks.len(), 1, "same name reuses the deck");
        assert_eq!(decks[0].card_count, 2);
    }
}
