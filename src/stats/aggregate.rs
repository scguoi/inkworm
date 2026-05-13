//! Pure read-only aggregations over `Stats`.
//!
//! Inputs are `&Stats` plus a "today" `NaiveDate` anchor. No IO, no state,
//! no caching. Accuracy is never stored — render-time function of totals.

use chrono::{Datelike, Duration, NaiveDate};

use crate::storage::stats::{DayStats, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub active_ms: u64,
    pub submits: u32,
    pub correct: u32,
    pub words: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekDayCell {
    pub date: NaiveDate,
    pub stats: DayStats,
    pub is_today: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekRow {
    pub iso_year: i32,
    pub iso_week: u32,
    pub totals: Totals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthRow {
    pub year: i32,
    pub month: u32,
    pub totals: Totals,
}

fn merge(into: &mut Totals, d: &DayStats) {
    into.active_ms = into.active_ms.saturating_add(d.active_ms);
    into.submits = into.submits.saturating_add(d.submits);
    into.correct = into.correct.saturating_add(d.correct);
    into.words = into.words.saturating_add(d.words);
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    let weekday = d.weekday().num_days_from_monday() as i64;
    d - Duration::days(weekday)
}

pub fn today_totals(stats: &Stats, today: NaiveDate) -> Totals {
    stats
        .days
        .get(&today)
        .map(|d| {
            let mut t = Totals::default();
            merge(&mut t, d);
            t
        })
        .unwrap_or_default()
}

pub fn week_totals(stats: &Stats, today: NaiveDate) -> Totals {
    let mon = monday_of(today);
    let sun = mon + Duration::days(6);
    let mut t = Totals::default();
    for (_d, ds) in stats.days.range(mon..=sun) {
        merge(&mut t, ds);
    }
    t
}

pub fn all_time_totals(stats: &Stats) -> Totals {
    let mut t = Totals::default();
    for ds in stats.days.values() {
        merge(&mut t, ds);
    }
    t
}

pub fn this_week(stats: &Stats, today: NaiveDate) -> [WeekDayCell; 7] {
    let mon = monday_of(today);
    let mut out = [WeekDayCell {
        date: mon,
        stats: DayStats::default(),
        is_today: false,
    }; 7];
    for (i, cell) in out.iter_mut().enumerate() {
        let date = mon + Duration::days(i as i64);
        cell.date = date;
        cell.stats = stats.days.get(&date).copied().unwrap_or_default();
        cell.is_today = date == today;
    }
    out
}

/// Returns up to `n` past ISO weeks (newest first), **excluding** the
/// current ISO week (which is already presented as the "This week" strip).
/// Empty weeks are padded with zero `Totals` to keep the row count stable.
pub fn recent_weeks(stats: &Stats, today: NaiveDate, n: usize) -> Vec<WeekRow> {
    let mut out = Vec::with_capacity(n);
    let current_mon = monday_of(today);
    for i in 1..=n {
        let mon = current_mon - Duration::days(7 * i as i64);
        let sun = mon + Duration::days(6);
        let mut t = Totals::default();
        for (_d, ds) in stats.days.range(mon..=sun) {
            merge(&mut t, ds);
        }
        let iso = mon.iso_week();
        out.push(WeekRow {
            iso_year: iso.year(),
            iso_week: iso.week(),
            totals: t,
        });
    }
    out
}

/// Returns up to `n` past months (newest first), **including** the current
/// month. Empty months are padded with zero `Totals`.
pub fn recent_months(stats: &Stats, today: NaiveDate, n: usize) -> Vec<MonthRow> {
    let mut out = Vec::with_capacity(n);
    let (mut year, mut month) = (today.year(), today.month());
    for _ in 0..n {
        let start = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let end = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
        };
        let mut t = Totals::default();
        for (_d, ds) in stats.days.range(start..end) {
            merge(&mut t, ds);
        }
        out.push(MonthRow {
            year,
            month,
            totals: t,
        });
        // step back one month
        if month == 1 {
            month = 12;
            year -= 1;
        } else {
            month -= 1;
        }
    }
    out
}

/// Returns the (iso_year, iso_week) for the given date. Public so UI can
/// show "This week (ISO 2026-W20)" in the header.
pub fn iso_week_label(d: NaiveDate) -> (i32, u32) {
    let iso = d.iso_week();
    (iso.year(), iso.week())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn ds(active_ms: u64, submits: u32, correct: u32, words: u32) -> DayStats {
        DayStats {
            active_ms,
            submits,
            correct,
            words,
        }
    }

    fn stats_with(entries: &[(NaiveDate, DayStats)]) -> Stats {
        let mut s = Stats::empty();
        for (d, ds) in entries {
            s.days.insert(*d, *ds);
        }
        s
    }

    #[test]
    fn today_totals_picks_correct_day() {
        let s = stats_with(&[
            (day(2026, 5, 12), ds(1000, 2, 2, 5)),
            (day(2026, 5, 13), ds(2000, 3, 2, 9)),
        ]);
        let t = today_totals(&s, day(2026, 5, 13));
        assert_eq!(
            t,
            Totals {
                active_ms: 2000,
                submits: 3,
                correct: 2,
                words: 9
            }
        );
    }

    #[test]
    fn today_totals_returns_zero_for_empty_stats() {
        let s = Stats::empty();
        assert_eq!(today_totals(&s, day(2026, 5, 13)), Totals::default());
    }

    #[test]
    fn week_totals_sums_mon_through_sun() {
        // 2026-05-13 is a Wednesday → week is 2026-05-11 (Mon) .. 2026-05-17 (Sun).
        let s = stats_with(&[
            (day(2026, 5, 10), ds(999, 9, 9, 9)), // Sun before — excluded
            (day(2026, 5, 11), ds(1000, 1, 1, 5)),
            (day(2026, 5, 13), ds(2000, 2, 1, 8)),
            (day(2026, 5, 17), ds(3000, 1, 0, 4)),
            (day(2026, 5, 18), ds(777, 7, 7, 7)), // Mon after — excluded
        ]);
        let t = week_totals(&s, day(2026, 5, 13));
        assert_eq!(
            t,
            Totals {
                active_ms: 6000,
                submits: 4,
                correct: 2,
                words: 17
            }
        );
    }

    #[test]
    fn this_week_marks_today_cell() {
        let s = stats_with(&[(day(2026, 5, 13), ds(1, 1, 1, 1))]);
        let cells = this_week(&s, day(2026, 5, 13));
        assert_eq!(cells.len(), 7);
        assert_eq!(cells[0].date, day(2026, 5, 11)); // Mon
        assert_eq!(cells[6].date, day(2026, 5, 17)); // Sun
        assert!(!cells[0].is_today);
        assert!(cells[2].is_today); // Wed
        assert_eq!(cells[2].stats.submits, 1);
    }

    #[test]
    fn this_week_handles_year_boundary() {
        // 2026-01-01 is a Thursday → Mon = 2025-12-29.
        let cells = this_week(&Stats::empty(), day(2026, 1, 1));
        assert_eq!(cells[0].date, day(2025, 12, 29));
        assert_eq!(cells[6].date, day(2026, 1, 4));
        assert!(cells[3].is_today); // Thu
    }

    #[test]
    fn recent_weeks_orders_newest_first_and_pads_empty() {
        let s = stats_with(&[(day(2026, 4, 27), ds(100, 1, 1, 1))]); // ISO 2026-W18 (Mon)
        let rows = recent_weeks(&s, day(2026, 5, 13), 4);
        assert_eq!(rows.len(), 4);
        // newest first = the week immediately before the current one
        let weeks: Vec<_> = rows.iter().map(|r| (r.iso_year, r.iso_week)).collect();
        assert_eq!(weeks[0], (2026, 19)); // current is W20, so newest excluded is W19
        assert_eq!(weeks[3], (2026, 16));
        // 2026-04-27 is in W18 → rows[1].totals carries it
        assert_eq!(rows[1].totals.active_ms, 100);
        assert_eq!(rows[0].totals, Totals::default());
    }

    #[test]
    fn recent_weeks_excludes_current_iso_week() {
        let s = stats_with(&[(day(2026, 5, 13), ds(9_999, 9, 9, 9))]); // in current week
        let rows = recent_weeks(&s, day(2026, 5, 13), 1);
        assert_eq!(rows[0].totals, Totals::default());
    }

    #[test]
    fn recent_months_includes_current_month() {
        let s = stats_with(&[(day(2026, 5, 13), ds(1234, 5, 4, 20))]);
        let rows = recent_months(&s, day(2026, 5, 13), 3);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].year, rows[0].month), (2026, 5));
        assert_eq!((rows[1].year, rows[1].month), (2026, 4));
        assert_eq!((rows[2].year, rows[2].month), (2026, 3));
        assert_eq!(rows[0].totals.active_ms, 1234);
        assert_eq!(rows[1].totals, Totals::default());
    }

    #[test]
    fn recent_months_handles_year_rollover() {
        let rows = recent_months(&Stats::empty(), day(2026, 2, 5), 4);
        let months: Vec<_> = rows.iter().map(|r| (r.year, r.month)).collect();
        assert_eq!(months, vec![(2026, 2), (2026, 1), (2025, 12), (2025, 11)]);
    }

    #[test]
    fn all_time_totals_sums_every_day() {
        let s = stats_with(&[
            (day(2025, 1, 1), ds(1, 1, 1, 1)),
            (day(2026, 5, 13), ds(2, 2, 2, 2)),
        ]);
        let t = all_time_totals(&s);
        assert_eq!(
            t,
            Totals {
                active_ms: 3,
                submits: 3,
                correct: 3,
                words: 3
            }
        );
    }
}
