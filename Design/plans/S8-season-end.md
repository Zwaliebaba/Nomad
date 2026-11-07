# Work package S8 — Season standings and the season end

Status: **Built** (Q190–Q197 → D-191 to D-198). Tasks 1–6 are built in `crates/nomad-sim/src/season/`, the server's stop on the result and its read model, and `nomad-harness --season`; §11 lists how it was built. S6b wires its capture and jump paths to it, and N4 fills its `standings` collection from it. Builds on S2 (galaxy, holdings and the D-041 control rule, built), S4 (claiming and holding completion, built), N2 (the season-end freeze of D-116, built) and S6b (capture and emergency jumps change ownership). It feeds S6b's comeback grant (D-167), S7's `CommanderView` (standings are public), N4's `standings` collection, the AI (A2–A4), and the away report (U5) and season panel (U6). This plan is also the brief for whoever builds S8. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

At every tick the simulation knows each commander's standing: systems controlled, holdings owned and deposit value held, in D-041's order. A commander who controls 60% of the claimable systems wins at once (D-020). Otherwise the standings at the season's final tick decide (D-020, D-116). The result is fixed in the state at that tick and announced publicly. The server writes it to a read model and freezes the Universe (D-116), whether the season ran its full length or ended early. The harness can run a season to its result, which α and γ need (doc 30 §3, §5 item 1).

## 2. Inputs

- **Season and win:**
  - D-020: seasons of 8–12 weeks; most systems controlled at season end wins; about 60% wins early;
  - D-106: 8 weeks by default;
  - D-116: at the final tick the standings and winner are fixed, the Universe stops advancing, and a new season is a new Universe.
- **Control:**
  - D-041: a strict majority of a system's sites as completed holdings; outposts do not count; tie-breaks are holdings owned, deposit value held, then whoever reached the final count first;
  - doc 10 §6: the early win needs 60% of claimable systems, rounded up (6 of 9 at stage 1);
  - D-066: deposit value is ore 1 and crystal 3, × richness;
  - D-099 item 6: raider havens have no sites.
- **Information:** D-075 and doc 16 §2: standings and control are public.
- **Offline rules:**
  - D-167: the comeback grant counts the holdings a commander is behind "the standings leader";
  - the S6 plan §4.5–§4.6: capture changes the owner; a mother station in emergency-jump transit is not a holding for control.
- **UI and read models:**
  - D-092 and doc 17 §6: the season panel shows standings, the season clock and progress toward the early win; the away report keeps 7 days of history;
  - doc 17 §4: the SECTOR view's season summary shows the early-win threshold;
  - doc 20 §6: "read-model tables, for example standings";
  - doc 11 §8: read models are projections, not the authoritative state;
  - the N4 plan §4.2 and §4.4: a `standings` collection, sent in the galaxy tier every 5 s; fields are added by the package that owns them (D-184).
- **AI:** doc 13 §5 (the Strategist scores goals from public data); D-078 (no bias for or against the human).
- **Code:**
  - `crates/nomad-sim`: `holding.rs` (`control_from_counts`, `control_states`, computed on demand), `Holding { core_complete, mother_station }`, `SeasonConfig { seed, season_start_unix_ms }` (no season length), `TickOutputs`; `DepositValue` in `nomad-types`.
  - `crates/nomad-server`: `season_end_tick(weeks)`, `meta.season_weeks`, `Runner::season_over`, `Phase::SeasonOver`, `SubmitError::SeasonOver`, the hub's `RejectReason::SeasonOver`. The server alone knows when the season ends; the simulation does not.
  - `HUMAN_COMMANDER` is `CommanderId(1)` (N2).

## 3. Scope

**In scope:**

- The **standing** of every commander, and the **ranking** with D-041's tie-breaks (Q192, Q193).
- A **cached control map** and the standings in the state, recomputed when ownership changes (Q190).
- The **early win** (Q191).
- The **season's final tick** inside the simulation (Q194), and the result fixed at it.
- **Public events:** control changed, standings changed, one system from the early win (Q196), season ended.
- **The server:** stopping at the result, including an early win during catch-up; the read model (Q195); `status`.
- **Views** for S7, N4, the AI, S6b's comeback grant and the UI.
- **Harness:** a `--season` result report.

**Out of scope:**

- The season panel and the away report themselves (U5, U6).
- Replicating the standings (N4). Whichever of S8 and N4 lands second fills N4's `standings` collection.
- Starting the next season (D-116: a new Universe; the start screen is U6), and carrying blueprints and doctrine presets over (D-035).
- Late joiners (outside the MVP), and alliances pooling control (D-040).
- Doc 10 §11 item 2's alternative of counting average control over the last 2 weeks. It stays a watch item for A6.

## 4. Design

### 4.1 The standing

*Agreed* rule (D-041); the details are *Derived* unless marked.

- **Systems controlled:** the claimable systems in state `Controlled(c)` (D-041). Raider havens are not claimable and never count (doc 10 §4.1).
- **Holdings owned:** completed holdings only (doc 10 §5: a holding is a site with a completed core). Outposts do not count, as for control (D-041).
  - The mother station counts while it is a holding.
  - In emergency-jump transit it is not a holding for control (S6 plan §4.6), so it counts for nothing until it lands.
- **Deposit value held:** the sum of D-066 values over the deposits at those holdings' sites, as exact integers (richness in per mille), in `nomad-types`' `DepositValue` units.
- **Since** *(Proposed, Q192):* the tick from which the commander's triple (systems, holdings, deposit value) has been unchanged.
- **Order:**
  1. systems, more first;
  2. holdings, more first;
  3. deposit value, more first;
  4. *since*, earlier first (Q192).

  Commanders equal on all four share a rank (Q193).
- Every commander has a standing from tick 0. At season start each holds one holding, its mother station, and controls nothing: 1 of 7 home sites is not a majority.

### 4.2 When standings change

*Proposed* (Q190).

- Control and standings change only when the set of completed holdings or their owners changes:
  - an outpost completes (S4, economy phase 8);
  - a holding is captured (S6b, combat phase 7);
  - a fallen mother station leaves its site, and lands as a holding again (S6b).

  Destroying an outpost changes nothing, since outposts never count.
- Those code paths set a "standings dirty" flag. In phase 9 (derived state, doc 11 §4.1), a dirty tick recomputes the control map and the standings, compares them with the stored ones, emits events (§4.5), and clears the flag.
- **State:** `UniverseState` gains a `season` record:
  - the cached control map per claimable system;
  - the standings, with each commander's *since*;
  - the result, once fixed.
  
  `control_state()` and `control_states()` read the cache. S7's control coverage (D-073) reads the same cache, so it no longer recounts holdings every sensors pass.
- **A pure function** computes standings from the current holdings at any point in a tick. S6b uses it for the comeback grant "after the fall" (D-167), which happens in phase 7 before phase 9 runs.
- **Cost:** one pass over the holdings per change, and changes happen a few times a day. Negligible at stage 2.

### 4.3 The early win

*Agreed* rule (D-020, doc 10 §6); the trigger is *Proposed* (Q191).

- **Threshold:** ⌈share × claimable systems⌉, with the share a rules value of 600‰ (doc 10 §6). That gives 6 of 9 at stage 1 and 12 of about 20 at stage 2.
  - *Derived:* the threshold is more than half the systems, so at most one commander can reach it.
- **Trigger:** in phase 9, right after the recompute. On the first tick on which a commander's systems reach the threshold, the result is fixed: that commander, with reason *early win*, at that tick.
- That tick becomes the season's final tick, and D-116 applies to it (§4.6).
- *Derived* consequence: an early win needs at least 6 majorities, which is 24–25 completed holdings at stage 1 (4 of 7, 4 of 6, 5 of 8). That is about 40% of the sector's 60 sites, against a fair share of 15.

### 4.4 The scheduled end

*Agreed* (D-116); where the final tick lives is *Proposed* (Q194).

- **`SeasonConfig` gains the final tick.** `create` sets it from the season length (D-106: 8 weeks by default, 8–12 allowed), using today's `season_end_tick`. The server's `meta.season_weeks` stays as a record of the creation parameters.
- **At the step that computes the final tick,** after phase 9, if no result is fixed yet, the result is fixed from the standings: reason *season end*, winners the commanders ranked first.
- **Nothing follows the result.** The server and the harness stop stepping (D-116).
  - *Agreed (D-234):* `step` on a decided Universe changes nothing and returns empty outputs, so a caller that keeps stepping cannot change the final state.
  - Sieges, Reinforced exits and outposts due after the final tick never happen (S6 plan §4.4).
- **Public:** the season's start and final tick are public (doc 17 §6: "the season clock"), so the season panel and the AI read them through `CommanderView`.
- **Snapshot format** goes up once. There is no migration before β (D-099 item 9). Stability pins are re-recorded.

### 4.5 Events

*Derived* from D-075 (all public); the near-win event is *Proposed* (Q196).

`TickOutputs` gains `season_events`, which are public announcements in S6's plumbing (recipients: everyone):

- `ControlChanged { system, from, to }`;
- `StandingsChanged { standings }`, at most once per tick;
- `NearEarlyWin { commander, systems, threshold }` when a commander reaches threshold − 1, and `NearEarlyWinEnded { commander }` when they drop back (Q196);
- `SeasonEnded { tick, reason, winners, standings }`.

The away report's S8 content (U5): control gained and lost by the reader, changes in the reader's rank, near-win warnings, and the result.

### 4.6 The server

*Derived* from D-116 and the N2 build; the read model is *Proposed* (Q195).

- **Stopping:** after every step, live and in catch-up, the simulation thread checks whether the state holds a result. If it does, it takes today's `season_over` path:
  - a final snapshot;
  - `ServerEvent::SeasonOver`;
  - `Phase::SeasonOver`;
  - inputs refused with `SubmitError::SeasonOver`.
  
  This replaces the check against `season_end_tick(meta.season_weeks)`. An early win during catch-up (an outpost completing overnight) ends catch-up at that tick, because catch-up never runs past the end.
- **Tests** that set a short season (`ServerConfig::season_end`) now set the simulation's final tick at creation instead.
- **The read model** (Q195), written by the persistence thread from `TickOutputs`:
  - `standings_history (tick, commander, rank, systems, holdings, deposit_value, since)`, appended on every `StandingsChanged`;
  - `season_result (tick, reason, winners)`, written once.
- **Recovery:** they are projections (doc 11 §8). After recovery the server compares the newest history rows with the recovered state's standings, and appends a row if they differ, so the projection never lags the authoritative state.
- **`status`** prints the current standings and, for an ended season, the result.
- **The wire:** N4's `standings` collection carries standings, the near-win state and the result. The `SeasonOver` rejection already exists.

### 4.7 Views

*Derived.*

- **Read-only accessors:** `standings()`, `season_result()`, the cached control state, `early_win_threshold()`, the season's start and final tick.
- All of it is public (D-075). S7's `CommanderView` exposes it under *public*, and the AI reads it there.
- **S6b's comeback grant** (D-167): the standings leader is the commander ranked first after the fall; with joint leaders, the one with the most holdings. "Holdings behind" is the leader's holdings minus the fallen commander's, clamped to the grant's 1–3 kits.

## 5. Timeline check

*Derived* from the agreed rules; the holdings are made up.

**A season decided by the tie-breaks.** Stage 1, 9 claimable systems, 8-week season, final tick at day 56 00:00.

| Commander | Systems | Holdings | Deposit value | Since | Rank |
|---|---|---|---|---|---|
| C3 | 2 (H3, C) | 13 | 17.5 | day 49 | 1 |
| C1 | 2 (H1, N1) | 13 | 16.0 | day 44 | 2 |
| C2 | 2 (H2, N2) | 11 | 15.0 | day 51 | 3 |
| C4 | 1 (H4) | 12 | 14.5 | day 40 | 4 |

C1 and C3 tie on systems and holdings; deposit value decides. The *since* tie-break would only matter if the deposit values were also equal.

**An early win.** On day 38 at 14:05, C2's outpost in N3 completes. C2 now holds 4 of N3's 6 sites, its sixth majority (H2, N2, N3, H3, C, N1). That makes 26 holdings. Phase 9 of that tick fixes the result, *early win*. The server writes the final snapshot and refuses commands from then on. Two hours earlier, when the outpost was deployed, C2 already controlled 5 systems, so everyone had a public `NearEarlyWin` warning, and the outpost could be destroyed at any time (D-054).

**Most MVP seasons will be decided by the tie-breaks** (§9 item 1). Majorities need 4–5 holdings per system, and four commanders share 60 sites.

## 6. Questions

Numbered Q190–Q197 (Q198–Q199 unused). Each has a recommendation and alternatives.

- **✅ Q190 (D-191) — Where standings are computed, and when.**
  *Recommendation:* in the simulation state, recomputed in phase 9 on any tick where completed holdings or their owners changed, with the control map cached there. The *since* tie-break needs this tick precision, and S6b and the AI need the standings inside the step. *Alternatives:* recompute every second (the same results, with *since* rounded to a second); compute only at the season end (no live standings, and the third tie-break needs a separate change log); a server read model only (outside the deterministic state, so neither the AI nor S6b's comeback grant can read it).
- **✅ Q191 (D-192) — When the early win triggers.** D-020 says "reaching" the threshold wins early.
  *Recommendation:* the season ends on the first tick on which a commander controls at least ⌈60% × claimable systems⌉ (6 of 9), and that tick is the final tick (D-116). Control only changes through visible, slow events (2 h outposts, scheduled sieges), so there is time to react before the decisive tick. *Alternatives:* the threshold must be held for 24 h; for 48 h, about one counter-siege cycle; no early win before week 4.
- **✅ Q192 (D-193) — "Whoever reached the final count first" (D-041's third tie-break).**
  *Recommendation:* among commanders tied on systems, holdings and deposit value, the one whose triple has been unchanged since the earliest tick wins: they reached the tied position first. *Alternatives:* the first tick the commander ever reached its final system count, even if it later fell and recovered; the tick its system count last changed, ignoring holdings and value.
- **✅ Q193 (D-194) — A tie after every tie-break** (same triple since the same tick).
  *Recommendation:* joint winners. *Alternatives:* the lower `CommanderId`, as in D-057's capture tie-break (the human is `CommanderId(1)`, so this would systematically favour the human, against D-078); a seeded draw from the season stream.
- **✅ Q194 (D-195) — The final tick in the simulation.** Today only the server knows when the season ends.
  *Recommendation:* `SeasonConfig` carries the final tick, set at creation from the season length (D-106). The step that computes it fixes the result, views show the season clock, and the server stops when the simulation reports a result, at the full length or at an early win. *Alternatives:* the server keeps the end tick and calls a pure `final_standings(state)` after the last tick (the simulation still detects early wins, but the AI cannot see the end); a journaled operator input `EndSeason` at the final tick.
- **✅ Q195 (D-196) — The read model** (doc 20 §6).
  *Recommendation:* an append-only `standings_history` (a row per commander per change) and a `season_result` written once, both projections that are re-synced after recovery; `status` prints them. Changes are rare, so the history is tiny, and it serves the season panel and the away report's rank changes. *Alternatives:* only `season_result`; a `standings` table of current values, overwritten on each change.
- **✅ Q196 (D-197) — A warning one system before the early win.**
  *Recommendation:* a public announcement when a commander reaches threshold − 1, and another when they drop back. It reveals nothing new (control is public, D-075), but it makes the threat visible. An offline player then learns of it from the away report and notifications instead of finding the season over. *Alternatives:* no announcement, with the season panel showing progress only; announce at threshold − 2 as well.
- **✅ Q197 (D-198) — When S8 is built.** Doc 30 lists S8 in α's contents, but D-095's critical path does not place it.
  *Recommendation:* now, in parallel with S5b and S6a. It needs only built code (holdings, control, the server's season end), and S6b's comeback grant (D-167) needs its standings leader. *Alternatives:* after S6b and S7, with S6b computing the leader itself in the meantime; last in α, with the harness season runs.

## 7. Tasks, in order

1. **Rules and state:** a `season` rules section (early-win share 600‰); `SeasonConfig`'s final tick (Q194), set by `create` from the season length. Snapshot format bump; pins re-recorded.
2. **Standings:** the standing, the ranking with *since* (Q192, Q193), the cached control map, the dirty flag from outpost completion (S4) and from S6b's capture and jump hooks (whichever lands second wires them), the phase-9 recompute (Q190), the pure function for S6b.
3. **Result:** the early win (Q191), the final tick, `SeasonEnded`, `NearEarlyWin` (Q196), and `step`'s guard after the result.
4. **Server:** stopping on the result, live and in catch-up; the read-model tables and their re-sync after recovery (Q195); `status`.
5. **Views:** the accessors (§4.7); S7's `CommanderView` and N4's `standings` collection, if those have landed.
6. **Harness:** `--season` prints the standings history and the result, and checks them against a brute-force recount of the final holdings. Until A2 exists, a scripted claim scenario (S4's builders, with a scripted capture once S6b lands) drives control changes and an early win.
7. **Docs:** doc 10 §6 notes for Q190–Q197; doc 20 §6's read-model tables; doc 30's S8 row; this plan's status.

## 8. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** the same seed and script give the same hash. A snapshot taken one tick before the final tick, and one taken during an outpost's last minute before an early win, restore to the same result.
- **Ranking tests:**
  - every tie-break in order (Q192, Q193);
  - a property test over random holding sets, in which the ranking matches a brute-force sort and is consistent across permutations of insertion order;
  - outposts and a mother station in transit count for nothing;
  - havens never count.
- **Result tests:**
  - an early win at exactly the threshold (6 of 9) fixes the result on that tick, and 5 of 9 does not;
  - the scheduled end fixes the result at the final tick;
  - joint winners;
  - `step` after the result changes nothing.
- **Server tests:**
  - an early win stops the server at that tick, live and during catch-up, with a final snapshot;
  - commands are refused afterwards;
  - the read model matches the state after a crash and recovery;
  - `status` shows the result.
- **Events:** `ControlChanged` and `StandingsChanged` fire exactly when the recount differs; `NearEarlyWin` fires at threshold − 1 and clears when the count drops back.
- **Harness:** `nomad-harness --season` runs a scripted season to its result, and the result matches the brute-force recount.

## 9. Stress test

1. **Tie-breaks will decide many MVP seasons.** Four commanders share 60 sites, and a majority needs 4–5 holdings per system. Most systems will be Contested or Open, and one or two systems each will be common, so "holdings owned" often decides. That is D-041 working as agreed, but A2's expansion scoring must value control and holdings, not only income (A2 Q214).
2. **The early win is a cliff.** One outpost completion can end the season (Q191). The counterplay is real: outposts can be destroyed at any time (D-054), the deploy is visible for 2 h, and Q196 makes the threat public. If playtests find instant wins anticlimactic, Q191's hold-time alternatives remain available.
3. **Identity bias in ID tie-breaks.** `HUMAN_COMMANDER` is `CommanderId(1)`. Any rule that breaks ties by lower `CommanderId` favours the human. Q193 avoids that for the season. D-057's capture tie-break already has this property, but exact ties in core damage are rare (§10).
4. **The control cache adds a second source of truth.** If a new ownership change path forgets the dirty flag, the cache goes stale. The harness's brute-force recount at the end of every `--season` run, and a debug assertion comparing cache and recount on dirty ticks in tests, catch it.
5. **Season end and AI planning.** The Strategist can see the final tick (Q194), so it can stop expanding when a holding can no longer pay back before the end (A2 §4.4). That is rational play, and the human can see the same clock.

## 10. Conflicts and gaps found

1. **D-116 against D-020:** D-116 fixes the result "at the season's final tick", and the server stops only at the scheduled one. D-020's early win needs an earlier final tick. That is a gap, not a contradiction. Q191 and Q194 make the early-win tick the final tick.
2. **D-167 depends on S8:** S6b's comeback grant needs "the standings leader", but D-095 orders S6b before S8. Q197 builds S8 first; otherwise S6b computes the leader itself.
3. **D-057 and D-163 against D-078:** the capture tie goes to the lower `CommanderId`, and the human is `CommanderId(1)`, so every exact tie favours the human. Exact ties are rare. This plan does not reopen D-057; Q193 keeps S8's own tie-break free of the bias.

## 11. As built

*Derived* unless marked; the *Proposed* items need the owner's approval.

- **Where:** `crates/nomad-sim/src/season/` (the record, the count and the ranking, phase 9), the `season` rules section (`early_win_share_permille` 600, `rules/season.rs`), the S8 announcement kinds in `announce.rs`, and the public shapes `Standing`, `SeasonResult` and `SeasonEndReason` in `nomad-types` (data only, D-086). In the server: the stop on the result, `meta.final_tick`, the read model (`store/read_model.rs`) and `status`. `nomad-harness --season`.
- **Who has a standing:** every commander with a record, every owner of a holding, and everyone who already had one, so nobody drops out. Ranks are competition ranks (1, 1, 3). Within a shared rank the list is in identity order only to be deterministic; nothing reads that order as a tie-break (D-229).
- **At creation** the standings are counted without announcements. The four commanders start equal on everything and share rank 1, so a season in which nobody claims ends with four joint winners (D-194).
- **The dirty flag** (D-191) is set by an outpost's core completing (S4) and by placing a completed holding directly (the generator, scenario fixtures, tests). `holding::counts_for_control` is the one predicate for what counts: S6b changes it for the mother station in emergency-jump transit, and its capture, jump and landing call `season::mark_dirty`. `control_state()` reads the cache, recounting while the flag is set. `season_problems()` compares the cache with a recount (§9 item 4); the tests and the harness check it.
- **The early win** (D-192): the threshold is at least 1, so a galaxy without claimable systems has none. The share must be more than 500‰ and at most 1,000‰, so at most one commander can reach it. The decisive tick announces `SeasonEnded`, not a change in the near-win set.
- **The near-win warning** (D-197) means exactly one system short, and only while the threshold is at least 2.
- **The final tick** (D-195): `SeasonConfig::final_tick`, which `SeasonConfig::new` sets to 8 weeks (D-106). `create` sets it from `--season-weeks`, or from `CreateParams::final_tick` for short test seasons. *Agreed (D-234):* `step` on a decided Universe returns at once, changes nothing and counts every input as rejected.
- **The server** stops as soon as the state holds a result, live or in catch-up, and `ServerEvent::SeasonOver` carries the result. An early win ends catch-up on its tick. No tick remains to carry `CatchUpEnd` after it, so that final state keeps catch-up mode on. Replay stops at a result, and recovery also checks the final tick against `meta`.
- **The read model** (D-196): the persistence thread writes the rows as the announcements arrive, after the journal before them. At every start a re-sync drops rows after the recovered tick, which belong to ticks a crash undid and are computed again. If the newest rows still differ from the state, it appends the state's standings at the tick they last changed (the latest *since*), and it makes `season_result` match. So the history is append-only during play, and the re-sync is the only deletion. `create` writes the tick-0 standings. Database schema format 2.
- **Views (§4.7):** `standings()`, `standings_now()` for S6b's comeback grant, `Standings::leaders()` and `leader_holdings()` (the D-167 reference), `season_result()`, `season_over()`, `final_tick()`, `claimable_systems()`, `early_win_threshold()` and `near_early_win()`. None of them is on the wire yet: N4's `standings` collection takes them, and the protocol stays at version 4. S7's `CommanderView` exposes them as public data when it lands.
- **The harness:** `--season` claims by script with `scenario::place_outpost`, outposts placed without the MULE trip, every `--claim-hours` (12 by default, rotating who claims first). `--early-win` makes commander 2 claim every hour. The run is checked against an independent recount of the final holdings. With seed 7, commander 2 wins early at day 1 00:59 (tick 899,997), after a near-win warning at day 0 19:59. Without `--early-win`, the 14 days end at the final tick, and commander 2 is first with 3 systems.
- **Formats:** snapshot 14 (13 was D-217's diminishing yield), rules `nomad-rules@8`.
