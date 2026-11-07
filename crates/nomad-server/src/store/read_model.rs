//! The season read model (S8 plan §4.6; doc 20 §6; D-196): `standings_history` and
//! `season_result`.
//!
//! - **`standings_history`** grows by one row per commander for every standings change the
//!   simulation announces, at the tick of the change. Changes are rare, so it stays small,
//!   and it serves the season panel and the away report's rank changes (D-092, D-196).
//! - **`season_result`** is written once, when the simulation fixes the result (D-116): a row
//!   per winner, joint winners included (D-194).
//! - **Projections, not state** (doc 11 §8): at every start [`Store::sync_read_model`]
//!   re-syncs both with the recovered state. Rows after the recovered tick belong to ticks a
//!   crash undid, and the run that follows computes those ticks again, so they are dropped.
//!   If the newest standings still differ from the state's, the state's are appended at the
//!   tick they last changed. The projection therefore never lags or leads the state.

use std::path::Path;

use nomad_sim::UniverseState;
use nomad_types::{CommanderId, DepositValue, SeasonEndReason, SeasonResult, Standing, Tick};
use rusqlite::{Connection, Transaction, params};

use super::{Store, begin, db_error, tick_from_sql, tick_to_sql};
use crate::error::ServerError;

/// What [`Store::sync_read_model`] changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadModelSync {
    /// Standings rows dropped because they were after the recovered tick.
    pub dropped_rows: usize,
    /// Whether the state's standings were appended.
    pub appended: bool,
    /// Whether the result rows were rewritten.
    pub result_written: bool,
}

impl ReadModelSync {
    /// Whether the read model already matched the state.
    pub fn unchanged(&self) -> bool {
        *self == Self::default()
    }
}

/// The text of a reason in `season_result.reason`.
const fn reason_text(reason: SeasonEndReason) -> &'static str {
    match reason {
        SeasonEndReason::EarlyWin => "early_win",
        SeasonEndReason::FinalTick => "final_tick",
    }
}

fn parse_reason(text: &str) -> Option<SeasonEndReason> {
    match text {
        "early_win" => Some(SeasonEndReason::EarlyWin),
        "final_tick" => Some(SeasonEndReason::FinalTick),
        _ => None,
    }
}

impl Store {
    /// Appends the standings after `tick`, at which they changed, in one transaction.
    pub fn append_standings(
        &mut self,
        tick: Tick,
        standings: &[Standing],
    ) -> Result<(), ServerError> {
        let path = self.path.clone();
        let tx = begin(&mut self.conn, &path, "append standings")?;
        insert_standings(&tx, &path, tick, standings)?;
        tx.commit()
            .map_err(|e| db_error(&path, "commit standings", e))
    }

    /// Writes the season's result, replacing any earlier rows.
    pub fn write_season_result(&mut self, result: &SeasonResult) -> Result<(), ServerError> {
        let path = self.path.clone();
        let tx = begin(&mut self.conn, &path, "write season result")?;
        replace_result(&tx, &path, Some(result))?;
        tx.commit()
            .map_err(|e| db_error(&path, "commit season result", e))
    }

    /// The newest standings in the history, with the tick they changed at; `None` if the
    /// history is empty.
    pub fn latest_standings(&self) -> Result<Option<(Tick, Vec<Standing>)>, ServerError> {
        latest(&self.conn, &self.path)
    }

    /// Every standings change in the history, oldest first.
    pub fn standings_history(&self) -> Result<Vec<(Tick, Vec<Standing>)>, ServerError> {
        let fail = |e| db_error(&self.path, "read standings", e);
        let mut statement = self
            .conn
            .prepare_cached("SELECT DISTINCT tick FROM standings_history ORDER BY tick")
            .map_err(fail)?;
        let ticks = statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(fail)?
            .collect::<Result<Vec<i64>, _>>()
            .map_err(fail)?;
        ticks
            .into_iter()
            .map(|raw| {
                let tick = tick_from_sql(raw)
                    .ok_or_else(|| self.corrupt(format!("standings tick {raw}")))?;
                Ok((tick, standings_at(&self.conn, &self.path, tick)?))
            })
            .collect()
    }

    /// The season's result, if it is fixed.
    pub fn season_result(&self) -> Result<Option<SeasonResult>, ServerError> {
        result(&self.conn, &self.path)
    }

    /// Re-syncs the read model with `state`, the recovered Universe (doc 11 §8, D-196): drops
    /// standings after its tick, appends its standings if the newest rows differ, and makes
    /// the result rows match its result. One transaction.
    pub fn sync_read_model(&mut self, state: &UniverseState) -> Result<ReadModelSync, ServerError> {
        let mut sync = ReadModelSync::default();
        let standings = state.standings().ranked();
        // The standings last changed at the latest `since`: every change resets the changed
        // commanders' `since` to its tick (D-193).
        let changed = standings
            .iter()
            .map(|s| s.since)
            .max()
            .unwrap_or(Tick::ZERO);
        let path = self.path.clone();
        let path = path.as_path();
        let fail = |context| move |e| db_error(path, context, e);
        let tx = begin(&mut self.conn, path, "sync read model")?;
        sync.dropped_rows = tx
            .execute(
                "DELETE FROM standings_history WHERE tick > ?1",
                [tick_to_sql(state.tick())?],
            )
            .map_err(fail("drop standings"))?;
        let newest = latest(&tx, path)?;
        if newest.as_ref().map(|(_, rows)| rows.as_slice()) != Some(standings) {
            tx.execute(
                "DELETE FROM standings_history WHERE tick >= ?1",
                [tick_to_sql(changed)?],
            )
            .map_err(fail("replace standings"))?;
            insert_standings(&tx, path, changed, standings)?;
            sync.appended = true;
        }
        if result(&tx, path)?.as_ref() != state.season_result() {
            replace_result(&tx, path, state.season_result())?;
            sync.result_written = true;
        }
        tx.commit().map_err(fail("commit read model"))?;
        Ok(sync)
    }
}

fn corrupt(path: &Path, detail: String) -> ServerError {
    ServerError::CorruptDatabase {
        path: path.to_owned(),
        detail,
    }
}

fn latest(conn: &Connection, path: &Path) -> Result<Option<(Tick, Vec<Standing>)>, ServerError> {
    let newest: Option<i64> = conn
        .query_row("SELECT max(tick) FROM standings_history", [], |row| {
            row.get(0)
        })
        .map_err(|e| db_error(path, "read standings", e))?;
    let Some(raw) = newest else {
        return Ok(None);
    };
    let tick = tick_from_sql(raw).ok_or_else(|| corrupt(path, format!("standings tick {raw}")))?;
    Ok(Some((tick, standings_at(conn, path, tick)?)))
}

/// The standings rows of `tick`, best first: by rank, then identity, as the simulation
/// orders them.
fn standings_at(conn: &Connection, path: &Path, tick: Tick) -> Result<Vec<Standing>, ServerError> {
    let fail = |e| db_error(path, "read standings", e);
    let mut statement = conn
        .prepare_cached(
            "SELECT commander, rank, systems, holdings, deposit_value, since
             FROM standings_history WHERE tick = ?1 ORDER BY rank, commander",
        )
        .map_err(fail)?;
    let rows = statement
        .query_map([tick_to_sql(tick)?], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(fail)?;
    let mut standings = Vec::new();
    for row in rows {
        let (commander, rank, systems, holdings, value, since) = row.map_err(fail)?;
        let bad = |what: &str| {
            corrupt(
                path,
                format!("standings_history.{what} at tick {}", tick.get()),
            )
        };
        standings.push(Standing {
            commander: CommanderId(u32::try_from(commander).map_err(|_| bad("commander"))?),
            rank: u32::try_from(rank).map_err(|_| bad("rank"))?,
            systems: u32::try_from(systems).map_err(|_| bad("systems"))?,
            holdings: u32::try_from(holdings).map_err(|_| bad("holdings"))?,
            deposit_value: DepositValue::from_milli(
                u64::try_from(value).map_err(|_| bad("deposit_value"))?,
            ),
            since: tick_from_sql(since).ok_or_else(|| bad("since"))?,
        });
    }
    Ok(standings)
}

fn result(conn: &Connection, path: &Path) -> Result<Option<SeasonResult>, ServerError> {
    let fail = |e| db_error(path, "read season result", e);
    let mut statement = conn
        .prepare_cached("SELECT winner, tick, reason FROM season_result ORDER BY winner")
        .map_err(fail)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(fail)?;
    let mut found: Option<SeasonResult> = None;
    for row in rows {
        let (winner, tick, reason) = row.map_err(fail)?;
        let bad = |what: String| corrupt(path, format!("season_result: {what}"));
        let winner =
            CommanderId(u32::try_from(winner).map_err(|_| bad(format!("winner {winner}")))?);
        let tick = tick_from_sql(tick).ok_or_else(|| bad(format!("tick {tick}")))?;
        let reason = parse_reason(&reason).ok_or_else(|| bad(format!("reason {reason:?}")))?;
        match &mut found {
            None => {
                found = Some(SeasonResult {
                    tick,
                    reason,
                    winners: vec![winner],
                });
            }
            Some(r) if r.tick == tick && r.reason == reason => r.winners.push(winner),
            Some(_) => return Err(bad("the rows disagree on the tick or reason".to_owned())),
        }
    }
    Ok(found)
}

fn insert_standings(
    tx: &Transaction<'_>,
    path: &Path,
    tick: Tick,
    standings: &[Standing],
) -> Result<(), ServerError> {
    let mut statement = tx
        .prepare_cached(
            "INSERT OR REPLACE INTO standings_history
                 (tick, commander, rank, systems, holdings, deposit_value, since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(|e| db_error(path, "append standings", e))?;
    for s in standings {
        let value = i64::try_from(s.deposit_value.milli()).map_err(|_| {
            ServerError::Encode(format!(
                "deposit value {} does not fit SQLite",
                s.deposit_value
            ))
        })?;
        statement
            .execute(params![
                tick_to_sql(tick)?,
                s.commander.0,
                s.rank,
                s.systems,
                s.holdings,
                value,
                tick_to_sql(s.since)?,
            ])
            .map_err(|e| db_error(path, "append standings", e))?;
    }
    Ok(())
}

fn replace_result(
    tx: &Transaction<'_>,
    path: &Path,
    result: Option<&SeasonResult>,
) -> Result<(), ServerError> {
    tx.execute("DELETE FROM season_result", [])
        .map_err(|e| db_error(path, "write season result", e))?;
    let Some(result) = result else {
        return Ok(());
    };
    for winner in &result.winners {
        tx.execute(
            "INSERT INTO season_result (winner, tick, reason) VALUES (?1, ?2, ?3)",
            params![
                winner.0,
                tick_to_sql(result.tick)?,
                reason_text(result.reason)
            ],
        )
        .map_err(|e| db_error(path, "write season result", e))?;
    }
    Ok(())
}
