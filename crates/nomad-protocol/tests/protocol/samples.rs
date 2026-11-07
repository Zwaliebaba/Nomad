//! Sample messages: at least one of every variant.

use std::collections::{BTreeMap, BTreeSet};

use nomad_protocol::{
    Announcement, AuthToken, ByeReason, CatchUpProgress, ClientMessage, CommandOutcome,
    EntityChanges, EntityEntry, EntityKind, Location, Notification, NotificationKind,
    PROTOCOL_VERSION, RejectReason, ServerMessage, ViewChanges, ViewSnapshot,
};
use nomad_types::{
    BuildItem, Command, CommanderId, ControlState, DoctrineCommand, DoctrinePreset, EconomyCommand,
    FactionId, FleetDoctrine, FleetId, FleetOrder, FleetRole, HoldingDoctrine, HoldingId,
    HoldingOrder, InvalidReason, LaneKind, ModuleKind, Owner, PositionMetres, PowerProfile,
    QuarterHour, Queueing, ReplacementEntry, RulesOfEngagement, SeasonClock, ShipClass, ShipId,
    SimId, SiteId, Slot, StationAbility, SystemId, SystemKind, Tick, UniverseId,
};

pub const YOU: CommanderId = CommanderId(1);
pub const HOSTILE: CommanderId = CommanderId(2);
pub const RAIDERS: FactionId = FactionId(1);

pub fn id(raw: u64) -> SimId {
    SimId::from_raw(raw)
}

pub fn point(x: f64, y: f64) -> PositionMetres {
    PositionMetres::new(x, y)
}

/// A point in system `system`.
pub fn at(system: u64, x: f64, y: f64) -> Location {
    Location {
        system: SystemId(id(system)),
        position: point(x, y),
    }
}

/// One of every fleet order (doc 15 §3.4).
pub fn fleet_orders() -> Vec<FleetOrder> {
    vec![
        FleetOrder::Move {
            to: at(2, 12_500.0, -3_000.5),
        },
        FleetOrder::Attack { target: id(77) },
        FleetOrder::AttackMove {
            to: at(3, -190_000.0, 0.25),
        },
        FleetOrder::Patrol {
            route: vec![at(2, 0.0, 0.0), at(2, 1_000.0, 1_000.0), at(2, -5.0, 7.0)],
        },
        FleetOrder::Guard { object: id(12) },
        FleetOrder::StationKeep { relative_to: None },
        FleetOrder::StationKeep {
            relative_to: Some(id(900)),
        },
        FleetOrder::Dock {
            at: HoldingId(id(16)),
        },
        FleetOrder::Launch,
        FleetOrder::Retreat { to: None },
        FleetOrder::Retreat {
            to: Some(HoldingId(id(16))),
        },
        FleetOrder::Jump {
            system: SystemId(id(10)),
        },
        FleetOrder::SetRulesOfEngagement {
            rules: RulesOfEngagement::WeaponsFree,
        },
        FleetOrder::SetRulesOfEngagement {
            rules: RulesOfEngagement::ReturnFire,
        },
        FleetOrder::SetRulesOfEngagement {
            rules: RulesOfEngagement::HoldFire,
        },
    ]
}

/// One of every command, with every queueing mode.
pub fn commands() -> Vec<Command> {
    let mut commands = vec![
        Command::Fleet {
            fleet: FleetId(id(501)),
            order: FleetOrder::Launch,
            queue: Queueing::Append,
            retreat: false,
        },
        Command::Fleet {
            fleet: FleetId(id(501)),
            order: FleetOrder::Retreat { to: None },
            queue: Queueing::Replace,
            retreat: true,
        },
        Command::Holding {
            holding: HoldingId(id(16)),
            order: HoldingOrder::LaunchReady,
        },
        Command::Holding {
            holding: HoldingId(id(16)),
            order: HoldingOrder::Ability {
                ability: StationAbility::ShieldSurge { sector: 2 },
            },
        },
        Command::Holding {
            holding: HoldingId(id(16)),
            order: HoldingOrder::Ability {
                ability: StationAbility::FlakScreen,
            },
        },
        Command::Holding {
            holding: HoldingId(id(16)),
            order: HoldingOrder::TurretsHold { hold: true },
        },
        Command::CreateFleet {
            ships: vec![ShipId(id(601)), ShipId(id(602))],
        },
        Command::CreateFleet { ships: vec![] },
        Command::MergeFleets {
            fleet: FleetId(id(502)),
            into: FleetId(id(501)),
        },
        Command::Economy(EconomyCommand::Build {
            holding: HoldingId(id(16)),
            item: BuildItem::Module {
                kind: ModuleKind::Refinery,
                slot: Slot::Inner(2),
            },
        }),
        Command::Economy(EconomyCommand::Build {
            holding: HoldingId(id(16)),
            item: BuildItem::Ship {
                class: ShipClass::Warden,
            },
        }),
        Command::Economy(EconomyCommand::Build {
            holding: HoldingId(id(16)),
            item: BuildItem::CoreKit,
        }),
        Command::Economy(EconomyCommand::Cancel {
            holding: HoldingId(id(16)),
            item: id(700),
        }),
        Command::Economy(EconomyCommand::SetPowerProfile {
            holding: HoldingId(id(16)),
            profile: PowerProfile::ShieldsFirst,
        }),
        Command::Economy(EconomyCommand::SetRally {
            holding: HoldingId(id(16)),
            fleet: Some(FleetId(id(501))),
        }),
        Command::Economy(EconomyCommand::SetRepair {
            holding: HoldingId(id(16)),
            on: true,
        }),
        Command::Economy(EconomyCommand::Convoy {
            fleet: FleetId(id(501)),
            route: vec![HoldingId(id(17)), HoldingId(id(16))],
        }),
        Command::Economy(EconomyCommand::LoadCoreKit {
            fleet: FleetId(id(501)),
        }),
        Command::Economy(EconomyCommand::DeployCoreKit {
            fleet: FleetId(id(501)),
            site: SiteId(id(40)),
        }),
        Command::SetExposureWindow {
            start: QuarterHour::at(19, 0).expect("19:00 is a quarter hour"),
        },
        Command::SetExposureWindow {
            start: QuarterHour::at(23, 45).expect("23:45 is a quarter hour"),
        },
    ];
    commands.extend(doctrine_commands());
    commands
}

/// One of every doctrine command (A1 plan §5.1).
fn doctrine_commands() -> Vec<Command> {
    vec![
        Command::Doctrine(DoctrineCommand::SetPreset {
            preset: DoctrinePreset::Fortress,
        }),
        Command::Doctrine(DoctrineCommand::SetHolding {
            holding: HoldingId(id(16)),
            doctrine: Some(HoldingDoctrine {
                zone_radius_m: 12_000,
                zone_rules: RulesOfEngagement::ReturnFire,
                alert_profile: PowerProfile::ShieldsFirst,
                surge_below_permille: 350,
                flak_light_hostiles: None,
                evacuation_radius_m: 25_000,
                rebuild_modules: false,
            }),
        }),
        Command::Doctrine(DoctrineCommand::SetHolding {
            holding: HoldingId(id(16)),
            doctrine: None,
        }),
        Command::Doctrine(DoctrineCommand::SetFleet {
            fleet: FleetId(id(501)),
            doctrine: Some(FleetDoctrine {
                role: Some(FleetRole::Patrol {
                    route: vec![at(3, 1_000.0, 2_000.0), at(3, -1_000.0, 2_000.0)],
                }),
                leash_m: 9_000,
                retreat_below_permille: 450,
                repair_at_dock: true,
            }),
        }),
        Command::Doctrine(DoctrineCommand::SetFleet {
            fleet: FleetId(id(502)),
            doctrine: Some(FleetDoctrine {
                role: Some(FleetRole::Escort {
                    fleet: FleetId(id(501)),
                }),
                leash_m: 6_000,
                retreat_below_permille: 300,
                repair_at_dock: false,
            }),
        }),
        Command::Doctrine(DoctrineCommand::SetFleet {
            fleet: FleetId(id(503)),
            doctrine: None,
        }),
        Command::Doctrine(DoctrineCommand::SetReplacement {
            entries: vec![
                ReplacementEntry {
                    fleet: FleetId(id(501)),
                    class: ShipClass::Vx,
                    count: 3,
                    role: Some(FleetRole::Garrison {
                        holding: HoldingId(id(16)),
                    }),
                },
                ReplacementEntry {
                    fleet: FleetId(id(502)),
                    class: ShipClass::Warden,
                    count: 1,
                    role: Some(FleetRole::Reserve {
                        holding: HoldingId(id(17)),
                    }),
                },
            ],
        }),
        Command::Doctrine(DoctrineCommand::Release {
            fleet: FleetId(id(501)),
        }),
    ]
}

/// One of every client message, a command for every fleet order and every other command.
pub fn client_messages() -> Vec<ClientMessage> {
    let mut messages = vec![
        ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            build: "nomad-client 0.1.0 (test)".to_owned(),
        },
        ClientMessage::Auth {
            token: AuthToken::new("0123456789abcdef".to_owned()),
        },
        ClientMessage::Ping { nonce: 0 },
        ClientMessage::Ping { nonce: u64::MAX },
    ];
    for (seq, order) in (1_u64..).zip(fleet_orders()) {
        messages.push(ClientMessage::Command {
            client_seq: seq,
            command: Command::Fleet {
                fleet: FleetId(id(500 + seq)),
                order,
                queue: Queueing::Replace,
                retreat: false,
            },
        });
    }
    for (seq, command) in (100_u64..).zip(commands()) {
        messages.push(ClientMessage::Command {
            client_seq: seq,
            command,
        });
    }
    messages
}

fn entry(kind: EntityKind, owner: Option<Owner>, place: Option<(u64, f64, f64)>) -> EntityEntry {
    EntityEntry {
        kind,
        owner,
        location: place.map(|(system, x, y)| at(system, x, y)),
        last_seen: None,
    }
}

const fn commander(id: CommanderId) -> Owner {
    Owner::Commander(id)
}

/// A view with one entity of every kind, spread over every collection.
pub fn view_snapshot() -> ViewSnapshot {
    let public = BTreeMap::from([
        (id(1), entry(EntityKind::Sector, None, None)),
        (
            id(2),
            entry(EntityKind::System(SystemKind::Home), None, None),
        ),
        (
            id(3),
            entry(EntityKind::System(SystemKind::Standard), None, None),
        ),
        (
            id(10),
            entry(EntityKind::System(SystemKind::Hub), None, None),
        ),
        (
            id(11),
            entry(EntityKind::System(SystemKind::RaiderHaven), None, None),
        ),
        (
            id(12),
            entry(EntityKind::Site, None, Some((2, 60_000.0, 0.0))),
        ),
        (
            id(13),
            entry(EntityKind::Gate, None, Some((2, 0.0, 190_000.0))),
        ),
        (id(14), entry(EntityKind::Lane(LaneKind::Jump), None, None)),
        (id(15), entry(EntityKind::Lane(LaneKind::Trunk), None, None)),
        (
            id(16),
            entry(
                EntityKind::Holding,
                Some(commander(YOU)),
                Some((2, 60_000.0, 0.0)),
            ),
        ),
    ]);
    let control = BTreeMap::from([
        (SystemId(id(2)), ControlState::Controlled(YOU)),
        (SystemId(id(3)), ControlState::Contested),
        (SystemId(id(10)), ControlState::Open),
    ]);
    let own = BTreeMap::from([
        (
            id(16),
            entry(
                EntityKind::Holding,
                Some(commander(YOU)),
                Some((2, 60_000.0, 0.0)),
            ),
        ),
        (
            id(501),
            entry(
                EntityKind::Fleet,
                Some(commander(YOU)),
                Some((2, 61_000.5, -20.0)),
            ),
        ),
    ]);
    let sensed = BTreeMap::from([
        (
            id(900),
            entry(
                EntityKind::Fleet,
                Some(commander(HOSTILE)),
                Some((2, 70_000.0, 5.0)),
            ),
        ),
        (
            id(902),
            entry(
                EntityKind::Fleet,
                Some(Owner::Npc(RAIDERS)),
                Some((10, 1_500.0, -800.0)),
            ),
        ),
    ]);
    let last_known = BTreeMap::from([(
        id(901),
        EntityEntry {
            last_seen: Some(Tick::new(35_999)),
            ..entry(
                EntityKind::Fleet,
                Some(commander(HOSTILE)),
                Some((3, -1.0, 2.0)),
            )
        },
    )]);
    ViewSnapshot {
        tick: Tick::new(36_000),
        public,
        control,
        own,
        sensed,
        last_known,
    }
}

/// A delta that touches every collection.
pub fn view_changes() -> ViewChanges {
    let moved = entry(
        EntityKind::Fleet,
        Some(commander(YOU)),
        Some((2, 61_100.0, -20.0)),
    );
    ViewChanges {
        public: EntityChanges {
            upserted: BTreeMap::from([(
                id(17),
                entry(
                    EntityKind::Holding,
                    Some(commander(HOSTILE)),
                    Some((3, 1.0, 1.0)),
                ),
            )]),
            removed: BTreeSet::new(),
        },
        control: BTreeMap::from([(SystemId(id(3)), ControlState::Controlled(HOSTILE))]),
        own: EntityChanges {
            upserted: BTreeMap::from([(id(501), moved)]),
            removed: BTreeSet::from([id(502)]),
        },
        sensed: EntityChanges {
            upserted: BTreeMap::new(),
            removed: BTreeSet::from([id(900)]),
        },
        last_known: EntityChanges {
            upserted: BTreeMap::from([(
                id(900),
                EntityEntry {
                    last_seen: Some(Tick::new(36_001)),
                    ..entry(
                        EntityKind::Fleet,
                        Some(commander(HOSTILE)),
                        Some((2, 70_010.0, 5.0)),
                    )
                },
            )]),
            removed: BTreeSet::from([id(901)]),
        },
    }
}

/// One of every server message, with every variant of each payload enum.
pub fn server_messages() -> Vec<ServerMessage> {
    let mut messages = vec![
        ServerMessage::Welcome {
            commander: YOU,
            universe: UniverseId(0x0123_4567_89ab_cdef),
            tick: Tick::new(36_000),
            clock: SeasonClock::new(1_793_923_200_000),
        },
        ServerMessage::CatchUp {
            progress: CatchUpProgress {
                reached: Tick::new(1_000),
                target: Tick::new(864_000),
            },
        },
        ServerMessage::CommandResult {
            client_seq: 1,
            result: CommandOutcome::Applied {
                tick: Tick::new(36_001),
            },
        },
        ServerMessage::ViewSnapshot(view_snapshot()),
        ServerMessage::ViewSnapshot(ViewSnapshot::default()),
        ServerMessage::ViewDelta {
            tick: Tick::new(36_001),
            changes: view_changes(),
        },
        ServerMessage::ViewDelta {
            tick: Tick::new(36_002),
            changes: ViewChanges::default(),
        },
        ServerMessage::Announcement(Announcement::Reinforced {
            holding: HoldingId(id(16)),
            siege_start: Tick::new(500_000),
        }),
        ServerMessage::Announcement(Announcement::SiegeStarted {
            holding: HoldingId(id(16)),
            siege_end: Tick::new(536_000),
        }),
        ServerMessage::Pong { nonce: 0 },
        ServerMessage::Pong { nonce: u64::MAX },
    ];
    for reason in [
        ByeReason::VersionMismatch {
            server: PROTOCOL_VERSION,
        },
        ByeReason::AuthenticationFailed,
        ByeReason::ShuttingDown,
        ByeReason::ProtocolViolation,
        ByeReason::HandshakeTimeout,
        ByeReason::Resync,
        ByeReason::Replaced,
    ] {
        messages.push(ServerMessage::Bye { reason });
    }
    for (seq, reason) in (2_u64..).zip([
        RejectReason::CatchingUp,
        RejectReason::RateLimited,
        RejectReason::NotAuthenticated,
        RejectReason::Invalid {
            reason: InvalidReason::NotFound,
        },
        RejectReason::Invalid {
            reason: InvalidReason::NothingToLaunch,
        },
        RejectReason::Invalid {
            reason: InvalidReason::AbilityCooldown,
        },
        RejectReason::Invalid {
            reason: InvalidReason::InvalidWindow,
        },
        RejectReason::Invalid {
            reason: InvalidReason::WindowLocked,
        },
        RejectReason::Invalid {
            reason: InvalidReason::InvalidDoctrine,
        },
        RejectReason::Invalid {
            reason: InvalidReason::NotUnderDirectControl,
        },
        RejectReason::Invalid {
            reason: InvalidReason::NotPermitted,
        },
        RejectReason::SeasonOver,
    ]) {
        messages.push(ServerMessage::CommandResult {
            client_seq: seq,
            result: CommandOutcome::Rejected { reason },
        });
    }
    for (kind, subject) in [
        (NotificationKind::Breach, Some(id(16))),
        (NotificationKind::Siege, Some(id(16))),
        (NotificationKind::Raid, None),
    ] {
        messages.push(ServerMessage::Notification(Notification {
            tick: Tick::new(36_000),
            subject,
            kind,
        }));
    }
    messages
}

/// The variant index of a client message. The match has no wildcard, so a new variant does
/// not compile until the samples cover it.
pub const fn client_variant(message: &ClientMessage) -> usize {
    match message {
        ClientMessage::Hello { .. } => 0,
        ClientMessage::Auth { .. } => 1,
        ClientMessage::Command { .. } => 2,
        ClientMessage::Ping { .. } => 3,
    }
}

/// Number of [`ClientMessage`] variants.
pub const CLIENT_VARIANTS: usize = 4;

/// The variant index of a server message (no wildcard, as above).
pub const fn server_variant(message: &ServerMessage) -> usize {
    match message {
        ServerMessage::Bye { .. } => 0,
        ServerMessage::Welcome { .. } => 1,
        ServerMessage::CatchUp { .. } => 2,
        ServerMessage::CommandResult { .. } => 3,
        ServerMessage::ViewSnapshot(_) => 4,
        ServerMessage::ViewDelta { .. } => 5,
        ServerMessage::Announcement(_) => 6,
        ServerMessage::Notification(_) => 7,
        ServerMessage::Pong { .. } => 8,
    }
}

/// Number of [`ServerMessage`] variants.
pub const SERVER_VARIANTS: usize = 9;

/// The variant index of a fleet order (no wildcard, as above).
pub const fn order_variant(order: &FleetOrder) -> usize {
    match order {
        FleetOrder::Move { .. } => 0,
        FleetOrder::Attack { .. } => 1,
        FleetOrder::AttackMove { .. } => 2,
        FleetOrder::Patrol { .. } => 3,
        FleetOrder::Guard { .. } => 4,
        FleetOrder::StationKeep { .. } => 5,
        FleetOrder::Dock { .. } => 6,
        FleetOrder::Launch => 7,
        FleetOrder::Retreat { .. } => 8,
        FleetOrder::Jump { .. } => 9,
        FleetOrder::SetRulesOfEngagement { .. } => 10,
    }
}

/// Number of [`FleetOrder`] variants.
pub const ORDER_VARIANTS: usize = 11;

/// The variant index of a command (no wildcard, as above).
pub const fn command_variant(command: &Command) -> usize {
    match command {
        Command::Fleet { .. } => 0,
        Command::Holding { .. } => 1,
        Command::CreateFleet { .. } => 2,
        Command::MergeFleets { .. } => 3,
        Command::Economy(..) => 4,
        Command::SetExposureWindow { .. } => 5,
        Command::Doctrine(..) => 6,
    }
}

/// Number of [`Command`] variants.
pub const COMMAND_VARIANTS: usize = 7;
