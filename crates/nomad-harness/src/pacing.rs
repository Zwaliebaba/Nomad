//! D-070's pacing measures with D-218 §5.4's definitions (amended by D-283), computed from
//! hourly samples of one commander's economy. `--economy` uses them for the scripted
//! builders; the α `--season` runs are to use the same functions for AI commanders.
//!
//! - **Storage fill:** the alloy pool cap ÷ alloy income, at the start and as the means over
//!   days 21–35 (crystal is reported alongside).
//! - **First expansion:** when the first core kit started, from the first online minute (the
//!   scripted builders are always online, so from the season start).
//! - **Hours per new holding over days 0–3:** 72 h ÷ the holdings completed by hour 72.
//! - **Full cap-fleet rebuild:** the cost of a fleet at the mean command cap over days 21–35,
//!   in the Garrison stand-in's composition (A2 §4.7: by value about 30% VX, 40% LANCER and
//!   30% WARDEN), ÷ the mean income over the same days, in hours of the binding resource.
//! - **Build-out (D-283):** the time from a holding's core completion to its full build-out.
//! - **Late income:** the mean alloy income over days 42–56 ÷ 600 A/h.
//!
//! *Derived:* a run too short for a window measures the last 7 days instead (the last third,
//! if it is shorter than 21 days), and the report marks the result as a proxy. This rests on
//! the scripted builders' economy becoming stationary once they have claimed the sites they
//! reach (by about day 10): in the D-279 tuning study the measures were identical to two
//! decimals over days 10–14, 14–21 and 21–25. A full 56-day run takes hours in a release
//! build, so tuning runs use 14 or 21 days and the α runs confirm with the full windows.

use nomad_sim::economy::rules::EconomyRules;
use nomad_sim::{ShipClass, Stock};

/// Hours in a day.
const DAY_H: u64 = 24;

/// One commander's economy at the end of an hour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Hours since the season start.
    pub hour: u64,
    /// Everything produced since the season start, milli-units.
    pub produced: Stock,
    /// The pool cap, milli-units.
    pub cap: Stock,
    /// The command cap.
    pub command_cap: u32,
    /// Completed holdings, the mother station included.
    pub holdings: u32,
}

/// The hours a measure averages over, `(from, to]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The first hour, exclusive.
    pub from: u64,
    /// The last hour, inclusive.
    pub to: u64,
    /// Whether the run was too short and a later stretch stands in for D-218's window.
    pub proxy: bool,
}

impl Window {
    /// D-218's window from `from_day` to `to_day` for a run of `run_h` hours, or the proxy.
    pub fn days(from_day: u64, to_day: u64, run_h: u64) -> Self {
        if run_h >= to_day * DAY_H {
            return Self {
                from: from_day * DAY_H,
                to: to_day * DAY_H,
                proxy: false,
            };
        }
        let span = if run_h >= 21 * DAY_H {
            7 * DAY_H
        } else {
            (run_h / 3).max(1)
        };
        Self {
            from: run_h.saturating_sub(span),
            to: run_h,
            proxy: true,
        }
    }

    /// "days a–b", with a proxy marked.
    pub fn label(&self) -> String {
        let day = |h: u64| h as f64 / DAY_H as f64;
        format!(
            "days {:.0}–{:.0}{}",
            day(self.from),
            day(self.to),
            if self.proxy {
                ", proxy for D-218's window"
            } else {
                ""
            }
        )
    }
}

/// Mean income per hour over `w` in units (alloy, crystal), from the produced totals.
pub fn income(samples: &[Sample], w: Window) -> Option<(f64, f64)> {
    let at = |h: u64| {
        if h == 0 {
            Some(Stock::ZERO)
        } else {
            samples.iter().find(|s| s.hour == h).map(|s| s.produced)
        }
    };
    let (a, b) = (at(w.from)?, at(w.to)?);
    let hours = (w.to - w.from) as f64;
    Some((
        (b.alloy.saturating_sub(a.alloy)) as f64 / 1_000.0 / hours,
        (b.crystal.saturating_sub(a.crystal)) as f64 / 1_000.0 / hours,
    ))
}

/// Means of the pool cap (units) and command cap over `w`.
pub fn caps(samples: &[Sample], w: Window) -> Option<(f64, f64, f64)> {
    let inside: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.hour > w.from && s.hour <= w.to)
        .collect();
    if inside.is_empty() {
        return None;
    }
    let n = inside.len() as f64;
    let sum = |f: &dyn Fn(&Sample) -> f64| inside.iter().map(|s| f(s)).sum::<f64>() / n;
    Some((
        sum(&|s| s.cap.alloy as f64 / 1_000.0),
        sum(&|s| s.cap.crystal as f64 / 1_000.0),
        sum(&|s| f64::from(s.command_cap)),
    ))
}

/// Alloy and crystal per command point of a fleet in the Garrison stand-in's composition
/// (A2 §4.7): by value 30% VX, 40% LANCER, 30% WARDEN, value counted in alloy.
pub fn garrison_per_command(rules: &EconomyRules) -> (f64, f64) {
    let shares = [
        (ShipClass::Vx, 0.3),
        (ShipClass::Lancer, 0.4),
        (ShipClass::Warden, 0.3),
    ];
    let (mut command, mut alloy, mut crystal) = (0.0, 0.0, 0.0);
    for (class, share) in shares {
        let Some(ship) = rules.ship(class) else {
            continue;
        };
        let per_alloy = share / ship.cost.alloy.max(1) as f64;
        command += per_alloy * f64::from(ship.command);
        alloy += per_alloy * ship.cost.alloy as f64;
        crystal += per_alloy * ship.cost.crystal as f64;
    }
    if command == 0.0 {
        return (0.0, 0.0);
    }
    (alloy / command, crystal / command)
}

/// Hours of income a fleet of `command` points in the Garrison composition costs: (alloy
/// hours, crystal hours). The rebuild takes the larger.
pub fn rebuild_hours(per_command: (f64, f64), command: f64, income: (f64, f64)) -> (f64, f64) {
    let hours = |cost: f64, rate: f64| {
        if cost <= 0.0 {
            0.0
        } else if rate <= 0.0 {
            f64::INFINITY
        } else {
            cost / rate
        }
    };
    (
        hours(per_command.0 * command, income.0),
        hours(per_command.1 * command, income.1),
    )
}

/// 72 h ÷ the holdings completed by hour 72 (`completions` in hours), or `None` if none was.
pub fn hours_per_holding_days_0_3(completions: &[f64]) -> Option<f64> {
    let early = completions.iter().filter(|&&h| h <= 72.0).count();
    (early > 0).then(|| 72.0 / early as f64)
}

/// The median of `values`, or NaN for none.
pub fn median(values: &[f64]) -> f64 {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        f64::midpoint(v[mid - 1], v[mid])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomad_sim::Rules;

    fn sample(hour: u64, alloy: u64, crystal: u64, cap: u64, command_cap: u32) -> Sample {
        Sample {
            hour,
            produced: Stock::units(alloy, crystal),
            cap: Stock::units(cap, 0),
            command_cap,
            holdings: 1,
        }
    }

    #[test]
    fn windows_are_d218s_or_a_marked_proxy() {
        assert_eq!(
            Window::days(21, 35, 56 * 24),
            Window {
                from: 504,
                to: 840,
                proxy: false
            }
        );
        // 21 days: the last 7 days stand in.
        assert_eq!(
            Window::days(21, 35, 21 * 24),
            Window {
                from: 336,
                to: 504,
                proxy: true
            }
        );
        // 7 days: the last third.
        assert_eq!(Window::days(42, 56, 168).from, 112);
        assert!(Window::days(42, 56, 168).label().contains("proxy"));
        assert!(!Window::days(0, 1, 24).proxy);
    }

    #[test]
    fn income_and_caps_are_window_means() {
        let samples: Vec<Sample> = (1..=10)
            .map(|h| {
                sample(
                    h,
                    600 * h,
                    60 * h,
                    1_000 * h,
                    100 + u32::try_from(h).unwrap(),
                )
            })
            .collect();
        let w = Window {
            from: 4,
            to: 10,
            proxy: false,
        };
        assert_eq!(income(&samples, w), Some((600.0, 60.0)));
        // Hours 5..=10.
        assert_eq!(caps(&samples, w), Some((7_500.0, 0.0, 107.5)));
        let from_start = Window {
            from: 0,
            to: 2,
            proxy: false,
        };
        assert_eq!(income(&samples, from_start), Some((600.0, 60.0)));
    }

    #[test]
    fn the_garrison_mix_costs_what_a2_says() {
        // A2 §5.3: a 300-command fleet costs about 54,000 A at the starting ship costs.
        let rules = Rules::embedded();
        let (alloy, crystal) = garrison_per_command(&rules.economy);
        assert!((alloy * 300.0 - 55_814.0).abs() < 1.0, "{alloy}");
        assert!((crystal * 300.0 - 3_628.0).abs() < 1.0, "{crystal}");
        let (a, c) = rebuild_hours((alloy, crystal), 300.0, (2_000.0, 0.0));
        assert!((a - 27.9).abs() < 0.1, "{a}");
        assert!(c.is_infinite());
    }

    #[test]
    fn hours_per_holding_counts_only_days_0_to_3() {
        assert_eq!(
            hours_per_holding_days_0_3(&[2.5, 28.1, 56.0, 80.0]),
            Some(24.0)
        );
        assert_eq!(hours_per_holding_days_0_3(&[100.0]), None);
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-12);
        assert!((median(&[4.0, 1.0, 2.0, 3.0]) - 2.5).abs() < 1e-12);
        assert!(median(&[]).is_nan());
    }
}
