//! Civil UTC dates from Unix seconds (Howard Hinnant's civil-from-days), with no clock and no dependency.

/// (year, month, day) of Unix seconds `secs`.
pub fn ymd(secs: i64) -> (i64, i64, i64) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// YYYY-MM-DD.
pub fn date_of(secs: i64) -> String {
    let (y, m, d) = ymd(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A Unix time as "YYYY-MM-DD HH:MM:SSZ" (UTC), the form CENTCOM writes the Research page's history in.
pub fn time_of(secs: i64) -> String {
    let s = secs.rem_euclid(86_400);
    format!("{} {:02}:{:02}:{:02}Z", date_of(secs), s / 3600, s % 3600 / 60, s % 60)
}

pub fn year_of(secs: i64) -> i32 {
    ymd(secs).0 as i32
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_are_civil_utc_dates() {
        assert_eq!(super::date_of(0), "1970-01-01");
        assert_eq!(super::date_of(1_791_473_646), "2026-10-08");
        assert_eq!(super::date_of(951_782_400), "2000-02-29");
        assert_eq!(super::year_of(1_791_473_646), 2026);
    }
}
