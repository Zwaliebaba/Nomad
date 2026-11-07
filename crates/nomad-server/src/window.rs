//! The human's exposure window at creation: `create --window` (D-157; S6 plan §4.3).
//!
//! A window starts on a quarter hour of the UTC day (D-155); players think in local time.
//! `--window` takes:
//!
//! - `HH:MM`: local time on this computer, converted with its UTC offset now;
//! - `HH:MMZ`: UTC;
//! - `HH:MM+hh:mm` or `HH:MM-hh:mm`: local time at that UTC offset, for a client whose time
//!   zone differs from the server's, for example a server in a container (D-152).
//!
//! The default is 19:00 local (D-157). The UTC start must be a quarter hour. Windows are
//! UTC ranges, so the local window moves when daylight saving time changes; the
//! activity-derived window absorbs that within days (doc 11 §2). Reading the local offset is
//! the server shell's job: the simulation reads no clock and knows no time zone (D-050).

use std::fmt;
use std::str::FromStr;

use nomad_sim::QuarterHour;

use crate::error::ServerError;

const MINUTES_PER_DAY: i32 = 1_440;

/// A window start as given on the command line: a time of day, local unless an offset is
/// given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSpec {
    /// Minutes after midnight in the given or local time.
    minutes_of_day: u16,
    /// Minutes east of UTC; `None` for this computer's local time.
    offset_minutes: Option<i16>,
}

impl WindowSpec {
    /// The default: 19:00 local (D-157).
    pub const DEFAULT: Self = Self {
        minutes_of_day: 19 * 60,
        offset_minutes: None,
    };

    /// Whether the start is in this computer's local time.
    pub const fn is_local(self) -> bool {
        self.offset_minutes.is_none()
    }

    /// The UTC quarter hour the window starts at. `local_offset_minutes` is this computer's
    /// UTC offset, used when no offset was given; `None` if it is unknown.
    pub fn resolve(self, local_offset_minutes: Option<i32>) -> Result<QuarterHour, ServerError> {
        let offset = match self.offset_minutes {
            Some(offset) => i32::from(offset),
            None => local_offset_minutes.ok_or_else(|| {
                ServerError::InvalidParameter(
                    "the local UTC offset is unknown; give the window with an offset, for \
                     example --window 19:00+01:00, or in UTC, for example --window 18:00Z"
                        .to_owned(),
                )
            })?,
        };
        let utc = (i32::from(self.minutes_of_day) - offset).rem_euclid(MINUTES_PER_DAY);
        let quarter = u32::try_from(utc).unwrap_or(0) / QuarterHour::MINUTES;
        let on_quarter = u32::try_from(utc).is_ok_and(|m| m.is_multiple_of(QuarterHour::MINUTES));
        u8::try_from(quarter)
            .ok()
            .and_then(QuarterHour::new)
            .filter(|_| on_quarter)
            .ok_or_else(|| {
                ServerError::InvalidParameter(format!(
                    "the window would start at {} UTC, which is not on a quarter hour (D-155)",
                    clock_text(utc)
                ))
            })
    }
}

impl FromStr for WindowSpec {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let bad = || format!("not a window start: {text:?} (HH:MM, HH:MMZ or HH:MM+hh:mm)");
        let text = text.trim();
        let (time, offset) = if let Some(time) = text.strip_suffix(['Z', 'z']) {
            (time, Some(0))
        } else if let Some(at) = text.find(['+', '-']) {
            let (time, offset) = text.split_at(at);
            let sign: i16 = if offset.starts_with('-') { -1 } else { 1 };
            let minutes = hours_minutes(offset.get(1..).unwrap_or_default(), 14).ok_or_else(bad)?;
            let minutes = i16::try_from(minutes).map_err(|_| bad())?;
            (time, Some(sign * minutes))
        } else {
            (text, None)
        };
        let minutes_of_day = hours_minutes(time, 23).ok_or_else(bad)?;
        Ok(Self {
            minutes_of_day,
            offset_minutes: offset,
        })
    }
}

impl fmt::Display for WindowSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = clock_text(i32::from(self.minutes_of_day));
        match self.offset_minutes {
            None => write!(f, "{time} local"),
            Some(0) => write!(f, "{time}Z"),
            Some(offset) => {
                let sign = if offset < 0 { '-' } else { '+' };
                write!(f, "{time}{sign}{}", clock_text(i32::from(offset.abs())))
            }
        }
    }
}

/// `HH:MM` with hours up to `max_hours`, as minutes.
fn hours_minutes(text: &str, max_hours: u16) -> Option<u16> {
    let (hours, minutes) = text.split_once(':')?;
    if hours.is_empty() || hours.len() > 2 || minutes.len() != 2 {
        return None;
    }
    let hours: u16 = hours.parse().ok()?;
    let minutes: u16 = minutes.parse().ok()?;
    (hours <= max_hours && minutes < 60).then_some(hours * 60 + minutes)
}

fn clock_text(minutes: i32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// This computer's UTC offset now, in minutes east of UTC, if the system can tell.
pub fn local_offset_minutes() -> Option<i32> {
    time::UtcOffset::current_local_offset()
        .ok()
        .map(|offset| i32::from(offset.whole_minutes()))
}

/// A window as text, for example `19:00–23:00 UTC`.
pub fn window_text(start: QuarterHour, length_minutes: u32) -> String {
    let from = i32::try_from(start.minutes_of_day()).unwrap_or(0);
    let to = (from + i32::try_from(length_minutes).unwrap_or(0)).rem_euclid(MINUTES_PER_DAY);
    format!("{}–{} UTC", clock_text(from), clock_text(to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> WindowSpec {
        text.parse().unwrap()
    }

    #[test]
    fn local_utc_and_explicit_offsets() {
        let q = |h, m| QuarterHour::at(h, m).unwrap();
        // Local 19:00 at UTC+1 is 18:00 UTC; at UTC−5, midnight UTC.
        assert_eq!(WindowSpec::DEFAULT.resolve(Some(60)).unwrap(), q(18, 0));
        assert_eq!(spec("19:00").resolve(Some(-300)).unwrap(), q(0, 0));
        assert_eq!(spec("18:00Z").resolve(None).unwrap(), q(18, 0));
        assert_eq!(spec("19:00+01:00").resolve(Some(600)).unwrap(), q(18, 0));
        assert_eq!(spec("06:30-05:30").resolve(None).unwrap(), q(12, 0));
        // Nepal is UTC+05:45: still a quarter hour.
        assert_eq!(spec("19:00").resolve(Some(345)).unwrap(), q(13, 15));
        assert_eq!(spec("1:15z").resolve(None).unwrap(), q(1, 15));
        // Off the quarter hour in UTC, or with no known local offset: refused.
        assert!(spec("19:10Z").resolve(None).is_err());
        assert!(spec("19:00+00:20").resolve(None).is_err());
        assert!(spec("19:00").resolve(None).is_err());
        assert!(spec("19:00Z").resolve(None).is_ok());
    }

    #[test]
    fn malformed_starts_are_refused() {
        for text in [
            "",
            "19",
            "19:0",
            "24:00",
            "19:60",
            "-1:00",
            "19:00+15:00",
            "19:00+1",
            "ab:cd",
            "19:00Zz",
            "119:00",
        ] {
            assert!(text.parse::<WindowSpec>().is_err(), "{text:?}");
        }
        assert_eq!(spec("19:00").to_string(), "19:00 local");
        assert_eq!(spec("19:00Z").to_string(), "19:00Z");
        assert_eq!(spec("06:30-05:30").to_string(), "06:30-05:30");
    }

    #[test]
    fn windows_read_as_utc_ranges() {
        let q = |h, m| QuarterHour::at(h, m).unwrap();
        assert_eq!(window_text(q(19, 0), 240), "19:00–23:00 UTC");
        assert_eq!(window_text(q(22, 30), 240), "22:30–02:30 UTC");
    }
}
