//! Simulation Investment Lab routes (模拟投资实验室): CRUD, run-a-round, round history,
//! leaderboard, and the auto-round schedule. The round run goes through `crate::simlab::run_round`
//! (shared with the scheduler). Management-screen semantics (direct store calls); masters run
//! read-only inside the round engine, so there's no new gated-tool surface here.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{
    CreateSimulationRequest, SetSimScheduleRequest, SimLeaderboardRowDto, SimRoundDto, SimulationDto,
};

use getmasters_core::simlab::BENCHMARK_SLUG;

use crate::state::{AppError, AppState};

/// Cap on rounds returned by the history endpoint.
const MAX_ROUNDS: usize = 100;

/// Load a simulation and assert it belongs to the project (404 otherwise).
fn load_owned(
    state: &AppState,
    project_id: &str,
    sim_id: &str,
) -> Result<getmasters_core::store::SimulationRow, AppError> {
    let store = state.agent.store();
    store.get_project(project_id)?;
    let sim = store
        .get_simulation(sim_id)?
        .filter(|s| s.project_id == project_id)
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "simulation not found"))?;
    Ok(sim)
}

#[utoipa::path(
    post,
    path = "/projects/{id}/simulations",
    operation_id = "create_simulation",
    params(("id" = String, Path, description = "Project id")),
    request_body = CreateSimulationRequest,
    responses(
        (status = 200, description = "The created simulation", body = SimulationDto),
        (status = 400, description = "No valid universe/participants")
    ),
    tag = "simlab"
)]
pub async fn create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateSimulationRequest>,
) -> Result<Json<SimulationDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;

    // Normalize the universe; reject an empty one.
    let universe: Vec<String> = body
        .universe
        .iter()
        .filter_map(|s| getmasters_core::market::normalize_symbol(s))
        .collect();
    if universe.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "股票池为空或均非有效 A 股代码",
        ));
    }
    if body.starting_cash <= 0.0 {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "初始资金需大于 0"));
    }

    // Validate participant masters exist (project store → global fallback).
    let mut participants: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for slug in &body.participants {
        match crate::master::load_master_any(&state, &id, slug) {
            Ok(Some(_)) => participants.push(slug.clone()),
            _ => missing.push(slug.clone()),
        }
    }
    if participants.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            format!("没有有效的参赛大师（未找到：{}）", missing.join("、")),
        ));
    }

    let universe_json = serde_json::to_string(&universe).unwrap_or_else(|_| "[]".into());
    let constraints_json = serde_json::to_string(&body.constraints).ok();

    let sim_id = store.create_simulation(
        &id,
        &body.name,
        body.scenario.as_deref(),
        &universe_json,
        body.starting_cash,
        constraints_json.as_deref(),
    )?;

    for slug in &participants {
        store.add_sim_participant(&sim_id, slug, body.starting_cash)?;
    }
    // A benchmark line (fixed buy-and-hold) is added automatically when configured.
    if body
        .constraints
        .benchmark
        .as_deref()
        .and_then(getmasters_core::market::normalize_symbol)
        .is_some()
    {
        store.add_sim_participant(&sim_id, BENCHMARK_SLUG, body.starting_cash)?;
    }

    let sim = store
        .get_simulation(&sim_id)?
        .ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "created sim vanished"))?;
    crate::simlab::to_dto(store, &sim)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/simulations",
    operation_id = "list_simulations",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's simulations (newest first)", body = [SimulationDto])),
    tag = "simlab"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SimulationDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let mut out = Vec::new();
    for sim in store.list_simulations(&id)? {
        out.push(
            crate::simlab::to_dto(store, &sim)
                .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?,
        );
    }
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/simulations/{sid}",
    operation_id = "get_simulation",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses((status = 200, description = "The simulation with its leaderboard", body = SimulationDto)),
    tag = "simlab"
)]
pub async fn get(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Json<SimulationDto>, AppError> {
    let sim = load_owned(&state, &id, &sid)?;
    crate::simlab::to_dto(state.agent.store(), &sim)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/simulations/{sid}",
    operation_id = "delete_simulation",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses((status = 204, description = "Deleted")),
    tag = "simlab"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    load_owned(&state, &id, &sid)?;
    let store = state.agent.store();
    // Sweep the per-round master run sessions (`sim:<sid>:<slug>`) so they don't linger after the
    // simulation (and its decisions/valuations, which cascade) is gone.
    if let Ok(sessions) = store.session_ids_titled_like(&format!("sim:{sid}:%")) {
        for s in &sessions {
            let _ = store.delete_session(s);
        }
    }
    store.delete_simulation(&sid)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/projects/{id}/simulations/{sid}/rounds",
    operation_id = "run_simulation_round",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses(
        (status = 202, description = "Round started in the background; poll GET .../{sid} until state is not 'running'"),
        (status = 400, description = "The simulation has ended"),
        (status = 409, description = "A round is already running")
    ),
    tag = "simlab"
)]
pub async fn run_round(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let sim = load_owned(&state, &id, &sid)?;
    if sim.state == "ended" {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "该模拟盘已结束"));
    }
    // Claim synchronously so a busy sim gets an immediate 409; then run the (possibly slow,
    // multi-master) round on a background task so the request never blocks or times out. Results
    // land in the DB as they complete; the UI polls GET .../{sid}.
    if !state.agent.store().claim_simulation(&sid)? {
        return Err(AppError::new(StatusCode::CONFLICT, "该模拟盘正在运行本轮，请稍候"));
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::simlab::run_round_claimed(&st, sim).await {
            tracing::warn!(simulation = %sid, error = %e, "sim round failed");
        }
    });
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(
    put,
    path = "/projects/{id}/simulations/{sid}/state/{state}",
    operation_id = "set_simulation_state",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id"),
        ("state" = String, Path, description = "active | paused | ended")
    ),
    responses(
        (status = 200, description = "The updated simulation", body = SimulationDto),
        (status = 400, description = "Invalid or non-transitionable state")
    ),
    tag = "simlab"
)]
pub async fn set_state(
    State(app): State<AppState>,
    Path((id, sid, target)): Path<(String, String, String)>,
) -> Result<Json<SimulationDto>, AppError> {
    let sim = load_owned(&app, &id, &sid)?;
    if !matches!(target.as_str(), "active" | "paused" | "ended") {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "无效的状态"));
    }
    // Don't stomp a round in flight (state == 'running'); pause/resume/end operate on a settled sim.
    if sim.state == "running" {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "本轮进行中，请稍候再更改状态",
        ));
    }
    app.agent.store().set_simulation_state(&sid, &target)?;
    let sim = app
        .agent
        .store()
        .get_simulation(&sid)?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "simulation not found"))?;
    crate::simlab::to_dto(app.agent.store(), &sim)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[utoipa::path(
    post,
    path = "/projects/{id}/simulations/{sid}/reset",
    operation_id = "reset_simulation",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses(
        (status = 200, description = "The simulation reset to round 0 (config kept)", body = SimulationDto),
        (status = 409, description = "A round is in flight")
    ),
    tag = "simlab"
)]
pub async fn reset(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Json<SimulationDto>, AppError> {
    let sim = load_owned(&state, &id, &sid)?;
    if sim.state == "running" {
        return Err(AppError::new(StatusCode::CONFLICT, "本轮进行中，请稍候再重置"));
    }
    let store = state.agent.store();
    // Rounds cascade their decisions/valuations; also sweep the per-round run sessions.
    if let Ok(sessions) = store.session_ids_titled_like(&format!("sim:{sid}:%")) {
        for s in &sessions {
            let _ = store.delete_session(s);
        }
    }
    store.reset_simulation(&sid)?;
    let sim = store
        .get_simulation(&sid)?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "simulation not found"))?;
    crate::simlab::to_dto(store, &sim)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/simulations/{sid}/rounds",
    operation_id = "list_simulation_rounds",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses((status = 200, description = "Round history with per-master decisions + reasoning, newest first", body = [SimRoundDto])),
    tag = "simlab"
)]
pub async fn rounds(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Json<Vec<SimRoundDto>>, AppError> {
    load_owned(&state, &id, &sid)?;
    let store = state.agent.store();
    let out: Vec<SimRoundDto> = store
        .list_sim_rounds(&sid)?
        .into_iter()
        .take(MAX_ROUNDS)
        .map(|r| crate::simlab::round_detail(store, &r))
        .collect();
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/simulations/{sid}/leaderboard",
    operation_id = "get_simulation_leaderboard",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    responses((status = 200, description = "Cumulative-return leaderboard with equity series", body = [SimLeaderboardRowDto])),
    tag = "simlab"
)]
pub async fn leaderboard(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Json<Vec<SimLeaderboardRowDto>>, AppError> {
    load_owned(&state, &id, &sid)?;
    crate::simlab::leaderboard(state.agent.store(), &sid)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[utoipa::path(
    put,
    path = "/projects/{id}/simulations/{sid}/schedule",
    operation_id = "set_simulation_schedule",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Simulation id")
    ),
    request_body = SetSimScheduleRequest,
    responses(
        (status = 200, description = "The updated simulation", body = SimulationDto),
        (status = 400, description = "Invalid cron expression")
    ),
    tag = "simlab"
)]
pub async fn set_schedule(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
    Json(body): Json<SetSimScheduleRequest>,
) -> Result<Json<SimulationDto>, AppError> {
    let sim = load_owned(&state, &id, &sid)?;
    let store = state.agent.store();
    // Any change replaces the (single) sim-driving schedule.
    store.delete_sim_schedules(&sid)?;
    if let Some(cron) = body.cron_expr.as_deref().filter(|c| !c.trim().is_empty()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let next = crate::scheduler::first_fire(cron, now)
            .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
        store.create_sim_schedule(
            &id,
            &sid,
            "cron",
            Some(cron),
            next,
            body.deliver_notify,
            body.deliver_email,
        )?;
    }
    crate::simlab::to_dto(store, &sim)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}
