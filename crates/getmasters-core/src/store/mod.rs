//! SQLite store over `rusqlite` (bundled — no system libsqlite3 / `sqlite3` CLI needed).
//!
//! Phase 0 persists projects, sessions, and messages. The connection is wrapped in a
//! `Mutex`; methods are synchronous and short-lived (no lock is ever held across an
//! `.await`), which is sufficient for Phase 0's tiny query surface. A pool / `spawn_blocking`
//! can replace this later without changing call sites.

mod migrations;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use getmasters_proto::{
    AuditEntryDto, FolderAccess, FolderGrant, MessageDto, ProjectDto, SessionDto,
};

use crate::error::Result;

/// A chunk joined to its document (for retrieval + citations).
#[derive(Clone, Debug)]
pub struct ChunkRow {
    pub id: String,
    pub document_id: String,
    pub project_id: String,
    pub ordinal: i64,
    pub text: String,
    pub location: Option<String>,
    /// Source document path (for citations).
    pub path: String,
}

/// One indexed memory section (a `##` heading block of `MEMORY.md`/`USER.md`).
#[derive(Clone, Debug)]
pub struct MemoryRow {
    pub id: String,
    /// `"fact"` (MEMORY.md) | `"user"` (USER.md).
    pub kind: String,
    pub title: String,
    pub body: String,
    /// The backing file name (`MEMORY.md` / `USER.md`).
    pub source_file: String,
}

/// One indexed skill (an agent-authored `skills/<slug>.md`).
#[derive(Clone, Debug)]
pub struct SkillRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub body: String,
    pub source_file: String,
}

/// A flashcard deck with its count of currently-due cards (Phase 3a, FR-13/14).
#[derive(Clone, Debug)]
pub struct DeckRow {
    pub id: String,
    pub name: String,
    pub card_count: i64,
    pub due_count: i64,
    pub created_at: i64,
}

/// One flashcard with its SM-2 scheduling state.
#[derive(Clone, Debug)]
pub struct CardRow {
    pub id: String,
    pub deck_id: String,
    pub project_id: String,
    pub front: String,
    pub back: String,
    /// `"qa"` | `"cloze"`.
    pub kind: String,
    pub ease_factor: f64,
    pub interval_days: i64,
    pub repetitions: i64,
    pub lapses: i64,
    /// Epoch ms when the card is next due for review.
    pub due_at: i64,
}

/// Per-deck review aggregates — the weak-area signal for an adaptive study plan (Phase 3b).
#[derive(Clone, Debug)]
pub struct DeckStatRow {
    pub name: String,
    pub cards: i64,
    pub due: i64,
    /// Total lapses across the deck's cards (higher = weaker).
    pub lapses: i64,
    /// Mean ease factor across the deck's cards (lower = weaker); 0.0 for an empty deck.
    pub avg_ease: f64,
}

/// A project's active adaptive study plan (Phase 3b, FR-15).
#[derive(Clone, Debug)]
pub struct StudyPlanRow {
    pub title: String,
    /// Epoch ms of the target deadline.
    pub deadline_at: i64,
    /// The agent-authored day-by-day plan (markdown).
    pub body: String,
}

/// One indexed recipe (a human-authored `recipes/<name>.yaml`) — metadata only (Phase 3c, FR-16).
#[derive(Clone, Debug)]
pub struct RecipeRow {
    pub name: String,
    pub title: String,
    pub description: String,
    pub source_file: String,
}

/// One tracked instrument on the asset lifecycle spine (investing vertical, ADR-0016).
/// `state` walks `watching → holding → sold`; the watchlist and the ledger are states of the
/// same row. The snapshot fields record the *first interest* (price/date at watch time, D10).
#[derive(Clone, Debug)]
pub struct AssetRow {
    pub id: String,
    pub project_id: String,
    /// Canonical symbol, e.g. `sh600519`.
    pub symbol: String,
    pub name: String,
    /// Market id, e.g. `cn-a`.
    pub market: String,
    /// `"stock"` | `"fund"`.
    pub kind: String,
    /// `"watching"` | `"holding"` | `"sold"`.
    pub state: String,
    /// Why the user cared, extracted from conversation.
    pub watch_reason: Option<String>,
    /// Epoch ms of first interest.
    pub watched_at: i64,
    pub snapshot_price: Option<f64>,
    /// `YYYY-MM-DD` the snapshot price is for.
    pub snapshot_date: Option<String>,
}

/// Outcome of an asset untrack attempt — deletion is a lifecycle-guarded operation
/// (ADR-0016: only a `watching` row may be removed; holdings are a ledger, not a list entry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteAssetOutcome {
    Deleted,
    NotFound,
    /// The row exists but is not in `watching` state — refuse.
    NotWatching,
}

/// One cached market quote with provenance (ADR-0017). Global — a quote is a fact about the
/// market, not project data. `validation` is `unverified` until dual-source cross-validation.
#[derive(Clone, Debug)]
pub struct PriceRow {
    pub symbol: String,
    pub market: String,
    pub name: Option<String>,
    /// `YYYY-MM-DD` the quote is for.
    pub trade_date: String,
    pub close: Option<f64>,
    pub prev_close: Option<f64>,
    pub change_pct: Option<f64>,
    /// Adapter id, e.g. `eastmoney`.
    pub source: String,
    /// Epoch ms.
    pub fetched_at: i64,
    /// `"unverified"` | `"verified"` | `"disputed"`.
    pub validation: String,
}

/// One indexed master (`masters/<slug>.md`) — listing metadata only (Phase 4a, FR-39/46).
#[derive(Clone, Debug)]
pub struct MasterRow {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub default_model: String,
    pub source_file: String,
    /// Master backend (Phase 4i, ADR-0014): `"internal"` (persona-over-model) | `"acp"` (external CLI).
    pub backend: String,
}

/// A per-project external MCP connector (Phase 4d, FR-20): the stdio command to spawn a third-party
/// MCP server. `args`/`env` are decoded from their JSON columns; `env` is the *only* environment the
/// child receives (credential stripping, ADR-0008).
#[derive(Clone, Debug)]
pub struct ConnectorRow {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub enabled: bool,
}

/// A Master Team: a group of masters + a coordinator (Phase 4b, FR-38/40). DB-owned config.
#[derive(Clone, Debug)]
pub struct MasterTeamRow {
    pub slug: String,
    pub name: String,
    pub summary: String,
    /// Master slug that answers unaddressed briefs (the coordinator).
    pub coordinator_slug: String,
    /// Member master slugs.
    pub members: Vec<String>,
}

/// A scheduled automation: fire a recipe once or on a cron expression (Phase 3d, FR-17).
#[derive(Clone, Debug)]
pub struct ScheduleRow {
    pub id: String,
    pub project_id: String,
    pub recipe_name: String,
    /// JSON map of recipe param overrides (may be empty).
    pub params: String,
    /// `"once"` | `"cron"`.
    pub kind: String,
    pub cron_expr: Option<String>,
    /// Epoch ms of the next fire; `None` when disabled/done.
    pub next_run_at: Option<i64>,
    pub enabled: bool,
    /// Push an on-device OS notification with the run output (Phase 3e, FR-27).
    pub deliver_notify: bool,
    /// Email the run output to the configured address (opt-in `send`; Phase 3e, FR-27).
    pub deliver_email: bool,
}

/// One recorded firing of a schedule (run history).
#[derive(Clone, Debug)]
pub struct ScheduledRunRow {
    pub started_at: i64,
    /// `"ok"` | `"error"`.
    pub status: String,
    pub session_id: Option<String>,
    pub summary: Option<String>,
}

/// A captured file revision (for revert/undo).
#[derive(Clone, Debug)]
pub struct RevisionRow {
    pub id: String,
    pub session_id: Option<String>,
    pub tool: String,
    pub op: String,
    pub path: String,
    pub prior_content: Option<String>,
    pub existed: bool,
    pub move_from: Option<String>,
}

/// One row to record in the audit log (docs/06).
#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub session_id: Option<String>,
    pub tool: String,
    /// JSON args (already redacted by the caller).
    pub args: Option<String>,
    /// `"auto"` | `"approved"` | `"denied"`.
    pub decision: String,
    pub result_summary: Option<String>,
}

/// Handle to the Masters SQLite database.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

impl Store {
    /// Open (or create) the database at `path`, set pragmas, and run migrations.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        // Register sqlite-vec (if the feature is on) before ANY connection is opened.
        crate::knowledge::register_vec_extension();
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open an in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        crate::knowledge::register_vec_extension();
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::run(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("store mutex poisoned")
    }

    /// Run a closure with the raw connection (used by the sqlite-vec vector backend, which
    /// needs direct access to the `vec0` virtual table).
    pub fn with_conn<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        f(&self.lock())
    }

    // --- Projects -----------------------------------------------------------

    /// Create a project and return its id.
    pub fn create_project(&self, name: &str, instructions: Option<&str>) -> Result<String> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO projects (id, name, instructions, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![id, name, instructions, ts],
        )?;
        Ok(id)
    }

    fn map_project(r: &rusqlite::Row) -> rusqlite::Result<ProjectDto> {
        Ok(ProjectDto {
            id: r.get(0)?,
            name: r.get(1)?,
            instructions: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
        })
    }

    pub fn get_project(&self, id: &str) -> Result<ProjectDto> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, name, instructions, created_at, updated_at FROM projects WHERE id = ?1",
            [id],
            Self::map_project,
        )
        .optional()?
        .ok_or_else(|| crate::CoreError::NotFound(format!("project {id}")))
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectDto>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, instructions, created_at, updated_at FROM projects ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_project)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Set a project's instructions (the auto-injected per-project system prompt).
    pub fn set_project_instructions(&self, id: &str, instructions: &str) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "UPDATE projects SET instructions = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![instructions, ts, id],
        )?;
        Ok(())
    }

    /// The project's system instructions, if any.
    pub fn project_instructions(&self, project_id: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row(
                "SELECT instructions FROM projects WHERE id = ?1",
                [project_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(v)
    }

    // --- Sessions -----------------------------------------------------------

    pub fn create_session(
        &self,
        project_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<SessionDto> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO sessions (id, project_id, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![id, project_id, title, ts],
        )?;
        Ok(SessionDto {
            id,
            project_id: project_id.map(str::to_string),
            title: title.map(str::to_string),
            team_slug: None,
            created_at: ts,
            updated_at: ts,
        })
    }

    /// Bind a session to a master team (it becomes a group chat for that team, Phase 4c).
    pub fn bind_session_team(&self, session_id: &str, team_slug: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE sessions SET team_slug = ?1 WHERE id = ?2",
            rusqlite::params![team_slug, session_id],
        )?;
        Ok(())
    }

    fn map_session(r: &rusqlite::Row) -> rusqlite::Result<SessionDto> {
        Ok(SessionDto {
            id: r.get(0)?,
            project_id: r.get(1)?,
            title: r.get(2)?,
            team_slug: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    }

    pub fn get_session(&self, id: &str) -> Result<SessionDto> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, project_id, title, team_slug, created_at, updated_at FROM sessions WHERE id = ?1",
            [id],
            Self::map_session,
        )
        .optional()?
        .ok_or_else(|| crate::CoreError::NotFound(format!("session {id}")))
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionDto>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, team_slug, created_at, updated_at FROM sessions ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_session)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Delete a session with its messages and events (group scratch cleanup, Phase 4c/4f).
    /// Audit rows are deliberately kept — they are the durable record of gated tool calls.
    pub fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM messages WHERE session_id = ?1", [id])?;
        conn.execute("DELETE FROM events WHERE session_id = ?1", [id])?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Session ids whose title matches a SQL `LIKE` pattern — used by the daemon's startup GC
    /// of orphaned group scratch sessions (`group:%:%`).
    pub fn session_ids_titled_like(&self, pattern: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id FROM sessions WHERE title LIKE ?1")?;
        let rows = stmt
            .query_map([pattern], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    // --- Messages -----------------------------------------------------------

    /// Persist a message and return it. `role` is `"user" | "assistant" | "tool"`. The author
    /// defaults to the role (ordinary chat); use [`Self::insert_message_attributed`] for group chat.
    pub fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<MessageDto> {
        self.insert_message_attributed(session_id, role, role, content, None)
    }

    /// Persist a message attributed to `author` (Phase 4c group chat). `author` is `"user"` or an
    /// master slug; `addressed_to` is a JSON array of slugs (`["@all"]`) or `None`.
    pub fn insert_message_attributed(
        &self,
        session_id: &str,
        author: &str,
        role: &str,
        content: &str,
        addressed_to: Option<&str>,
    ) -> Result<MessageDto> {
        let id = new_id();
        let ts = now_ms();
        {
            let conn = self.lock();
            conn.execute(
                "INSERT INTO messages (id, session_id, role, author, addressed_to, content, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, session_id, role, author, addressed_to, content, ts],
            )?;
            conn.execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![ts, session_id],
            )?;
        }
        Ok(MessageDto {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            author: author.to_string(),
            addressed_to: addressed_to.map(str::to_string),
            content: content.to_string(),
            token_usage: None,
            created_at: ts,
        })
    }

    /// Record the provider-reported token usage for a persisted message (the agent loop calls
    /// this after `Done` carries usage; absent for backends that don't report it).
    pub fn set_message_token_usage(&self, id: &str, tokens: i64) -> Result<()> {
        self.lock().execute(
            "UPDATE messages SET token_usage = ?1 WHERE id = ?2",
            rusqlite::params![tokens, id],
        )?;
        Ok(())
    }

    fn map_message(r: &rusqlite::Row) -> rusqlite::Result<MessageDto> {
        let role: String = r.get(2)?;
        // `author` is NULL for pre-0015 rows — fall back to the role.
        let author: Option<String> = r.get(3)?;
        Ok(MessageDto {
            id: r.get(0)?,
            session_id: r.get(1)?,
            author: author.unwrap_or_else(|| role.clone()),
            role,
            addressed_to: r.get(4)?,
            content: r.get(5)?,
            token_usage: r.get(7)?,
            created_at: r.get(6)?,
        })
    }

    /// Fetch a single message by id.
    pub fn get_message(&self, id: &str) -> Result<MessageDto> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, session_id, role, author, addressed_to, content, created_at, token_usage
             FROM messages WHERE id = ?1",
            [id],
            Self::map_message,
        )
        .optional()?
        .ok_or_else(|| crate::CoreError::NotFound(format!("message {id}")))
    }

    // --- Events (session event log, migration 0019) --------------------------

    /// Append one session event. Call sites are best-effort — a failed append is logged by the
    /// caller, never failing the turn.
    pub fn append_event(
        &self,
        session_id: &str,
        kind: &str,
        payload: Option<&str>,
    ) -> Result<getmasters_proto::EventDto> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO events (id, session_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, session_id, kind, payload, ts],
        )?;
        Ok(getmasters_proto::EventDto {
            id,
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            payload: payload.map(str::to_string),
            created_at: ts,
        })
    }

    /// All events for a session, oldest first.
    pub fn list_events(&self, session_id: &str) -> Result<Vec<getmasters_proto::EventDto>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, kind, payload, created_at
             FROM events WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map([session_id], |r| {
                Ok(getmasters_proto::EventDto {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    kind: r.get(2)?,
                    payload: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // --- Folder grants ------------------------------------------------------

    /// Create a folder grant (permission scope). `project_id` may be `None` for ad-hoc/global.
    pub fn create_folder_grant(
        &self,
        project_id: Option<&str>,
        path: &str,
        access: FolderAccess,
    ) -> Result<FolderGrant> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO folder_grants (id, project_id, path, access, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, project_id, path, access.as_str(), ts],
        )?;
        Ok(FolderGrant {
            id,
            project_id: project_id.map(str::to_string),
            path: path.to_string(),
            access,
            created_at: ts,
        })
    }

    /// All grants for a project (or, with `None`, the project-less/global grants).
    pub fn list_folder_grants(&self, project_id: Option<&str>) -> Result<Vec<FolderGrant>> {
        let conn = self.lock();
        let map_row = |r: &rusqlite::Row| {
            Ok(FolderGrant {
                id: r.get(0)?,
                project_id: r.get(1)?,
                path: r.get(2)?,
                access: FolderAccess::from_str_lenient(&r.get::<_, String>(3)?),
                created_at: r.get(4)?,
            })
        };
        let rows = match project_id {
            Some(pid) => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, path, access, created_at FROM folder_grants
                     WHERE project_id = ?1 ORDER BY created_at",
                )?;
                let v = stmt
                    .query_map([pid], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                v
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, path, access, created_at FROM folder_grants
                     WHERE project_id IS NULL ORDER BY created_at",
                )?;
                let v = stmt
                    .query_map([], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                v
            }
        };
        Ok(rows)
    }

    // --- File revisions (revert) --------------------------------------------

    /// Record a captured file revision.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_revision(&self, row: &RevisionRow) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO file_revisions
               (id, session_id, tool, op, path, prior_content, existed, move_from, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                row.id,
                row.session_id,
                row.tool,
                row.op,
                row.path,
                row.prior_content,
                row.existed as i64,
                row.move_from,
                ts
            ],
        )?;
        Ok(())
    }

    /// The most recent revision for a session (for undo), if any.
    pub fn last_revision(&self, session_id: &str) -> Result<Option<RevisionRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, session_id, tool, op, path, prior_content, existed, move_from
                 FROM file_revisions WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
                [session_id],
                |r| {
                    Ok(RevisionRow {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        tool: r.get(2)?,
                        op: r.get(3)?,
                        path: r.get(4)?,
                        prior_content: r.get(5)?,
                        existed: r.get::<_, i64>(6)? != 0,
                        move_from: r.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Remove a revision row (after a successful revert).
    pub fn delete_revision(&self, id: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM file_revisions WHERE id = ?1", [id])?;
        Ok(())
    }

    // --- Knowledge / RAG ----------------------------------------------------

    /// Upsert a document by (project, path); returns its id. Resets `content_hash`/`indexed_at`.
    pub fn upsert_document(
        &self,
        project_id: &str,
        path: &str,
        content_hash: &str,
        mime: Option<&str>,
    ) -> Result<String> {
        let ts = now_ms();
        let conn = self.lock();
        // Reuse the existing id if the document is already known.
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM documents WHERE project_id = ?1 AND path = ?2",
                rusqlite::params![project_id, path],
                |r| r.get(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(new_id);
        conn.execute(
            "INSERT INTO documents (id, project_id, path, content_hash, mime, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(project_id, path) DO UPDATE SET content_hash = ?4, mime = ?5, indexed_at = ?6",
            rusqlite::params![id, project_id, path, content_hash, mime, ts],
        )?;
        Ok(id)
    }

    /// The `(id, content_hash)` of a known document, for incremental re-index skips.
    pub fn get_document_by_path(
        &self,
        project_id: &str,
        path: &str,
    ) -> Result<Option<(String, String)>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, content_hash FROM documents WHERE project_id = ?1 AND path = ?2",
                rusqlite::params![project_id, path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a document's chunks (+ their embeddings/FTS rows) before re-indexing.
    pub fn delete_document_chunks(&self, document_id: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        // Remove FTS rows by their stored rowid.
        let rowids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT fts_rowid FROM chunks WHERE document_id = ?1 AND fts_rowid IS NOT NULL",
            )?;
            let v = stmt
                .query_map([document_id], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for rowid in rowids {
            tx.execute("DELETE FROM search_fts WHERE rowid = ?1", [rowid])?;
        }
        // Chunks cascade to chunk_embeddings via the FK.
        tx.execute("DELETE FROM chunks WHERE document_id = ?1", [document_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Insert a chunk and return its id.
    pub fn insert_chunk(
        &self,
        document_id: &str,
        project_id: &str,
        ordinal: i64,
        text: &str,
        location: Option<&str>,
    ) -> Result<String> {
        let id = new_id();
        self.lock().execute(
            "INSERT INTO chunks (id, document_id, project_id, ordinal, text, location)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, document_id, project_id, ordinal, text, location],
        )?;
        Ok(id)
    }

    /// Index a chunk's text in FTS and record the resulting rowid on the chunk.
    pub fn fts_index(&self, chunk_id: &str, project_id: &str, body: &str) -> Result<()> {
        let conn = self.lock();
        let rowid = fts_add(&conn, "chunk", chunk_id, project_id, body)?;
        conn.execute(
            "UPDATE chunks SET fts_rowid = ?1 WHERE id = ?2",
            rusqlite::params![rowid, chunk_id],
        )?;
        Ok(())
    }

    /// FTS keyword search within a project (plus global); returns `(chunk_id, score)` high=better.
    pub fn fts_search(
        &self,
        project_id: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<(String, f32)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT ref_id, bm25(search_fts) AS rank FROM search_fts
             WHERE search_fts MATCH ?1 AND (project_id = ?2 OR project_id = 'global')
             ORDER BY rank LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![query, project_id, k as i64], |r| {
                let id: String = r.get(0)?;
                let rank: f64 = r.get(1)?;
                // bm25 is lower=better; negate so higher=better.
                Ok((id, -(rank as f32)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Add (or replace) a chunk embedding (packed little-endian f32).
    pub fn embeddings_add(&self, chunk_id: &str, embedding: &[u8], dim: usize) -> Result<()> {
        self.lock().execute(
            "INSERT INTO chunk_embeddings (chunk_id, embedding, dim) VALUES (?1, ?2, ?3)
             ON CONFLICT(chunk_id) DO UPDATE SET embedding = ?2, dim = ?3",
            rusqlite::params![chunk_id, embedding, dim as i64],
        )?;
        Ok(())
    }

    /// All `(chunk_id, embedding)` for a project (plus global), for brute-force search.
    pub fn embeddings_for_project(&self, project_id: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT ce.chunk_id, ce.embedding FROM chunk_embeddings ce
             JOIN chunks c ON c.id = ce.chunk_id
             WHERE c.project_id = ?1 OR c.project_id = 'global'",
        )?;
        let rows = stmt
            .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Fetch chunks (joined to their document) by id, for assembling cited context.
    pub fn get_chunks_by_ids(&self, ids: &[String]) -> Result<Vec<ChunkRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT c.id, c.document_id, c.project_id, c.ordinal, c.text, c.location, d.path
             FROM chunks c JOIN documents d ON d.id = c.document_id
             WHERE c.id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params = rusqlite::params_from_iter(ids.iter());
        let rows = stmt
            .query_map(params, |r| {
                Ok(ChunkRow {
                    id: r.get(0)?,
                    document_id: r.get(1)?,
                    project_id: r.get(2)?,
                    ordinal: r.get(3)?,
                    text: r.get(4)?,
                    location: r.get(5)?,
                    path: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Knowledge index status for a project: `(documents, chunks, last_indexed_at)`.
    pub fn knowledge_status(&self, project_id: &str) -> Result<(i64, i64, Option<i64>)> {
        let conn = self.lock();
        let docs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        )?;
        let chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        )?;
        let last: Option<i64> = conn.query_row(
            "SELECT MAX(indexed_at) FROM documents WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        )?;
        Ok((docs, chunks, last))
    }

    /// The indexed documents for a project: `(path, mime, indexed_at)`, newest first.
    pub fn list_documents(&self, project_id: &str) -> Result<Vec<(String, Option<String>, i64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path, mime, indexed_at FROM documents
             WHERE project_id = ?1 ORDER BY indexed_at DESC",
        )?;
        let rows = stmt
            .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // --- Memory (file-backed, ADR-0007) -------------------------------------

    /// Replace all indexed sections for one memory file (`MEMORY.md`/`USER.md`) atomically:
    /// drop the file's old rows (+ their FTS entries) and re-insert `sections` as `(title, body)`.
    /// `kind` is `"fact"` (MEMORY.md) or `"user"` (USER.md). Mirrors the ingest "delete-by-source
    /// then re-insert" approach so the index never drifts from the file.
    pub fn replace_memories_for_file(
        &self,
        project_id: &str,
        source_file: &str,
        kind: &str,
        sections: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let rowids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT fts_rowid FROM memories
                 WHERE project_id = ?1 AND source_file = ?2 AND fts_rowid IS NOT NULL",
            )?;
            let v = stmt
                .query_map(rusqlite::params![project_id, source_file], |r| {
                    r.get::<_, i64>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for rowid in rowids {
            fts_delete(&tx, rowid)?;
        }
        tx.execute(
            "DELETE FROM memories WHERE project_id = ?1 AND source_file = ?2",
            rusqlite::params![project_id, source_file],
        )?;
        let ts = now_ms();
        for (title, body) in sections {
            let id = new_id();
            tx.execute(
                "INSERT INTO memories (id, project_id, kind, title, body, source_file, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, project_id, kind, title, body, source_file, ts],
            )?;
            let rowid = fts_add(&tx, "memory", &id, project_id, &format!("{title}\n{body}"))?;
            tx.execute(
                "UPDATE memories SET fts_rowid = ?1 WHERE id = ?2",
                rusqlite::params![rowid, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn map_memory(r: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
        Ok(MemoryRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            body: r.get(3)?,
            source_file: r.get(4)?,
        })
    }

    /// All indexed memory sections for a project.
    pub fn list_memories(&self, project_id: &str) -> Result<Vec<MemoryRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, kind, title, body, source_file FROM memories
             WHERE project_id = ?1 ORDER BY source_file, updated_at",
        )?;
        let rows = stmt
            .query_map([project_id], Self::map_memory)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// FTS recall over a project's memories, best match first.
    pub fn search_memories(
        &self,
        project_id: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<MemoryRow>> {
        let conn = self.lock();
        let ids = fts_search_kind(&conn, project_id, "memory", query, k)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(row) = conn
                .query_row(
                    "SELECT id, kind, title, body, source_file FROM memories WHERE id = ?1",
                    [&id],
                    Self::map_memory,
                )
                .optional()?
            {
                out.push(row);
            }
        }
        Ok(out)
    }

    // --- Skills (file-backed, ADR-0006) -------------------------------------

    /// Upsert a skill by `(project_id, slug)`, keeping its FTS row in sync.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_skill(
        &self,
        project_id: &str,
        slug: &str,
        name: &str,
        summary: &str,
        body: &str,
        source_file: &str,
    ) -> Result<String> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let existing: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT id, fts_rowid FROM skills WHERE project_id = ?1 AND slug = ?2",
                rusqlite::params![project_id, slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let ts = now_ms();
        let fts_body = format!("{name}\n{summary}\n{body}");
        let id = match existing {
            Some((id, old_rowid)) => {
                if let Some(rowid) = old_rowid {
                    fts_delete(&tx, rowid)?;
                }
                tx.execute(
                    "UPDATE skills SET name = ?1, summary = ?2, body = ?3, source_file = ?4, updated_at = ?5
                     WHERE id = ?6",
                    rusqlite::params![name, summary, body, source_file, ts, id],
                )?;
                id
            }
            None => {
                let id = new_id();
                tx.execute(
                    "INSERT INTO skills (id, project_id, slug, name, summary, body, source_file, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![id, project_id, slug, name, summary, body, source_file, ts],
                )?;
                id
            }
        };
        let rowid = fts_add(&tx, "skill", &id, project_id, &fts_body)?;
        tx.execute(
            "UPDATE skills SET fts_rowid = ?1 WHERE id = ?2",
            rusqlite::params![rowid, id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    fn map_skill(r: &rusqlite::Row) -> rusqlite::Result<SkillRow> {
        Ok(SkillRow {
            id: r.get(0)?,
            slug: r.get(1)?,
            name: r.get(2)?,
            summary: r.get(3)?,
            body: r.get(4)?,
            source_file: r.get(5)?,
        })
    }

    /// A single skill by slug.
    pub fn get_skill(&self, project_id: &str, slug: &str) -> Result<Option<SkillRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, slug, name, summary, body, source_file FROM skills
                 WHERE project_id = ?1 AND slug = ?2",
                rusqlite::params![project_id, slug],
                Self::map_skill,
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a project skill by slug (removing its FTS row too).
    pub fn delete_skill(&self, project_id: &str, slug: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let existing: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT id, fts_rowid FROM skills WHERE project_id = ?1 AND slug = ?2",
                rusqlite::params![project_id, slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, rowid)) = existing {
            if let Some(rowid) = rowid {
                fts_delete(&tx, rowid)?;
            }
            tx.execute("DELETE FROM skills WHERE id = ?1", rusqlite::params![id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// All skills for a project.
    pub fn list_skills(&self, project_id: &str) -> Result<Vec<SkillRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, slug, name, summary, body, source_file FROM skills
             WHERE project_id = ?1 ORDER BY name",
        )?;
        let rows = stmt
            .query_map([project_id], Self::map_skill)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// FTS recall over a project's skills, best match first.
    pub fn search_skills(&self, project_id: &str, query: &str, k: usize) -> Result<Vec<SkillRow>> {
        let conn = self.lock();
        let ids = fts_search_kind(&conn, project_id, "skill", query, k)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(row) = conn
                .query_row(
                    "SELECT id, slug, name, summary, body, source_file FROM skills WHERE id = ?1",
                    [&id],
                    Self::map_skill,
                )
                .optional()?
            {
                out.push(row);
            }
        }
        Ok(out)
    }

    // --- Study (flashcards + SM-2, Phase 3a) --------------------------------

    /// Find-or-create a deck by `(project_id, name)`; returns its id.
    pub fn upsert_deck(
        &self,
        project_id: &str,
        name: &str,
        source_doc_id: Option<&str>,
    ) -> Result<String> {
        let conn = self.lock();
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM decks WHERE project_id = ?1 AND name = ?2",
                rusqlite::params![project_id, name],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        let id = new_id();
        conn.execute(
            "INSERT INTO decks (id, project_id, name, source_doc_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, project_id, name, source_doc_id, now_ms()],
        )?;
        Ok(id)
    }

    /// Add a flashcard to a deck (new cards are due immediately). Returns the card id.
    pub fn add_card(
        &self,
        deck_id: &str,
        project_id: &str,
        front: &str,
        back: &str,
        kind: &str,
    ) -> Result<String> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO cards (id, deck_id, project_id, front, back, kind, due_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)",
            rusqlite::params![id, deck_id, project_id, front, back, kind, ts],
        )?;
        Ok(id)
    }

    /// All decks for a project with card + due counts (`due_at <= now`).
    pub fn list_decks(&self, project_id: &str, now: i64) -> Result<Vec<DeckRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.name, d.created_at,
                    COUNT(c.id),
                    COALESCE(SUM(CASE WHEN c.due_at <= ?2 THEN 1 ELSE 0 END), 0)
             FROM decks d
             LEFT JOIN cards c ON c.deck_id = d.id
             WHERE d.project_id = ?1
             GROUP BY d.id ORDER BY d.name",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![project_id, now], |r| {
                Ok(DeckRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                    card_count: r.get(3)?,
                    due_count: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn map_card(r: &rusqlite::Row) -> rusqlite::Result<CardRow> {
        Ok(CardRow {
            id: r.get(0)?,
            deck_id: r.get(1)?,
            project_id: r.get(2)?,
            front: r.get(3)?,
            back: r.get(4)?,
            kind: r.get(5)?,
            ease_factor: r.get(6)?,
            interval_days: r.get(7)?,
            repetitions: r.get(8)?,
            lapses: r.get(9)?,
            due_at: r.get(10)?,
        })
    }

    const CARD_COLS: &'static str =
        "id, deck_id, project_id, front, back, kind, ease_factor, interval_days, repetitions, lapses, due_at";

    /// Cards due for review (`due_at <= now`), soonest first, optionally scoped to one deck.
    pub fn due_cards(
        &self,
        project_id: &str,
        deck_id: Option<&str>,
        now: i64,
        k: usize,
    ) -> Result<Vec<CardRow>> {
        let conn = self.lock();
        let sql = format!(
            "SELECT {} FROM cards WHERE project_id = ?1 AND due_at <= ?2 {} ORDER BY due_at LIMIT ?3",
            Self::CARD_COLS,
            if deck_id.is_some() {
                "AND deck_id = ?4"
            } else {
                ""
            },
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = match deck_id {
            Some(d) => stmt.query_map(
                rusqlite::params![project_id, now, k as i64, d],
                Self::map_card,
            )?,
            None => stmt.query_map(rusqlite::params![project_id, now, k as i64], Self::map_card)?,
        }
        .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A single card by id.
    pub fn get_card(&self, card_id: &str) -> Result<Option<CardRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!("SELECT {} FROM cards WHERE id = ?1", Self::CARD_COLS),
                [card_id],
                Self::map_card,
            )
            .optional()?;
        Ok(row)
    }

    /// Persist a card's updated SM-2 schedule after a review grade.
    pub fn update_card_schedule(
        &self,
        card_id: &str,
        ease_factor: f64,
        interval_days: i64,
        repetitions: i64,
        lapses: i64,
        due_at: i64,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE cards SET ease_factor = ?1, interval_days = ?2, repetitions = ?3,
                    lapses = ?4, due_at = ?5, updated_at = ?6 WHERE id = ?7",
            rusqlite::params![
                ease_factor,
                interval_days,
                repetitions,
                lapses,
                due_at,
                now_ms(),
                card_id
            ],
        )?;
        Ok(())
    }

    /// Per-deck review aggregates (the weak-area signal for an adaptive plan). `due` counts cards
    /// with `due_at <= now`; `avg_ease` is 0.0 for an empty deck.
    pub fn deck_stats(&self, project_id: &str, now: i64) -> Result<Vec<DeckStatRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT d.name,
                    COUNT(c.id),
                    COALESCE(SUM(CASE WHEN c.due_at <= ?2 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(c.lapses), 0),
                    COALESCE(AVG(c.ease_factor), 0.0)
             FROM decks d
             LEFT JOIN cards c ON c.deck_id = d.id
             WHERE d.project_id = ?1
             GROUP BY d.id ORDER BY d.name",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![project_id, now], |r| {
                Ok(DeckStatRow {
                    name: r.get(0)?,
                    cards: r.get(1)?,
                    due: r.get(2)?,
                    lapses: r.get(3)?,
                    avg_ease: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Upsert a project's single active study plan (regenerating replaces it).
    pub fn upsert_study_plan(
        &self,
        project_id: &str,
        title: &str,
        deadline_at: i64,
        body: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO study_plans (id, project_id, title, deadline_at, body, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(project_id) DO UPDATE SET
                title = ?3, deadline_at = ?4, body = ?5, updated_at = ?6",
            rusqlite::params![new_id(), project_id, title, deadline_at, body, ts],
        )?;
        Ok(())
    }

    /// The project's active study plan, if any.
    pub fn get_study_plan(&self, project_id: &str) -> Result<Option<StudyPlanRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT title, deadline_at, body FROM study_plans WHERE project_id = ?1",
                [project_id],
                |r| {
                    Ok(StudyPlanRow {
                        title: r.get(0)?,
                        deadline_at: r.get(1)?,
                        body: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    // --- Assets (investing vertical — the lifecycle spine, ADR-0016) --------

    fn map_asset(r: &rusqlite::Row) -> rusqlite::Result<AssetRow> {
        Ok(AssetRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            symbol: r.get(2)?,
            name: r.get(3)?,
            market: r.get(4)?,
            kind: r.get(5)?,
            state: r.get(6)?,
            watch_reason: r.get(7)?,
            watched_at: r.get(8)?,
            snapshot_price: r.get(9)?,
            snapshot_date: r.get(10)?,
        })
    }

    const ASSET_COLS: &'static str = "id, project_id, symbol, name, market, kind, state, \
         watch_reason, watched_at, snapshot_price, snapshot_date";

    /// Track an instrument as `watching` (silent-but-revocable, D8). Idempotent: an existing
    /// row keeps its original `watched_at` and snapshot — the snapshot is the *first-interest*
    /// record (D10) — and its state (never downgrades `holding`/`sold` back to `watching`);
    /// only `updated_at` moves, and a missing `watch_reason`/`name` may be back-filled.
    /// Returns `(row, newly_created)`.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_asset_watch(
        &self,
        project_id: &str,
        symbol: &str,
        name: &str,
        market: &str,
        kind: &str,
        reason: Option<&str>,
        snapshot_price: Option<f64>,
        snapshot_date: Option<&str>,
        now: i64,
    ) -> Result<(AssetRow, bool)> {
        let conn = self.lock();
        let existing = conn
            .query_row(
                &format!(
                    "SELECT {} FROM assets WHERE project_id = ?1 AND symbol = ?2",
                    Self::ASSET_COLS
                ),
                rusqlite::params![project_id, symbol],
                Self::map_asset,
            )
            .optional()?;
        if let Some(row) = existing {
            // First interest wins: keep watched_at/snapshot/state; back-fill reason only if empty.
            conn.execute(
                "UPDATE assets SET updated_at = ?1,
                        watch_reason = COALESCE(watch_reason, ?2)
                 WHERE id = ?3",
                rusqlite::params![now, reason, row.id],
            )?;
            let row = AssetRow {
                watch_reason: row.watch_reason.clone().or(reason.map(str::to_string)),
                ..row
            };
            return Ok((row, false));
        }
        let id = new_id();
        conn.execute(
            "INSERT INTO assets (id, project_id, symbol, name, market, kind, state,
                                 watch_reason, watched_at, snapshot_price, snapshot_date,
                                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'watching', ?7, ?8, ?9, ?10, ?8, ?8)",
            rusqlite::params![
                id,
                project_id,
                symbol,
                name,
                market,
                kind,
                reason,
                now,
                snapshot_price,
                snapshot_date
            ],
        )?;
        let row = conn.query_row(
            &format!("SELECT {} FROM assets WHERE id = ?1", Self::ASSET_COLS),
            [&id],
            Self::map_asset,
        )?;
        Ok((row, true))
    }

    /// All tracked assets for a project (optionally filtered by state), newest interest first.
    pub fn list_assets(&self, project_id: &str, state: Option<&str>) -> Result<Vec<AssetRow>> {
        let conn = self.lock();
        let sql = format!(
            "SELECT {} FROM assets WHERE project_id = ?1 {} ORDER BY watched_at DESC",
            Self::ASSET_COLS,
            if state.is_some() {
                "AND state = ?2"
            } else {
                ""
            }
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = match state {
            Some(s) => stmt
                .query_map(rusqlite::params![project_id, s], Self::map_asset)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            None => stmt
                .query_map([project_id], Self::map_asset)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };
        Ok(rows)
    }

    /// One tracked asset by canonical symbol.
    pub fn get_asset(&self, project_id: &str, symbol: &str) -> Result<Option<AssetRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!(
                    "SELECT {} FROM assets WHERE project_id = ?1 AND symbol = ?2",
                    Self::ASSET_COLS
                ),
                rusqlite::params![project_id, symbol],
                Self::map_asset,
            )
            .optional()?;
        Ok(row)
    }

    /// Untrack (delete) an asset — lifecycle-guarded: only a `watching` row may be removed
    /// (ADR-0016: holdings/sold are ledger records, not list entries).
    pub fn delete_asset(&self, project_id: &str, symbol: &str) -> Result<DeleteAssetOutcome> {
        let conn = self.lock();
        let n = conn.execute(
            "DELETE FROM assets WHERE project_id = ?1 AND symbol = ?2 AND state = 'watching'",
            rusqlite::params![project_id, symbol],
        )?;
        if n > 0 {
            return Ok(DeleteAssetOutcome::Deleted);
        }
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM assets WHERE project_id = ?1 AND symbol = ?2",
                rusqlite::params![project_id, symbol],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        Ok(if exists {
            DeleteAssetOutcome::NotWatching
        } else {
            DeleteAssetOutcome::NotFound
        })
    }

    // --- Market price cache (with provenance, ADR-0017) ---------------------

    /// Insert or refresh a cached quote (`UNIQUE(symbol, trade_date, source)` upsert).
    /// The cache only ever holds what an adapter actually returned — never a computed guess.
    pub fn insert_price(&self, p: &PriceRow) -> Result<()> {
        self.lock().execute(
            "INSERT INTO price_cache (id, symbol, market, name, trade_date, close, prev_close,
                                      change_pct, source, fetched_at, validation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(symbol, trade_date, source) DO UPDATE SET
                name = ?4, close = ?6, prev_close = ?7, change_pct = ?8,
                fetched_at = ?10, validation = ?11",
            rusqlite::params![
                new_id(),
                p.symbol,
                p.market,
                p.name,
                p.trade_date,
                p.close,
                p.prev_close,
                p.change_pct,
                p.source,
                p.fetched_at,
                p.validation
            ],
        )?;
        Ok(())
    }

    /// The most recent cached quote for a symbol (latest trade date, then latest fetch).
    pub fn latest_price(&self, symbol: &str) -> Result<Option<PriceRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT symbol, market, name, trade_date, close, prev_close, change_pct,
                        source, fetched_at, validation
                 FROM price_cache WHERE symbol = ?1
                 ORDER BY trade_date DESC, fetched_at DESC LIMIT 1",
                [symbol],
                |r| {
                    Ok(PriceRow {
                        symbol: r.get(0)?,
                        market: r.get(1)?,
                        name: r.get(2)?,
                        trade_date: r.get(3)?,
                        close: r.get(4)?,
                        prev_close: r.get(5)?,
                        change_pct: r.get(6)?,
                        source: r.get(7)?,
                        fetched_at: r.get(8)?,
                        validation: r.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    // --- Recipes (file-backed YAML index, Phase 3c) -------------------------

    /// Upsert a recipe's index row by `(project_id, name)` (the YAML file is the source of truth).
    pub fn upsert_recipe(
        &self,
        project_id: &str,
        name: &str,
        title: &str,
        description: &str,
        source_file: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO recipes (id, project_id, name, title, description, source_file, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(project_id, name) DO UPDATE SET
                title = ?4, description = ?5, source_file = ?6, updated_at = ?7",
            rusqlite::params![new_id(), project_id, name, title, description, source_file, ts],
        )?;
        Ok(())
    }

    fn map_recipe(r: &rusqlite::Row) -> rusqlite::Result<RecipeRow> {
        Ok(RecipeRow {
            name: r.get(0)?,
            title: r.get(1)?,
            description: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            source_file: r.get(3)?,
        })
    }

    /// All recipes for a project (metadata only), by name.
    pub fn list_recipes(&self, project_id: &str) -> Result<Vec<RecipeRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT name, title, description, source_file FROM recipes
             WHERE project_id = ?1 ORDER BY name",
        )?;
        let rows = stmt
            .query_map([project_id], Self::map_recipe)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One recipe's index row by name.
    pub fn get_recipe_meta(&self, project_id: &str, name: &str) -> Result<Option<RecipeRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT name, title, description, source_file FROM recipes
                 WHERE project_id = ?1 AND name = ?2",
                rusqlite::params![project_id, name],
                Self::map_recipe,
            )
            .optional()?;
        Ok(row)
    }

    // --- Masters (persona-over-Skill, Phase 4a) -----------------------------

    /// Upsert a master's index row by `(project_id, slug)` (file is the source of truth).
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_master(
        &self,
        project_id: &str,
        slug: &str,
        name: &str,
        summary: &str,
        default_model: &str,
        source_file: &str,
        backend: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO masters (id, project_id, slug, name, summary, default_model, source_file, backend, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(project_id, slug) DO UPDATE SET
                name = ?4, summary = ?5, default_model = ?6, source_file = ?7, backend = ?8, updated_at = ?9",
            rusqlite::params![new_id(), project_id, slug, name, summary, default_model, source_file, backend, ts],
        )?;
        Ok(())
    }

    fn map_master(r: &rusqlite::Row) -> rusqlite::Result<MasterRow> {
        Ok(MasterRow {
            slug: r.get(0)?,
            name: r.get(1)?,
            summary: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            default_model: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            source_file: r.get(4)?,
            backend: r
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "internal".to_string()),
        })
    }

    /// All masters for a project (listing metadata), by name.
    pub fn list_masters(&self, project_id: &str) -> Result<Vec<MasterRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT slug, name, summary, default_model, source_file, backend FROM masters
             WHERE project_id = ?1 ORDER BY name",
        )?;
        let rows = stmt
            .query_map([project_id], Self::map_master)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One master's index row by slug.
    pub fn get_master(&self, project_id: &str, slug: &str) -> Result<Option<MasterRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT slug, name, summary, default_model, source_file, backend FROM masters
                 WHERE project_id = ?1 AND slug = ?2",
                rusqlite::params![project_id, slug],
                Self::map_master,
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a master's index row.
    pub fn delete_master(&self, project_id: &str, slug: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM masters WHERE project_id = ?1 AND slug = ?2",
            rusqlite::params![project_id, slug],
        )?;
        Ok(())
    }

    // --- Global (standalone) masters (Masters sidebar) ----------------------
    // Identical to the project masters above but keyed on `slug` alone — a master that exists
    // independent of any project (the file under `<data_home>/masters/` is the source of truth).

    /// Upsert a global master's index row by `slug`.
    pub fn upsert_global_master(
        &self,
        slug: &str,
        name: &str,
        summary: &str,
        default_model: &str,
        source_file: &str,
        backend: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO global_masters (id, slug, name, summary, default_model, source_file, backend, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(slug) DO UPDATE SET
                name = ?3, summary = ?4, default_model = ?5, source_file = ?6, backend = ?7, updated_at = ?8",
            rusqlite::params![new_id(), slug, name, summary, default_model, source_file, backend, ts],
        )?;
        Ok(())
    }

    /// All global masters (listing metadata), by name.
    pub fn list_global_masters(&self) -> Result<Vec<MasterRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT slug, name, summary, default_model, source_file, backend FROM global_masters
             ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], Self::map_master)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One global master's index row by slug.
    pub fn get_global_master(&self, slug: &str) -> Result<Option<MasterRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT slug, name, summary, default_model, source_file, backend FROM global_masters
                 WHERE slug = ?1",
                rusqlite::params![slug],
                Self::map_master,
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a global master's index row.
    pub fn delete_global_master(&self, slug: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM global_masters WHERE slug = ?1",
            rusqlite::params![slug],
        )?;
        Ok(())
    }

    // --- Global (standalone) skills (Phase: cloud catalog sync) --------------

    /// Upsert a global skill by `slug` (no FTS — global skills are managed/synced content).
    pub fn upsert_global_skill(
        &self,
        slug: &str,
        name: &str,
        summary: &str,
        body: &str,
        source_file: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO global_skills (id, slug, name, summary, body, source_file, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(slug) DO UPDATE SET
                name = ?3, summary = ?4, body = ?5, source_file = ?6, updated_at = ?7",
            rusqlite::params![new_id(), slug, name, summary, body, source_file, ts],
        )?;
        Ok(())
    }

    /// All global skills (listing metadata), by name.
    pub fn list_global_skills(&self) -> Result<Vec<SkillRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, slug, name, summary, body, source_file FROM global_skills ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], Self::map_skill)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One global skill's index row by slug.
    pub fn get_global_skill(&self, slug: &str) -> Result<Option<SkillRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, slug, name, summary, body, source_file FROM global_skills WHERE slug = ?1",
                rusqlite::params![slug],
                Self::map_skill,
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a global skill's index row.
    pub fn delete_global_skill(&self, slug: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM global_skills WHERE slug = ?1",
            rusqlite::params![slug],
        )?;
        Ok(())
    }

    // --- Master Teams (group + router, Phase 4b) ----------------------------

    /// Upsert a team by `(project_id, slug)`. `members` is stored as a JSON array of master slugs.
    pub fn upsert_team(
        &self,
        project_id: &str,
        slug: &str,
        name: &str,
        summary: &str,
        coordinator_slug: &str,
        members: &[String],
    ) -> Result<()> {
        let members_json = serde_json::to_string(members).unwrap_or_else(|_| "[]".into());
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO master_teams
                (id, project_id, slug, name, summary, coordinator_slug, members, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(project_id, slug) DO UPDATE SET
                name = ?4, summary = ?5, coordinator_slug = ?6, members = ?7, updated_at = ?8",
            rusqlite::params![
                new_id(),
                project_id,
                slug,
                name,
                summary,
                coordinator_slug,
                members_json,
                ts
            ],
        )?;
        Ok(())
    }

    fn map_team(r: &rusqlite::Row) -> rusqlite::Result<MasterTeamRow> {
        let members: String = r.get(3)?;
        Ok(MasterTeamRow {
            slug: r.get(0)?,
            name: r.get(1)?,
            summary: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            members: serde_json::from_str(&members).unwrap_or_default(),
            coordinator_slug: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    }

    const TEAM_COLS: &'static str = "slug, name, summary, members, coordinator_slug";

    /// All teams for a project, by name.
    pub fn list_teams(&self, project_id: &str) -> Result<Vec<MasterTeamRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM master_teams WHERE project_id = ?1 ORDER BY name",
            Self::TEAM_COLS
        ))?;
        let rows = stmt
            .query_map([project_id], Self::map_team)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One team by slug.
    pub fn get_team(&self, project_id: &str, slug: &str) -> Result<Option<MasterTeamRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!(
                    "SELECT {} FROM master_teams WHERE project_id = ?1 AND slug = ?2",
                    Self::TEAM_COLS
                ),
                rusqlite::params![project_id, slug],
                Self::map_team,
            )
            .optional()?;
        Ok(row)
    }

    /// Delete a team.
    pub fn delete_team(&self, project_id: &str, slug: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM master_teams WHERE project_id = ?1 AND slug = ?2",
            rusqlite::params![project_id, slug],
        )?;
        Ok(())
    }

    // --- External MCP connectors (Phase 4d) ---------------------------------

    /// Upsert an external connector by `(project_id, name)`. `args`/`env` are stored as JSON.
    pub fn upsert_connector(
        &self,
        project_id: &str,
        name: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
        enabled: bool,
    ) -> Result<()> {
        let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".into());
        let env_map: std::collections::BTreeMap<&str, &str> =
            env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let env_json = serde_json::to_string(&env_map).unwrap_or_else(|_| "{}".into());
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO project_connectors
                (id, project_id, name, command, args, env, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(project_id, name) DO UPDATE SET
                command = ?4, args = ?5, env = ?6, enabled = ?7, updated_at = ?8",
            rusqlite::params![
                new_id(),
                project_id,
                name,
                command,
                args_json,
                env_json,
                enabled as i64,
                ts
            ],
        )?;
        Ok(())
    }

    fn map_connector(r: &rusqlite::Row) -> rusqlite::Result<ConnectorRow> {
        let args: String = r.get(2)?;
        let env: String = r.get(3)?;
        let env_map: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&env).unwrap_or_default();
        Ok(ConnectorRow {
            name: r.get(0)?,
            command: r.get(1)?,
            args: serde_json::from_str(&args).unwrap_or_default(),
            env: env_map.into_iter().collect(),
            enabled: r.get::<_, i64>(4)? != 0,
        })
    }

    const CONNECTOR_COLS: &'static str = "name, command, args, env, enabled";

    /// All connectors for a project, by name.
    pub fn list_connectors(&self, project_id: &str) -> Result<Vec<ConnectorRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM project_connectors WHERE project_id = ?1 ORDER BY name",
            Self::CONNECTOR_COLS
        ))?;
        let rows = stmt
            .query_map([project_id], Self::map_connector)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One connector by name.
    pub fn get_connector(&self, project_id: &str, name: &str) -> Result<Option<ConnectorRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!(
                    "SELECT {} FROM project_connectors WHERE project_id = ?1 AND name = ?2",
                    Self::CONNECTOR_COLS
                ),
                rusqlite::params![project_id, name],
                Self::map_connector,
            )
            .optional()?;
        Ok(row)
    }

    /// Enable or disable a connector.
    pub fn set_connector_enabled(&self, project_id: &str, name: &str, enabled: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE project_connectors SET enabled = ?1, updated_at = ?2
             WHERE project_id = ?3 AND name = ?4",
            rusqlite::params![enabled as i64, now_ms(), project_id, name],
        )?;
        Ok(())
    }

    /// Delete a connector.
    pub fn delete_connector(&self, project_id: &str, name: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM project_connectors WHERE project_id = ?1 AND name = ?2",
            rusqlite::params![project_id, name],
        )?;
        Ok(())
    }

    // --- Schedules (Scheduler, Phase 3d) ------------------------------------

    /// Create a schedule; returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn create_schedule(
        &self,
        project_id: &str,
        recipe_name: &str,
        params: &str,
        kind: &str,
        cron_expr: Option<&str>,
        next_run_at: Option<i64>,
        deliver_notify: bool,
        deliver_email: bool,
    ) -> Result<String> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO schedules
                (id, project_id, recipe_name, params, kind, cron_expr, next_run_at, enabled,
                 deliver_notify, deliver_email, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?10)",
            rusqlite::params![
                id,
                project_id,
                recipe_name,
                params,
                kind,
                cron_expr,
                next_run_at,
                deliver_notify as i64,
                deliver_email as i64,
                ts
            ],
        )?;
        Ok(id)
    }

    fn map_schedule(r: &rusqlite::Row) -> rusqlite::Result<ScheduleRow> {
        Ok(ScheduleRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            recipe_name: r.get(2)?,
            params: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            kind: r.get(4)?,
            cron_expr: r.get(5)?,
            next_run_at: r.get(6)?,
            enabled: r.get::<_, i64>(7)? != 0,
            deliver_notify: r.get::<_, i64>(8)? != 0,
            deliver_email: r.get::<_, i64>(9)? != 0,
        })
    }

    const SCHEDULE_COLS: &'static str =
        "id, project_id, recipe_name, params, kind, cron_expr, next_run_at, enabled, \
         deliver_notify, deliver_email";

    /// All schedules for a project (newest first).
    pub fn list_schedules(&self, project_id: &str) -> Result<Vec<ScheduleRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM schedules WHERE project_id = ?1 ORDER BY created_at DESC",
            Self::SCHEDULE_COLS
        ))?;
        let rows = stmt
            .query_map([project_id], Self::map_schedule)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One schedule by id.
    pub fn get_schedule(&self, id: &str) -> Result<Option<ScheduleRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                &format!(
                    "SELECT {} FROM schedules WHERE id = ?1",
                    Self::SCHEDULE_COLS
                ),
                [id],
                Self::map_schedule,
            )
            .optional()?;
        Ok(row)
    }

    /// Schedules due to fire now (`enabled = 1 AND next_run_at <= now`), across all projects.
    pub fn due_schedules(&self, now: i64) -> Result<Vec<ScheduleRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM schedules
             WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1
             ORDER BY next_run_at",
            Self::SCHEDULE_COLS
        ))?;
        let rows = stmt
            .query_map([now], Self::map_schedule)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Set a schedule's next fire time (and enabled flag — e.g. a one-off disables after firing).
    pub fn set_schedule_next(
        &self,
        id: &str,
        next_run_at: Option<i64>,
        enabled: bool,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE schedules SET next_run_at = ?1, enabled = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![next_run_at, enabled as i64, now_ms(), id],
        )?;
        Ok(())
    }

    /// Enable/disable a schedule without changing its next fire time.
    pub fn set_schedule_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE schedules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![enabled as i64, now_ms(), id],
        )?;
        Ok(())
    }

    /// Set a schedule's delivery flags (Phase 3e, FR-27).
    pub fn set_schedule_delivery(
        &self,
        id: &str,
        deliver_notify: bool,
        deliver_email: bool,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE schedules SET deliver_notify = ?1, deliver_email = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![deliver_notify as i64, deliver_email as i64, now_ms(), id],
        )?;
        Ok(())
    }

    /// Delete a schedule (and its run history, via cascade).
    pub fn delete_schedule(&self, id: &str) -> Result<()> {
        self.lock()
            .execute("DELETE FROM schedules WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Record one firing of a schedule (run history).
    pub fn record_scheduled_run(
        &self,
        schedule_id: &str,
        project_id: &str,
        status: &str,
        session_id: Option<&str>,
        summary: Option<&str>,
    ) -> Result<()> {
        self.lock().execute(
            "INSERT INTO scheduled_runs (id, schedule_id, project_id, started_at, status, session_id, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![new_id(), schedule_id, project_id, now_ms(), status, session_id, summary],
        )?;
        Ok(())
    }

    /// A schedule's run history (newest first).
    pub fn list_scheduled_runs(&self, schedule_id: &str) -> Result<Vec<ScheduledRunRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT started_at, status, session_id, summary FROM scheduled_runs
             WHERE schedule_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt
            .query_map([schedule_id], |r| {
                Ok(ScheduledRunRow {
                    started_at: r.get(0)?,
                    status: r.get(1)?,
                    session_id: r.get(2)?,
                    summary: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // --- Project extensions (FR-19) -----------------------------------------

    /// Enable/disable a built-in server for a project (upsert; absent row = enabled).
    pub fn set_project_extension(
        &self,
        project_id: &str,
        extension: &str,
        enabled: bool,
    ) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO project_extensions (project_id, extension, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id, extension) DO UPDATE SET enabled = ?3, updated_at = ?4",
            rusqlite::params![project_id, extension, enabled as i64, ts],
        )?;
        Ok(())
    }

    /// The explicit per-extension overrides for a project (`(extension, enabled)`); only rows that
    /// exist are returned — anything absent is enabled by default.
    pub fn list_project_extensions(&self, project_id: &str) -> Result<Vec<(String, bool)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT extension, enabled FROM project_extensions WHERE project_id = ?1 ORDER BY extension",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The built-in servers explicitly disabled for a project (the set hosting consults).
    pub fn disabled_extensions(
        &self,
        project_id: &str,
    ) -> Result<std::collections::HashSet<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT extension FROM project_extensions WHERE project_id = ?1 AND enabled = 0",
        )?;
        let rows = stmt
            .query_map([project_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(rows)
    }

    // --- Settings -----------------------------------------------------------

    /// Read a non-secret setting.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(v)
    }

    /// Upsert a non-secret setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            rusqlite::params![key, value, ts],
        )?;
        Ok(())
    }

    // --- Audit log ----------------------------------------------------------

    /// Append an audit entry for a gated tool call.
    pub fn insert_audit(&self, entry: &AuditEntry) -> Result<()> {
        let id = new_id();
        let ts = now_ms();
        self.lock().execute(
            "INSERT INTO audit_log (id, session_id, tool, args, decision, result_summary, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                entry.session_id,
                entry.tool,
                entry.args,
                entry.decision,
                entry.result_summary,
                ts
            ],
        )?;
        Ok(())
    }

    /// All audit entries for a session, oldest first (full rows for the desktop viewer).
    pub fn audit_entries(&self, session_id: &str) -> Result<Vec<AuditEntryDto>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, tool, args, decision, result_summary, created_at FROM audit_log
             WHERE session_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([session_id], |r| {
                Ok(AuditEntryDto {
                    id: r.get(0)?,
                    tool: r.get(1)?,
                    args: r.get(2)?,
                    decision: r.get(3)?,
                    result_summary: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// All audit rows for a session, oldest first (returns `(tool, decision, result_summary)`).
    pub fn list_audit(&self, session_id: &str) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT tool, decision, result_summary FROM audit_log
             WHERE session_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_messages(&self, session_id: &str) -> Result<Vec<MessageDto>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, author, addressed_to, content, created_at, token_usage
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([session_id], Self::map_message)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Insert a row into the shared `search_fts` index and return its rowid. Operates on an open
/// connection so callers can batch it inside a transaction (memory/skills sync atomically).
fn fts_add(
    conn: &Connection,
    kind: &str,
    ref_id: &str,
    project_id: &str,
    body: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO search_fts (body, kind, ref_id, project_id) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![body, kind, ref_id, project_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete a single FTS row by rowid.
fn fts_delete(conn: &Connection, rowid: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM search_fts WHERE rowid = ?1", [rowid])?;
    Ok(())
}

/// FTS keyword search restricted to one `kind` within a project; returns `ref_id`s best-first.
/// The query is sanitized to bare tokens so arbitrary recall text never trips FTS5 syntax.
fn fts_search_kind(
    conn: &Connection,
    project_id: &str,
    kind: &str,
    query: &str,
    k: usize,
) -> rusqlite::Result<Vec<String>> {
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT ref_id FROM search_fts
         WHERE search_fts MATCH ?1 AND kind = ?2 AND project_id = ?3
         ORDER BY bm25(search_fts) LIMIT ?4",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![sanitized, kind, project_id, k as i64],
            |r| r.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Reduce free text to space-separated alphanumeric tokens (an FTS5-safe OR-able query). Empty
/// when no usable token remains.
fn sanitize_fts_query(query: &str) -> String {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let s = Store::open_in_memory().unwrap();
        // Re-running migrations on the same connection must not error.
        migrations::run(&mut s.lock()).unwrap();
    }

    #[test]
    fn session_and_message_round_trip() {
        let s = Store::open_in_memory().unwrap();
        let session = s.create_session(None, Some("test")).unwrap();
        s.insert_message(&session.id, "user", "hello").unwrap();
        let assistant = s
            .insert_message(&session.id, "assistant", "echo: hello")
            .unwrap();

        let msgs = s.list_messages(&session.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].id, assistant.id);

        let fetched = s.get_session(&session.id).unwrap();
        assert_eq!(fetched.id, session.id);
        assert!(s
            .list_sessions()
            .unwrap()
            .iter()
            .any(|x| x.id == session.id));
    }

    #[test]
    fn asset_watch_round_trip_and_idempotence() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("inv", None).unwrap();

        let (row, created) = s
            .upsert_asset_watch(
                &pid,
                "sh600519",
                "贵州茅台",
                "cn-a",
                "stock",
                Some("用户询问该股基本面"),
                Some(1700.0),
                Some("2026-07-15"),
                1_000,
            )
            .unwrap();
        assert!(created);
        assert_eq!(row.state, "watching");
        assert_eq!(row.watched_at, 1_000);
        assert_eq!(row.snapshot_price, Some(1700.0));

        // Re-track: idempotent — first interest wins (watched_at + snapshot preserved).
        let (row2, created2) = s
            .upsert_asset_watch(
                &pid,
                "sh600519",
                "贵州茅台",
                "cn-a",
                "stock",
                Some("second time"),
                Some(9999.0),
                Some("2026-07-16"),
                2_000,
            )
            .unwrap();
        assert!(!created2);
        assert_eq!(row2.watched_at, 1_000);
        assert_eq!(row2.snapshot_price, Some(1700.0));
        assert_eq!(row2.watch_reason.as_deref(), Some("用户询问该股基本面"));

        let listed = s.list_assets(&pid, Some("watching")).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].symbol, "sh600519");

        assert_eq!(
            s.delete_asset(&pid, "sh600519").unwrap(),
            DeleteAssetOutcome::Deleted
        );
        assert_eq!(
            s.delete_asset(&pid, "sh600519").unwrap(),
            DeleteAssetOutcome::NotFound
        );
        assert!(s.list_assets(&pid, None).unwrap().is_empty());
    }

    #[test]
    fn asset_delete_refused_unless_watching() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("inv", None).unwrap();
        s.upsert_asset_watch(
            &pid,
            "sz000001",
            "平安银行",
            "cn-a",
            "stock",
            None,
            None,
            None,
            1,
        )
        .unwrap();
        // Simulate the lifecycle progressing (holding tools land in V1).
        s.lock()
            .execute(
                "UPDATE assets SET state = 'holding' WHERE project_id = ?1 AND symbol = ?2",
                rusqlite::params![pid, "sz000001"],
            )
            .unwrap();
        assert_eq!(
            s.delete_asset(&pid, "sz000001").unwrap(),
            DeleteAssetOutcome::NotWatching
        );
        // And a re-track never downgrades the state back to watching.
        let (row, created) = s
            .upsert_asset_watch(
                &pid,
                "sz000001",
                "平安银行",
                "cn-a",
                "stock",
                None,
                None,
                None,
                2,
            )
            .unwrap();
        assert!(!created);
        assert_eq!(row.state, "holding");
    }

    #[test]
    fn price_cache_upsert_and_latest() {
        let s = Store::open_in_memory().unwrap();
        let mk = |trade_date: &str, close: f64, fetched_at: i64| PriceRow {
            symbol: "sh600519".into(),
            market: "cn-a".into(),
            name: Some("贵州茅台".into()),
            trade_date: trade_date.into(),
            close: Some(close),
            prev_close: Some(close - 10.0),
            change_pct: Some(0.5),
            source: "fixture".into(),
            fetched_at,
            validation: "unverified".into(),
        };
        s.insert_price(&mk("2026-07-14", 1690.0, 100)).unwrap();
        s.insert_price(&mk("2026-07-15", 1700.0, 200)).unwrap();
        // Same (symbol, trade_date, source) refreshes in place — no duplicate row.
        s.insert_price(&mk("2026-07-15", 1701.0, 300)).unwrap();

        let latest = s.latest_price("sh600519").unwrap().unwrap();
        assert_eq!(latest.trade_date, "2026-07-15");
        assert_eq!(latest.close, Some(1701.0));
        assert_eq!(latest.fetched_at, 300);
        assert_eq!(latest.validation, "unverified");
        assert!(s.latest_price("sz999999").unwrap().is_none());
    }

    #[test]
    fn project_instructions_round_trip() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", Some("be terse")).unwrap();
        assert_eq!(
            s.project_instructions(&pid).unwrap().as_deref(),
            Some("be terse")
        );
    }

    #[test]
    fn missing_session_is_not_found() {
        let s = Store::open_in_memory().unwrap();
        assert!(matches!(
            s.get_session("nope"),
            Err(crate::CoreError::NotFound(_))
        ));
    }

    #[test]
    fn folder_grants_round_trip() {
        let s = Store::open_in_memory().unwrap();
        let g = s
            .create_folder_grant(None, "/tmp/work", FolderAccess::ReadWrite)
            .unwrap();
        assert_eq!(g.access, FolderAccess::ReadWrite);
        let grants = s.list_folder_grants(None).unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].path, "/tmp/work");
        assert!(grants[0].access.allows_write());
    }

    #[test]
    fn audit_log_round_trip() {
        let s = Store::open_in_memory().unwrap();
        let session = s.create_session(None, None).unwrap();
        s.insert_audit(&AuditEntry {
            session_id: Some(session.id.clone()),
            tool: "files.create".into(),
            args: Some(r#"{"path":"a.txt"}"#.into()),
            decision: "approved".into(),
            result_summary: Some("created a.txt".into()),
        })
        .unwrap();
        let rows = s.list_audit(&session.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "files.create");
        assert_eq!(rows[0].1, "approved");
    }

    #[test]
    fn memories_replace_and_search() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        s.replace_memories_for_file(
            &pid,
            "MEMORY.md",
            "fact",
            &[
                ("Deadline".into(), "The thesis is due in March.".into()),
                ("Tooling".into(), "We use Rust and SQLite.".into()),
            ],
        )
        .unwrap();
        assert_eq!(s.list_memories(&pid).unwrap().len(), 2);

        let hits = s.search_memories(&pid, "thesis deadline", 5).unwrap();
        assert_eq!(hits[0].title, "Deadline");

        // Re-writing the file drops the old sections (no drift).
        s.replace_memories_for_file(
            &pid,
            "MEMORY.md",
            "fact",
            &[("Tooling".into(), "We use Rust and SQLite.".into())],
        )
        .unwrap();
        let after = s.list_memories(&pid).unwrap();
        assert_eq!(after.len(), 1);
        assert!(s.search_memories(&pid, "thesis", 5).unwrap().is_empty());
    }

    #[test]
    fn skills_upsert_and_search() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        s.upsert_skill(
            &pid,
            "summarize-pdf",
            "Summarize a PDF",
            "Turn a PDF into bullet notes",
            "1. read\n2. outline\n3. write",
            "skills/summarize-pdf.md",
        )
        .unwrap();
        assert_eq!(s.list_skills(&pid).unwrap().len(), 1);
        assert_eq!(
            s.get_skill(&pid, "summarize-pdf").unwrap().unwrap().name,
            "Summarize a PDF"
        );
        let hits = s.search_skills(&pid, "summarize pdf", 5).unwrap();
        assert_eq!(hits[0].slug, "summarize-pdf");

        // Upsert same slug updates in place (idempotent), no FTS row leak.
        s.upsert_skill(
            &pid,
            "summarize-pdf",
            "Summarize a PDF",
            "Updated summary",
            "1. read",
            "skills/summarize-pdf.md",
        )
        .unwrap();
        assert_eq!(s.list_skills(&pid).unwrap().len(), 1);
        assert_eq!(s.search_skills(&pid, "summarize", 5).unwrap().len(), 1);
    }

    #[test]
    fn recipes_index_roundtrips() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        s.upsert_recipe(
            &pid,
            "weekly-digest",
            "Weekly Digest",
            "Summarize new inbox files",
            "recipes/weekly-digest.yaml",
        )
        .unwrap();
        assert_eq!(s.list_recipes(&pid).unwrap().len(), 1);
        assert_eq!(
            s.get_recipe_meta(&pid, "weekly-digest")
                .unwrap()
                .unwrap()
                .title,
            "Weekly Digest"
        );

        // Upsert same name updates in place (idempotent).
        s.upsert_recipe(
            &pid,
            "weekly-digest",
            "Weekly Digest v2",
            "Updated",
            "recipes/weekly-digest.yaml",
        )
        .unwrap();
        let rows = s.list_recipes(&pid).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Weekly Digest v2");
        assert!(s.get_recipe_meta(&pid, "missing").unwrap().is_none());
    }

    #[test]
    fn schedules_crud_due_and_runs() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        let sid = s
            .create_schedule(
                &pid,
                "digest",
                "{}",
                "cron",
                Some("0 9 * * *"),
                Some(1_000),
                true,  // deliver_notify
                false, // deliver_email
            )
            .unwrap();

        assert_eq!(s.list_schedules(&pid).unwrap().len(), 1);
        let row = s.get_schedule(&sid).unwrap().unwrap();
        assert_eq!(row.recipe_name, "digest");
        assert!(row.enabled);
        // Delivery flags round-trip (Phase 3e); default email off.
        assert!(row.deliver_notify);
        assert!(!row.deliver_email);

        // Toggling delivery persists.
        s.set_schedule_delivery(&sid, false, true).unwrap();
        let row = s.get_schedule(&sid).unwrap().unwrap();
        assert!(!row.deliver_notify);
        assert!(row.deliver_email);

        // Due at now=2000 (next_run_at=1000 <= 2000); not due at now=500.
        assert_eq!(s.due_schedules(2_000).unwrap().len(), 1);
        assert!(s.due_schedules(500).unwrap().is_empty());

        // Record a run; history reflects it.
        s.record_scheduled_run(&sid, &pid, "ok", Some("sess-1"), Some("done"))
            .unwrap();
        let runs = s.list_scheduled_runs(&sid).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "ok");
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-1"));

        // Advance the next fire; disabling clears it from the due set.
        s.set_schedule_next(&sid, None, false).unwrap();
        assert!(s.due_schedules(10_000).unwrap().is_empty());

        s.set_schedule_enabled(&sid, true).unwrap();
        s.set_schedule_next(&sid, Some(5_000), true).unwrap();
        assert_eq!(s.due_schedules(10_000).unwrap().len(), 1);

        // Delete cascades the run history.
        s.delete_schedule(&sid).unwrap();
        assert!(s.list_schedules(&pid).unwrap().is_empty());
        assert!(s.list_scheduled_runs(&sid).unwrap().is_empty());
    }

    #[test]
    fn attributed_messages_and_team_binding() {
        let s = Store::open_in_memory().unwrap();
        let sess = s.create_session(None, Some("g")).unwrap();
        assert_eq!(sess.team_slug, None);

        // Ordinary insert derives author from role.
        let u = s.insert_message(&sess.id, "user", "hi").unwrap();
        assert_eq!(u.author, "user");
        assert_eq!(u.addressed_to, None);

        // Attributed insert keeps author + addressed_to.
        let a = s
            .insert_message_attributed(
                &sess.id,
                "architect",
                "assistant",
                "hello",
                Some("[\"architect\"]"),
            )
            .unwrap();
        assert_eq!(a.author, "architect");
        assert_eq!(a.role, "assistant");
        assert_eq!(a.addressed_to.as_deref(), Some("[\"architect\"]"));

        let msgs = s.list_messages(&sess.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].author, "architect");

        s.bind_session_team(&sess.id, "squad").unwrap();
        assert_eq!(
            s.get_session(&sess.id).unwrap().team_slug.as_deref(),
            Some("squad")
        );
    }

    #[test]
    fn connectors_crud_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        s.upsert_connector(
            &pid,
            "filesystem",
            "npx",
            &[
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
            &[("TOKEN".into(), "abc".into())],
            true,
        )
        .unwrap();

        let all = s.list_connectors(&pid).unwrap();
        assert_eq!(all.len(), 1);
        let c = s.get_connector(&pid, "filesystem").unwrap().unwrap();
        assert_eq!(c.command, "npx");
        assert_eq!(
            c.args,
            vec!["-y", "@modelcontextprotocol/server-filesystem"]
        );
        assert_eq!(c.env, vec![("TOKEN".to_string(), "abc".to_string())]);
        assert!(c.enabled);

        s.set_connector_enabled(&pid, "filesystem", false).unwrap();
        assert!(
            !s.get_connector(&pid, "filesystem")
                .unwrap()
                .unwrap()
                .enabled
        );

        // Upsert overwrites command/args in place.
        s.upsert_connector(&pid, "filesystem", "node", &["server.js".into()], &[], true)
            .unwrap();
        let c = s.get_connector(&pid, "filesystem").unwrap().unwrap();
        assert_eq!(c.command, "node");
        assert_eq!(s.list_connectors(&pid).unwrap().len(), 1);

        s.delete_connector(&pid, "filesystem").unwrap();
        assert!(s.list_connectors(&pid).unwrap().is_empty());
    }

    #[test]
    fn teams_crud_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        s.upsert_team(
            &pid,
            "build-squad",
            "Build Squad",
            "Ships features.",
            "architect",
            &["architect".to_string(), "writer".to_string()],
        )
        .unwrap();

        let teams = s.list_teams(&pid).unwrap();
        assert_eq!(teams.len(), 1);
        let t = s.get_team(&pid, "build-squad").unwrap().unwrap();
        assert_eq!(t.name, "Build Squad");
        assert_eq!(t.coordinator_slug, "architect");
        assert_eq!(t.members, vec!["architect", "writer"]);

        // Upsert overwrites in place.
        s.upsert_team(
            &pid,
            "build-squad",
            "Build Squad",
            "",
            "writer",
            &["writer".to_string()],
        )
        .unwrap();
        let t = s.get_team(&pid, "build-squad").unwrap().unwrap();
        assert_eq!(t.coordinator_slug, "writer");
        assert_eq!(t.members, vec!["writer"]);
        assert_eq!(s.list_teams(&pid).unwrap().len(), 1);

        s.delete_team(&pid, "build-squad").unwrap();
        assert!(s.list_teams(&pid).unwrap().is_empty());
    }

    #[test]
    fn project_extension_toggle_state() {
        let s = Store::open_in_memory().unwrap();
        let pid = s.create_project("p", None).unwrap();
        // Absent → enabled (no rows, nothing disabled).
        assert!(s.disabled_extensions(&pid).unwrap().is_empty());

        s.set_project_extension(&pid, "memory", false).unwrap();
        assert!(s.disabled_extensions(&pid).unwrap().contains("memory"));
        assert_eq!(
            s.list_project_extensions(&pid).unwrap(),
            vec![("memory".to_string(), false)]
        );

        // Re-enable (upsert in place) → not disabled.
        s.set_project_extension(&pid, "memory", true).unwrap();
        assert!(s.disabled_extensions(&pid).unwrap().is_empty());
    }
}
