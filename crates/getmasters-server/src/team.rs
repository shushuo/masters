//! **Master Teams** (Phase 4b, FR-38/40; ADR-0010) — a group of masters + a master router.
//!
//! Two operations, both faithful to docs/09 §3/§5's split: the router only *recommends*, the Core
//! *orchestrates*.
//! - [`route`] is read-only: rank the team's member masters against a brief (via the pure
//!   [`getmasters_core::masters::router`]) and report the selection — it executes nothing.
//! - [`run`] dispatches the routed (or manually overridden) master through the gated/audited 4a
//!   [`crate::master::run`]. This slice is **single dispatch**; parallel fan-out, sequential staging,
//!   and the multi-master group chat are deferred (ADR-0010/0012).

use getmasters_core::masters::router::{self, RankedMaster};
use getmasters_core::masters::Master;
use getmasters_core::store::MasterTeamRow;
use getmasters_proto::{MasterRunResult, RankedMasterDto, RouteResultDto, TeamRunResult};

use crate::master::master_store;
use crate::state::AppState;

/// Load a team by slug (404-able by the caller).
pub fn load_team(state: &AppState, project_id: &str, slug: &str) -> Result<MasterTeamRow, String> {
    state
        .agent
        .store()
        .get_team(project_id, slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("team '{slug}' not found"))
}

/// Load each member master's full file (skipping any whose file is missing), as `(slug, Master)`.
fn member_masters(
    state: &AppState,
    project_id: &str,
    team: &MasterTeamRow,
) -> Vec<(String, Master)> {
    let store = master_store(state, project_id);
    team.members
        .iter()
        .filter_map(|slug| match store.load(slug) {
            Ok(Some(e)) => Some((slug.clone(), e)),
            _ => None,
        })
        .collect()
}

fn to_ranked_dto(r: RankedMaster) -> RankedMasterDto {
    RankedMasterDto {
        slug: r.slug,
        name: r.name,
        score: r.score,
    }
}

/// Rank a team's masters against a brief and report the selection. Read-only.
pub fn route(
    state: &AppState,
    project_id: &str,
    slug: &str,
    brief: &str,
) -> Result<RouteResultDto, String> {
    let team = load_team(state, project_id, slug)?;
    let masters = member_masters(state, project_id, &team);
    let ranked = router::rank(brief, &masters);
    let selected_slug = router::select(&ranked, &team.coordinator_slug);
    Ok(RouteResultDto {
        ranked: ranked.into_iter().map(to_ranked_dto).collect(),
        selected_slug,
    })
}

/// Route (or honor a manual override) and dispatch the chosen master through the gated run path.
pub async fn run(
    state: &AppState,
    project_id: &str,
    slug: &str,
    brief: &str,
    override_slug: Option<&str>,
) -> Result<TeamRunResult, String> {
    let team = load_team(state, project_id, slug)?;
    let selected_slug = match override_slug.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            let masters = member_masters(state, project_id, &team);
            router::select(&router::rank(brief, &masters), &team.coordinator_slug)
        }
    };
    if selected_slug.is_empty() {
        return Err("team has no master to dispatch (no match and no coordinator)".into());
    }
    let result: MasterRunResult =
        crate::master::run(state, project_id, &selected_slug, brief).await?;
    Ok(TeamRunResult {
        selected_slug,
        result,
    })
}
