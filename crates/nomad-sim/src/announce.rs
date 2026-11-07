//! Announcements: what the simulation tells commanders, as data (S6 plan §4.8; doc 16 §2,
//! D-075).
//!
//! Each tick's [`crate::TickOutputs::announcements`] carries them, each with its tick, its
//! recipients and what it says. The simulation keeps no event log: everything an
//! announcement says can be rebuilt from the state and the outputs, and replay reproduces the
//! outputs. The server's read models (the away report's 7-day history, D-092) and the wire
//! (`nomad-protocol`'s announcements, N4) are fed from here; S7 and N4 decide what else a
//! commander sees. Deadlines are sent, not countdowns (doc 11 §3).
//!
//! S6a announces window moves (public, D-075) and lapsed orders (to the owner, since online
//! status is private). S6b announces the siege lifecycle: a breach, a siege's start and Held
//! go to the owner and the attackers with breach credit (D-075); a capture, a fallen mother
//! station's site and its landing are public, because ownership is (D-044); a mother
//! station's jump and the emergency jumps of docked ships go to their owner, because they
//! are as invisible as lane transit (D-166). *Proposed* shapes (S6 plan §4.8);
//! `OrdersLapsed`, the window's deadlines in `WindowMoved`, `FleetsJumped`, `StationLanded`,
//! `MotherStationFell` and `OutpostDestroyed` are additions for the away report, the window
//! panel and the public ownership changes.
//!
//! S8 announces control changes, standings changes, the near-win warning and the season's
//! end, all public (D-075, D-197; S8 plan §4.5; [`crate::season`]).

use nomad_types::{ControlState, QuarterHour, SeasonResult, SiteId, Standing, SystemId};

use crate::economy::Stock;
use crate::fleet::FleetId;
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::time::Tick;

/// One announcement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    /// The tick it happened on.
    pub tick: Tick,
    /// Who it is for.
    pub to: Recipients,
    /// What it says.
    pub kind: AnnouncementKind,
}

/// Who an announcement is for (doc 16 §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipients {
    /// Everyone: public information (D-044, D-075).
    Public,
    /// These commanders, in identity order.
    Commanders(Vec<CommanderId>),
}

impl Recipients {
    /// Whether `commander` receives it.
    pub fn includes(&self, commander: CommanderId) -> bool {
        match self {
            Self::Public => true,
            Self::Commanders(list) => list.contains(&commander),
        }
    }
}

/// What an announcement says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnouncementKind {
    /// A commander's exposure window moved when it closed (D-155, D-157). Public (D-075).
    WindowMoved {
        /// Whose window.
        commander: CommanderId,
        /// The new start, UTC.
        start: QuarterHour,
        /// The tick its next occurrence opens.
        opens: Tick,
        /// The tick that occurrence closes.
        closes: Tick,
    },
    /// The disconnect grace ended with the commander offline: these fleets dropped their
    /// direct orders and station-keep (doc 12 §5.3, D-162). To the owner only (D-075).
    OrdersLapsed {
        /// Whose fleets.
        commander: CommanderId,
        /// The fleets, in identity order.
        fleets: Vec<FleetId>,
    },

    // S8: season standings and the result, all public (D-075; S8 plan §4.5).
    /// A system's control state changed (D-041).
    ControlChanged {
        /// The system.
        system: SystemId,
        /// Its state before.
        from: ControlState,
        /// Its state now.
        to: ControlState,
    },
    /// The standings changed: at most once per tick (D-191).
    StandingsChanged {
        /// The new standings, best first.
        standings: Vec<Standing>,
    },
    /// A commander is one system short of the early win (D-197).
    NearEarlyWin {
        /// Who.
        commander: CommanderId,
        /// The systems they control.
        systems: u32,
        /// The early-win threshold (D-192).
        threshold: u32,
    },
    /// A commander who was one system short of the early win dropped back (D-197).
    NearEarlyWinEnded {
        /// Who.
        commander: CommanderId,
    },
    /// The season ended: this is its final tick (D-116, D-192, D-195).
    SeasonEnded {
        /// The result.
        result: SeasonResult,
        /// The final standings, best first.
        standings: Vec<Standing>,
    },
    // S6b: the siege lifecycle.
    /// A sector reached 0: the holding is Reinforced until `exit_tick`, when its siege
    /// starts (D-015, D-158). To the owner and the attackers (D-075).
    Breached {
        /// The holding.
        holding: HoldingId,
        /// The breached sector, which opens when the siege starts.
        sector: u8,
        /// The siege starts then: the Reinforced exit (D-033, D-158).
        exit_tick: Tick,
        /// The commanders with breach credit, in identity order (D-057).
        attackers: Vec<CommanderId>,
    },
    /// A siege started: the sector is open and the clock runs until `end_tick` (D-057). To
    /// the owner and the attackers.
    SiegeStarted {
        /// The holding.
        holding: HoldingId,
        /// The siege clock runs out then (D-015).
        end_tick: Tick,
        /// The open sector.
        open_sector: u8,
    },
    /// The holding held: its sectors are back to at least 50% and it cannot be breached
    /// until `breachable_from` (D-015, D-057). To the owner and the attackers.
    Held {
        /// The holding.
        holding: HoldingId,
        /// The siege clock ran out, or no attacker was online at the exit.
        reason: HeldReason,
        /// The protection ends then (D-170).
        breachable_from: Tick,
    },
    /// A holding fell and changed owner (D-057, D-163). Public (D-044).
    Captured {
        /// The holding.
        holding: HoldingId,
        /// The old owner.
        from: CommanderId,
        /// The new owner, protected until `breachable_from`.
        to: CommanderId,
        /// The new owner's protection ends then (D-057, D-170).
        breachable_from: Tick,
    },
    /// The old owner's docked ships emergency-jump to their nearest holding after a capture,
    /// or with their fallen mother station (D-057, D-166). To the owner (invisible, D-074).
    FleetsJumped {
        /// Whose fleets.
        commander: CommanderId,
        /// Each fleet, where it jumps to and when it arrives, in identity order.
        fleets: Vec<(FleetId, SiteId, Tick)>,
    },
    /// A mother station fell: its other modules are destroyed and the site passes to `to`
    /// as an outpost under construction (D-034, D-057). Public (D-044).
    MotherStationFell {
        /// The site.
        site: SiteId,
        /// The commander whose mother station it was.
        from: CommanderId,
        /// The commander who gains the site.
        to: CommanderId,
        /// The new outpost.
        outpost: HoldingId,
    },
    /// The fallen mother station's emergency jump (D-164 to D-167). To the owner.
    MotherStationJumped {
        /// The station, which lands as this holding again.
        holding: HoldingId,
        /// Where it fell.
        from_site: SiteId,
        /// Where it lands; `None` if no site is free and it waits by a holding until one
        /// frees up (D-164).
        to_site: Option<SiteId>,
        /// When it arrives, unless it waits where it fell.
        arrival_tick: Option<Tick>,
        /// The landed station cannot be breached until then (D-034), if it lands.
        protected_until: Option<Tick>,
        /// The comeback grant paid into the pool (D-167).
        grant: Stock,
    },
    /// A fallen mother station landed: a holding again, protected until `breachable_from`
    /// (D-034, D-166). Public (D-044).
    StationLanded {
        /// Whose station.
        commander: CommanderId,
        /// The holding.
        holding: HoldingId,
        /// The site.
        site: SiteId,
        /// The protection ends then (D-034, D-170).
        breachable_from: Tick,
    },
    /// An outpost under construction was destroyed (D-054). Public (D-044).
    OutpostDestroyed {
        /// The outpost, which no longer exists.
        holding: HoldingId,
        /// Its site, free again.
        site: SiteId,
        /// Whose it was.
        owner: CommanderId,
    },
}

/// Why a holding held (S6 plan §4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldReason {
    /// The siege clock ran out with the core standing (D-015).
    Clock,
    /// No attacker counted as online when the siege was due (D-057, D-160).
    NoAttackerOnline,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_decide_who_hears_it() {
        assert!(Recipients::Public.includes(CommanderId(9)));
        let owner = Recipients::Commanders(vec![CommanderId(2)]);
        assert!(owner.includes(CommanderId(2)));
        assert!(!owner.includes(CommanderId(3)));
    }
}
