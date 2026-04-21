//! Clock abstraction for testable time-dependent logic.

use chrono::{DateTime, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
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
}
