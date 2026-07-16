//! Investing vertical slice 1 (docs/11, ADR-0015..0017) — the ask→track closed loop:
//! idempotent workspace seeding, the assets list/untrack HTTP surface, cached quotes with
//! provenance, and the D8 silent-tracking dispatch (a group master calling `assets.track_asset`
//! headlessly through the gate). All offline: the core `FixtureFetcher` + `MockProvider`.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::market::testing::FixtureFetcher;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{AssetDto, InvestingWorkspaceDto, QuoteDto};
use getmasters_server::{build_app, group, AppState};

const TOKEN: &str = "investing-token";

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-inv-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn state_with(store: &Store, dir: &std::path::Path, fetcher: Arc<FixtureFetcher>) -> AppState {
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    AppState::new(agent, TOKEN.to_string())
        .with_config(cfg)
        .with_market_fetcher(fetcher)
}

async fn serve(state: AppState) -> String {
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn workspace_seeds_idempotently_and_respects_user_masters() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
    let state = state_with(&store, &dir, fetcher);

    // A user-authored master already squatting the `risk` slug must never be clobbered.
    let global = state.global_master_store();
    global
        .create_with_slug(
            "risk",
            &Master {
                name: "My Risk".into(),
                summary: "user-owned".into(),
                persona: "mine".into(),
                default_model: "mock:m".into(),
                allowed_skills: vec![],
                allowed_tools: vec![],
                output_contract: String::new(),
                origin: "learned".into(),
                body: "user content".into(),
                backend: "internal".into(),
                acp: None,
            },
        )
        .unwrap();

    let base = serve(state.clone()).await;
    let client = reqwest::Client::new();

    let ws: InvestingWorkspaceDto = client
        .post(format!("{base}/investing/workspace"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ws.team_slug, "investing");
    assert_eq!(ws.coordinator, "chief");
    assert_eq!(ws.members, vec!["chief", "analyst", "risk", "coach"]);

    // Seeded masters exist globally (system origin), the user-owned one is untouched.
    let chief = global.load("chief").unwrap().unwrap();
    assert_eq!(chief.origin, "system");
    assert!(chief.body.contains("【合规边界（不可违反）】"));
    let risk = global.load("risk").unwrap().unwrap();
    assert_eq!(risk.origin, "learned");
    assert_eq!(risk.body, "user content");

    // The standing team + the compliance instructions (written only when empty).
    let team = store
        .get_team(&ws.project_id, "investing")
        .unwrap()
        .unwrap();
    assert_eq!(team.coordinator_slug, "chief");
    let instructions = store
        .project_instructions(&ws.project_id)
        .unwrap()
        .unwrap_or_default();
    assert!(instructions.contains("不构成投资建议"));

    // Second call: idempotent — same workspace, instructions not duplicated/overwritten.
    store
        .set_project_instructions(&ws.project_id, "user edited")
        .unwrap();
    let ws2: InvestingWorkspaceDto = client
        .post(format!("{base}/investing/workspace"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ws2.project_id, ws.project_id);
    assert_eq!(
        store
            .project_instructions(&ws.project_id)
            .unwrap()
            .as_deref(),
        Some("user edited"),
        "non-empty instructions are user content and must never be overwritten"
    );

    // Proactive-touch recipes + schedules seeded once — the second call added no duplicates.
    let schedules = store.list_schedules(&ws.project_id).unwrap();
    let touch: Vec<&str> = schedules.iter().map(|s| s.recipe_name.as_str()).collect();
    assert_eq!(schedules.len(), 3, "{touch:?}");
    assert!(touch.contains(&"weekly-watch-digest"));
    assert!(touch.contains(&"watch-mover-sentinel"));
    assert!(touch.contains(&"earnings-sentinel"));
    for s in &schedules {
        assert!(s.deliver_notify, "touch schedules deliver via notify");
        assert!(!s.deliver_email, "email stays opt-in");
        assert!(s.next_run_at.is_some(), "cron schedules get a first fire");
    }
}

#[tokio::test]
async fn briefings_feed_hides_silent_and_failed_runs() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::new(vec![]));
    let state = state_with(&store, &dir, fetcher);
    let pid = store.create_project("inv", None).unwrap();

    // A schedule to hang runs off (the feed joins runs → schedules for the recipe name).
    let sid = store
        .create_schedule(
            &pid,
            "weekly-watch-digest",
            "{}",
            "cron",
            Some("0 12 * * SUN"),
            Some(1),
            true,
            false,
        )
        .unwrap();

    // One real briefing: a run session whose final assistant message is the report body.
    let real = store
        .create_session(Some(&pid), Some("recipe:weekly-watch-digest"))
        .unwrap();
    store.insert_message(&real.id, "user", "prompt").unwrap();
    store
        .insert_message(&real.id, "assistant", "## 本周关注周报\n一切正常。")
        .unwrap();
    store
        .record_scheduled_run(&sid, &pid, "ok", Some(&real.id), Some("本周关注周报"))
        .unwrap();

    // A silent pass (NO_ALERT) and a failed run — both must be hidden from the feed.
    let silent = store
        .create_session(Some(&pid), Some("recipe:weekly-watch-digest"))
        .unwrap();
    store.insert_message(&silent.id, "user", "prompt").unwrap();
    store
        .insert_message(&silent.id, "assistant", "NO_ALERT")
        .unwrap();
    // scheduled_runs is UNIQUE(schedule_id, started_at) — space the records out.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    store
        .record_scheduled_run(&sid, &pid, "ok", Some(&silent.id), Some("NO_ALERT"))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    store
        .record_scheduled_run(&sid, &pid, "error", None, Some("boom"))
        .unwrap();

    let base = serve(state).await;
    let client = reqwest::Client::new();
    let briefings: Vec<getmasters_proto::BriefingDto> = client
        .get(format!("{base}/projects/{pid}/briefings"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(briefings.len(), 1);
    assert_eq!(briefings[0].recipe_name, "weekly-watch-digest");
    assert!(briefings[0].body.contains("本周关注周报"));
    // No recipe file on disk in this test — the title falls back to the slug.
    assert_eq!(briefings[0].title, "weekly-watch-digest");
}

#[tokio::test]
async fn assets_list_and_untrack_roundtrip() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::new(vec![]));
    let state = state_with(&store, &dir, fetcher);
    let pid = store.create_project("inv", None).unwrap();
    store
        .upsert_asset_watch(
            &pid,
            "sh600519",
            "贵州茅台",
            "cn-a",
            "stock",
            Some("test"),
            Some(1700.0),
            Some("2026-07-15"),
            1_000,
        )
        .unwrap();

    let base = serve(state).await;
    let client = reqwest::Client::new();

    let assets: Vec<AssetDto> = client
        .get(format!("{base}/projects/{pid}/assets"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].symbol, "sh600519");
    assert_eq!(assets[0].state, "watching");
    assert_eq!(assets[0].snapshot_price, Some(1700.0));

    // Untrack → 204; again → 404.
    let res = client
        .delete(format!("{base}/projects/{pid}/assets/sh600519"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);
    let res = client
        .delete(format!("{base}/projects/{pid}/assets/sh600519"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);

    // A holding is lifecycle-guarded → 409.
    store
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
    store
        .with_conn(|conn| {
            conn.execute(
                "UPDATE assets SET state = 'holding' WHERE symbol = 'sz000001'",
                [],
            )
        })
        .unwrap();
    let res = client
        .delete(format!("{base}/projects/{pid}/assets/sz000001"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);
}

#[tokio::test]
async fn quotes_come_with_provenance_and_hit_the_cache() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
    let state = state_with(&store, &dir, fetcher.clone());
    let pid = store.create_project("inv", None).unwrap();

    let base = serve(state).await;
    let client = reqwest::Client::new();
    let url = format!("{base}/projects/{pid}/quotes?symbols=sh600519,sz999999");

    let quotes: Vec<QuoteDto> = client
        .get(&url)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // The unknown symbol degrades to omission — never a fabricated number, never a 500.
    assert_eq!(quotes.len(), 1);
    assert_eq!(quotes[0].symbol, "sh600519");
    assert_eq!(quotes[0].close, Some(1700.0));
    assert_eq!(quotes[0].source, "fixture");
    assert_eq!(quotes[0].validation, "unverified");
    assert!(!quotes[0].stale);

    // Second request is served from the cache: the upstream saw exactly one quote fetch
    // (plus one failed fetch for the unknown symbol on each pass).
    let quotes2: Vec<QuoteDto> = client
        .get(&url)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(quotes2.len(), 1);
    let calls = fetcher.call_count();
    assert_eq!(
        calls, 3,
        "known symbol fetched once (cached on the 2nd pass); unknown symbol retried each pass"
    );
}

/// The D8 closed loop: a group master's persona drives `assets.track_asset` during its answer;
/// dispatch is headless (`without_approval`) so the Write executes silently — but through the
/// gate — and the asset row lands.
#[tokio::test]
async fn group_master_tracks_asset_silently() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
    let state = state_with(&store, &dir, fetcher);
    let pid = store.create_project("inv", None).unwrap();

    // One test master on the mock model (the seeded pack's personas are asserted in
    // master_templates tests; here we exercise the dispatch mechanics deterministically).
    let ms = MasterStore::new(state.project_dir(&pid), pid.clone(), store.clone());
    ms.create(&Master {
        name: "Analyst".into(),
        summary: "answers with a tracked asset".into(),
        persona: "You are the analyst.".into(),
        default_model: "mock:m".into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "learned".into(),
        body: String::new(),
        backend: "internal".into(),
        acp: None,
    })
    .unwrap();
    store
        .upsert_team(
            &pid,
            "squad",
            "Squad",
            "",
            "analyst",
            &["analyst".to_string()],
        )
        .unwrap();

    let session = group::start(&state, &pid, "squad", None).unwrap();
    // The MockProvider sentinel drives the deterministic tool exchange through the real gate.
    let res = group::post(
        &state,
        &session.id,
        "怎么看茅台？ [[tool:assets.track_asset|symbol=600519|name=贵州茅台|reason=用户询问茅台]]",
        None,
    )
    .await
    .unwrap();
    assert_eq!(res.replies.len(), 1, "errors: {:?}", res.errors);

    // The silent track landed: watching, canonical symbol, with the extracted reason.
    let assets = store.list_assets(&pid, Some("watching")).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].symbol, "sh600519");
    assert_eq!(assets[0].watch_reason.as_deref(), Some("用户询问茅台"));

    // And the group transcript stayed clean (user + the one attributed reply).
    let transcript = store.list_messages(&session.id).unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[1].author, "analyst");
}
