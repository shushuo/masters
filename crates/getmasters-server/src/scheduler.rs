//! **Scheduler** (Phase 3d, FR-17) — fires project recipes once at a time or on a recurring cron
//! expression, while the daemon is running (docs/02 §5). The DB owns the schedule + run history
//! (lean core stays cron-free); this module owns the cron math + the tick loop, reusing
//! [`crate::recipe::run_loaded`] so a scheduled run is gated/audited exactly like a manual one.

use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};
use cron::Schedule;

use crate::state::AppState;

/// How often the loop wakes to check for due schedules.
const TICK_SECS: u64 = 30;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Normalize a cron expression to the `cron` crate's seconds-first form: a standard 5-field
/// expression (`min hour dom month dow`) gets a `0` seconds field prepended; 6/7-field expressions
/// (already seconds-first, optionally with a year) pass through.
fn normalize(expr: &str) -> Result<String, String> {
    match expr.split_whitespace().count() {
        5 => Ok(format!("0 {}", expr.trim())),
        6 | 7 => Ok(expr.trim().to_string()),
        n => Err(format!("cron expression must have 5-7 fields, got {n}")),
    }
}

/// The next fire time strictly after `now_ms` (epoch ms) for a cron expression, or `None` if the
/// schedule has no future occurrence.
pub fn next_after(expr: &str, now_ms: i64) -> Result<Option<i64>, String> {
    let schedule =
        Schedule::from_str(&normalize(expr)?).map_err(|e| format!("invalid cron '{expr}': {e}"))?;
    let now = Utc
        .timestamp_millis_opt(now_ms)
        .single()
        .ok_or_else(|| "invalid timestamp".to_string())?;
    Ok(schedule.after(&now).next().map(|dt| dt.timestamp_millis()))
}

/// Validate a cron expression and return its first fire time after now (used when creating a cron
/// schedule).
pub fn first_fire(expr: &str, now_ms: i64) -> Result<Option<i64>, String> {
    next_after(expr, now_ms)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// Fire every schedule due at `now_ms`: run its recipe, record the outcome, and advance its next
/// fire (cron → next occurrence; once → disabled). Errors are recorded, never propagated, so one
/// bad schedule doesn't stall the loop. Public for deterministic testing (call it directly instead
/// of waiting on the timer).
pub async fn run_due(state: &AppState, now_ms: i64) {
    let store = state.agent.store().clone();
    let due = match store.due_schedules(now_ms) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "scheduler: failed to query due schedules");
            return;
        }
    };

    for sched in due {
        let params: HashMap<String, String> =
            serde_json::from_str(&sched.params).unwrap_or_default();
        let recipe_store = crate::recipe::RecipeStore::new(
            state.project_dir(&sched.project_id),
            sched.project_id.clone(),
            store.clone(),
        );

        let outcome = match recipe_store.load(&sched.recipe_name) {
            Ok(Some(recipe)) => {
                crate::recipe::run_loaded(state, &sched.project_id, &recipe, &params).await
            }
            Ok(None) => Err(format!("recipe '{}' not found", sched.recipe_name)),
            Err(e) => Err(e),
        };

        let (status, session_id, summary) = match &outcome {
            Ok(r) => (
                "ok",
                Some(r.session_id.clone()),
                truncate(&r.message.content, 200),
            ),
            Err(e) => {
                tracing::warn!(schedule = %sched.id, error = %e, "scheduler: run failed");
                ("error", None, truncate(e, 200))
            }
        };
        let _ = store.record_scheduled_run(
            &sched.id,
            &sched.project_id,
            status,
            session_id.as_deref(),
            Some(&summary),
        );

        // Deliver the output (Phase 3e, FR-27) — only for a successful run, over the channels the
        // schedule opted into. Gated/audited inside `deliver`; never propagates.
        if let Ok(r) = &outcome {
            if sched.deliver_notify || sched.deliver_email {
                crate::delivery::deliver(
                    state,
                    &sched.project_id,
                    Some(&r.session_id),
                    &sched.recipe_name,
                    &r.message.content,
                    sched.deliver_notify,
                    sched.deliver_email,
                )
                .await;
            }
        }

        // Advance the schedule.
        if sched.kind == "cron" {
            let next = sched
                .cron_expr
                .as_deref()
                .and_then(|e| next_after(e, now_ms).ok().flatten());
            let _ = store.set_schedule_next(&sched.id, next, next.is_some());
        } else {
            // One-off: done after a single fire.
            let _ = store.set_schedule_next(&sched.id, None, false);
        }
    }
}

/// Spawn the background tick loop. It runs until the tokio runtime shuts down (v1 fires only while
/// the daemon is alive — docs/02 §5).
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(TICK_SECS));
        loop {
            ticker.tick().await;
            run_due(&state, now_ms()).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_after_daily_9am() {
        // 2021-01-01T00:00:00Z → next "every day at 09:00" is 2021-01-01T09:00:00Z.
        let base = 1_609_459_200_000; // 2021-01-01T00:00:00Z
        let next = next_after("0 9 * * *", base).unwrap().unwrap();
        assert_eq!(next, 1_609_491_600_000); // 2021-01-01T09:00:00Z
    }

    #[test]
    fn next_after_is_strictly_future() {
        // Exactly at 09:00 → the next daily 09:00 is the following day, not the same instant.
        let at_9 = 1_609_491_600_000;
        let next = next_after("0 9 * * *", at_9).unwrap().unwrap();
        assert_eq!(next, at_9 + 86_400_000);
    }

    #[test]
    fn accepts_six_field_seconds_form() {
        // Native cron-crate form (sec min hour dom month dow): every 30s.
        let base = 1_609_459_200_000;
        let next = next_after("30 * * * * *", base).unwrap().unwrap();
        assert_eq!(next, base + 30_000);
    }

    #[test]
    fn rejects_bad_field_count() {
        assert!(next_after("* * *", 0).is_err());
        assert!(next_after("not a cron", 0).is_err());
    }
}
