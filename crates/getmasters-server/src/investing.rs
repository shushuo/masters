//! **Investing workspace seeding** — the idempotent bootstrap behind `POST /investing/workspace`
//! (docs/11 §5 decision e; ADR-0015 L3 content). Lazily, on first use (no startup seeding —
//! fresh installs stay clean):
//!
//! 1. ensure the system default project exists (the quick-chat precedent),
//! 2. seed the four investing masters into the **global** master store under stable ASCII
//!    slugs (catalog semantics: `system`/`builtin` rows are overwritten so template fixes
//!    propagate; a user-owned same-slug master is never clobbered),
//! 3. upsert the standing `investing` team (coordinator `chief`),
//! 4. write the compliance declaration into the project's instructions **only when empty**
//!    (auto-injected last in prompt assembly, ADR-0011; a non-empty value is user content and
//!    is never overwritten — acceptable to share the default project under the D2 full pivot).
//!
//! Chat sessions are then opened through the existing `POST /projects/{id}/teams/investing/session`.

use getmasters_proto::InvestingWorkspaceDto;

use crate::master_templates;
use crate::routes::masters::from_dto;
use crate::state::AppState;

/// The standing investing team's stable slug.
pub const INVESTING_TEAM_SLUG: &str = "investing";

/// The compliance declaration block written into the (empty) default project's instructions —
/// the prompt-assembly layer of the three-layer compliance system (docs/11 §7).
const PROJECT_COMPLIANCE_INSTRUCTIONS: &str = "\
本项目是投资研究工作台。所有回答遵守：只给事实、数据、风险与分析框架，不给买卖指令、\
目标价、评级或收益预期；一切数字须来自工具返回并注明来源与「数据截至」日期，无数据时\
明确说「暂无数据」；每份分析以「ⓘ 以上为事实与风险梳理，不构成投资建议」结尾。";

/// Idempotently ensure the investing workspace (default project + seeded masters + standing
/// team + compliance instructions). Safe to call on every UI mount.
pub fn ensure_workspace(state: &AppState) -> Result<InvestingWorkspaceDto, String> {
    let project_id = state.ensure_default_project()?;
    let store = state.agent.store();
    let master_store = state.global_master_store();

    let four = master_templates::investing();
    let members: Vec<String> = four.iter().map(|(slug, _)| slug.to_string()).collect();

    for (slug, mut dto) in four {
        dto.origin = "system".to_string();
        // Catalog semantics: overwrite system/builtin rows (template fixes propagate),
        // never clobber a user-authored master that happens to share the slug.
        if let Ok(Some(existing)) = master_store.load(slug) {
            if existing.origin != "system" && existing.origin != "builtin" {
                tracing::info!(%slug, origin = %existing.origin, "keeping user-owned master; not seeding over it");
                continue;
            }
        }
        master_store
            .create_with_slug(slug, &from_dto(dto))
            .map_err(|e| format!("failed to seed master '{slug}': {e}"))?;
    }

    store
        .upsert_team(
            &project_id,
            INVESTING_TEAM_SLUG,
            "投资专家团",
            "你的私人投研团队",
            "chief",
            &members,
        )
        .map_err(|e| e.to_string())?;

    let instructions = store
        .project_instructions(&project_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if instructions.trim().is_empty() {
        store
            .set_project_instructions(&project_id, PROJECT_COMPLIANCE_INSTRUCTIONS)
            .map_err(|e| e.to_string())?;
    }

    Ok(InvestingWorkspaceDto {
        project_id,
        team_slug: INVESTING_TEAM_SLUG.to_string(),
        coordinator: "chief".to_string(),
        members,
    })
}
