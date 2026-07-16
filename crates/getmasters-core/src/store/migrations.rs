//! Hand-rolled embedded migrations.
//!
//! Three tables don't warrant a migration framework. Each entry in [`MIGRATIONS`] is applied
//! in order inside a transaction; a `schema_version` table records how many have run, so
//! startup is idempotent. Phase 1+ migrations only ever ADD (columns/tables) — the Phase 0
//! columns are a strict subset of the illustrative schema in docs/05 §2.

use rusqlite::Connection;

/// Ordered SQL migrations. Append-only — never edit or reorder an applied entry.
pub const MIGRATIONS: &[&str] = &[
    // 0001 — Phase 0 core tables.
    r#"
    CREATE TABLE projects (
        id           TEXT PRIMARY KEY,
        name         TEXT NOT NULL,
        instructions TEXT,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        id         TEXT PRIMARY KEY,
        project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
        title      TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE messages (
        id          TEXT PRIMARY KEY,
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        role        TEXT NOT NULL,
        content     TEXT NOT NULL,
        token_usage INTEGER,
        created_at  INTEGER NOT NULL
    );

    CREATE INDEX idx_messages_session ON messages(session_id, created_at);
    "#,
    // 0002 — Phase 1a: folder grants (permission scope) + audit log (docs/05 §2, docs/06).
    // Strict subset; `author_master_id` is added in a later migration when `masters` exists.
    r#"
    CREATE TABLE folder_grants (
        id         TEXT PRIMARY KEY,
        project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
        path       TEXT NOT NULL,
        access     TEXT NOT NULL,          -- 'read' | 'read_write'
        created_at INTEGER NOT NULL
    );

    CREATE TABLE audit_log (
        id             TEXT PRIMARY KEY,
        session_id     TEXT,
        tool           TEXT NOT NULL,
        args           TEXT,               -- JSON, secrets redacted
        decision       TEXT NOT NULL,      -- 'auto' | 'approved' | 'denied'
        result_summary TEXT,
        created_at     INTEGER NOT NULL
    );

    CREATE INDEX idx_audit_session ON audit_log(session_id, created_at);
    "#,
    // 0003 — Phase 1b: app settings (non-secret; API keys live in the OS keychain, docs/06 §4).
    r#"
    CREATE TABLE settings (
        key        TEXT PRIMARY KEY,
        value      TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
    // 0005 — Phase 2a: Knowledge/RAG (docs/05 §2-3). Stable tables only; the sqlite-vec
    // `chunk_vectors` virtual table is created at runtime (extension- + dim-dependent).
    r#"
    CREATE TABLE documents (
        id           TEXT PRIMARY KEY,
        project_id   TEXT NOT NULL,
        path         TEXT NOT NULL,
        content_hash TEXT NOT NULL,       -- detects changes for incremental re-index
        mime         TEXT,
        indexed_at   INTEGER NOT NULL,
        UNIQUE(project_id, path)
    );

    CREATE TABLE chunks (
        id          TEXT PRIMARY KEY,
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        project_id  TEXT NOT NULL,        -- denormalized for project-scoped ranking
        ordinal     INTEGER NOT NULL,
        text        TEXT NOT NULL,
        location    TEXT,                 -- e.g. 'heading: Intro' / 'lines 10-22'
        fts_rowid   INTEGER               -- rowid of the matching search_fts row (for re-index delete)
    );
    CREATE INDEX idx_chunks_document ON chunks(document_id);
    CREATE INDEX idx_chunks_project ON chunks(project_id);

    -- Brute-force vector store (always present; vec0 mirrors this when the extension loads).
    CREATE TABLE chunk_embeddings (
        chunk_id  TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
        embedding BLOB NOT NULL,          -- little-endian f32
        dim       INTEGER NOT NULL
    );

    -- FTS5 keyword index for hybrid retrieval.
    CREATE VIRTUAL TABLE search_fts USING fts5(body, kind UNINDEXED, ref_id UNINDEXED, project_id UNINDEXED);
    "#,
    // 0004 — Phase 1b: file revision log for revert/undo of write/destructive file ops.
    // `prior_content` is the pre-image (text) for edit/delete; `existed` records whether the
    // target existed before; `move_from` is the original path for moves.
    r#"
    CREATE TABLE file_revisions (
        id            TEXT PRIMARY KEY,
        session_id    TEXT,
        tool          TEXT NOT NULL,
        op            TEXT NOT NULL,        -- 'create' | 'edit' | 'delete' | 'move'
        path          TEXT NOT NULL,        -- the affected (destination) path
        prior_content TEXT,                 -- pre-image for edit/delete
        existed       INTEGER NOT NULL,     -- 1 if the path existed before the op
        move_from     TEXT,                 -- original path for moves
        created_at    INTEGER NOT NULL
    );

    CREATE INDEX idx_revisions_session ON file_revisions(session_id, created_at);
    "#,
    // 0006 — Phase 2b: file-backed Memory + Skills index tables (ADR-0006/0007). The Markdown
    // files on disk (MEMORY.md/USER.md, skills/<slug>.md) are the source of truth; these rows
    // index them for FTS recall via the existing `search_fts` table (kind='memory'/'skill').
    r#"
    CREATE TABLE memories (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL,
        kind        TEXT NOT NULL,        -- 'fact' (MEMORY.md) | 'user' (USER.md)
        title       TEXT NOT NULL,
        body        TEXT NOT NULL,
        source_file TEXT NOT NULL,        -- the backing file (MEMORY.md / USER.md)
        fts_rowid   INTEGER,              -- rowid of the matching search_fts row
        updated_at  INTEGER NOT NULL,
        UNIQUE(project_id, source_file, title)
    );
    CREATE INDEX idx_memories_project ON memories(project_id);

    CREATE TABLE skills (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL,
        slug        TEXT NOT NULL,
        name        TEXT NOT NULL,
        summary     TEXT NOT NULL,
        body        TEXT NOT NULL,
        source_file TEXT NOT NULL,        -- skills/<slug>.md
        fts_rowid   INTEGER,
        updated_at  INTEGER NOT NULL,
        UNIQUE(project_id, slug)
    );
    CREATE INDEX idx_skills_project ON skills(project_id);
    "#,
    // 0007 — Phase 2c: per-project enable/disable of built-in MCP servers (FR-19). Additive
    // override state, like folder_grants: an ABSENT row means the server is enabled, so existing
    // projects keep every server and no backfill is needed. Only `enabled = 0` rows suppress hosting.
    r#"
    CREATE TABLE project_extensions (
        project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        extension  TEXT NOT NULL,         -- 'files' | 'knowledge' | 'memory' | 'skills' | ...
        enabled    INTEGER NOT NULL,      -- 1 | 0
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (project_id, extension)
    );
    "#,
    // 0008 — Phase 3a: Study (flashcards + SM-2 review, FR-13/14). Unlike Memory/Skills these are
    // NOT file-backed: SM-2 scheduling state (ease factor, interval, repetitions, due date) is
    // structured data the agent/user don't hand-edit as Markdown, so the DB is the source of truth
    // (like documents/chunks). The agent (LLM) authors the cards and persists them via a gated tool.
    r#"
    CREATE TABLE decks (
        id            TEXT PRIMARY KEY,
        project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        name          TEXT NOT NULL,
        source_doc_id TEXT,                -- optional originating knowledge document
        created_at    INTEGER NOT NULL,
        UNIQUE(project_id, name)
    );
    CREATE INDEX idx_decks_project ON decks(project_id);

    CREATE TABLE cards (
        id           TEXT PRIMARY KEY,
        deck_id      TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
        project_id   TEXT NOT NULL,        -- denormalized for project-scoped due queries
        front        TEXT NOT NULL,
        back         TEXT NOT NULL,
        kind         TEXT NOT NULL,        -- 'qa' | 'cloze'
        ease_factor  REAL NOT NULL DEFAULT 2.5,
        interval_days INTEGER NOT NULL DEFAULT 0,
        repetitions  INTEGER NOT NULL DEFAULT 0,
        lapses       INTEGER NOT NULL DEFAULT 0,
        due_at       INTEGER NOT NULL,     -- epoch ms; new cards are due immediately
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_cards_deck ON cards(deck_id);
    CREATE INDEX idx_cards_due ON cards(project_id, due_at);
    "#,
    // 0009 — Phase 3b: adaptive study plans (FR-15). One active plan per project (UNIQUE), DB-owned
    // like the rest of the Study track: the agent authors a day-by-day plan (prioritizing weak decks
    // from the SM-2 review stats) and persists it; regenerating replaces it.
    r#"
    CREATE TABLE study_plans (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        title       TEXT NOT NULL,
        deadline_at INTEGER NOT NULL,     -- epoch ms of the target deadline
        body        TEXT NOT NULL,        -- agent-authored day-by-day markdown
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        UNIQUE(project_id)
    );
    "#,
    // 0010 — Phase 3c: Recipes (FR-16) — human-authored, parameterized automations. Like Skills, the
    // YAML file (`recipes/<name>.yaml`) is the source of truth; this table indexes them for listing.
    // Only metadata (name/title/description) is indexed — the full recipe (params/prompt) is read
    // from the file on demand.
    r#"
    CREATE TABLE recipes (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        name        TEXT NOT NULL,
        title       TEXT NOT NULL,
        description TEXT,
        source_file TEXT NOT NULL,        -- recipes/<name>.yaml
        updated_at  INTEGER NOT NULL,
        UNIQUE(project_id, name)
    );
    CREATE INDEX idx_recipes_project ON recipes(project_id);
    "#,
    // 0011 — Phase 3d: Scheduler (FR-17). A schedule fires a project recipe once at a time or on a
    // recurring cron expression. The DB owns the schedule + run history (ints/strings only); the cron
    // evaluation + the firing loop live in the daemon, so the lean core stays cron-free.
    r#"
    CREATE TABLE schedules (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        recipe_name TEXT NOT NULL,
        params      TEXT,                 -- JSON map of recipe param overrides
        kind        TEXT NOT NULL,        -- 'once' | 'cron'
        cron_expr   TEXT,                 -- when kind = 'cron'
        next_run_at INTEGER,              -- epoch ms of the next fire; NULL when done/disabled
        enabled     INTEGER NOT NULL,     -- 1 | 0
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE INDEX idx_schedules_due ON schedules(enabled, next_run_at);

    CREATE TABLE scheduled_runs (
        id          TEXT PRIMARY KEY,
        schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
        project_id  TEXT NOT NULL,
        started_at  INTEGER NOT NULL,
        status      TEXT NOT NULL,        -- 'ok' | 'error'
        session_id  TEXT,
        summary     TEXT,
        UNIQUE(schedule_id, started_at)
    );
    CREATE INDEX idx_scheduled_runs ON scheduled_runs(schedule_id, started_at);
    "#,
    // 0012 — Phase 3e: Outbound delivery (FR-27). Per-schedule delivery flags decide whether a
    // routine's output is pushed out after it runs: an on-device OS notification and/or an opt-in
    // email digest. Off by default (absent = 0). The SMTP transport + email config live in the
    // daemon (server-only `lettre` dep); the lean core only persists these two flags.
    r#"
    ALTER TABLE schedules ADD COLUMN deliver_notify INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE schedules ADD COLUMN deliver_email  INTEGER NOT NULL DEFAULT 0;
    "#,
    // 0013 — Phase 4a: Masters (persona-over-Skill, FR-39/46). Editable Markdown masters/<slug>.md is
    // the source of truth (like Skills); this table indexes the listing metadata (name/summary/model).
    // The full master (persona, allowed_tools, …) is read from the file. No FTS — list/get suffices in
    // this slice; the route_brief recall is deferred.
    r#"
    CREATE TABLE masters (
        id            TEXT PRIMARY KEY,
        project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        slug          TEXT NOT NULL,
        name          TEXT NOT NULL,
        summary       TEXT,
        default_model TEXT,
        source_file   TEXT NOT NULL,        -- masters/<slug>.md
        updated_at    INTEGER NOT NULL,
        UNIQUE(project_id, slug)
    );
    CREATE INDEX idx_masters_project ON masters(project_id);
    "#,
    // 0014 — Phase 4b: Master Teams + router (FR-38/40). A team is a group of masters + a coordinator
    // (the master that answers unaddressed briefs). DB-owned structured config (members as a JSON array
    // of master slugs — docs/05's master_team_members join, with stage/role, is illustrative and only
    // matters for the deferred sequential-chaining slice). The router (route_brief) ranks members.
    r#"
    CREATE TABLE master_teams (
        id               TEXT PRIMARY KEY,
        project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        slug             TEXT NOT NULL,
        name             TEXT NOT NULL,
        summary          TEXT,
        coordinator_slug TEXT,                 -- master slug that answers unaddressed briefs
        members          TEXT NOT NULL,        -- JSON array of master slugs
        created_at       INTEGER NOT NULL,
        updated_at       INTEGER NOT NULL,
        UNIQUE(project_id, slug)
    );
    CREATE INDEX idx_master_teams_project ON master_teams(project_id);
    "#,
    // 0015 — Phase 4c: multi-master group chat (ADR-0012, FR-43). Messages gain an author (`user` or
    // a master slug; null = derive from role) + addressed_to (JSON array of slugs / `["@all"]`), and a
    // session can be bound to a master team (a group chat). All nullable — ordinary chat is unaffected.
    r#"
    ALTER TABLE messages ADD COLUMN author       TEXT;
    ALTER TABLE messages ADD COLUMN addressed_to TEXT;
    ALTER TABLE sessions ADD COLUMN team_slug    TEXT;
    "#,
    // 0016 — Phase 4d: external MCP servers (ADR-0005, FR-20). A per-project stdio connector: the
    // command + args + env to spawn a third-party MCP server, hosted alongside the built-ins. Its tools
    // route through the same Permission & Audit gate (unknown tools default to Write → gated). Remote
    // (SSE/HTTP) transports are deferred. `args`/`env` are JSON, like the master_teams members column.
    r#"
    CREATE TABLE project_connectors (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        name        TEXT NOT NULL,        -- tool namespace prefix (e.g. 'filesystem')
        command     TEXT NOT NULL,        -- executable to spawn
        args        TEXT NOT NULL,        -- JSON array of arguments
        env         TEXT NOT NULL,        -- JSON object of env vars (the ONLY env the child sees)
        enabled     INTEGER NOT NULL DEFAULT 1,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        UNIQUE(project_id, name)
    );
    CREATE INDEX idx_project_connectors_project ON project_connectors(project_id);
    "#,
    // 0017 — Phase 4i: external master agents (ACP coding harnesses; ADR-0014). A master gains a
    // `backend` discriminator so listings can badge ACP masters without reading every file: "internal"
    // (the default persona-over-model master) or "acp" (an external ACP-compatible CLI Masters drives).
    // The master file stays the source of truth for the ACP launch config; this column is index-only.
    r#"
    ALTER TABLE masters ADD COLUMN backend TEXT NOT NULL DEFAULT 'internal';
    "#,
    // 0018 — Masters sidebar: standalone (global) masters that exist independent of any project
    // (extends ADR-0011, which scoped masters to projects). Mirrors `masters` minus `project_id`:
    // the file `<data_home>/masters/<slug>.md` is the source of truth, this table indexes the
    // listing metadata, keyed on `slug` alone (UNIQUE). Project masters (table `masters`) are
    // untouched. The user-starred default master + the system default project are plain `settings`
    // rows (no schema), used to back "quick chat".
    r#"
    CREATE TABLE global_masters (
        id            TEXT PRIMARY KEY,
        slug          TEXT NOT NULL UNIQUE,
        name          TEXT NOT NULL,
        summary       TEXT,
        default_model TEXT,
        source_file   TEXT NOT NULL,        -- masters/<slug>.md (under the data home)
        backend       TEXT NOT NULL DEFAULT 'internal',
        updated_at    INTEGER NOT NULL
    );
    "#,
    // 0019 — Cloud catalog sync: standalone (global) skills, the skills analogue of `global_masters`
    // (0018). System skills synced from the cloud catalog live independent of any project; the file
    // `<data_home>/skills/<slug>.md` is the source of truth and this table indexes the listing
    // metadata, keyed on `slug` alone (UNIQUE). Project skills (table `skills`) are untouched. No FTS
    // here — global skills are managed/synced content, not agent-recalled during a project run.
    r#"
    CREATE TABLE global_skills (
        id          TEXT PRIMARY KEY,
        slug        TEXT NOT NULL UNIQUE,
        name        TEXT NOT NULL,
        summary     TEXT,
        body        TEXT,
        source_file TEXT NOT NULL,          -- skills/<slug>.md (under the data home)
        updated_at  INTEGER NOT NULL
    );
    "#,
    // 0020 — session event log (the managed-agents "session = durable event log" slice): an
    // append-only record of a run's activity beyond the message transcript — tool calls/results,
    // approval requests + decisions, completion/errors. Written best-effort by the agent loop and
    // the permission gate; read via GET /sessions/{id}/events. Turn resume/wake builds on this
    // later; ints/strings only (lean core).
    r#"
    CREATE TABLE events (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        kind       TEXT NOT NULL,            -- tool_call | tool_result | approval_requested | approval_decided | complete | error
        payload    TEXT,                     -- JSON detail (redacted where applicable)
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_events_session ON events(session_id, created_at);
    "#,
    // 0021 — investing vertical: the asset lifecycle spine (ADR-0016). One `assets` table carries
    // an instrument through `watching → holding → sold` — the watchlist and the ledger are states
    // of the same row, not separate features. Slice 1 writes only `watching` plus the
    // point-in-time snapshot (price/date/reason at first interest, docs/11 D10); `positions` and
    // `txns` are schema'd now so the progressive-accumulation upgrade (V1) is a state transition,
    // not a migration. Ints/strings/reals only (lean core).
    r#"
    CREATE TABLE assets (
        id             TEXT PRIMARY KEY,
        project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        symbol         TEXT NOT NULL,              -- canonical, e.g. 'sh600519'
        name           TEXT NOT NULL,
        market         TEXT NOT NULL DEFAULT 'cn-a',
        kind           TEXT NOT NULL DEFAULT 'stock',    -- stock | fund
        state          TEXT NOT NULL DEFAULT 'watching', -- watching | holding | sold
        watch_reason   TEXT,                       -- why the user cared, extracted from conversation
        watched_at     INTEGER NOT NULL,           -- epoch ms of first interest
        snapshot_price REAL,                       -- close at watch time (nullable: partial data OK)
        snapshot_date  TEXT,                       -- 'YYYY-MM-DD' the snapshot price is for
        created_at     INTEGER NOT NULL,
        updated_at     INTEGER NOT NULL,
        UNIQUE(project_id, symbol)
    );
    CREATE INDEX idx_assets_project ON assets(project_id, state);
    CREATE TABLE positions (
        id         TEXT PRIMARY KEY,
        asset_id   TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
        quantity   REAL,                           -- all nullable: progressive accumulation
        cost       REAL,
        account    TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE txns (
        id         TEXT PRIMARY KEY,
        asset_id   TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
        kind       TEXT NOT NULL,                  -- buy | sell | dividend
        quantity   REAL,
        price      REAL,
        fee        REAL,
        traded_at  INTEGER,
        note       TEXT,
        created_at INTEGER NOT NULL
    );
    "#,
    // 0022 — market data cache with provenance (ADR-0017). Global, not project-scoped: a quote is
    // a fact about the market, shared by every project. Every row carries its source and fetch
    // time; `validation` starts life 'unverified' (single source) — dual-source cross-validation
    // ('verified'/'disputed') is the deferred upgrade. Never a fabricated number: the cache only
    // holds what an adapter actually returned.
    r#"
    CREATE TABLE price_cache (
        id         TEXT PRIMARY KEY,
        symbol     TEXT NOT NULL,
        market     TEXT NOT NULL,
        name       TEXT,
        trade_date TEXT NOT NULL,                  -- 'YYYY-MM-DD' the quote is for
        close      REAL,
        prev_close REAL,
        change_pct REAL,
        source     TEXT NOT NULL,                  -- adapter id, e.g. 'eastmoney'
        fetched_at INTEGER NOT NULL,               -- epoch ms
        validation TEXT NOT NULL DEFAULT 'unverified', -- unverified | verified | disputed
        UNIQUE(symbol, trade_date, source)
    );
    CREATE INDEX idx_price_cache_symbol ON price_cache(symbol, trade_date);
    "#,
];

/// Apply any pending migrations. Safe to call on every startup.
pub fn run(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);")?;

    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version > current {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
        }
    }
    Ok(())
}
