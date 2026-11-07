//! Dome sectors: maxima, regeneration and the regeneration demand for S4 (doc 15 §5.1–§5.2;
//! D-140 to D-142).
//!
//! Strength is in integer milli-points. Regeneration per tick is the allocated power times
//! `regen_points_per_s / regen_mw_per_sector` (D-142: MW × 50 / 120 per second), with the
//! remainder carried to the next tick, so a sector regenerates exactly over time without
//! floating point (D-049). The demand S5b reports to S4 is what fills the sector's deficit in
//! one second, at most the sector's full draw, which scales with its maximum (D-142; S4 caps
//! it the same way by pylon).
//!
//! **Time:** each holding's dome is refreshed (it comes up or goes, its maxima follow its
//! pylons) and its demand is set once a second, in the holding's economy slot just before
//! S4 allocates power there (doc 11 §4.1). Regeneration runs every tick, and only for
//! sectors below their maximum, so an idle Universe pays almost nothing for its domes.

use nomad_types::Slot;

use crate::economy::Module;
use crate::economy::rules::{KW_PER_MW, MILLI};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::rules::Rules;
use crate::state::UniverseState;
use crate::time::TICKS_PER_SECOND;

/// The dome's radius in metres, the same for every core (D-141): the slot template's, in
/// the station layout's voxels.
pub(crate) fn radius_m(rules: &Rules) -> f64 {
    f64::from(rules.economy.slots.dome_radius_voxels) * rules.mother_station.voxel_m
}

/// The grid frame of a holding's slots, as in [`crate::fleet::station::grid_to_system`]: a
/// grid point in voxels to a displacement in the system frame. Computed once and used for
/// many points; the arithmetic is the same, so the results are bit-identical.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GridFrame {
    east: Vec2,
    north: Vec2,
    voxel_m: f64,
}

impl GridFrame {
    pub(crate) fn new(rules: &Rules) -> Self {
        let layout = &rules.mother_station;
        let north = Vec2::from_angle(layout.grid_north_azimuth_deg.to_radians());
        Self {
            east: Vec2::new(-north.y, north.x),
            north,
            voxel_m: layout.voxel_m,
        }
    }

    pub(crate) fn to_system(self, grid: [i32; 2]) -> Vec2 {
        let g = Vec2::new(f64::from(grid[0]), f64::from(grid[1]));
        Vec2::new(g.dot(self.east), g.dot(self.north)) * self.voxel_m
    }
}

/// The direction of each sector's pylon slot from the dome centre, in the system frame.
/// A sector's arc is centred on its pylon (D-046), so the sector of a crossing point is the
/// one whose pylon direction is nearest.
pub(crate) fn pylon_directions(rules: &Rules) -> [Vec2; 4] {
    let frame = GridFrame::new(rules);
    rules
        .economy
        .slots
        .pylons
        .map(|p| frame.to_system(p).normalized().unwrap_or(Vec2::ZERO))
}

/// The sector whose arc contains the direction `offset` from the dome centre: the nearest
/// pylon direction, the lower index on a boundary. With the template's pylons at grid
/// azimuths 225°, 315°, 45° and 135° these are D-046's arcs N 180–270°, E 270–360°,
/// S 0–90° and W 90–180°.
pub(crate) fn sector_at(directions: &[Vec2; 4], offset: Vec2) -> u8 {
    let mut best = (0_u8, f64::NEG_INFINITY);
    for (i, direction) in (0_u8..).zip(directions) {
        let dot = direction.dot(offset);
        if dot > best.1 {
            best = (i, dot);
        }
    }
    best.0
}

/// Whether `holding` projects a dome: its core is complete and present (doc 15 §5.1).
pub(crate) fn has_dome(state: &UniverseState, holding: HoldingId) -> bool {
    state
        .holdings
        .get(&holding)
        .is_some_and(|h| h.core_complete)
        && state
            .economy
            .holdings
            .get(&holding)
            .and_then(|e| e.slots.get(&Slot::Core))
            .is_some_and(|&core| state.entities.has::<Module>(core))
}

/// Each sector's maximum in milli-points: 25% of a full sector from the core, plus 75% if the
/// sector's own pylon is complete and working (D-140, D-141).
pub(crate) fn maxima(state: &UniverseState, holding: HoldingId) -> [u64; 4] {
    let d = &state.rules.defence;
    let full = d.sector_full_points.saturating_mul(MILLI);
    state.pylons_per_sector(holding).map(|pylon| {
        let share = d.core_share_percent + if pylon { d.pylon_share_percent } else { 0 };
        full.saturating_mul(share) / 100
    })
}

/// The full regeneration draw of a sector with maximum `max`, in kW: the full sector's draw,
/// scaled by the maximum (D-142).
fn full_draw_kw(rules: &Rules, max: u64) -> u64 {
    let full = rules
        .defence
        .sector_full_points
        .saturating_mul(MILLI)
        .max(1);
    let draw = rules
        .economy
        .power
        .regen_mw_per_sector
        .saturating_mul(KW_PER_MW);
    u64::try_from(u128::from(draw) * u128::from(max) / u128::from(full)).unwrap_or(draw)
}

/// Brings `holding`'s dome up to date: it comes up at its maximum, goes when the core does,
/// and each sector keeps its maximum (D-140), its strength clamped to it. Runs once a second
/// in the holding's economy slot, and before anything that needs the maxima of this tick
/// (an ability, S6's switches).
pub(crate) fn refresh(state: &mut UniverseState, holding: HoldingId) {
    let up = has_dome(state, holding);
    let maxima = if up { maxima(state, holding) } else { [0; 4] };
    let d = state.defence.holdings.entry(holding).or_default();
    if !up {
        d.dome_up = false;
        return;
    }
    if !d.dome_up {
        // A new dome comes up at its maximum: the core projects it (D-053).
        d.dome_up = true;
        for (sector, &max) in d.sectors.iter_mut().zip(&maxima) {
            *sector = super::Sector {
                strength: max,
                max,
                ..super::Sector::default()
            };
        }
        return;
    }
    for (sector, &max) in d.sectors.iter_mut().zip(&maxima) {
        sector.max = max;
        if sector.strength >= max {
            sector.strength = max;
            sector.carry = 0;
            sector.breach_reported = false;
        }
    }
}

/// One tick of regeneration for every closed sector below its maximum, from S4's allocation
/// (D-142). A sector that reaches its maximum re-arms its breach event.
pub(crate) fn regenerate(state: &mut UniverseState) {
    let points = state.rules.defence.regen_points_per_s;
    let per_tick = state
        .rules
        .economy
        .power
        .regen_mw_per_sector
        .saturating_mul(TICKS_PER_SECOND)
        .max(1);
    let economy = &state.economy.holdings;
    for (holding, d) in &mut state.defence.holdings {
        if !d.dome_up {
            continue;
        }
        for (i, sector) in (0_u8..).zip(d.sectors.iter_mut()) {
            if sector.open || sector.strength >= sector.max {
                continue;
            }
            let kw = economy
                .get(holding)
                .map_or(0, |e| e.allocation.regeneration_kw(i));
            let total = kw.saturating_mul(points).saturating_add(sector.carry);
            sector.carry = total % per_tick;
            sector.strength = sector.strength.saturating_add(total / per_tick);
            if sector.strength >= sector.max {
                sector.strength = sector.max;
                sector.carry = 0;
                sector.breach_reported = false;
            }
        }
    }
}

/// Sets `holding`'s regeneration demand per sector for S4's allocation (S4 plan §4.6), right
/// before it in the same tick: what fills each sector's deficit in one second, at most its
/// full draw (D-142); nothing while open or full.
pub(crate) fn set_demand(state: &mut UniverseState, holding: HoldingId) {
    let points = state.rules.defence.regen_points_per_s.max(1);
    let mw = state.rules.economy.power.regen_mw_per_sector;
    let sectors = state
        .defence
        .holdings
        .get(&holding)
        .filter(|d| d.dome_up)
        .map(|d| d.sectors);
    let demand: [u64; 4] = std::array::from_fn(|i| {
        let Some(sector) = sectors.map(|s| s[i]) else {
            return 0;
        };
        if sector.open || sector.strength >= sector.max {
            return 0;
        }
        // kW for one second: deficit (milli-points) × MW / points per second.
        let deficit = sector.max - sector.strength;
        let needed =
            u64::try_from((u128::from(deficit) * u128::from(mw)).div_ceil(u128::from(points)))
                .unwrap_or(u64::MAX);
        needed.min(full_draw_kw(&state.rules, sector.max))
    });
    if let Some(e) = state.economy.holdings.get_mut(&holding) {
        e.regen_demand_kw = demand;
    }
}
