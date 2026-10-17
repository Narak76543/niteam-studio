//! When the Gemini free tier's daily quota comes back: "Requests per day (RPD) quotas reset at
//! midnight Pacific time" (ai.google.dev/gemini-api/docs/rate-limits, checked 2026-10-17).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc, Weekday};

/// The `n`-th Sunday (1-based) of a month.
fn nth_sunday(year: i32, month: u32, n: u32) -> Option<NaiveDate> {
    NaiveDate::from_weekday_of_month_opt(year, month, Weekday::Sun, n as u8)
}

/// US Pacific offset from UTC at `utc`: −7 h during daylight saving time (second Sunday of
/// March, 2:00 local, to the first Sunday of November, 2:00 local), −8 h otherwise.
fn pacific_offset(utc: DateTime<Utc>) -> Duration {
    let y = utc.year();
    let dst = match (nth_sunday(y, 3, 2), nth_sunday(y, 11, 1)) {
        (Some(start), Some(end)) => {
            // 2:00 PST = 10:00 UTC; 2:00 PDT = 9:00 UTC.
            let start = start.and_hms_opt(10, 0, 0).map(|t| t.and_utc());
            let end = end.and_hms_opt(9, 0, 0).map(|t| t.and_utc());
            start.is_some_and(|s| utc >= s) && end.is_some_and(|e| utc < e)
        }
        _ => false,
    };
    Duration::hours(if dst { -7 } else { -8 })
}

/// The next midnight in US Pacific time, as UTC.
pub fn next_pacific_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now + pacific_offset(now);
    let next_day = local.date_naive().succ_opt().unwrap_or(local.date_naive());
    let midnight: NaiveDateTime = next_day.and_hms_opt(0, 0, 0).unwrap_or(local.naive_utc());
    // The offset at the target time (DST may change in between).
    let guess = midnight.and_utc() - pacific_offset(now);
    midnight.and_utc() - pacific_offset(guess)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn midnight_pacific_in_summer_and_winter() {
        // 17 Oct 2026 02:00 UTC = 16 Oct 19:00 PDT → reset 17 Oct 00:00 PDT = 07:00 UTC.
        assert_eq!(next_pacific_midnight(utc("2026-10-17T02:00:00Z")), utc("2026-10-17T07:00:00Z"));
        // 10 Dec 2026 12:00 UTC = 04:00 PST → reset 11 Dec 00:00 PST = 08:00 UTC.
        assert_eq!(next_pacific_midnight(utc("2026-12-10T12:00:00Z")), utc("2026-12-11T08:00:00Z"));
    }

    #[test]
    fn dst_ends_overnight() {
        // 31 Oct 2026 12:00 UTC (PDT); DST ends 1 Nov 2026 → 1 Nov 00:00 is still PDT (07:00 UTC).
        assert_eq!(next_pacific_midnight(utc("2026-10-31T12:00:00Z")), utc("2026-11-01T07:00:00Z"));
        // 1 Nov 2026 12:00 UTC = 04:00 PST → 2 Nov 00:00 PST = 08:00 UTC.
        assert_eq!(next_pacific_midnight(utc("2026-11-01T12:00:00Z")), utc("2026-11-02T08:00:00Z"));
    }
}
