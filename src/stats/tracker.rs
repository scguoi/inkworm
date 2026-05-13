//! Pure in-memory active-time + submit accounting.
//!
//! Time accumulates only when a keystroke arrives within IDLE_THRESHOLD_MS
//! of the previous one. No background timer; no time advances unless an
//! event arrives.

use chrono::{DateTime, Local, NaiveDate, Utc};
use std::collections::BTreeMap;

use crate::storage::stats::{DayStats, Stats, STATS_SCHEMA_VERSION};

pub const IDLE_THRESHOLD_MS: i64 = 30_000;

pub struct StatsTracker {
    last_activity: Option<DateTime<Utc>>,
    today: NaiveDate,
    today_stats: DayStats,
    history: BTreeMap<NaiveDate, DayStats>,
}

impl StatsTracker {
    pub fn from_stats(mut stats: Stats, today: NaiveDate) -> Self {
        let today_stats = stats.days.remove(&today).unwrap_or_default();
        Self {
            last_activity: None,
            today,
            today_stats,
            history: stats.days,
        }
    }

    pub fn snapshot(&self) -> Stats {
        let mut days = self.history.clone();
        days.insert(self.today, self.today_stats);
        Stats {
            schema_version: STATS_SCHEMA_VERSION,
            days,
        }
    }

    pub fn today_stats(&self) -> &DayStats {
        &self.today_stats
    }

    pub fn on_keystroke(&mut self, now: DateTime<Utc>) {
        let today_local = now.with_timezone(&Local).date_naive();
        if today_local != self.today {
            // Rollover: move today into history, reset, drop session.
            self.history.insert(self.today, self.today_stats);
            self.today = today_local;
            self.today_stats = DayStats::default();
            self.last_activity = None;
        }
        if let Some(prev) = self.last_activity {
            if now >= prev {
                let delta_ms = now.signed_duration_since(prev).num_milliseconds();
                if delta_ms <= IDLE_THRESHOLD_MS {
                    self.today_stats.active_ms =
                        self.today_stats.active_ms.saturating_add(delta_ms as u64);
                }
                // delta > threshold or clock rewind → no accumulation.
            }
            // now < prev → clock rewind, no accumulation.
        }
        self.last_activity = Some(now);
    }

    pub fn on_submit(&mut self, now: DateTime<Utc>, was_correct: bool, words: u32) {
        self.on_keystroke(now);
        self.today_stats.submits = self.today_stats.submits.saturating_add(1);
        if was_correct {
            self.today_stats.correct = self.today_stats.correct.saturating_add(1);
        }
        self.today_stats.words = self.today_stats.words.saturating_add(words);
    }

    pub fn flush_idle(&mut self, now: DateTime<Utc>) {
        if let Some(prev) = self.last_activity {
            if now >= prev && now.signed_duration_since(prev).num_milliseconds() > IDLE_THRESHOLD_MS
            {
                self.last_activity = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(ymd: (i32, u32, u32), hms: (u32, u32, u32)) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(ymd.0, ymd.1, ymd.2, hms.0, hms.1, hms.2)
            .unwrap()
    }

    fn fresh() -> StatsTracker {
        let today = t((2026, 5, 13), (12, 0, 0))
            .with_timezone(&Local)
            .date_naive();
        StatsTracker::from_stats(Stats::empty(), today)
    }

    #[test]
    fn first_keystroke_records_zero_time() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn keystrokes_within_threshold_accumulate_delta() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 5))); // +5s
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 12))); // +7s
        assert_eq!(tr.today_stats().active_ms, 12_000);
    }

    #[test]
    fn gap_over_threshold_does_not_accumulate() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_keystroke(t((2026, 5, 13), (12, 1, 0))); // +60s, over 30s threshold
        assert_eq!(tr.today_stats().active_ms, 0);
        // Next pair within threshold accumulates again from the new anchor.
        tr.on_keystroke(t((2026, 5, 13), (12, 1, 10))); // +10s from 12:01:00
        assert_eq!(tr.today_stats().active_ms, 10_000);
    }

    #[test]
    fn submit_increments_correct_and_words() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.on_submit(t((2026, 5, 13), (12, 0, 3)), true, 5);
        let d = tr.today_stats();
        assert_eq!(d.submits, 1);
        assert_eq!(d.correct, 1);
        assert_eq!(d.words, 5);
        assert_eq!(d.active_ms, 3_000); // 12:00:00 → 12:00:03
    }

    #[test]
    fn submit_increments_total_even_when_wrong() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (12, 0, 0)), false, 4);
        let d = tr.today_stats();
        assert_eq!(d.submits, 1);
        assert_eq!(d.correct, 0);
        assert_eq!(d.words, 4);
    }

    #[test]
    fn day_rollover_moves_today_to_history_and_resets() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (23, 59, 0)), true, 3);
        // Crossing local midnight: pick a time well past local midnight in UTC
        // for any timezone the test runs in (UTC+14..UTC-12 worst cases).
        tr.on_keystroke(t((2026, 5, 14), (23, 0, 0))); // ~24h later
        let snap = tr.snapshot();
        assert!(!snap.days.is_empty(), "previous day should be in history");
        assert_eq!(tr.today_stats().submits, 0);
        assert_eq!(tr.today_stats().words, 0);
    }

    #[test]
    fn day_rollover_clears_last_activity() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (23, 59, 0)));
        // After rollover, a "first" keystroke on the new day must not accumulate
        // the multi-hour gap into the new day.
        tr.on_keystroke(t((2026, 5, 14), (23, 0, 0)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn flush_idle_clears_session_after_threshold() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.flush_idle(t((2026, 5, 13), (12, 0, 45))); // > 30s
                                                      // Next keystroke should NOT accumulate the gap.
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 50)));
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn flush_idle_noop_when_within_threshold() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 0)));
        tr.flush_idle(t((2026, 5, 13), (12, 0, 20))); // < 30s
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 25))); // +25s from 12:00:00
        assert_eq!(tr.today_stats().active_ms, 25_000);
    }

    #[test]
    fn clock_rewind_does_not_subtract_time() {
        let mut tr = fresh();
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 10)));
        tr.on_keystroke(t((2026, 5, 13), (12, 0, 5))); // earlier than prev
        assert_eq!(tr.today_stats().active_ms, 0);
    }

    #[test]
    fn snapshot_round_trips_through_stats() {
        let mut tr = fresh();
        tr.on_submit(t((2026, 5, 13), (12, 0, 0)), true, 3);
        tr.on_submit(t((2026, 5, 13), (12, 0, 2)), true, 4);
        let snap = tr.snapshot();
        let today = t((2026, 5, 13), (12, 0, 0))
            .with_timezone(&Local)
            .date_naive();
        let d = snap.days.get(&today).expect("today present");
        assert_eq!(d.submits, 2);
        assert_eq!(d.correct, 2);
        assert_eq!(d.words, 7);
    }
}
