use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use croner::Cron;

// scheduled tasks start a fresh agent run, which isn't something to do every few minutes
pub const MIN_INTERVAL: Duration = Duration::hours(1);
// how many upcoming runs are checked against the minimum interval
const CHECKED_RUNS: usize = 24;
const CRON_FIELDS: usize = 5;

// a standard five-field cron expression evaluated in an iana timezone, so "9am" stays 9am across dst
#[derive(Debug)]
pub struct Schedule {
    cron: Cron,
    timezone: Tz,
}

impl Schedule {
    pub fn parse(pattern: &str, timezone: &str) -> Result<Self, String> {
        let pattern = pattern.trim();
        if pattern.split_whitespace().count() != CRON_FIELDS {
            return Err(
                "the schedule must be a five-field cron expression, like `0 9 * * 1-5`".into(),
            );
        }
        let cron = Cron::from_str(pattern).map_err(|error| format!("invalid schedule: {error}"))?;
        let timezone = Tz::from_str(timezone).map_err(|_| {
            format!("unknown timezone {timezone}; use an IANA name like Europe/Berlin")
        })?;
        let schedule = Self { cron, timezone };
        schedule.check_interval()?;
        Ok(schedule)
    }

    // the first run strictly after `after`; none if the expression can never match
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local = after.with_timezone(&self.timezone);
        self.cron
            .find_next_occurrence(&local, false)
            .ok()
            .map(|next| next.with_timezone(&Utc))
    }

    fn check_interval(&self) -> Result<(), String> {
        let mut previous = self
            .next_after(Utc::now())
            .ok_or("the schedule never runs")?;
        for _ in 0..CHECKED_RUNS {
            let Some(next) = self.next_after(previous) else {
                break;
            };
            if next - previous < MIN_INTERVAL {
                return Err("scheduled tasks can run at most once an hour".into());
            }
            previous = next;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn computes_runs_in_the_task_timezone() {
        let schedule = Schedule::parse("0 9 * * 1-5", "America/New_York").unwrap();
        // friday 2026-10-02 14:00 utc is 10:00 in new york, so the next weekday 9am is monday
        let after = Utc.with_ymd_and_hms(2026, 10, 2, 14, 0, 0).unwrap();
        let next = schedule.next_after(after).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 10, 5, 13, 0, 0).unwrap());
        assert_eq!(
            schedule.next_after(next).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 6, 13, 0, 0).unwrap()
        );
    }

    #[test]
    fn rejects_bad_schedules() {
        assert!(
            Schedule::parse("*/5 * * * *", "UTC")
                .unwrap_err()
                .contains("once an hour")
        );
        assert!(
            Schedule::parse("0 9 * *", "UTC")
                .unwrap_err()
                .contains("five-field")
        );
        assert!(
            Schedule::parse("0 0 9 * * *", "UTC")
                .unwrap_err()
                .contains("five-field")
        );
        assert!(
            Schedule::parse("0 9 * * *", "Mars/Olympus")
                .unwrap_err()
                .contains("unknown timezone")
        );
        assert!(Schedule::parse("0 25 * * *", "UTC").is_err());
        assert!(Schedule::parse("30 */2 * * *", "Europe/Berlin").is_ok());
    }
}
