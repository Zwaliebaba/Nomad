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
//! status is private). The siege kinds (breach, siege start, Held, capture, emergency jump)
//! arrive with S6b. *Proposed* shapes (S6 plan §4.8); `OrdersLapsed` and the window's
//! deadlines in `WindowMoved` are additions for the away report and the window panel.

use nomad_types::QuarterHour;

use crate::fleet::FleetId;
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
