//! The built-in **Study** MCP server (Phase 3a, FR-13/14). Lives in `getmasters-core` (needs the
//! `Store`). Project-scoped (ADR-0011).
//!
//! Tools: `save_flashcards` (write), `start_review` (read), `grade_card` (write), `list_decks`
//! (read). The agent (LLM) authors flashcards from retrieved material and persists them; SM-2
//! scheduling is owned by [`super::sm2`]. The Core permission gate runs before any of these
//! dispatch — `save_flashcards`/`grade_card` are Writes, the listing/review reads are auto-allowed.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::store::Store;

use super::{NewCard, StudyStore};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CardInput {
    /// The prompt side (question, or the cloze sentence with a blank).
    pub front: String,
    /// The answer side.
    pub back: String,
    /// `"qa"` (default) or `"cloze"`.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SaveFlashcardsParams {
    /// The deck to add the cards to (created if it doesn't exist).
    pub deck: String,
    /// The generated flashcards.
    pub cards: Vec<CardInput>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct StartReviewParams {
    /// Limit the review to one deck (by name). Omit to draw due cards from the whole project.
    #[serde(default)]
    pub deck: Option<String>,
    /// Max cards to return (default 20).
    #[serde(default)]
    pub k: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct GradeCardParams {
    /// The id of the card just reviewed (from `start_review`).
    pub card_id: String,
    /// Recall quality, 0–5 (0 = blackout, 3 = correct-with-effort, 5 = perfect).
    pub quality: u8,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListDecksParams {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReviewStatsParams {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateStudyPlanParams {
    /// A short title for the plan (e.g. the exam or goal).
    pub title: String,
    /// Days from now until the deadline (e.g. 10 for "exam in 10 days").
    pub deadline_days: i64,
    /// The day-by-day plan (markdown), prioritizing weak decks from `review_stats`.
    pub plan: String,
}

/// A card to quiz (front only — the answer is revealed when the user grades it).
#[derive(serde::Serialize)]
struct DueCard {
    card_id: String,
    front: String,
    kind: String,
}

/// A deck summary with its due count.
#[derive(serde::Serialize)]
struct DeckSummary {
    name: String,
    cards: i64,
    due: i64,
}

/// Per-deck review aggregates surfaced to the agent so a plan can prioritize weak decks.
#[derive(serde::Serialize)]
struct DeckStat {
    name: String,
    cards: i64,
    due: i64,
    lapses: i64,
    avg_ease: f64,
}

/// Study server scoped to one project.
#[derive(Clone)]
pub struct StudyServer {
    study: StudyStore,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl StudyServer {
    pub fn new(project_id: impl Into<String>, store: Store) -> Self {
        Self {
            study: StudyStore::new(project_id, store),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this). `save_flashcards`/`grade_card`
    /// mutate study state; `start_review`/`list_decks` are reads.
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[
            ("save_flashcards", Write),
            ("start_review", Read),
            ("grade_card", Write),
            ("list_decks", Read),
            ("review_stats", Read),
            ("create_study_plan", Write),
        ]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

#[tool_router]
impl StudyServer {
    #[tool(description = "Save generated flashcards into a deck (Q/A or cloze) for later review")]
    async fn save_flashcards(
        &self,
        Parameters(p): Parameters<SaveFlashcardsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cards: Vec<NewCard> = p
            .cards
            .into_iter()
            .map(|c| NewCard {
                front: c.front,
                back: c.back,
                kind: c.kind.unwrap_or_else(|| "qa".into()),
            })
            .collect();
        Ok(match self.study.save_flashcards(&p.deck, &cards) {
            Ok((_, n)) => ok(format!("saved {n} card(s) to deck '{}'", p.deck)),
            Err(e) => err(format!("save_flashcards failed: {e}")),
        })
    }

    #[tool(description = "Start a spaced-repetition review: return the cards due now (front side)")]
    async fn start_review(
        &self,
        Parameters(p): Parameters<StartReviewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let k = p.k.unwrap_or(20);
        Ok(match self.study.due_cards(p.deck.as_deref(), k) {
            Ok(rows) => {
                let due: Vec<DueCard> = rows
                    .into_iter()
                    .map(|c| DueCard {
                        card_id: c.id,
                        front: c.front,
                        kind: c.kind,
                    })
                    .collect();
                ok(serde_json::to_string(&due).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("start_review failed: {e}")),
        })
    }

    #[tool(description = "Grade a reviewed card (quality 0-5); advances its SM-2 schedule")]
    async fn grade_card(
        &self,
        Parameters(p): Parameters<GradeCardParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.study.grade_card(&p.card_id, p.quality) {
            Ok(Some((back, due_at))) => ok(format!(
                "graded; answer: {back}; next due at {due_at} (epoch ms)"
            )),
            Ok(None) => err(format!("unknown card id '{}'", p.card_id)),
            Err(e) => err(format!("grade_card failed: {e}")),
        })
    }

    #[tool(description = "List the project's flashcard decks with their due counts")]
    async fn list_decks(
        &self,
        Parameters(_): Parameters<ListDecksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.study.list_decks() {
            Ok(rows) => {
                let decks: Vec<DeckSummary> = rows
                    .into_iter()
                    .map(|d| DeckSummary {
                        name: d.name,
                        cards: d.card_count,
                        due: d.due_count,
                    })
                    .collect();
                ok(serde_json::to_string(&decks).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("list_decks failed: {e}")),
        })
    }

    #[tool(
        description = "Review stats per deck (cards/due/lapses/avg ease) to find weak areas before \
                       planning"
    )]
    async fn review_stats(
        &self,
        Parameters(_): Parameters<ReviewStatsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.study.review_stats() {
            Ok(rows) => {
                let stats: Vec<DeckStat> = rows
                    .into_iter()
                    .map(|s| DeckStat {
                        name: s.name,
                        cards: s.cards,
                        due: s.due,
                        lapses: s.lapses,
                        avg_ease: s.avg_ease,
                    })
                    .collect();
                ok(serde_json::to_string(&stats).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("review_stats failed: {e}")),
        })
    }

    #[tool(
        description = "Save an adaptive day-by-day study plan toward a deadline (prioritize weak decks)"
    )]
    async fn create_study_plan(
        &self,
        Parameters(p): Parameters<CreateStudyPlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(
            match self
                .study
                .create_study_plan(&p.title, p.deadline_days, &p.plan)
            {
                Ok(()) => ok(format!(
                    "saved study plan '{}' ({} day(s) out)",
                    p.title, p.deadline_days
                )),
                Err(e) => err(format!("create_study_plan failed: {e}")),
            },
        )
    }
}

#[tool_handler]
impl ServerHandler for StudyServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters Study server: generate flashcards from material and run SM-2 spaced-repetition \
             reviews."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
