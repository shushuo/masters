//! **Portable team/master bundles** (Phase 4h; ADR-0010) — export a team + the full definition of
//! every master it references as a self-contained JSON [`TeamBundle`], and import a bundle into another
//! project (recreating the masters as `masters/<slug>.md` files + DB rows, then the team).
//!
//! This is pure orchestration over existing primitives — no new storage, no file format, no core
//! change: [`crate::team::load_team`] + [`MasterStore`] for the source, the 4a `Master↔MasterDto`
//! conversions ([`crate::routes::masters::to_dto`]/`from_dto`) for the wire shape, and
//! [`MasterStore::create`] + [`getmasters_core::store::Store::upsert_team`] for the destination (both
//! overwrite a same-slug file/row, so re-import is idempotent).

use getmasters_core::skills::slugify;
use getmasters_proto::{BundleImportResult, TeamBundle};

use crate::master::master_store;
use crate::routes::masters::{from_dto, to_dto};
use crate::state::AppState;
use crate::team::load_team;

/// Export a project's team as a self-contained bundle: the team fields + every referenced master
/// (members ∪ coordinator) loaded in full. Masters whose file is missing are skipped.
pub fn export(state: &AppState, project_id: &str, team_slug: &str) -> Result<TeamBundle, String> {
    let team = load_team(state, project_id, team_slug)?;
    let store = master_store(state, project_id);

    // Members in order, plus the coordinator if it isn't already a member (dedup, order-preserving).
    let mut slugs = team.members.clone();
    if !team.coordinator_slug.is_empty() && !slugs.contains(&team.coordinator_slug) {
        slugs.push(team.coordinator_slug.clone());
    }

    let mut masters = Vec::new();
    for slug in &slugs {
        if let Ok(Some(master)) = store.load(slug) {
            masters.push(to_dto(slug.clone(), master));
        }
    }

    Ok(TeamBundle {
        version: 1,
        name: team.name,
        summary: team.summary,
        coordinator_slug: team.coordinator_slug,
        members: team.members,
        masters,
    })
}

/// Import a bundle into a project: recreate each master (file + DB row), then the team. Overwrites any
/// same-slug master/team already present. Returns the recreated team slug + the imported master slugs.
pub fn import(
    state: &AppState,
    project_id: &str,
    bundle: TeamBundle,
) -> Result<BundleImportResult, String> {
    let store = master_store(state, project_id);

    let mut masters = Vec::new();
    for dto in bundle.masters {
        let master = from_dto(dto);
        let slug = store.create(&master).map_err(|e| e.to_string())?;
        masters.push(slug);
    }

    let team_slug = slugify(&bundle.name);
    state
        .agent
        .store()
        .upsert_team(
            project_id,
            &team_slug,
            &bundle.name,
            &bundle.summary,
            &bundle.coordinator_slug,
            &bundle.members,
        )
        .map_err(|e| e.to_string())?;

    Ok(BundleImportResult { team_slug, masters })
}
