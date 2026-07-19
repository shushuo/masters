//! Simulation Investment Lab (模拟投资实验室) — offline integration.
//!
//! Exercises the round loop end-to-end with the core `FixtureFetcher` + `MockProvider`: the
//! deterministic benchmark participant drives the rebalance/valuation path (no LLM), a master
//! participant exercises the reasoning-capture + hold-on-unparsed path, P&L accrues when the market
//! moves between rounds, and a `simulation_id` schedule fires a round through `scheduler::run_due`.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::market::testing::FixtureFetcher;
use getmasters_core::market::Announcement;
use getmasters_core::masters::Master;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::{PriceRow, Store};
use getmasters_proto::{
    CreateSimulationRequest, SimConstraintsDto, SimLeaderboardRowDto, SimRoundDto, SimulationDto,
};
use getmasters_server::{build_app, scheduler, AppState};

const TOKEN: &str = "simlab-token";

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-sim-{}", uuid::Uuid::new_v4()));
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

fn a_master(slug_hint: &str) -> Master {
    Master {
        name: slug_hint.into(),
        summary: "test trader".into(),
        persona: "你是一位模拟交易大师".into(),
        default_model: "mock:m".into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "learned".into(),
        body: "trade well".into(),
        backend: "internal".into(),
        acp: None,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// POST a round (202, background) then poll the sim detail until it settles back from `running`.
async fn run_round_and_wait(
    client: &reqwest::Client,
    base: &str,
    pid: &str,
    sid: &str,
) -> SimulationDto {
    let res = client
        .post(format!("{base}/projects/{pid}/simulations/{sid}/rounds"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202, "round accepted for background run");
    for _ in 0..200 {
        let sim: SimulationDto = client
            .get(format!("{base}/projects/{pid}/simulations/{sid}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if sim.state != "running" {
            return sim;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("round did not finish in time");
}

async fn latest_round(client: &reqwest::Client, base: &str, pid: &str, sid: &str) -> SimRoundDto {
    let rounds: Vec<SimRoundDto> = client
        .get(format!("{base}/projects/{pid}/simulations/{sid}/rounds"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    rounds.into_iter().next().expect("at least one round")
}

#[tokio::test]
async fn round_loop_benchmark_pnl_and_master_hold() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    // Seed a recent disclosure so we can assert it lands in the master's brief (RETuning evidence).
    let fetcher = Arc::new(
        FixtureFetcher::single("sh600519", "贵州茅台", 1700.0).with_announcements(
            "sh600519",
            vec![Announcement {
                ann_id: "a1".into(),
                symbol: "sh600519".into(),
                title: "2026年半年度报告".into(),
                ann_date: "2026-07-10".into(),
                ann_time: now_ms() - 24 * 60 * 60 * 1000,
                url: None,
                source: "cninfo".into(),
            }],
        ),
    );
    let state = state_with(&store, &dir, fetcher);

    let pid = store.create_project("sim-proj", None).unwrap();
    state
        .global_master_store()
        .create_with_slug("trader", &a_master("交易员"))
        .unwrap();

    let base = serve(state.clone()).await;
    let client = reqwest::Client::new();

    // Create a simulation: one master + an auto-added benchmark (sh600519 buy-and-hold).
    let body = CreateSimulationRequest {
        name: "熊市防御".into(),
        scenario: Some("只做沪深主板".into()),
        universe: vec!["600519".into()],
        starting_cash: 100_000.0,
        constraints: SimConstraintsDto {
            benchmark: Some("sh600519".into()),
            ..Default::default()
        },
        participants: vec!["trader".into()],
    };
    let sim: SimulationDto = client
        .post(format!("{base}/projects/{pid}/simulations"))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sim.round_no, 0);
    let slugs: Vec<&str> = sim
        .participants
        .iter()
        .map(|p| p.master_slug.as_str())
        .collect();
    assert!(slugs.contains(&"trader"));
    assert!(
        slugs.contains(&"__benchmark__"),
        "benchmark line auto-added"
    );
    let sid = sim.id.clone();

    // Round 1: at 1700 the benchmark is fully invested (return ~0), the master holds (echo → unparsed).
    let sim1 = run_round_and_wait(&client, &base, &pid, &sid).await;
    assert_eq!(sim1.round_no, 1);
    let r1 = latest_round(&client, &base, &pid, &sid).await;
    assert_eq!(r1.round_no, 1);
    let bench = r1
        .decisions
        .iter()
        .find(|d| d.master_slug == "__benchmark__")
        .unwrap();
    assert!(
        (bench.return_pct.unwrap()).abs() < 1e-6,
        "benchmark flat at entry"
    );
    let trader = r1
        .decisions
        .iter()
        .find(|d| d.master_slug == "trader")
        .unwrap();
    assert!(!trader.parsed, "echo reply is unparseable → held");
    let reasoning = trader.reasoning.as_deref().unwrap_or("");
    assert!(
        reasoning.contains("模拟盘"),
        "the master's reasoning (echoed brief) was captured"
    );
    assert!(
        reasoning.contains("2026年半年度报告"),
        "the recent disclosure was injected as evidence into the brief"
    );

    // The market moves +10%: insert a later-dated quote so round 2 marks to 1870.
    store
        .insert_price(&PriceRow {
            symbol: "sh600519".into(),
            market: "cn-a".into(),
            name: Some("贵州茅台".into()),
            trade_date: "2026-07-16".into(),
            close: Some(1870.0),
            prev_close: Some(1700.0),
            change_pct: Some(10.0),
            source: "fixture".into(),
            fetched_at: now_ms(),
            validation: "unverified".into(),
        })
        .unwrap();

    // Round 2: benchmark rides the move to +10%; the master (all cash) stays flat.
    let sim2 = run_round_and_wait(&client, &base, &pid, &sid).await;
    assert_eq!(sim2.round_no, 2);
    let r2 = latest_round(&client, &base, &pid, &sid).await;
    assert_eq!(r2.round_no, 2);
    let bench2 = r2
        .decisions
        .iter()
        .find(|d| d.master_slug == "__benchmark__")
        .unwrap();
    assert!(
        (bench2.return_pct.unwrap() - 0.10).abs() < 1e-3,
        "benchmark ~ +10%, got {:?}",
        bench2.return_pct
    );

    // Leaderboard: benchmark ranks first, with a 2-point equity series.
    let lb: Vec<SimLeaderboardRowDto> = client
        .get(format!(
            "{base}/projects/{pid}/simulations/{sid}/leaderboard"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(lb[0].master_slug, "__benchmark__");
    assert_eq!(lb[0].equity.len(), 2);

    // Round history carries per-master reasoning.
    let rounds: Vec<SimRoundDto> = client
        .get(format!("{base}/projects/{pid}/simulations/{sid}/rounds"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].round_no, 2, "newest first");

    // Pause → a new round can't start (claim only claims `active`); resume restores it.
    let paused: SimulationDto = client
        .put(format!(
            "{base}/projects/{pid}/simulations/{sid}/state/paused"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(paused.state, "paused");
    let blocked = client
        .post(format!("{base}/projects/{pid}/simulations/{sid}/rounds"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), 409, "paused sim rejects a new round");
    let resumed: SimulationDto = client
        .put(format!(
            "{base}/projects/{pid}/simulations/{sid}/state/active"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resumed.state, "active");

    // Benchmark alpha: the trader (all cash, flat) trails the +10% benchmark by ~ -10%.
    let after_lb: Vec<SimLeaderboardRowDto> = client
        .get(format!(
            "{base}/projects/{pid}/simulations/{sid}/leaderboard"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let trader_row = after_lb.iter().find(|r| r.master_slug == "trader").unwrap();
    assert!(
        (trader_row.alpha.unwrap() + 0.10).abs() < 1e-3,
        "trader alpha ~ -10% vs benchmark, got {:?}",
        trader_row.alpha
    );

    // Report: a Markdown export carrying the conditions, leaderboard, and each round's reasoning.
    let report: getmasters_proto::SimReportDto = client
        .get(format!("{base}/projects/{pid}/simulations/{sid}/report"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        report.markdown.contains("熊市防御"),
        "report has the sim name"
    );
    assert!(
        report.markdown.contains("## 排行榜"),
        "report has the leaderboard"
    );
    assert!(
        report.markdown.contains("第 1 轮"),
        "report has round detail"
    );
    assert!(
        report.markdown.contains("2026年半年度报告"),
        "report carries the master reasoning (which echoed the evidence)"
    );

    // Reset: back to round 0 under the same conditions (config + participants kept).
    let reset: SimulationDto = client
        .post(format!("{base}/projects/{pid}/simulations/{sid}/reset"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reset.round_no, 0);
    assert_eq!(reset.state, "active");
    assert_eq!(reset.participants.len(), 2, "participants kept");
    let rounds_after: Vec<SimRoundDto> = client
        .get(format!("{base}/projects/{pid}/simulations/{sid}/rounds"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(rounds_after.is_empty(), "rounds cleared on reset");
}

#[tokio::test]
async fn streaming_round_emits_events_and_settles() {
    use getmasters_core::agent::AgentEvent;
    use getmasters_server::group::GroupStreamEvent;

    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
    let state = state_with(&store, &dir, fetcher);
    let pid = store.create_project("sim-proj", None).unwrap();
    state
        .global_master_store()
        .create_with_slug("trader", &a_master("交易员"))
        .unwrap();

    let universe = serde_json::to_string(&vec!["sh600519"]).unwrap();
    let sid = store
        .create_simulation(&pid, "流式盘", None, &universe, 100_000.0, None)
        .unwrap();
    store
        .add_sim_participant(&sid, "trader", 100_000.0)
        .unwrap();

    // Drive the streaming orchestrator directly (no real WebSocket needed) and drain its channel.
    let mut turn = getmasters_server::simlab::stream_round(&state, &sid)
        .await
        .unwrap();
    let mut saw_round_start = false;
    let mut saw_master_delta = false;
    let mut saw_master_complete = false;
    while let Some(ev) = turn.events.recv().await {
        match ev {
            GroupStreamEvent::RoundStart { round, addressed } => {
                assert_eq!(round, 1);
                assert!(addressed.contains(&"trader".to_string()));
                saw_round_start = true;
            }
            GroupStreamEvent::Master { author, event, .. } => {
                assert_eq!(author, "trader");
                match event {
                    AgentEvent::Delta(_) => saw_master_delta = true,
                    AgentEvent::Complete { .. } => saw_master_complete = true,
                    _ => {}
                }
            }
        }
    }
    assert!(saw_round_start, "emitted RoundStart");
    assert!(saw_master_delta, "streamed the master's reasoning tokens");
    assert!(saw_master_complete, "emitted the master's completion");

    // The channel closing means the round settled: round advanced + state released.
    let sim = store.get_simulation(&sid).unwrap().unwrap();
    assert_eq!(sim.round_no, 1);
    assert_eq!(sim.state, "active");
    let rounds = store.list_sim_rounds(&sid).unwrap();
    assert_eq!(rounds.len(), 1);
    assert_eq!(store.list_round_decisions(&rounds[0].id).unwrap().len(), 1);
}

#[tokio::test]
async fn scheduled_round_fires_through_run_due() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
    let state = state_with(&store, &dir, fetcher);
    let pid = store.create_project("sim-proj", None).unwrap();

    let universe = serde_json::to_string(&vec!["sh600519"]).unwrap();
    let constraints = serde_json::to_string(&SimConstraintsDto {
        benchmark: Some("sh600519".into()),
        ..Default::default()
    })
    .unwrap();
    let sid = store
        .create_simulation(
            &pid,
            "定投模拟",
            None,
            &universe,
            100_000.0,
            Some(&constraints),
        )
        .unwrap();
    store
        .add_sim_participant(&sid, "__benchmark__", 100_000.0)
        .unwrap();

    // A cron schedule already due (next_run_at in the past) → run_due fires exactly one round.
    store
        .create_sim_schedule(
            &pid,
            &sid,
            "cron",
            Some("0 0 * * *"),
            Some(now_ms() - 1000),
            false,
            false,
        )
        .unwrap();
    scheduler::run_due(&state, now_ms()).await;

    let rounds = store.list_sim_rounds(&sid).unwrap();
    assert_eq!(rounds.len(), 1, "the scheduler ran one sim round");
    // The schedule advanced to a future occurrence (not stuck re-firing).
    let sched = store.sim_schedule(&sid).unwrap().unwrap();
    assert!(sched.next_run_at.unwrap() > now_ms());
}
