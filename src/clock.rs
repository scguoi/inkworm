//! Clock abstraction for testable time-dependent logic.

use chrono::{DateTime, Local, NaiveDate, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    /// Today's date in the user's local timezone. Default impl computes from
    /// `now()` so test clocks only need to override `now`.
    fn today_local(&self) -> NaiveDate {
        self.now().with_timezone(&Local).date_naive()
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Test-only clock returning a fixed instant.
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fixed_clock_returns_configured_time() {
        let t = Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap();
        let c = FixedClock(t);
        assert_eq!(c.now(), t);
    }

    #[test]
    fn system_clock_returns_monotonic_times() {
        let c = SystemClock;
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }

    #[test]
    fn fixed_clock_today_local_uses_local_zone() {
        use chrono::{Local, NaiveDate};
        let utc = Utc.with_ymd_and_hms(2026, 4, 27, 12, 0, 0).unwrap();
        let c = FixedClock(utc);
        let expected: NaiveDate = utc.with_timezone(&Local).date_naive();
        assert_eq!(c.today_local(), expected);
    }
}
