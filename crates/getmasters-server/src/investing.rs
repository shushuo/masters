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

use getmasters_proto::{InvestingWorkspaceDto, RecipeDto, RecipeParamDto};

use crate::master_templates;
use crate::routes::masters::from_dto;
use crate::state::AppState;

/// The standing investing team's stable slug.
pub const INVESTING_TEAM_SLUG: &str = "investing";

/// The silent-pass sentinel (docs/11 M8): a proactive-touch recipe whose run has nothing worth
/// saying outputs exactly this token — the scheduler records the run but **skips delivery**, and
/// the briefings surface hides it. "超阈值才说话，静默通过不打扰."
pub const NO_ALERT: &str = "NO_ALERT";

/// Whether a proactive-touch run output is a silent pass (nothing delivered, hidden from the
/// briefings feed): empty, or leading with the [`NO_ALERT`] sentinel.
pub fn is_silent(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.is_empty() || trimmed.starts_with(NO_ALERT)
}

/// The two slice-2 proactive-touch recipes (docs/11 M8 三件套 minus the earnings sentinel, which
/// needs the disclosure data face). Cron times are **UTC** (the scheduler's cron math runs in
/// UTC): 07:30 UTC = 15:30 Beijing (post-close weekdays), 12:00 UTC Sunday = 20:00 Beijing.
/// Day-of-week uses names — the `cron` crate does not accept `0` for Sunday.
fn touch_recipes() -> Vec<(RecipeDto, &'static str)> {
    let disclaimer = "结尾固定一行：ⓘ 以上为事实与风险梳理，不构成投资建议。";
    let weekly = RecipeDto {
        name: "weekly-watch-digest".into(),
        title: "每周关注周报".into(),
        description: "每周日晚（北京时间）汇总关注列表的行情与值得注意的变化；关注为空时静默跳过。"
            .into(),
        parameters: vec![],
        prompt: format!(
            "你是投资研究工作台的周报撰写者，遵守合规边界（只给事实、数据与风险，不给买卖\
             指令、点位、评级或收益预期；数字必须来自工具返回并注明来源与「数据截至」日期）。\n\
             用 assets.list_assets 查看 watching 状态的关注列表；如果列表为空，只输出 \
             {NO_ALERT}，不要输出其他任何内容。\n\
             否则逐个用 market.get_quote 取最新行情，写一份《本周关注周报》：\n\
             1. 每个标的一行：名称 代码 收盘 涨跌%（数据截至日期 · 来源；stale 时注明数据可能过期）；\n\
             2. 之后用两三句话指出本周值得注意的事实性变化（不做方向性判断）。\n{disclaimer}"
        ),
        extensions: vec!["assets".into(), "market".into()],
    };
    let movers = RecipeDto {
        name: "watch-mover-sentinel".into(),
        title: "关注异动哨兵".into(),
        description: "工作日收盘后检查关注列表：仅当有标的涨跌幅超过阈值（{{threshold}}%）才提醒；\
                      否则静默跳过。"
            .into(),
        parameters: vec![RecipeParamDto {
            key: "threshold".into(),
            description: "触发提醒的涨跌幅绝对值（百分比）".into(),
            default: Some("4".into()),
        }],
        prompt: format!(
            "你是投资研究工作台的异动哨兵，遵守合规边界（只给事实与数据，不给买卖指令）。\n\
             用 assets.list_assets 获取 watching 状态的标的；如果列表为空，只输出 {NO_ALERT}。\n\
             否则逐个用 market.get_quote 取最新行情。如果没有任何标的的涨跌幅绝对值 ≥ \
             {{{{threshold}}}}%，只输出 {NO_ALERT}，不要输出其他任何内容。\n\
             否则只列出超过阈值的标的：名称 代码 收盘 涨跌%（数据截至日期 · 来源），每个配一句\
             事实性说明（如成交/公告线索，无可核实来源就不写原因）。\n{disclaimer}"
        ),
        extensions: vec!["assets".into(), "market".into()],
    };
    let earnings = RecipeDto {
        name: "earnings-sentinel".into(),
        title: "财报哨兵".into(),
        description: "每晚检查关注标的的法定披露：仅当有定期报告/业绩类公告才提醒；否则静默跳过。"
            .into(),
        parameters: vec![],
        prompt: format!(
            "你是投资研究工作台的财报哨兵，遵守合规边界（只给事实，不给买卖指令或预期判断）。\n\
             用 assets.list_assets 获取 watching 状态的标的；如果列表为空，只输出 {NO_ALERT}。\n\
             否则逐个用 market.list_announcements（days=2）查询近两天的法定披露公告，只保留标题\
             含「年度报告」「半年度报告」「季度报告」「业绩预告」「业绩快报」之一的条目（摘要类、\
             更正类除外）。如果一条都没有，只输出 {NO_ALERT}，不要输出其他任何内容。\n\
             否则逐条列出：标的名称 代码 《公告标题》（公告日期 · 来源），附文档链接；并提醒\
             用户可以在对话中要求解读这份报告。\n{disclaimer}"
        ),
        extensions: vec!["assets".into(), "market".into()],
    };
    vec![
        (weekly, "0 12 * * SUN"),
        (movers, "30 7 * * MON-FRI"),
        // 13:30 UTC = 21:30 Beijing — after the evening disclosure wave.
        (earnings, "30 13 * * MON-FRI"),
    ]
}

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

    // Proactive-touch recipes + schedules (slice 2). Both are seeded **only when absent** —
    // a recipe file or schedule the user has edited/retuned is user content and is never
    // overwritten (unlike the system masters above, these are user-level automations).
    let recipe_store = crate::recipe::RecipeStore::new(
        state.project_dir(&project_id),
        project_id.clone(),
        store.clone(),
    );
    let existing_schedules = store
        .list_schedules(&project_id)
        .map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    for (recipe, cron) in touch_recipes() {
        if recipe_store.load(&recipe.name)?.is_none() {
            recipe_store.save(&recipe)?;
        }
        if !existing_schedules
            .iter()
            .any(|s| s.recipe_name == recipe.name)
        {
            let next = crate::scheduler::first_fire(cron, now)?;
            store
                .create_schedule(
                    &project_id,
                    &recipe.name,
                    "{}",
                    "cron",
                    Some(cron),
                    next,
                    true,  // deliver_notify — the on-device channel (ADR-0009)
                    false, // deliver_email stays opt-in
                )
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(InvestingWorkspaceDto {
        project_id,
        team_slug: INVESTING_TEAM_SLUG.to_string(),
        coordinator: "chief".to_string(),
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_pass_predicate() {
        assert!(is_silent(""));
        assert!(is_silent("   \n"));
        assert!(is_silent("NO_ALERT"));
        assert!(is_silent("  NO_ALERT\n（本周无超阈值标的）"));
        assert!(!is_silent("本周关注周报：…"));
        // The sentinel must LEAD the output — merely mentioning it doesn't silence a report.
        assert!(!is_silent("列表为空时输出 NO_ALERT"));
    }

    #[test]
    fn touch_recipes_are_well_formed() {
        let recipes = touch_recipes();
        assert_eq!(recipes.len(), 3);
        for (recipe, cron) in &recipes {
            // 5-field standard cron (the scheduler normalizes to seconds-first).
            assert_eq!(cron.split_whitespace().count(), 5, "{}", recipe.name);
            // Every touch recipe must know the silent-pass contract and the compliance tail.
            assert!(recipe.prompt.contains(NO_ALERT), "{}", recipe.name);
            assert!(recipe.prompt.contains("不构成投资建议"), "{}", recipe.name);
            assert!(recipe.extensions.iter().any(|e| e == "market"));
        }
        // The mover sentinel keeps its threshold as a substitutable param with a default.
        let (movers, _) = &recipes[1];
        assert_eq!(movers.parameters[0].key, "threshold");
        assert!(movers.prompt.contains("{{threshold}}"));
    }
}
