//! Creating a Universe (N2 plan §4.1, §6 task 3; D-020, D-079, D-106, D-107).
//!
//! `create` generates the MVP stage-1 Universe (doc 10 §4.4) from a seed and writes the
//! `meta` row and the tick-0 snapshot. Tick 0 is the season start: the creation time, or
//! earlier with `backdate`, which lets an operator test catch-up.
//!
//! The roster names the human, [`HUMAN_COMMANDER`], with the exposure window it was given;
//! the AI commanders get the simulation's default window until A5 gives them schedules
//! (S6 plan §4.1, §4.3; D-157).

use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_roster};
use nomad_sim::snapshot::{self, encode, state_hash};
use nomad_sim::{
    CommanderId, CommanderSetup, DEFAULT_WINDOW_START, QuarterHour, Rules, SeasonConfig, Tick,
};
use nomad_types::UniverseId;
use tracing::info;

use crate::clock::Clock;
use crate::error::ServerError;
use crate::ids::{Difficulty, random_u64, random_universe_id, universe_id_text};
use crate::net::identity::Identity;
use crate::paths::{DATABASE_FILE, DataDir, UniverseLock};
use crate::server::season_end_tick;
use crate::store::{Meta, SnapshotRecord, Store};

/// Season lengths `create` accepts, in weeks (D-020).
pub const SEASON_WEEKS: RangeInclusive<u32> = 8..=12;
/// The default season length (D-106).
pub const DEFAULT_SEASON_WEEKS: u32 = 8;
/// AI commanders in an MVP stage-1 Universe: one per home system after the human's
/// (doc 10 §3, D-038). The stage-1 generator supports no other number.
pub const STAGE1_AI_COMMANDERS: u32 = 3;
const _: () = assert!(STAGE1_AI_COMMANDERS as usize + 1 == STAGE1_COMMANDERS);
/// The local human commander. AI commanders follow it: 2, 3, 4. The human gets home
/// system H1 (doc 10 §4.4).
pub const HUMAN_COMMANDER: CommanderId = CommanderId(1);
/// Longest display name, in characters.
pub const NAME_MAX_CHARS: usize = 64;

/// What to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateParams {
    /// The season seed; random if `None`.
    pub seed: Option<u64>,
    /// AI commanders; stage 1 has exactly [`STAGE1_AI_COMMANDERS`].
    pub ai_commanders: u32,
    /// Their difficulty (D-079).
    pub difficulty: Difficulty,
    /// Season length in weeks, within [`SEASON_WEEKS`].
    pub season_weeks: u32,
    /// The season's final tick, overriding the one `season_weeks` gives: a short season for
    /// tests and operator trials. It goes into the simulation's season configuration
    /// (D-195); `None` for a normal Universe.
    pub final_tick: Option<Tick>,
    /// Optional display name (D-107).
    pub name: Option<String>,
    /// Puts the season start this far before now, so the first `run` catches up. An
    /// operator and test tool; zero for a normal Universe.
    pub backdate: Duration,
    /// Where the human's exposure window starts, UTC (D-157). The default here is 19:00 UTC;
    /// the command line's default is the local 19:00.
    pub window_start: QuarterHour,
}

impl Default for CreateParams {
    fn default() -> Self {
        Self {
            seed: None,
            ai_commanders: STAGE1_AI_COMMANDERS,
            difficulty: Difficulty::Normal,
            season_weeks: DEFAULT_SEASON_WEEKS,
            final_tick: None,
            name: None,
            backdate: Duration::ZERO,
            window_start: DEFAULT_WINDOW_START,
        }
    }
}

/// A Universe that was just created.
#[derive(Clone, Debug)]
pub struct Created {
    /// Its identity.
    pub id: UniverseId,
    /// Its directory.
    pub dir: PathBuf,
    /// Its `meta` row.
    pub meta: Meta,
    /// The state hash of tick 0.
    pub state_hash: u64,
}

/// Creates a new Universe in `data`, as `build`, at `clock`'s wall-clock time.
pub fn create(
    data: &DataDir,
    params: &CreateParams,
    clock: &dyn Clock,
    build: &str,
) -> Result<Created, ServerError> {
    let name = validate(params)?;
    let seed = params.seed.unwrap_or_else(random_u64);
    let now = clock.utc_ms();
    let backdate_ms = i64::try_from(params.backdate.as_millis())
        .map_err(|_| ServerError::InvalidParameter("backdate is too long".to_owned()))?;
    let season_start_unix_ms = now.saturating_sub(backdate_ms);
    // The simulation holds the final tick and fixes the result at it (D-195).
    let final_tick = params
        .final_tick
        .unwrap_or_else(|| season_end_tick(params.season_weeks));

    // Generate and encode before touching the disk, so a failure leaves nothing behind.
    let state = generate_stage1_roster(
        SeasonConfig {
            seed,
            season_start_unix_ms,
            final_tick,
        },
        stage1_roster(params.window_start, params.difficulty),
        Rules::embedded(),
    )?;
    let hash = state_hash(&state)?;
    let record = SnapshotRecord {
        tick: Tick::ZERO,
        build: build.to_owned(),
        format: snapshot::FORMAT,
        state_hash: hash,
        bytes: encode(&state)?,
    };

    let universes = data.universes();
    fs::create_dir_all(&universes)
        .map_err(|e| ServerError::io(format!("cannot create {}", universes.display()), e))?;
    let (id, dir) = new_universe_dir(data)?;
    let lock = UniverseLock::acquire(&dir)?;
    let meta = Meta {
        universe_id: universe_id_text(id),
        name,
        build: build.to_owned(),
        seed,
        season_start_unix_ms,
        season_weeks: params.season_weeks,
        final_tick,
        ai_commanders: params.ai_commanders,
        difficulty: params.difficulty,
        created_at_unix_ms: now,
        // No rules file exists yet (D-104 arrives with S3).
        rules_hash: None,
        journal_floor: Tick::ZERO,
    };
    // The tick-0 standings start the read model (D-196). The certificate, key and token are
    // created once and kept (D-151).
    let stored = Store::create(&dir.join(DATABASE_FILE), &meta, &record)
        .and_then(|mut store| store.sync_read_model(&state))
        .and_then(|_| Identity::load_or_create(&dir).map(|_| ()));
    if let Err(e) = stored {
        drop(lock);
        // Best effort: the error that matters is the one being returned.
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    info!(universe = %meta.universe_id, seed, dir = %dir.display(), "Universe created");
    Ok(Created {
        id,
        dir,
        meta,
        state_hash: hash,
    })
}

/// The stage-1 roster `create` uses: the human, [`HUMAN_COMMANDER`], with its window starting
/// at `human_window` UTC, then the AI commanders 2, 3 and 4 on their seeded schedules, which
/// set their presence and initial windows (S6 plan §4.1, §4.3; D-080, D-157, D-204), all at
/// `difficulty` (D-079, D-231).
pub fn stage1_roster(
    human_window: QuarterHour,
    difficulty: Difficulty,
) -> [CommanderSetup; STAGE1_COMMANDERS] {
    std::array::from_fn(|index| {
        let id = CommanderId(HUMAN_COMMANDER.0 + u32::try_from(index).unwrap_or(u32::MAX));
        if id == HUMAN_COMMANDER {
            CommanderSetup::human(id, human_window)
        } else {
            CommanderSetup::scheduled_ai(id, difficulty.into())
        }
    })
}

/// Checks the parameters and returns the cleaned display name.
fn validate(params: &CreateParams) -> Result<Option<String>, ServerError> {
    let invalid = |message: String| Err(ServerError::InvalidParameter(message));
    if !SEASON_WEEKS.contains(&params.season_weeks) {
        return invalid(format!(
            "season length {} weeks is outside {}–{} weeks (D-020)",
            params.season_weeks,
            SEASON_WEEKS.start(),
            SEASON_WEEKS.end()
        ));
    }
    if params.final_tick == Some(Tick::ZERO) {
        return invalid("the final tick must come after tick 0".to_owned());
    }
    if params.ai_commanders != STAGE1_AI_COMMANDERS {
        return invalid(format!(
            "{} AI commanders: the MVP stage-1 Universe has exactly {STAGE1_AI_COMMANDERS} \
             (doc 10 §4.4)",
            params.ai_commanders
        ));
    }
    let Some(name) = &params.name else {
        return Ok(None);
    };
    let name = name.trim();
    if name.is_empty() {
        return invalid("the name is empty".to_owned());
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return invalid(format!(
            "the name is longer than {NAME_MAX_CHARS} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return invalid("the name contains control characters".to_owned());
    }
    Ok(Some(name.to_owned()))
}

/// Creates the directory of a fresh Universe ID.
fn new_universe_dir(data: &DataDir) -> Result<(UniverseId, PathBuf), ServerError> {
    // 40 random bits: a collision is unlikely; a few retries make it irrelevant.
    for _ in 0..16 {
        let id = random_universe_id();
        let dir = data.universe(id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((id, dir)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(ServerError::io(
                    format!("cannot create {}", dir.display()),
                    e,
                ));
            }
        }
    }
    Err(ServerError::InvalidParameter(
        "could not find an unused Universe ID".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_decisions() {
        let params = CreateParams::default();
        assert_eq!(params.season_weeks, 8); // D-106
        assert_eq!(params.ai_commanders, 3); // N2 plan §4.1, D-038
        assert_eq!(params.difficulty, Difficulty::Normal); // D-079
        assert!(matches!(validate(&params), Ok(None)));
    }

    #[test]
    fn out_of_range_parameters_are_rejected() {
        let bad = [
            CreateParams {
                season_weeks: 7,
                ..CreateParams::default()
            },
            CreateParams {
                season_weeks: 13,
                ..CreateParams::default()
            },
            CreateParams {
                ai_commanders: 9,
                ..CreateParams::default()
            },
            CreateParams {
                final_tick: Some(Tick::ZERO),
                ..CreateParams::default()
            },
            CreateParams {
                name: Some("  ".to_owned()),
                ..CreateParams::default()
            },
            CreateParams {
                name: Some("x".repeat(65)),
                ..CreateParams::default()
            },
            CreateParams {
                name: Some("a\nb".to_owned()),
                ..CreateParams::default()
            },
        ];
        for params in bad {
            assert!(validate(&params).is_err(), "{params:?}");
        }
        let named = CreateParams {
            name: Some(" Kestrel ".to_owned()),
            season_weeks: 12,
            ..CreateParams::default()
        };
        assert_eq!(validate(&named).unwrap(), Some("Kestrel".to_owned()));
    }
}
