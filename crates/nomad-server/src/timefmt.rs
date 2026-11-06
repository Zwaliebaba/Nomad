//! Text forms of Universe time and durations for logs and `status`.

use std::fmt::Write;
use std::time::Duration;

use nomad_sim::{TICKS_PER_SECOND, Ticks};

const MILLIS_PER_DAY: i64 = 86_400_000;

/// UTC milliseconds since the Unix epoch as ISO 8601, for example `2026-11-06T09:30:00Z`.
pub fn utc_text(unix_ms: i64) -> String {
    let days = unix_ms.div_euclid(MILLIS_PER_DAY);
    let ms_of_day = unix_ms.rem_euclid(MILLIS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let secs = ms_of_day / 1_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A span of Universe time, for example `2 d 3 h 4 min`.
pub fn ticks_text(ticks: Ticks) -> String {
    duration_text(Duration::from_secs(ticks.get() / TICKS_PER_SECOND))
}

/// A duration in days, hours, minutes and seconds, largest units first, at most three.
pub fn duration_text(duration: Duration) -> String {
    let secs = duration.as_secs();
    let parts = [
        (secs / 86_400, "d"),
        (secs / 3_600 % 24, "h"),
        (secs / 60 % 60, "min"),
        (secs % 60, "s"),
    ];
    let mut out = String::new();
    for (value, unit) in parts
        .iter()
        .skip_while(|(value, _)| *value == 0)
        .take(3)
        .filter(|(value, _)| *value > 0)
    {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{value} {unit}");
    }
    if out.is_empty() {
        if duration.is_zero() {
            "0 s".to_owned()
        } else {
            format!("{} ms", duration.as_millis())
        }
    } else {
        out
    }
}

/// A byte count in B, KiB or MiB.
pub fn bytes_text(bytes: u64) -> String {
    const KIB: u64 = 1_024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / (KIB * KIB) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc_text(0), "1970-01-01T00:00:00Z");
        // 2026-11-06T00:00:00Z
        assert_eq!(utc_text(1_793_923_200_000), "2026-11-06T00:00:00Z");
        assert_eq!(
            utc_text(1_793_923_200_000 + 34_200_999),
            "2026-11-06T09:30:00Z"
        );
        assert_eq!(utc_text(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(utc_text(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(Duration::ZERO), "0 s");
        assert_eq!(duration_text(Duration::from_millis(250)), "250 ms");
        assert_eq!(duration_text(Duration::from_secs(61)), "1 min 1 s");
        assert_eq!(duration_text(Duration::from_hours(6)), "6 h");
        assert_eq!(
            duration_text(Duration::from_secs(2 * 86_400 + 3 * 3_600 + 4 * 60 + 5)),
            "2 d 3 h 4 min"
        );
        assert_eq!(ticks_text(Ticks::from_hours(6)), "6 h");
    }

    #[test]
    fn byte_counts() {
        assert_eq!(bytes_text(512), "512 B");
        assert_eq!(bytes_text(2_048), "2.0 KiB");
        assert_eq!(bytes_text(3 * 1_024 * 1_024), "3.0 MiB");
    }
}
