//! The **SM-2** spaced-repetition algorithm (FR-14), as a pure function over scheduling state.
//!
//! SM-2 (SuperMemo 2) tracks three values per card: an *ease factor* (how easy the card is, ≥ 1.3),
//! the current *interval* in days, and the *repetition* count (consecutive successful reviews). A
//! review is graded `quality` 0–5. The classic recurrence (Wozniak 1990):
//!
//! - `quality < 3` (a lapse): reset `repetitions` to 0 and `interval` to 1 day (the card is
//!   relearned), and count a lapse.
//! - `quality >= 3` (recall): bump `repetitions`; the interval grows `1 → 6 → round(prev * EF)`.
//! - In both cases the ease factor updates
//!   `EF' = EF + (0.1 - (5 - q) * (0.08 + (5 - q) * 0.02))`, clamped to a floor of 1.3.
//!
//! This module is deliberately free of clocks and I/O: the caller supplies `now_ms`, and the
//! server/store own persistence. That keeps it trivially unit-testable and clock-independent.

/// A card's SM-2 scheduling state (the subset SM-2 reads and writes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Schedule {
    pub ease_factor: f64,
    pub interval_days: i64,
    pub repetitions: i64,
    pub lapses: i64,
    /// Epoch milliseconds when the card is next due.
    pub due_at: i64,
}

impl Schedule {
    /// The starting state for a freshly created card (due immediately at `now_ms`).
    pub fn new(now_ms: i64) -> Self {
        Self {
            ease_factor: 2.5,
            interval_days: 0,
            repetitions: 0,
            lapses: 0,
            due_at: now_ms,
        }
    }
}

const MIN_EASE: f64 = 1.3;
/// Milliseconds in a day (also reused for deadline math in adaptive study plans).
pub const DAY_MS: i64 = 86_400_000;

/// Apply one SM-2 review of `quality` (0–5; anything ≥ 6 is clamped to 5) at `now_ms`, returning
/// the new schedule. `quality < 3` is a lapse (relearn from a 1-day interval); otherwise the
/// interval steps `1 → 6 → round(prev * EF)`.
pub fn schedule(prev: Schedule, quality: u8, now_ms: i64) -> Schedule {
    let q = quality.min(5) as f64;

    // Ease factor update (applies on every review), floored at 1.3.
    let ease_factor =
        (prev.ease_factor + (0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02))).max(MIN_EASE);

    let (repetitions, interval_days, lapses) = if quality < 3 {
        // Lapse: relearn from scratch.
        (0, 1, prev.lapses + 1)
    } else {
        let repetitions = prev.repetitions + 1;
        // Canonical SM-2 multiplies by the *pre-update* ease factor for the 3rd+ repetition.
        let interval_days = match repetitions {
            1 => 1,
            2 => 6,
            _ => ((prev.interval_days as f64) * prev.ease_factor).round() as i64,
        }
        .max(1);
        (repetitions, interval_days, prev.lapses)
    };

    Schedule {
        ease_factor,
        interval_days,
        repetitions,
        lapses,
        due_at: now_ms + interval_days * DAY_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    #[test]
    fn first_two_successful_reviews_step_1_then_6_days() {
        let s0 = Schedule::new(NOW);
        let s1 = schedule(s0, 5, NOW);
        assert_eq!(s1.repetitions, 1);
        assert_eq!(s1.interval_days, 1);
        assert_eq!(s1.due_at, NOW + DAY_MS);

        let s2 = schedule(s1, 5, NOW);
        assert_eq!(s2.repetitions, 2);
        assert_eq!(s2.interval_days, 6);
        assert_eq!(s2.due_at, NOW + 6 * DAY_MS);
    }

    #[test]
    fn third_review_uses_ease_factor() {
        // Drive to rep 2 (interval 6), then a perfect review multiplies by the (raised) EF.
        let s = schedule(schedule(Schedule::new(NOW), 5, NOW), 5, NOW);
        let s3 = schedule(s, 5, NOW);
        assert_eq!(s3.repetitions, 3);
        // Interval uses the pre-update EF (2.7): round(6 * 2.7) = 16. The stored EF then ticks to 2.8.
        assert_eq!(s3.interval_days, 16);
        assert!(
            (s3.ease_factor - 2.8).abs() < 1e-9,
            "ef = {}",
            s3.ease_factor
        );
    }

    #[test]
    fn low_quality_lapses_and_resets() {
        let s2 = schedule(schedule(Schedule::new(NOW), 5, NOW), 5, NOW);
        assert_eq!(s2.interval_days, 6);
        let lapsed = schedule(s2, 1, NOW);
        assert_eq!(lapsed.repetitions, 0);
        assert_eq!(lapsed.interval_days, 1);
        assert_eq!(lapsed.lapses, 1);
        assert_eq!(lapsed.due_at, NOW + DAY_MS);
        // Ease factor drops but never below the 1.3 floor.
        assert!(lapsed.ease_factor < 2.5);
        assert!(lapsed.ease_factor >= MIN_EASE);
    }

    #[test]
    fn ease_factor_floored_at_1_3() {
        let mut s = Schedule::new(NOW);
        // Repeated worst-quality reviews must not push EF below 1.3.
        for _ in 0..10 {
            s = schedule(s, 0, NOW);
        }
        assert!(s.ease_factor >= MIN_EASE);
        assert_eq!(s.lapses, 10);
    }

    #[test]
    fn quality_3_is_a_pass_not_a_lapse() {
        let s1 = schedule(Schedule::new(NOW), 3, NOW);
        assert_eq!(s1.repetitions, 1);
        assert_eq!(s1.lapses, 0);
        assert_eq!(s1.interval_days, 1);
    }
}
