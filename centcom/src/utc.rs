//! Times as the fleet writes them: seconds since 1970 shown in UTC, as the bus's posts give them ("21:08:29Z").

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970, now.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// (year, month, day) of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// Days from 1970-01-01 to a date (the inverse of `civil`).
fn days(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parts(at: f64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = at.floor() as i64;
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400) as u32;
    (y, mo, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// "21:08:29Z".
pub fn stamp(at: f64) -> String {
    let (_, _, _, h, m, s) = parts(at);
    format!("{h:02}:{m:02}:{s:02}Z")
}

/// "10-03 21:08Z": a time far enough back to need its day.
pub fn day_stamp(at: f64) -> String {
    let (_, mo, d, h, m, _) = parts(at);
    format!("{mo:02}-{d:02} {h:02}:{m:02}Z")
}

/// "2026-10-05 05:13:44Z": a date in full.
pub fn full(at: f64) -> String {
    let (y, mo, d, h, m, s) = parts(at);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

/// The time alone if it is today (UTC) as of `now`, else with its day.
pub fn when(at: f64, now: f64) -> String {
    if (at.floor() as i64).div_euclid(86_400) == (now.floor() as i64).div_euclid(86_400) { stamp(at) } else { day_stamp(at) }
}

/// "2 h 05 min ago", "40 s ago", "3 d ago".
pub fn ago(at: f64, now: f64) -> String {
    let s = (now - at).max(0.0) as u64;
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h {:02} min ago", s / 3600, s % 3600 / 60),
        _ => format!("{} d ago", s / 86_400),
    }
}

/// "2026-10-03T21:08:29.7038196Z" (or without fractions, or with "+00:00") to seconds since 1970.
pub fn parse(text: &str) -> Option<f64> {
    let t = text.trim();
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, da) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let time = time.trim_end_matches('Z').trim_end_matches("+00:00");
    let mut c = time.split(':');
    let (h, mi): (i64, i64) = (c.next()?.parse().ok()?, c.next()?.parse().ok()?);
    let s: f64 = c.next()?.parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    Some((days(y, mo, da) * 86_400 + h * 3600 + mi * 60) as f64 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_and_read_as_the_bus_writes_them() {
        // 1791061711.19 is the card guard's stop at 2026-10-03 21:08:31Z.
        assert_eq!(stamp(1791061711.19), "21:08:31Z");
        assert_eq!(day_stamp(1791061711.19), "10-03 21:08Z");
        assert_eq!(parse("2026-10-03T21:08:29.7038196Z"), Some(1791061709.7038196));
        assert_eq!(parse("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse("2024-02-29T12:00:00+00:00"), Some(1709208000.0));
        assert_eq!(parse("2026-13-01T00:00:00Z"), None);
        for at in [0.0, 951_782_400.0, 1_709_208_000.0, 1_791_061_711.0, 4_102_444_800.0] {
            let (y, mo, d, h, mi, s) = parts(at);
            assert_eq!(parse(&format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")), Some(at), "{at}");
        }
        assert_eq!(when(1791061711.0, 1791061711.0 + 60.0), "21:08:31Z");
        assert_eq!(when(1791061711.0, 1791061711.0 + 86_400.0), "10-03 21:08Z");
        assert_eq!(ago(100.0, 100.0 + 7_500.0), "2 h 05 min ago");
        assert_eq!(ago(100.0, 140.0), "40 s ago");
    }
}
