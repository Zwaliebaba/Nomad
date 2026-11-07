# Work package S5 — Combat

Status: **Built** (Q128–Q138 → D-139 to D-149; S5a details D-188; S5b readings D-227; D-228, decided from S5b, moves the peripheral slots outside the dome with S6b). S5a (ship combat, tasks 1–7) is built in `crates/nomad-sim/src/combat/` and `nomad-harness --combat skirmish|battle|stress`; S5b (station combat, tasks 8–13) in `crates/nomad-sim/src/defence/` and `nomad-harness --combat breach|core`. Measured as D-147 says (seed 1): a full sector breaches in 7.5 min with the Deputy's default SHLD SURGE (4.7 min without abilities), and the core dies in 12.8 min through an open sector with turrets absent, matching the §5 hand calculations. *Derived, as built since:* D-284 raised LANCER hull to 1,400 so the counter triangle holds at equal value; `nomad-harness --combat triangle` checks it and the 12/8/0 against 8/4/8 battle is a counter test, not a time-to-kill target (doc 15 §12 note). Written together with `S4-economy.md`, because power, modules and module damage are shared between the two. Builds on S3 (ships, fleets, orders, the cruise-drop hook) and, for its second half, on S4 (modules and power). This plan is also the brief for whoever builds S5.

## 1. Goal

Ships and stations fight. Weapons fire travelling projectiles, lock narrows spread, the counter triangle decides matchups, and damage destroys ships. The dome blocks hostile fire by sector and regenerates from allocated power. Turrets and the station abilities work, and a breach is reported for S6's siege lifecycle. All of it is deterministic, integer where doc 15 says so, and covered by tests. The harness can measure the four D-058 time-to-kill targets, which MVP-α needs within ±50% (D-094).

## 2. Inputs

- **Combat:** doc 15 §2 (2D, swept hits, integers), §3.1–3.2 (classes, multipliers; D-052), §4 (projectiles, lock, fire control, flak, ranging laser, damage order; D-052), §5 (dome, power and regeneration, turrets and abilities; D-053), §10 (raiders never damage domes; D-056), §11 (TTK targets; D-058), §12 (starting values; D-058).
- **Already decided on the S3 side:** D-113 (new fleets on *weapons free*), D-114 (an engagement ends after 30 s with no hostile within 20 km and no fire taken; built here and in S7), D-115 (a mixed fleet's weapon range is the shortest), D-055 (jump shroud: cannot be targeted or fire).
- **Dome geometry:** D-046 (sector arcs centred on pylons), D-100 (the hit circle is the globe's equator), doc 10 §8 (36 voxels ≈ 1.26 km).
- **Power and modules:** `S4-economy.md` §4.5–§4.6 (allocation, the shared interfaces).
- **Hooks for later packages:** doc 12 §3–§4 (stakes rule, shield lifecycle; S6), D-057 (attackers, capture by core damage; S6), doc 16 §3 (detection; S7), doc 13 §4 (the Deputy's alert and ability triggers; A1).
- **Simulation:** doc 11 §4.1 (combat phase 7 every tick), §5 and D-049 (seeded streams, `SimId` order, `libm`), D-052 (integer damage, percentage multipliers), D-104 (rules file).
- **Code:** `crates/nomad-sim` fleets (`Ship`, `Motion`, `Shroud`, cruise mode, `set_fleet_engaged`), the neighbour grid, `Owner`, `TickOutputs`, snapshot format 6.

## 3. Scope

S5 is built in two parts (Q128).

**S5a — ship combat** (needs only S3):

- `Hull` on ships; weapons per class from the rules file.
- Targeting by explicit order and by the auto-target rule (Q133), under the rules of engagement (Q134).
- Lock and spread, linear-lead fire control, the seeded spread draw.
- Projectiles with swept hit tests against ship circles (Q132), torpedoes, flak bursts with area damage, range expiry.
- The multiplier table, the ranging laser, raider damage rules.
- Damage application in doc 15 order, and destruction.
- The jump shroud and cruise mode block firing; engagement state with D-114's 30 s timer.
- Hostility from `Owner`; friendly fire as Q135.
- Harness: the skirmish and fleet-battle TTK scenarios.

**S5b — station combat** (needs S4's modules and power):

- The dome: four sector pools, maxima from pylons (Q129, Q130), regeneration from allocated power (Q131), hit tests against the dome circle, weak and breach states, a breach event.
- Turrets as module weapons firing out through the one-way dome; STOP (all turrets hold).
- Hit tests against module footprints; damage to modules, with the outcome at zero hull (disabled or destroyed) behind an S6 hook.
- Abilities: SHLD SURGE (Q136), FLAK SCREEN (Q137); PWR ROUTE is S4's.
- A damage ledger for S6 (who damaged the dome and when; core and total damage per attacker).
- Harness: the breach and core-kill TTK scenarios.

**Out of scope:**

- **The siege lifecycle** (Reinforced, Siege, Held, Fallen), capture, emergency jumps, outpost combat rules and the exposure window (S6). S5 provides the breach event, the sector-open and immune switches, and the ledger.
- **Detection** (S7). Until then every entity counts as visible, as in S3; §4.7 explains why targeting results are unaffected.
- **Deputy and AI behaviour** (A1–A4): ability triggers, retreat thresholds and target priority above the auto-target rule.
- **Salvage** (none, D-071) and munitions (D-065).

## 4. Design

### 4.1 Weapons and the rules file

*Agreed* model (D-052); starting values from doc 15 §12 (D-058).

- **A weapon** has a class (light cannon, torpedo, flak, battery, turret), damage per shot, a fire interval in ticks, a projectile speed, a range, a lock time, and spread from base to minimum (cannons and batteries 20→4 mrad, torpedoes 30→6 mrad).
- **Per class:** the weapon list (a WARDEN has 2 batteries and 1 flak), a hit radius (Q132), and a size class (Light, Medium, Heavy) for multipliers. Modules are Structure; dome sectors are Dome.
- **The multiplier table** (doc 15 §3.2) is rules data: `damage = base × multiplier / 100`, rounded down (D-052).
- **The rules file** gains a combat section, and its format version increases. **Rule check:** every weapon's range stays below its platform's sensor range times the smallest signature (0.6), so "in range" implies "detected by the shooter itself" (§4.7). S7 adds the sensor ranges this check needs; until then it is a test against doc 16's table.

### 4.2 Targeting, lock and fire

*Agreed* behaviour (D-052, doc 15 §4); the auto-target rule is *Proposed* (Q133).

- **Who may fire:** not in cruise mode, not shrouded, and the fleet's rules of engagement allow it (Q134). Docked ships never fire.
- **Target choice per weapon:**
  - an Attack order's target when it is in that weapon's range;
  - otherwise, under *weapons free*, the hostile in range with the highest multiplier for this weapon, then the nearest, then the lowest `SimId`. A weapon keeps its target until the target leaves range, dies or is shrouded, so lock is not thrown away.
- **Lock** rises linearly from 0 to 100% over the lock time while the target is in range and detected, and resets on a target change. The HSTL ranging laser adds 50% lock rate per allied weapon on the designated target (doc 15 §4).
- **Spread** is interpolated from base to minimum by lock. Each shot draws its angle from the system's combat stream (D-049) in a fixed order: (ship `SimId`, weapon index).
- **Fire control** aims with linear lead (`libm`), so a manoeuvring target can dodge.

### 4.3 Projectiles and damage

*Agreed* (doc 15 §4); storage is *Proposed*.

- **Projectiles** live in a per-system list ordered by a `ProjectileId` from a Universe-wide counter. They are not `hecs` entities: they are short-lived and numerous, and the list keeps the world lean. The id plays the part doc 15 gives the "projectile `SimId`" in the damage order.
- **Each tick,** a projectile's swept segment is tested against ship circles (Q132), module footprints (S5b) and dome circles (S5b), through S3's 2 km neighbour grid. The first hit along the segment counts. Torpedoes do not home. Flak bursts at the aim point after its flight time, damaging every hostile Light and Medium target within 150 m.
- **Projectiles expire** at their range. They pass through friendly ships (Q135).
- **Damage** for the tick is collected, then applied in `(target SimId, ProjectileId)` order (doc 15 §4). Hull reaching 0 destroys a ship at the end of the combat phase: it leaves its fleet, an empty fleet dissolves, and a `ShipDestroyed` event names the killer's owner.
- **Raider torpedoes** use their own damage, and a 0 multiplier against domes, so raiders can never breach (D-056).

### 4.4 Engagement and cruise mode

*Agreed* rule (D-114); implementation *Derived*.

- **A fleet becomes engaged** when it takes fire, or when a hostile is within 20 km (everything counts as visible until S7). Each trigger refreshes an `engaged_until` deadline 30 s ahead.
  > *Note (2026-11-07), as built:* "everything" means every hostile ship in space. Docked ships and fleets in lane transit are not in space, so they neither trigger nor can be hit. Shrouded hostiles trigger engagement but cannot be targeted or hit (D-188). Auto-targeting picks only ships; domes and modules need an explicit Attack (D-227).
- **While engaged** the fleet cannot cruise (S3's hook). Once the deadline passes it may cruise again.
- **"Taking fire"** means one of the fleet's ships was hit. These events also feed the Deputy's alert (doc 13 §4, A1).

### 4.5 The dome (S5b)

*Agreed* model (D-053, doc 15 §5.1); maxima and regeneration are *Proposed* (Q129–Q131).

- **A `Dome` per holding:** the centre, the radius (36 voxels in metres, Q130), and four sectors with strength in milli-points and a maximum.
- **Hit test:** a hostile projectile whose swept segment crosses the circle from outside damages the sector whose arc contains the crossing point (D-046 arcs, converted from grid azimuth to the system frame), using the Dome multiplier, and stops. Outgoing fire passes (one-way).
- **States:**
  - below 50% the sector reports weak;
  - at 0 it is a breach: a `SectorBreached { holding, sector, tick }` event for S6.
  - S6 sets `immune` (Reinforced: no damage taken) and `open(sector)` (Siege: no regeneration, no blocking, projectiles pass through to modules).
- **Regeneration** every tick from S4's allocation for that sector (Q131).
- **Ledger** for S6 (D-057): per holding, dome damage per commander with ticks, so S6 can find the attackers of the 10 minutes before a breach; and core damage and total damage per commander while a siege runs, for the capture rule.

### 4.6 Interfaces shared with S4

*Proposed;* the same table is in `S4-economy.md` §4.6.

| Interface | Owner | Used by |
|---|---|---|
| Module entities with `Hull`, `Disabled`, slot position and hit radius | S4 | S5 hit tests and damage |
| `PowerAllocation` per holding: MW per sector for regeneration, powered flag per turret, ability headroom | S4 (1 Hz, and on ability start) | S5 regeneration, turrets, abilities |
| `request_ability_power(holding, ability)` → granted or refused | S4 | S5 SHLD SURGE and FLAK SCREEN |
| `DamageApplied { target, amount, by }` and the module outcome (disabled or destroyed) | S5 | S4 production and supply; S6 attribution |
| `structure_outcome_at_zero(module)`: disabled or destroyed | S5, with the doc 12 §3 default; S6 adds the window | S5 damage application |
| Pylon count per sector, from completed pylon modules | S4 | S5 sector maximum (Q129) |

- **Until S6,** a peripheral module at 0 hull is **disabled** (the outside-window rule, which is the safe default). Inner modules can only be hit through an open sector, which only S6 opens. Recovery of disabled structures is Q138.

### 4.7 Detection before S7

*Derived* from doc 16 §3 and doc 15 §12.

- Every MVP weapon's range is below its own platform's detection range for the smallest signature. The tightest case is the LANCER: 10 km × 0.6 = 6 km against a 3 km torpedo range. Turrets use the holding's 40 km × 0.6 = 24 km.
- So a target in range is always detected by the shooter itself, and S7 cannot change who can be fired on.
- S7 changes only target acquisition beyond the shooter's own sensors (the shared picture) and the 20 km engagement trigger. Snapshot hashes will change when S7 lands, which is allowed before β (D-099 item 9).

### 4.8 Turrets and abilities (S5b)

*Agreed* mechanics (doc 15 §5.3); parameters *Proposed* (Q136, Q137).

- **Turrets** use the auto-target rule and fire outward through their own dome. They fire only while S4 reports them powered, and not while the holding's STOP is set.
- **SHLD SURGE** instantly restores 20% of a chosen sector's maximum, with a 90 s cooldown and 200 MW for 10 s. It is refused if the power is not available, or if the sector is open.
- **FLAK SCREEN:** see Q137; 90 MW while active, 60 s cooldown.
- **Commands:** `HoldingOrder::Ability`, `HoldingOrder::TurretsHold`; each with validator reasons.

### 4.9 Determinism, performance and snapshots

*Derived* from doc 11 §4–§5.

- **Order:** systems in `SystemId` order; weapons in (ship `SimId`, weapon index) order; random draws only from the system's combat stream, in that order; damage in `(target SimId, ProjectileId)` order. Kinematics in `f64` with `libm`; hull, damage, strength and regeneration as integers (milli-points for regeneration).
- **Cost:** doc 15 §13 estimates 200 ships and about 600 live projectiles for a large battle. Target search and sweeps go through the neighbour grid. The harness reports tick p99 for a 20-against-20 battle and a 100-against-100 stress battle, and stage-1 fast-forward speed over a combat-heavy day. Doc 11 §4.3 targets at least 500× real time at stage 1 over a season, which battles must not break.
- **Snapshots:** S5a and S5b each add one format bump: hull, weapon cooldowns and locks, projectiles and engagement deadlines in S5a; domes, ability cooldowns and the ledger in S5b. Pins are re-recorded each time.

## 5. TTK check against D-058

*Derived* from the doc 15 §12 starting values; the α harness replaces these estimates.

| Target | Estimate | Verdict |
|---|---|---|
| Skirmish, 6 against 6, 30–90 s | 6 VX against 6 VX: 32 dps per VX against 3,600 hull per side is **about 19 s**, plus 2 s of lock. At model-size hit radii almost every shot hits (Q132) | About 35% fast |
| Fleet battle, 20 against 20, 3–8 min | Depends on composition; not estimated by hand | Measured |
| Breach a full mother-station sector, 10 LANCER + 4 WARDEN, 5–10 min | Torpedoes 10 × 40 = 400/s, batteries 4 × 45 = 180/s, regeneration −50/s: 530/s net against 150,000, **4.7 min**. With SHLD SURGE on the Deputy's default trigger (below 30%), **about 9 min** | Just under without abilities; inside with them (Q136) |
| Kill the core in a siege, same fleet, 15–25 min | Torpedoes 10 × 60 = 600/s, batteries 4 × 45 = 180/s against 600,000: **12.8 min**, longer if torpedoes hit other modules on the way | About 15% fast |

- **A geometry result for S6 and the AI:** strike craft breaching from 3 km sit about 4.3 km from the dome centre. That is beyond turrets in inner slots, which reach at most about 3.6 km from the centre, so a covered strike group can out-range static defences, as doc 15 §12 intends. A group within 3 km of the core during a siege is inside the reach of the turrets on its side of the station, so "unopposed" in D-058's core-kill target must exclude turrets or count their losses (Q136).

## 6. Questions

Numbered Q128–Q138; each has a recommendation and alternatives.

- **✅ Q128 (D-139) — Split S5 and its order with S4.** S5a (ship combat) needs only S3 and can start now. S5b (dome, turrets, abilities, module damage) needs S4's modules and power.
  *Recommendation:* build S5a in parallel with S4, then S5b. *Alternatives:* S4 first, then all of S5; all of S5 now, with power assumed full until S4.
- **✅ Q129 (D-140) — Sector maximum (conflict in D-053).** D-053 and doc 15 §5.1 say a core alone gives 25% and "each pylon adds 25% to its sector", which never reaches the full 150,000 per sector that doc 15 §12 gives a four-pylon mother station. Doc 15 also says "a sector's strength *is* its pylon's projection".
  *Recommendation:* sector maximum = 25% from the core + 75% from that sector's pylon, so four pylons give full strength and an unbuilt pylon leaves its sector at 25%. *Alternatives:* 25% + 25% per pylon anywhere in the holding, capped at 100% (three pylons reach full); keep D-053 literally and define full as core + own pylon (each sector 50% of today's value).
- **✅ Q130 (D-141) — The same dome for every core.** The mother station's dome is 36 voxels (1.26 km) with 150,000 per full sector and 50/s regeneration. Nothing defines other cores' domes.
  *Recommendation:* every core has the same radius, maxima and regeneration; other holdings differ only through their 120 MW core supply and their pylons. One slot template then serves every holding (S4 Q119). *Alternatives:* other cores at half strength; a smaller radius (24 voxels) for other cores.
- **✅ Q131 (D-142) — Regeneration.** Regeneration = allocated MW × 50 / 120 per second (50/s at the full 120 MW). The maximum rate and the MW drawn scale with the sector's maximum, so a 25% sector regenerates at most 12.5/s and draws at most 30 MW.
  *Recommendation:* adopt. *Alternative:* a fixed 50/s and 120 MW for any sector size, so weak sectors refill fast.
- **✅ Q132 (D-143) — Ship hit radius.** Doc 15 §4 takes hit circles from the model's size, but ships use the station's voxel size (phase-1 doc 03), so at 35 m per voxel the models are large: VX 280 × 175 m, WARDEN about 600 m long. Spread (4–20 mrad, 6–30 m at 1.5 km) and dodging then barely matter, and skirmishes run about 35% short.
  *Recommendation:* the radius is half the model's short side, as rules values (VX ≈ 90 m, LANCER and WARDEN ≈ 120 m, MULE ≈ 50 m, HSTL ≈ 190 m), and TTK is tuned through hull at α. *Alternatives:* a separate, smaller collision radius (about a quarter of the model), so that lock and spread matter; half the model's long side.
- **✅ Q133 (D-144) — Auto-targeting.** With no explicit target, under *weapons free*, each weapon takes the hostile in range with the highest multiplier for that weapon, then the nearest, then the lowest `SimId`, and keeps it while it stays valid.
  *Recommendation:* adopt; the Deputy's target priority (doc 13 §4) overrides it with explicit targets. *Alternatives:* nearest first; lowest hull first.
- **✅ Q134 (D-145) — What *return fire* means.** Doc 15 §3.4 names it without a rule.
  *Recommendation:* a fleet on return fire engages only hostiles that damaged one of its ships, or one of its commander's structures within its weapon range, in the last 30 s (D-114's window). *Alternatives:* only hostiles that damaged this fleet; any hostile while the fleet is engaged.
- **✅ Q135 (D-146) — Friendly fire.** Doc 15 does not say whether flak bursts and projectiles hurt one's own side.
  *Recommendation:* none: projectiles pass through friendly ships, and flak damages only hostile targets. *Alternative:* full friendly fire (flak near one's own interceptors hurts them).
- **✅ Q136 (D-147) — SHLD SURGE and what "unopposed" means in D-058.** A surge restores 20% of a sector per 90 s, about 333/s on average, nearly 7× full regeneration. Breach time is 4.7 min without it and about 9 min with the Deputy's default trigger. Turrets on the attackers' side reach strike craft that are killing the core.
  *Recommendation:* keep the surge as a starting value. The breach target is measured with the Deputy's default ability use; the core-kill target is measured with the defender's turrets destroyed or absent. *Alternatives:* halve the surge to 10%; measure both targets with no defence at all.
- **✅ Q137 (D-148) — FLAK SCREEN parameters.** Doc 15 gives "10 s of flak bursts around the station", 90 MW and a 60 s cooldown, but no damage or reach.
  *Recommendation:* for 10 s the holding fires one burst per second at each hostile Light or Medium target within 2 km of the dome centre, at most 8 bursts per second, each with the WARDEN's flak values (30 damage, 150 m radius). *Alternatives:* a fixed 4 bursts per second; bursts only in the weakest sector's arc.
- **✅ Q138 (D-149) — Recovery of disabled structures (doc 12 §3).** A disabled structure restores "10 min after the last hostile leaves, or sooner if repaired", but "leaves" and the restored hull are not defined.
  *Recommendation:* it comes back online once no hostile has been inside its holding's ops area (20 km) for 10 min, at 10% hull, then repairs normally (D-071). REPAIR brings it back online at once. *Alternatives:* back at full hull; "leaves" means out of weapon range of the structure.

## 7. Tasks, in order

**S5a**

1. Rules: weapons, multipliers, hit radii, the range-versus-detection check as a test. Rules format bump.
2. `Hull` on ships and the `ShipView` hull percentage; hostility from `Owner`.
3. Weapons state: cooldowns, targets, lock; the auto-target rule; rules of engagement; Attack and Attack-move fire.
4. Projectiles: the per-system list, lead aim, the seeded spread draw, swept hits against ship circles, torpedoes, flak bursts, expiry.
5. Damage application order, destruction, fleet cleanup, `ShipDestroyed` events. Snapshot format bump.
6. Engagement: taking fire and the 20 km trigger, D-114's 30 s deadline, the cruise hook.
7. Harness `--combat skirmish|battle`, with TTK reports against D-058.

**S5b** (after S4 tasks 2 and 4)

8. Dome component, sector maxima from pylons, hit tests against the circle, weak and breach states and events, the `immune` and `open` switches for S6.
9. Regeneration from the power allocation.
10. Hit tests against module footprints, module damage, the outcome hook, the disabled state and its recovery (Q138).
11. Turrets and STOP; SHLD SURGE and FLAK SCREEN through S4's power requests.
12. The damage ledger for S6. Snapshot format bump.
13. Harness `--combat breach|core`, with and without abilities and turrets (Q136).

## 8. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** same seed and script give the same hash; snapshot and restore mid-battle (projectiles in flight, locks building, a sector regenerating) change nothing; the order in which commands arrive does not matter.
- **Combat tests:**
  - every multiplier in the doc 15 §3.2 table applies, rounded down;
  - lock reaches 100% at the lock time and resets on a target change;
  - spread follows lock; lead aim hits a target flying straight;
  - flak damages only Light and Medium targets within 150 m;
  - raiders never reduce a sector;
  - a shrouded or cruising ship neither fires nor can be targeted where doc 15 says so;
  - damage applies in the agreed order;
  - engagement ends 30 s after the last trigger.
- **Station tests (S5b):** hostile fire stops at the dome and outgoing fire passes; the sector hit matches the crossing azimuth; regeneration follows allocated power; a breach emits its event once; SHLD SURGE and FLAK SCREEN honour cooldowns and power; turrets hold fire unpowered or on STOP; a peripheral module at 0 hull is disabled and recovers per Q138.
- **Validator:** every new rejection reason is covered.
- **TTK:** `nomad-harness --combat` reports all four D-058 scenarios, each within ±50% of its target or listed as a tuning item for A6.
- **Performance:** the 100-against-100 stress battle runs in the harness with tick p99 reported, and a combat-heavy simulated day at stage 1 keeps at least 500× real time in a release build.

## 9. Stress test

1. **Lock and spread may be cosmetic** at model-size hit radii (Q132). If so, the counter triangle and hull carry all of the balance, and dodging, one of D-052's reasons for travelling projectiles, does not exist. Q132's alternative makes it real, at the cost of shots visibly passing through models.
2. **SHLD SURGE dominates regeneration** (Q136). Measured breach times will swing with Deputy doctrine, so A1's ability triggers are part of tuning.
3. **S5b waits on S4.** Sieges (S6) need S5b, and α needs sieges end to end, so the critical path runs S4 → S5b → S6. Splitting S5 (Q128) is what keeps S5a off that path.
4. **The geometry already favours the defender in sieges** (§5): turrets cover the near approaches to the core. That fits RK-02's warning, and the core-kill target must be measured honestly against it.
5. **No detection until S7** makes engagement triggers too generous: everything within 20 km counts as detected. S7 will change fleet behaviour, and the TTK numbers must be re-measured after it.
6. **Projectiles outside `hecs`** keep the entity world small, but they are a second storage path to snapshot and test. A round-trip test with projectiles in flight covers it.
