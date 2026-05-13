//! Learning statistics — pure in-memory tracking and aggregation.
//!
//! See spec: docs/superpowers/specs/2026-05-13-learning-stats-design.md

pub mod aggregate;
pub mod tracker;

pub use aggregate::{
    all_time_totals, recent_months, recent_weeks, this_week, today_totals, week_totals, MonthRow,
    Totals, WeekDayCell, WeekRow,
};
pub use tracker::{StatsTracker, IDLE_THRESHOLD_MS};

/// Counts English words in a drill reference string.
///
/// Whitespace-delimited. Empty / whitespace-only → 0.
pub fn count_words(english: &str) -> u32 {
    english.split_whitespace().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_words_empty_is_zero() {
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn count_words_whitespace_only_is_zero() {
        assert_eq!(count_words("   \t  "), 0);
    }

    #[test]
    fn count_words_single() {
        assert_eq!(count_words("hello"), 1);
    }

    #[test]
    fn count_words_trims_and_collapses() {
        assert_eq!(count_words("  hello   world  "), 2);
    }

    #[test]
    fn count_words_punctuation_does_not_split() {
        // "AI's" is one whitespace-delimited token, even though it contains
        // punctuation; this matches "I counted 5 words".
        assert_eq!(count_words("AI's mission is to learn fast"), 6);
    }
}
