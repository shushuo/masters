//! **Simulation Investment Lab** (模拟投资实验室) — round orchestration.
//!
//! Masters compete in a forward-in-time paper simulation (inspired by Alpha Arena + the RETuning
//! paper): each round every participant is handed its virtual portfolio + a one-shot market
//! snapshot and answers with a RETuning-style reasoning + a target allocation; the deterministic
//! engine (`getmasters_core::simlab`) parses, constraint-checks, and rebalances at the round's
//! close prices. Money-math is never the LLM's (NFR-INV-1). Kept fully separate from the real
//! `assets` ledger (ADR-0016). Masters run **read-only** (existing `market.*`/`knowledge.*` tools)
//! — no new gated write-tool — so the permission surface is unchanged.
//!
//! The run path is shared by the manual "run a round" endpoint and the scheduler (a
//! `schedules.simulation_id` branch in `run_due`), mirroring `recipe::run_loaded`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::future::join_all;
use futures::StreamExt;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::{AbortHandle, JoinHandle};

use getmasters_core::agent::AgentEvent;
use getmasters_core::market::{normalize_symbol, QuoteView};

use crate::group::GroupStreamEvent;
use getmasters_core::simlab::{
    enforce_constraints, portfolio_nav, rebalance, return_pct, SimConstraints, BENCHMARK_SLUG,
};
use getmasters_core::store::{SimulationRow, Store};
use getmasters_proto::{
    SimConstraintsDto, SimDecisionDto, SimLeaderboardRowDto, SimRoundDto, SimRoundResultDto,
    SimulationDto,
};

fn fmt_pct(v: Option<f64>) -> String {
    v.map(|p| format!("{:+.2}%", p * 100.0)).unwrap_or_else(|| "—".into())
}

use crate::state::AppState;

/// The decision contract handed to every master (RETuning-informed structure + a machine-parseable
/// target block). Chinese, compliance-framed.
const DECISION_CONTRACT: &str = r#"请按以下结构思考并作答（这是模拟推演，非真实交易，不构成投资建议，不荐股）：

一、分析框架：基于基本面 / 消息面 / 宏观 / 技术面，独立搭建你自己的分析框架，不要盲从他人观点。
二、证据评分：对每个候选标的，分别列出「看涨证据」与「看跌证据」，并权衡相互矛盾的证据。
三、反思：指出你框架中最大的不确定性与风险点。
四、决策：给出本轮目标配置（每个标的占组合净值的百分比，其余自动视为现金），用如下代码块表达：

```目标配置
<股票代码>: <百分比>
<股票代码>: <百分比>
现金: <百分比>
```

规则：只能配置给定股票池内的标的；只做多（权重为正）；各标的权重合计不超过 100%。若本轮维持不动，也请输出与当前持仓一致的目标配置。"#;

/// Look-back window (days) for the per-round disclosure evidence.
const ANNOUNCE_DAYS: u32 = 14;
/// Cap on announcements per symbol injected into a brief (keeps briefs bounded).
const ANNOUNCE_PER_SYMBOL: usize = 3;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Decode the stored constraints JSON into the core engine's constraints.
fn to_core_constraints(dto: &SimConstraintsDto) -> SimConstraints {
    SimConstraints {
        long_only: dto.long_only,
        max_weight: dto.max_weight,
        cash_floor: dto.cash_floor,
        benchmark: dto
            .benchmark
            .as_deref()
            .and_then(normalize_symbol)
            .or_else(|| dto.benchmark.clone()),
        fee_bps: dto.fee_bps,
    }
}

fn constraints_of(sim: &SimulationRow) -> SimConstraintsDto {
    sim.constraints
        .as_deref()
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or_default()
}

fn universe_of(sim: &SimulationRow) -> Vec<String> {
    let raw: Vec<String> = serde_json::from_str(&sim.universe).unwrap_or_default();
    raw.iter().filter_map(|s| normalize_symbol(s)).collect()
}

/// Plain text of a stored assistant message (content-block JSON renders its Text blocks).
fn message_text(content: &str) -> String {
    match serde_json::from_str::<Vec<getmasters_core::provider::ContentBlock>>(content) {
        Ok(blocks) => blocks
            .into_iter()
            .filter_map(|b| match b {
                getmasters_core::provider::ContentBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => content.to_string(),
    }
}

/// A per-participant pre-round snapshot (positions + cash + valued NAV).
struct Snapshot {
    participant_id: String,
    slug: String,
    positions: BTreeMap<String, f64>,
    cash: f64,
    nav: f64,
}

/// Build the round brief for one participant.
fn build_brief(
    sim: &SimulationRow,
    cons: &SimConstraintsDto,
    snap: &Snapshot,
    universe: &[String],
    quotes: &BTreeMap<String, QuoteView>,
    news: &BTreeMap<String, Vec<getmasters_core::store::AnnouncementRow>>,
) -> String {
    let scenario = sim.scenario.as_deref().unwrap_or("（无特定情景）");
    let mut cons_line = format!("只做多={}", if cons.long_only { "是" } else { "否" });
    if let Some(w) = cons.max_weight {
        cons_line.push_str(&format!("，单标的上限={:.0}%", w * 100.0));
    }
    if let Some(f) = cons.cash_floor {
        cons_line.push_str(&format!("，现金下限={:.0}%", f * 100.0));
    }
    if cons.fee_bps > 0.0 {
        cons_line.push_str(&format!("，交易费={}bp", cons.fee_bps));
    }

    let mut holdings = String::new();
    if snap.positions.is_empty() {
        holdings.push_str("（当前空仓）\n");
    } else {
        for (sym, qty) in &snap.positions {
            let px = quotes
                .get(sym)
                .and_then(|q| q.row.close)
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".into());
            holdings.push_str(&format!("- {sym}: {qty:.2} 股 @ 现价 {px}\n"));
        }
    }

    let mut quote_table = String::new();
    for sym in universe {
        match quotes.get(sym) {
            Some(q) => {
                let close = q
                    .row
                    .close
                    .map(|c| format!("{c:.2}"))
                    .unwrap_or_else(|| "—".into());
                let chg = q
                    .row
                    .change_pct
                    .map(|c| format!("{c:+.2}%"))
                    .unwrap_or_default();
                let name = q.row.name.as_deref().unwrap_or("");
                let flag = if q.stale {
                    " ⚠数据陈旧"
                } else if q.row.validation == "disputed" {
                    " ⚠双源存疑"
                } else {
                    ""
                };
                quote_table.push_str(&format!("- {sym} {name}: 收盘 {close} {chg}{flag}\n"));
            }
            None => quote_table.push_str(&format!("- {sym}: 暂无行情（本轮无法交易该标的）\n")),
        }
    }

    // Recent disclosures as shared evidence (RETuning multi-source; same for every master → fair).
    let mut news_section = String::new();
    let has_news = universe.iter().any(|s| news.get(s).is_some_and(|l| !l.is_empty()));
    if has_news {
        news_section.push_str("## 近期公告（证据）\n");
        for sym in universe {
            if let Some(list) = news.get(sym) {
                for a in list {
                    news_section.push_str(&format!("- {sym} {}（{}）\n", a.title, a.ann_date));
                }
            }
        }
        news_section.push('\n');
    }

    format!(
        "# 模拟盘：{name}\n情景：{scenario}\n约束：{cons_line}\n\n\
         ## 你的当前组合（{slug}）\n现金：{cash:.2}\n持仓：\n{holdings}组合估值(NAV)：{nav:.2}\n\n\
         ## 本轮行情快照\n{quote_table}\n\
         {news_section}## 可投股票池\n{universe}\n\n{contract}",
        name = sim.name,
        slug = snap.slug,
        cash = snap.cash,
        nav = snap.nav,
        universe = universe.join("、"),
        contract = DECISION_CONTRACT,
    )
}

/// One participant's applied decision + resulting valuation (built during a round).
struct Applied {
    decision: SimDecisionDto,
    leaderboard: SimLeaderboardRowDto,
}

/// Run one decision round of a simulation. Concurrency-guarded (a manual click and a scheduler tick
/// can't run the same sim twice). Returns the round result (leaderboard + per-master decisions).
/// Used by the scheduler (which awaits the result for its digest); the HTTP path claims separately
/// and runs [`run_round_claimed`] in the background so a slow multi-master round never blocks the
/// request.
pub async fn run_round(state: &AppState, sim_id: &str) -> Result<SimRoundResultDto, String> {
    let store = state.agent.store().clone();
    let sim = store
        .get_simulation(sim_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "simulation not found".to_string())?;
    if sim.state == "ended" {
        return Err("simulation has ended".into());
    }
    if !store.claim_simulation(sim_id).map_err(|e| e.to_string())? {
        return Err("该模拟盘正在运行本轮，请稍候".into());
    }
    run_round_claimed(state, sim).await
}

/// Run a round for an **already-claimed** simulation (state == `running`), releasing state
/// afterward (→ `active` on error, round advanced on success). The HTTP handler claims then spawns
/// this on a task; `run_round` calls it after claiming inline.
pub async fn run_round_claimed(
    state: &AppState,
    sim: SimulationRow,
) -> Result<SimRoundResultDto, String> {
    let store = state.agent.store().clone();
    let outcome = run_round_inner(state, &store, &sim).await;
    match &outcome {
        Ok((round_no, _)) => {
            let _ = store.finish_simulation_round(&sim.id, *round_no);
        }
        Err(_) => {
            let _ = store.set_simulation_state(&sim.id, "active");
        }
    }
    outcome.map(|(_, result)| result)
}

/// One master's captured reply for the deterministic settle (raw reasoning text + provenance).
struct MasterReply {
    raw: String,
    session_id: Option<String>,
    tokens: Option<i64>,
}

/// Everything a round needs after the shared preparation (one fair snapshot + participant state).
struct RoundPrep {
    round_id: String,
    round_no: i64,
    snaps: Vec<Snapshot>,
    prices: BTreeMap<String, f64>,
    quotes: BTreeMap<String, QuoteView>,
    quote_date: Option<String>,
    universe: Vec<String>,
    cons: SimConstraints,
    cons_dto: SimConstraintsDto,
    news: BTreeMap<String, Vec<getmasters_core::store::AnnouncementRow>>,
}

/// Phases 1–2 (shared by the synchronous and streaming round paths): take one fair quote +
/// disclosure snapshot for the whole universe, insert the round row, and capture each participant's
/// pre-round portfolio state.
async fn prepare_round(
    state: &AppState,
    store: &Store,
    sim: &SimulationRow,
) -> Result<RoundPrep, String> {
    let universe = universe_of(sim);
    let cons_dto = constraints_of(sim);
    let cons = to_core_constraints(&cons_dto);

    let market = state.market_data(store.clone());
    let now = now_ms();
    let mut quotes: BTreeMap<String, QuoteView> = BTreeMap::new();
    let mut prices: BTreeMap<String, f64> = BTreeMap::new();
    let mut quote_date: Option<String> = None;
    let mut bench_universe = universe.clone();
    if let Some(b) = &cons.benchmark {
        if !bench_universe.contains(b) {
            bench_universe.push(b.clone());
        }
    }
    for sym in &bench_universe {
        if let Ok(v) = market.quote(sym, now).await {
            if let Some(c) = v.row.close {
                prices.insert(sym.clone(), c);
            }
            if quote_date.is_none() {
                quote_date = Some(v.row.trade_date.clone());
            }
            quotes.insert(sym.clone(), v);
        }
    }

    // Recent disclosures as shared evidence (RETuning multi-source); best-effort, fetched once per
    // round so every master weighs the same material. Capped per symbol to keep briefs bounded.
    let mut news: BTreeMap<String, Vec<getmasters_core::store::AnnouncementRow>> = BTreeMap::new();
    for sym in &universe {
        if let Ok(list) = market.announcements(sym, ANNOUNCE_DAYS, now).await {
            if !list.is_empty() {
                news.insert(sym.clone(), list.into_iter().take(ANNOUNCE_PER_SYMBOL).collect());
            }
        }
    }

    let round_no = sim.round_no + 1;
    let round_id = store
        .insert_sim_round(&sim.id, round_no, quote_date.as_deref(), "ok")
        .map_err(|e| e.to_string())?;

    let participants = store
        .list_sim_participants(&sim.id)
        .map_err(|e| e.to_string())?;
    let mut snaps: Vec<Snapshot> = Vec::new();
    for p in &participants {
        let positions: BTreeMap<String, f64> = store
            .list_sim_positions(&p.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| (r.symbol, r.quantity))
            .collect();
        let (nav, _) = portfolio_nav(p.cash, &positions, &prices);
        snaps.push(Snapshot {
            participant_id: p.id.clone(),
            slug: p.master_slug.clone(),
            positions,
            cash: p.cash,
            nav,
        });
    }

    Ok(RoundPrep {
        round_id,
        round_no,
        snaps,
        prices,
        quotes,
        quote_date,
        universe,
        cons,
        cons_dto,
        news,
    })
}

/// Phase 4 (shared): apply each participant's decision deterministically against `prep`, persist the
/// decisions + valuations, and build the round result. `replies` is keyed by snapshot index (the
/// benchmark participant has no entry — it's settled as a fixed buy-and-hold). Pure of the LLM: the
/// engine does the money-math (NFR-INV-1).
fn settle_round(
    store: &Store,
    sim: &SimulationRow,
    prep: &RoundPrep,
    mut replies: HashMap<usize, Result<MasterReply, String>>,
) -> (i64, SimRoundResultDto) {
    let RoundPrep {
        round_id,
        round_no,
        snaps,
        prices,
        universe,
        cons,
        quote_date,
        ..
    } = prep;
    let starting = sim.starting_cash;
    let mut applied: Vec<Applied> = Vec::new();
    for (i, snap) in snaps.iter().enumerate() {
        let is_bench = snap.slug == BENCHMARK_SLUG;

        let (targets_clean, parsed, reasoning, session_id, tokens, summary) = if is_bench {
            let mut t = BTreeMap::new();
            if let Some(b) = &cons.benchmark {
                if prices.contains_key(b) {
                    t.insert(b.clone(), 100.0);
                }
            }
            (t, true, None, None, None, Some("基准：买入持有".to_string()))
        } else {
            match replies.remove(&i) {
                Some(Ok(reply)) => match getmasters_core::simlab::parse_targets(&reply.raw) {
                    Some(raw_targets) => {
                        let (clean, dropped) = enforce_constraints(&raw_targets, universe, cons);
                        let mut summary = summarize_targets(&clean);
                        if !dropped.is_empty() {
                            summary.push_str(&format!("（已忽略池外/越界：{}）", dropped.join("、")));
                        }
                        (
                            clean,
                            true,
                            Some(reply.raw),
                            reply.session_id,
                            reply.tokens,
                            Some(summary),
                        )
                    }
                    None => (
                        BTreeMap::new(),
                        false,
                        Some(reply.raw),
                        reply.session_id,
                        reply.tokens,
                        Some("未能解析决策，本轮维持不动".to_string()),
                    ),
                },
                Some(Err(e)) => (
                    BTreeMap::new(),
                    false,
                    None,
                    None,
                    None,
                    Some(format!("运行失败，本轮维持不动：{e}")),
                ),
                None => (BTreeMap::new(), false, None, None, None, None),
            }
        };

        // Rebalance (parsed decision) or hold (unparsed/failed → keep positions & cash).
        let (new_positions, new_cash, unvalued) = if parsed
            && (!targets_clean.is_empty() || is_bench)
        {
            let r = rebalance(&snap.positions, snap.nav, &targets_clean, prices, cons.fee_bps);
            (r.positions, r.cash, r.unvalued_count)
        } else {
            let positions: Vec<(String, f64, Option<f64>)> = snap
                .positions
                .iter()
                .map(|(s, q)| (s.clone(), *q, None))
                .collect();
            let (_, unvalued) = portfolio_nav(snap.cash, &snap.positions, prices);
            (positions, snap.cash, unvalued)
        };

        let _ = store.replace_sim_positions(&snap.participant_id, &new_positions);
        let _ = store.set_sim_participant_cash(&snap.participant_id, new_cash);

        let post_map: BTreeMap<String, f64> = new_positions
            .iter()
            .map(|(s, q, _)| (s.clone(), *q))
            .collect();
        let (nav, _) = portfolio_nav(new_cash, &post_map, prices);
        let ret = return_pct(nav, starting);

        let targets_json = serde_json::to_string(&targets_clean).ok();
        let _ = store.insert_sim_decision(
            round_id,
            &snap.participant_id,
            session_id.as_deref(),
            targets_json.as_deref(),
            summary.as_deref(),
            reasoning.as_deref(),
            parsed,
            tokens,
        );
        let _ = store.insert_sim_valuation(
            round_id,
            &snap.participant_id,
            Some(nav),
            new_cash,
            ret,
            unvalued as i64,
        );

        applied.push(Applied {
            decision: SimDecisionDto {
                master_slug: snap.slug.clone(),
                targets: targets_clean.into_iter().collect(),
                summary,
                reasoning,
                session_id,
                parsed,
                nav: Some(nav),
                return_pct: ret,
                tokens,
            },
            leaderboard: SimLeaderboardRowDto {
                master_slug: snap.slug.clone(),
                nav: Some(nav),
                cash: new_cash,
                return_pct: ret,
                alpha: None,
                equity: Vec::new(),
                unvalued_count: unvalued as i64,
            },
        });
    }

    let mut leaderboard: Vec<SimLeaderboardRowDto> =
        applied.iter().map(|a| a.leaderboard.clone()).collect();
    leaderboard.sort_by(|a, b| {
        b.return_pct
            .unwrap_or(f64::MIN)
            .partial_cmp(&a.return_pct.unwrap_or(f64::MIN))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let decisions: Vec<SimDecisionDto> = applied.into_iter().map(|a| a.decision).collect();

    (
        *round_no,
        SimRoundResultDto {
            round_no: *round_no,
            quote_date: quote_date.clone(),
            leaderboard,
            decisions,
        },
    )
}

async fn run_round_inner(
    state: &AppState,
    store: &Store,
    sim: &SimulationRow,
) -> Result<(i64, SimRoundResultDto), String> {
    let prep = prepare_round(state, store, sim).await?;

    // Dispatch masters in parallel (the benchmark is deterministic, no LLM); settle afterwards.
    let briefs: Vec<(usize, String)> = prep
        .snaps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.slug != BENCHMARK_SLUG)
        .map(|(i, s)| {
            (
                i,
                build_brief(sim, &prep.cons_dto, s, &prep.universe, &prep.quotes, &prep.news),
            )
        })
        .collect();
    let runs = briefs.into_iter().map(|(i, brief)| {
        let slug = prep.snaps[i].slug.clone();
        let title = format!("sim:{}:{}", sim.id, slug);
        async move {
            let r = crate::master::run_titled(state, &sim.project_id, &slug, &brief, &title).await;
            (i, r)
        }
    });
    let replies: HashMap<usize, Result<MasterReply, String>> = join_all(runs)
        .await
        .into_iter()
        .map(|(i, r)| {
            let reply = r.map(|run| MasterReply {
                raw: message_text(&run.message.content),
                session_id: Some(run.session_id),
                tokens: run.message.token_usage,
            });
            (i, reply)
        })
        .collect();

    Ok(settle_round(store, sim, &prep, replies))
}

// --- Live streaming of a round (WS) --------------------------------------------

/// A live simulation round: a channel of group-shaped stream events (reusing [`GroupStreamEvent`]),
/// that closes when the round is fully settled, plus the in-flight task handles and the sim id so a
/// Stop/disconnect can abort them and recover the sim's `running` state.
pub struct SimStreamTurn {
    pub events: mpsc::UnboundedReceiver<GroupStreamEvent>,
    pub aborts: Arc<Mutex<Vec<AbortHandle>>>,
    pub simulation_id: String,
}

/// Start a streamed round: claim the sim, then spawn an orchestrator that streams each master's
/// reasoning into the channel and settles the round deterministically once they finish. Returns
/// immediately (the WS handler drives it). Mirrors [`crate::group::stream_post`].
pub async fn stream_round(state: &AppState, sim_id: &str) -> Result<SimStreamTurn, String> {
    let store = state.agent.store().clone();
    let sim = store
        .get_simulation(sim_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "simulation not found".to_string())?;
    if sim.state == "ended" {
        return Err("simulation has ended".into());
    }
    if !store.claim_simulation(sim_id).map_err(|e| e.to_string())? {
        return Err("该模拟盘正在运行本轮，请稍候".into());
    }

    let (tx, rx) = mpsc::unbounded_channel::<GroupStreamEvent>();
    let aborts: Arc<Mutex<Vec<AbortHandle>>> = Arc::new(Mutex::new(Vec::new()));
    let aborts_for_task = aborts.clone();
    let state = state.clone();
    let orchestrator = tokio::spawn(async move {
        run_round_streaming(state, sim, aborts_for_task, tx).await;
    });
    aborts.lock().unwrap().push(orchestrator.abort_handle());

    Ok(SimStreamTurn {
        events: rx,
        aborts,
        simulation_id: sim_id.to_string(),
    })
}

/// The streaming orchestrator: prepare the round, emit `RoundStart`, stream each non-benchmark
/// master in parallel into `tx`, await them all, then settle deterministically and release the sim.
/// Dropping `tx` at the end closes the stream (the WS then emits `GroupComplete`).
async fn run_round_streaming(
    state: AppState,
    sim: SimulationRow,
    aborts: Arc<Mutex<Vec<AbortHandle>>>,
    tx: UnboundedSender<GroupStreamEvent>,
) {
    let store = state.agent.store().clone();
    let prep = match prepare_round(&state, &store, &sim).await {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(GroupStreamEvent::Master {
                round: 1,
                author: "__sim__".into(),
                event: AgentEvent::Error(e),
            });
            let _ = store.set_simulation_state(&sim.id, "active");
            return;
        }
    };

    let addressed: Vec<String> = prep
        .snaps
        .iter()
        .filter(|s| s.slug != BENCHMARK_SLUG)
        .map(|s| s.slug.clone())
        .collect();
    let _ = tx.send(GroupStreamEvent::RoundStart { round: 1, addressed });

    let mut handles: Vec<(usize, JoinHandle<Result<MasterReply, String>>)> = Vec::new();
    for (i, snap) in prep.snaps.iter().enumerate() {
        if snap.slug == BENCHMARK_SLUG {
            continue;
        }
        let brief = build_brief(&sim, &prep.cons_dto, snap, &prep.universe, &prep.quotes, &prep.news);
        let state = state.clone();
        let project_id = sim.project_id.clone();
        let sim_id = sim.id.clone();
        let slug = snap.slug.clone();
        let tx = tx.clone();
        let handle =
            tokio::spawn(
                async move { stream_sim_master(&state, &project_id, &sim_id, &slug, &brief, tx).await },
            );
        aborts.lock().unwrap().push(handle.abort_handle());
        handles.push((i, handle));
    }

    let mut replies: HashMap<usize, Result<MasterReply, String>> = HashMap::new();
    for (i, handle) in handles {
        let r = handle
            .await
            .unwrap_or_else(|_| Err("master task aborted".into()));
        replies.insert(i, r);
    }

    let (round_no, _result) = settle_round(&store, &sim, &prep, replies);
    let _ = store.finish_simulation_round(&sim.id, round_no);
    // tx is dropped here → the channel closes → the WS emits GroupComplete.
}

/// Stream one master's reasoning for a sim round into `tx` (tagged `round = 1`, `author = slug`),
/// returning its captured reply for the deterministic settle. The `sim:<sid>:<slug>` session is the
/// durable record. Mirrors [`crate::group::stream_master`] without the group-transcript post-back.
async fn stream_sim_master(
    state: &AppState,
    project_id: &str,
    sim_id: &str,
    slug: &str,
    brief: &str,
    tx: UnboundedSender<GroupStreamEvent>,
) -> Result<MasterReply, String> {
    let send = |event: AgentEvent| {
        let _ = tx.send(GroupStreamEvent::Master {
            round: 1,
            author: slug.to_string(),
            event,
        });
    };

    let master = match crate::master::load_master_any(state, project_id, slug) {
        Ok(Some(m)) => m,
        Ok(None) => {
            let e = format!("master '{slug}' not found");
            send(AgentEvent::Error(e.clone()));
            return Err(e);
        }
        Err(e) => {
            send(AgentEvent::Error(e.clone()));
            return Err(e);
        }
    };

    let store = state.agent.store();
    let session = match store.create_session(Some(project_id), Some(&format!("sim:{sim_id}:{slug}"))) {
        Ok(s) => s,
        Err(e) => {
            let e = e.to_string();
            send(AgentEvent::Error(e.clone()));
            return Err(e);
        }
    };

    let mut stream = match crate::master::run_master_stream(
        state, project_id, &session.id, slug, &master, Some(brief), &[],
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            send(AgentEvent::Error(e.clone()));
            return Err(e);
        }
    };

    let mut reply: Option<MasterReply> = None;
    while let Some(ev) = stream.next().await {
        match ev {
            AgentEvent::Complete { message_id } => {
                let (raw, tokens) = store
                    .get_message(&message_id)
                    .map(|m| (message_text(&m.content), m.token_usage))
                    .unwrap_or_default();
                send(AgentEvent::Complete { message_id });
                reply = Some(MasterReply {
                    raw,
                    session_id: Some(session.id.clone()),
                    tokens,
                });
            }
            other => send(other),
        }
    }
    reply.ok_or_else(|| "master produced no reply".to_string())
}

/// A compact "sh600519 40% · sz000001 20%" summary of target weights.
fn summarize_targets(targets: &BTreeMap<String, f64>) -> String {
    if targets.is_empty() {
        return "本轮全部持有现金".to_string();
    }
    let mut parts: Vec<(String, f64)> = targets.iter().map(|(s, w)| (s.clone(), *w)).collect();
    parts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    parts
        .iter()
        .map(|(s, w)| format!("{s} {w:.0}%"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Read the current leaderboard (latest valuation + equity series per participant).
pub fn leaderboard(store: &Store, sim_id: &str) -> Result<Vec<SimLeaderboardRowDto>, String> {
    let sim = store
        .get_simulation(sim_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "simulation not found".to_string())?;
    let participants = store
        .list_sim_participants(sim_id)
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for p in &participants {
        let series = store
            .sim_valuation_series(&p.id)
            .map_err(|e| e.to_string())?;
        let equity: Vec<f64> = series
            .iter()
            .map(|(_, v)| v.return_pct.unwrap_or(0.0))
            .collect();
        let latest = series.last().map(|(_, v)| v.clone());
        rows.push(SimLeaderboardRowDto {
            master_slug: p.master_slug.clone(),
            nav: latest.as_ref().and_then(|v| v.nav).or(Some(sim.starting_cash)),
            cash: latest.as_ref().map(|v| v.cash).unwrap_or(p.cash),
            return_pct: latest.as_ref().and_then(|v| v.return_pct).or(Some(0.0)),
            alpha: None,
            equity,
            unvalued_count: latest.as_ref().map(|v| v.unvalued_count).unwrap_or(0),
        });
    }
    // Excess return over the benchmark line (if one is in the field): each master's return minus
    // the benchmark's. The benchmark row itself carries no alpha.
    let bench_return = rows
        .iter()
        .find(|r| r.master_slug == BENCHMARK_SLUG)
        .and_then(|r| r.return_pct);
    if let Some(bench) = bench_return {
        for r in &mut rows {
            if r.master_slug != BENCHMARK_SLUG {
                r.alpha = r.return_pct.map(|ret| ret - bench);
            }
        }
    }
    rows.sort_by(|a, b| {
        b.return_pct
            .unwrap_or(f64::MIN)
            .partial_cmp(&a.return_pct.unwrap_or(f64::MIN))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(rows)
}

/// Build the full `SimulationDto` (config + leaderboard + schedule).
pub fn to_dto(store: &Store, sim: &SimulationRow) -> Result<SimulationDto, String> {
    let participants = leaderboard(store, &sim.id)?;
    let schedule_cron = store
        .sim_schedule(&sim.id)
        .ok()
        .flatten()
        .and_then(|s| s.cron_expr);
    Ok(SimulationDto {
        id: sim.id.clone(),
        name: sim.name.clone(),
        scenario: sim.scenario.clone(),
        universe: universe_of(sim),
        starting_cash: sim.starting_cash,
        constraints: constraints_of(sim),
        state: sim.state.clone(),
        round_no: sim.round_no,
        created_at: sim.created_at,
        participants,
        schedule_cron,
    })
}

/// Build a round's detail DTO (decisions + reasoning read from each run session).
pub fn round_detail(store: &Store, round: &getmasters_core::store::SimRoundRow) -> SimRoundDto {
    let participants = store
        .list_sim_participants(&round.simulation_id)
        .unwrap_or_default();
    let slug_by_pid: HashMap<String, String> = participants
        .iter()
        .map(|p| (p.id.clone(), p.master_slug.clone()))
        .collect();
    let vals: HashMap<String, getmasters_core::store::SimValuationRow> = store
        .list_round_valuations(&round.id)
        .unwrap_or_default()
        .into_iter()
        .map(|v| (v.participant_id.clone(), v))
        .collect();

    let mut decisions = Vec::new();
    for d in store.list_round_decisions(&round.id).unwrap_or_default() {
        let slug = slug_by_pid
            .get(&d.participant_id)
            .cloned()
            .unwrap_or_else(|| d.participant_id.clone());
        let targets: HashMap<String, f64> = d
            .targets
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();
        let reasoning = d
            .session_id
            .as_deref()
            .and_then(|sid| store.list_messages(sid).ok())
            .and_then(|msgs| {
                msgs.into_iter()
                    .rev()
                    .find(|m| m.role == "assistant")
                    .map(|m| message_text(&m.content))
            })
            .or(d.raw.clone());
        let val = vals.get(&d.participant_id);
        decisions.push(SimDecisionDto {
            master_slug: slug,
            targets,
            summary: d.summary,
            reasoning,
            session_id: d.session_id,
            parsed: d.parsed,
            nav: val.and_then(|v| v.nav),
            return_pct: val.and_then(|v| v.return_pct),
            tokens: d.tokens,
        });
    }
    SimRoundDto {
        round_no: round.round_no,
        quote_date: round.quote_date.clone(),
        status: round.status.clone(),
        run_at: round.run_at,
        decisions,
    }
}

/// Assemble a human-readable Markdown report of a simulation: the given conditions, the final
/// leaderboard (with benchmark alpha), and every round's decisions + full RETuning reasoning.
/// This is the "把大师的思考和结果作为参考" deliverable — savable and shareable.
pub fn build_report(store: &Store, sim: &SimulationRow) -> Result<String, String> {
    let lb = leaderboard(store, &sim.id)?;
    let cons = constraints_of(sim);
    let mut rounds: Vec<SimRoundDto> = store
        .list_sim_rounds(&sim.id)
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| round_detail(store, r))
        .collect();
    rounds.reverse(); // oldest-first reads better in a report

    let mut md = String::new();
    md.push_str(&format!("# 模拟盘报告：{}\n\n", sim.name));
    if let Some(s) = &sim.scenario {
        md.push_str(&format!("**情景**：{s}\n\n"));
    }
    let mut cons_line = format!("只做多={}", if cons.long_only { "是" } else { "否" });
    if let Some(w) = cons.max_weight {
        cons_line.push_str(&format!("，单标的上限={:.0}%", w * 100.0));
    }
    if let Some(f) = cons.cash_floor {
        cons_line.push_str(&format!("，现金下限={:.0}%", f * 100.0));
    }
    if let Some(b) = &cons.benchmark {
        cons_line.push_str(&format!("，基准={b}"));
    }
    if cons.fee_bps > 0.0 {
        cons_line.push_str(&format!("，交易费={}bp", cons.fee_bps));
    }
    md.push_str(&format!(
        "**初始资金**：{:.0}　**已进行**：{} 轮　**股票池**：{}\n\n**约束**：{}\n\n",
        sim.starting_cash,
        sim.round_no,
        universe_of(sim).join("、"),
        cons_line
    ));

    md.push_str("## 排行榜\n\n| 排名 | 大师 | 累计收益 | 超额 | 净值 |\n|---|---|---|---|---|\n");
    for (i, r) in lb.iter().enumerate() {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            i + 1,
            r.master_slug,
            fmt_pct(r.return_pct),
            if r.master_slug == BENCHMARK_SLUG {
                "—".to_string()
            } else {
                fmt_pct(r.alpha)
            },
            r.nav.map(|n| format!("{n:.0}")).unwrap_or_else(|| "—".into()),
        ));
    }
    md.push('\n');

    for rd in &rounds {
        md.push_str(&format!("## 第 {} 轮", rd.round_no));
        if let Some(d) = &rd.quote_date {
            md.push_str(&format!("（行情 {d}）"));
        }
        md.push_str("\n\n");
        for dec in &rd.decisions {
            md.push_str(&format!(
                "### {} · 收益 {}\n\n",
                dec.master_slug,
                fmt_pct(dec.return_pct)
            ));
            if let Some(s) = &dec.summary {
                md.push_str(&format!("**决策**：{s}\n\n"));
            }
            if let Some(reason) = &dec.reasoning {
                md.push_str(reason);
                md.push_str("\n\n");
            }
        }
    }

    md.push_str("\n---\n模拟推演，非真实交易，不构成投资建议，不荐股。\n");
    Ok(md)
}
