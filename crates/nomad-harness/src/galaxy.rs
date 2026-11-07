//! `--galaxy`: generate the MVP stage-1 sector for a seed and print a summary (Design doc 10
//! §4.4): systems by kind, sites, lanes with transit times, diameter, worst-case trip,
//! deposit values (doc 14 §5) and control states (D-041).

use std::collections::BTreeMap;
use std::io::{self, Write};

use nomad_sim::galaxy::{
    GalaxyReport, InvariantLimits, STAGE1_COMMANDERS, generate_stage1_with_rules, validate,
};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    CommanderId, ControlState, Rules, SeasonConfig, SiteId, SystemId, SystemKind, Ticks,
    UniverseState,
};

/// Commanders of the generated Universe: 1 human and 3 AI (doc 10 §3).
const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

/// Generates the stage-1 sector for `seed` with `rules` and writes its summary to `out`.
pub fn print(out: &mut impl Write, seed: u64, rules: Rules) -> Result<(), String> {
    let state = generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules)
        .map_err(|e| e.to_string())?;
    let limits = InvariantLimits::from_rules(state.rules());
    let report = validate(state.galaxy(), &limits);
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    Summary {
        state: &state,
        report: &report,
        limits: &limits,
        labels: labels(&state),
    }
    .write(out, seed, hash)
    .map_err(|e| e.to_string())
}

/// Labels as in the doc 10 §4.4 diagram: H1–H4, N1–N4, C and R, numbered in identity order.
fn labels(state: &UniverseState) -> BTreeMap<SystemId, String> {
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    state
        .galaxy()
        .systems()
        .map(|(id, system)| {
            let (prefix, numbered) = match system.kind {
                SystemKind::Home => ("H", true),
                SystemKind::Standard => ("N", true),
                SystemKind::Hub => ("C", false),
                SystemKind::RaiderHaven => ("R", false),
            };
            let n = seen.entry(prefix).or_default();
            *n += 1;
            let label = if numbered {
                format!("{prefix}{n}")
            } else {
                prefix.to_owned()
            };
            (id, label)
        })
        .collect()
}

struct Summary<'a> {
    state: &'a UniverseState,
    report: &'a GalaxyReport,
    limits: &'a InvariantLimits,
    labels: BTreeMap<SystemId, String>,
}

impl Summary<'_> {
    fn label(&self, id: SystemId) -> &str {
        self.labels.get(&id).map_or("?", String::as_str)
    }

    fn site(&self, site: SiteId) -> String {
        let system = self
            .state
            .galaxy()
            .site(site)
            .map_or("?", |s| self.label(s.system));
        format!("site #{} in {system}", site.sim_id().get())
    }

    fn write(&self, out: &mut impl Write, seed: u64, hash: u64) -> io::Result<()> {
        let galaxy = self.state.galaxy();
        let count = |kind| galaxy.systems().filter(|(_, s)| s.kind == kind).count();
        writeln!(out, "stage-1 galaxy | seed {seed} | state hash {hash:016x}")?;
        writeln!(
            out,
            "{} systems ({} home, {} standard, {} hub, {} raider haven) | {} sites | {} gates | \
             {} jump lanes",
            galaxy.systems().count(),
            count(SystemKind::Home),
            count(SystemKind::Standard),
            count(SystemKind::Hub),
            count(SystemKind::RaiderHaven),
            galaxy.sites().count(),
            galaxy.gates().count(),
            galaxy.lanes().count(),
        )?;
        self.write_systems(out)?;
        self.write_lanes(out)?;
        self.write_measurements(out)?;
        for (_, holding) in self.state.holdings() {
            writeln!(
                out,
                "commander {} mother station on {}",
                holding.owner.0,
                self.site(holding.site),
            )?;
        }
        if self.report.is_valid() {
            writeln!(
                out,
                "invariants: all checked invariants hold (doc 10 §4.3, 1-6)"
            )
        } else {
            for violation in &self.report.violations {
                writeln!(out, "violation: {violation}")?;
            }
            Ok(())
        }
    }

    fn write_systems(&self, out: &mut impl Write) -> io::Result<()> {
        let galaxy = self.state.galaxy();
        let control = self.state.control_states();
        writeln!(out, "systems:")?;
        for (id, system) in galaxy.systems() {
            let control = match control.get(&id) {
                Some(ControlState::Controlled(c)) => format!("controlled by commander {}", c.0),
                Some(ControlState::Contested) => "contested".to_owned(),
                Some(ControlState::Open) => "open".to_owned(),
                None => "not claimable".to_owned(),
            };
            let deposits: usize = system
                .sites
                .iter()
                .filter_map(|&s| galaxy.site(s))
                .map(|s| s.deposits.len())
                .sum();
            writeln!(
                out,
                "  {:<2} {:<12} #{:<3} {} sites, {} gates, {:>2} deposits, value {:>5} | {control}",
                self.label(id),
                format!("{:?}", system.kind),
                id.sim_id().get(),
                system.sites.len(),
                system.gates.len(),
                deposits,
                galaxy.system_value(id).unwrap_or_default().to_string(),
            )?;
        }
        Ok(())
    }

    fn write_lanes(&self, out: &mut impl Write) -> io::Result<()> {
        let galaxy = self.state.galaxy();
        writeln!(out, "jump lanes:")?;
        for (_, lane) in galaxy.lanes() {
            let Some((a, b)) = galaxy.lane_systems(lane) else {
                continue;
            };
            writeln!(
                out,
                "  {:<2} - {:<2} {:>2} min",
                self.label(a),
                self.label(b),
                lane.transit.get() / Ticks::from_minutes(1).get(),
            )?;
        }
        Ok(())
    }

    fn write_measurements(&self, out: &mut impl Write) -> io::Result<()> {
        let (report, limits) = (self.report, self.limits);
        for sector in &report.sectors {
            let diameter = sector
                .diameter_jumps
                .map_or_else(|| "unbounded".to_owned(), |d| format!("{d} jumps"));
            writeln!(
                out,
                "diameter {diameter} | cycle rank {} | dead ends {}",
                sector.cycle_rank, sector.dead_ends,
            )?;
            if let Some(trip) = sector.worst_trip {
                writeln!(
                    out,
                    "worst-case trip {:.2} h, {} to {} (limit {} h; {} m/s cruise, {} m/s in \
                     ops areas, {} s spool-up per jump)",
                    trip.duration_s / 3_600.0,
                    self.site(trip.from),
                    self.site(trip.to),
                    limits.max_sector_trip.get() / Ticks::from_hours(1).get(),
                    limits.slowest_warship_cruise_m_per_s,
                    limits.slowest_warship_tactical_m_per_s,
                    limits.gate_spool_up.get() / Ticks::from_secs(1).get(),
                )?;
            }
        }
        writeln!(
            out,
            "closest homes {} jumps apart | longest in-system trip {:.1} min | closest sites \
             {:.1} km",
            report
                .min_home_separation_jumps
                .map_or_else(|| "-".to_owned(), |j| j.to_string()),
            report.longest_in_system_trip_s / 60.0,
            report.closest_sites_m.unwrap_or(0.0) / 1_000.0,
        )?;

        let values: Vec<String> = report
            .home_neighbourhoods
            .iter()
            .map(|(&id, value)| format!("{} {value}", self.label(id)))
            .collect();
        let milli: Vec<u64> = report
            .home_neighbourhoods
            .values()
            .map(|v| v.milli())
            .collect();
        let spread = match (milli.iter().min(), milli.iter().max()) {
            (Some(&lo), Some(&hi)) if lo > 0 => {
                format!("{:.1} %", (hi - lo) as f64 * 100.0 / lo as f64)
            }
            _ => "-".to_owned(),
        };
        writeln!(
            out,
            "home neighbourhood values: {} | spread {spread} (limit {} %)",
            values.join(", "),
            limits.max_neighbourhood_spread_percent,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_lists_the_stage1_sector() {
        let mut out = Vec::new();
        print(&mut out, 7, Rules::embedded()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("10 systems (4 home, 4 standard, 1 hub, 1 raider haven)"));
        assert!(text.contains("60 sites | 26 gates | 13 jump lanes"));
        assert!(text.contains("diameter 4 jumps"));
        assert!(text.contains("all checked invariants hold"));
        assert_eq!(text.matches("mother station").count(), 4);
    }
}
