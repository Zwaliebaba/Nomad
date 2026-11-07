//! From the validator's reasons to what the client may learn (D-077, D-123; N3 plan §4.7).
//!
//! The validator (`nomad-sim`) says exactly why it refused a command. The client is told
//! less: "does not exist", "is not yours" and "is not visible to you" all become
//! [`InvalidReason::NotFound`], so probing identities reveals nothing about entities the
//! commander has not detected (doc 16 §6, doc 20 §9). The match has no wildcard: a new
//! validator reason does not compile until it is mapped here.
//!
//! The mapping hides only what the reason itself says. A reason that depends on where an
//! unseen target is (for example `WrongSystem`, or `SiteTaken` for an undetected outpost) would still leak; the visibility check that
//! S7 adds to the validator must therefore come before such checks.

use nomad_sim::CommandRejection;
use nomad_types::InvalidReason;

/// The wire reason for a validator rejection (D-123).
pub fn wire_reason(rejection: CommandRejection) -> InvalidReason {
    use CommandRejection as C;
    use InvalidReason as W;
    match rejection {
        // Existence, ownership and (with S7) visibility collapse into one reason.
        C::UnknownFleet
        | C::UnknownShip
        | C::UnknownHolding
        | C::UnknownTarget
        | C::UnknownSystem
        | C::NotOwner
        | C::HoldingNotOwned
        // Own queue items and sites (public topology): "does not exist" is NotFound too.
        | C::UnknownItem
        | C::UnknownSite
        // A command from someone who is not a commander of this Universe (S6a).
        | C::UnknownCommander => W::NotFound,
        C::InvalidPosition => W::InvalidPosition,
        C::WrongSystem => W::WrongSystem,
        C::Unreachable => W::Unreachable,
        C::AlreadyThere => W::AlreadyThere,
        C::Docked => W::Docked,
        C::NotDocked => W::NotDocked,
        C::InTransit => W::InTransit,
        C::NoCapacity => W::NoCapacity,
        C::NoDryDock => W::NoDryDock,
        C::NoHolding => W::NoHolding,
        C::NoWeapons => W::NoWeapons,
        C::FriendlyTarget => W::FriendlyTarget,
        C::SelfTarget => W::SelfTarget,
        C::EmptyRoute => W::EmptyRoute,
        C::EmptyFleet => W::EmptyFleet,
        C::DuplicateShip => W::DuplicateShip,
        C::NotTogether => W::NotTogether,
        C::NothingToLaunch => W::NothingToLaunch,
        C::Outpost => W::Outpost,
        C::NotBuildable => W::NotBuildable,
        C::NoSuchSlot => W::NoSuchSlot,
        C::WrongSlot => W::WrongSlot,
        C::SlotTaken => W::SlotTaken,
        C::QueueFull => W::QueueFull,
        C::NoMule => W::NoMule,
        C::CargoFull => W::CargoFull,
        C::NoKit => W::NoKit,
        C::NotClaimable => W::NotClaimable,
        C::SiteTaken => W::SiteTaken,
        C::TooFar => W::TooFar,
        C::RouteTooLong => W::RouteTooLong,
        C::DuplicateStop => W::DuplicateStop,
        C::NotDepot => W::NotDepot,
        // Station orders (S5b).
        C::NoSuchSector => W::NoSuchSector,
        C::AbilityCooldown => W::AbilityCooldown,
        C::SectorOpen => W::SectorOpen,
        C::NoPower => W::NoPower,
        C::InvalidWindow => W::InvalidWindow,
        C::WindowLocked => W::WindowLocked,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Every validator reason. The guard below has no wildcard, so a new reason fails to
    /// compile until it is listed.
    const ALL: [CommandRejection; 49] = [
        CommandRejection::UnknownFleet,
        CommandRejection::UnknownShip,
        CommandRejection::UnknownHolding,
        CommandRejection::UnknownTarget,
        CommandRejection::UnknownSystem,
        CommandRejection::NotOwner,
        CommandRejection::HoldingNotOwned,
        CommandRejection::InvalidPosition,
        CommandRejection::WrongSystem,
        CommandRejection::Unreachable,
        CommandRejection::AlreadyThere,
        CommandRejection::Docked,
        CommandRejection::NotDocked,
        CommandRejection::InTransit,
        CommandRejection::NoCapacity,
        CommandRejection::NoDryDock,
        CommandRejection::NoHolding,
        CommandRejection::NoWeapons,
        CommandRejection::FriendlyTarget,
        CommandRejection::SelfTarget,
        CommandRejection::EmptyRoute,
        CommandRejection::EmptyFleet,
        CommandRejection::DuplicateShip,
        CommandRejection::NotTogether,
        CommandRejection::NothingToLaunch,
        CommandRejection::UnknownItem,
        CommandRejection::UnknownSite,
        CommandRejection::Outpost,
        CommandRejection::NotBuildable,
        CommandRejection::NoSuchSlot,
        CommandRejection::WrongSlot,
        CommandRejection::SlotTaken,
        CommandRejection::QueueFull,
        CommandRejection::NoMule,
        CommandRejection::CargoFull,
        CommandRejection::NoKit,
        CommandRejection::NotClaimable,
        CommandRejection::SiteTaken,
        CommandRejection::TooFar,
        CommandRejection::RouteTooLong,
        CommandRejection::DuplicateStop,
        CommandRejection::NotDepot,
        CommandRejection::NoSuchSector,
        CommandRejection::AbilityCooldown,
        CommandRejection::SectorOpen,
        CommandRejection::NoPower,
        CommandRejection::UnknownCommander,
        CommandRejection::InvalidWindow,
        CommandRejection::WindowLocked,
    ];

    const fn listed(rejection: CommandRejection) -> bool {
        match rejection {
            CommandRejection::UnknownFleet
            | CommandRejection::UnknownShip
            | CommandRejection::UnknownHolding
            | CommandRejection::UnknownTarget
            | CommandRejection::UnknownSystem
            | CommandRejection::NotOwner
            | CommandRejection::HoldingNotOwned
            | CommandRejection::InvalidPosition
            | CommandRejection::WrongSystem
            | CommandRejection::Unreachable
            | CommandRejection::AlreadyThere
            | CommandRejection::Docked
            | CommandRejection::NotDocked
            | CommandRejection::InTransit
            | CommandRejection::NoCapacity
            | CommandRejection::NoDryDock
            | CommandRejection::NoHolding
            | CommandRejection::NoWeapons
            | CommandRejection::FriendlyTarget
            | CommandRejection::SelfTarget
            | CommandRejection::EmptyRoute
            | CommandRejection::EmptyFleet
            | CommandRejection::DuplicateShip
            | CommandRejection::NotTogether
            | CommandRejection::NothingToLaunch
            | CommandRejection::UnknownItem
            | CommandRejection::UnknownSite
            | CommandRejection::Outpost
            | CommandRejection::NotBuildable
            | CommandRejection::NoSuchSlot
            | CommandRejection::WrongSlot
            | CommandRejection::SlotTaken
            | CommandRejection::QueueFull
            | CommandRejection::NoMule
            | CommandRejection::CargoFull
            | CommandRejection::NoKit
            | CommandRejection::NotClaimable
            | CommandRejection::SiteTaken
            | CommandRejection::TooFar
            | CommandRejection::RouteTooLong
            | CommandRejection::DuplicateStop
            | CommandRejection::NotDepot
            | CommandRejection::NoSuchSector
            | CommandRejection::AbilityCooldown
            | CommandRejection::SectorOpen
            | CommandRejection::NoPower
            | CommandRejection::UnknownCommander
            | CommandRejection::InvalidWindow
            | CommandRejection::WindowLocked => true,
        }
    }

    #[test]
    fn every_reason_maps_and_existence_is_hidden() {
        assert!(ALL.iter().all(|&r| listed(r)));
        assert_eq!(ALL.iter().collect::<BTreeSet<_>>().len(), ALL.len());
        let hidden = [
            CommandRejection::UnknownFleet,
            CommandRejection::UnknownShip,
            CommandRejection::UnknownHolding,
            CommandRejection::UnknownTarget,
            CommandRejection::UnknownSystem,
            CommandRejection::NotOwner,
            CommandRejection::HoldingNotOwned,
            CommandRejection::UnknownItem,
            CommandRejection::UnknownSite,
            CommandRejection::UnknownCommander,
        ];
        for rejection in ALL {
            let wire = wire_reason(rejection);
            assert_eq!(
                wire == InvalidReason::NotFound,
                hidden.contains(&rejection),
                "{rejection:?} maps to {wire:?}"
            );
        }
        // Every other wire reason is reachable, and only from one validator reason.
        let others: BTreeSet<InvalidReason> = ALL
            .iter()
            .filter(|r| !hidden.contains(r))
            .map(|&r| wire_reason(r))
            .collect();
        assert_eq!(others.len(), ALL.len() - hidden.len());
    }
}
