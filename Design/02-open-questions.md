# 02 — Open questions (round 1)

Status: **Open.** Answers are recorded in `03-decision-log.md`, and the question is then marked answered here. The recommendation is mine and you are free to overrule it. **Blocking** means the next design document cannot be written without the answer.

## A. Universe and time

**Q1 — Time model.** *(Blocking: docs 10, 11, 14)*
Does the Universe run as one continuous real-time simulation at 1:1, with tactical combat on the seconds scale shown in the phase-1 HUD ("TTI 1.1 s")? Or is there a slower strategic clock, with real-time battles only inside holdings?
*Recommendation:* one continuous real-time simulation on a fixed tick (about 10 Hz). Calibrate timers in layers: seconds for tactical play, minutes for builds, hours for travel between systems. That gives a 24-hour Universe without a separate turn system.

**Q2 — Universe lifecycle.** *(Blocking: doc 10; see W-04)*
Is there one forever-persistent Universe, or seasons (for example 8–12 weeks) that end in a win condition and a reset?
*Recommendation:* seasons (P-06).

**Q3 — What does "up to 100 players" mean?** *(Blocking: docs 10, 20)*
Is it 100 commanders in one Universe, or 100 players online at once? What concurrency do you expect: 20–30% online is typical for the genre. Will several Universes run side by side?

**Q4 — Topology.** *(Blocking: doc 10)*
- Is the phase-1 hierarchy (Galaxy › Arm › Sector › System › Orbit › Holding › Ops grid) the real gameplay structure, or are some levels only flavour?
- Is a Universe exactly one galaxy?
- Is travel between systems only through gates and lanes?
- Can a holding have more than one ops grid ("OPS GRID 0")?
- Please confirm that Kestrel-7 is a *system*. The top bar says "SECTOR KESTREL-7".

## B. Players, conflict and the Deputy

**Q5 — Who are the hostiles?** *(Blocking: docs 12, 15)*
Are they other players (PvP), NPC factions such as raiders (PvE), or both? Is the siege fleet in the phase-1 scenario a player or an NPC? This changes the difficulty of W-01 by an order of magnitude: PvE-only makes offline defence a tuning problem, while PvP makes it a fairness problem.

**Q6 — The Deputy's authority.** *(Blocking: doc 12)*
Which of these may the Deputy do on its own?

1. Continue existing build queues
2. Start new builds or spend resources
3. Repair
4. Re-route power, for example to shield priority
5. Use abilities with cooldowns (SHLD SURGE, FLAK SCREEN)
6. Launch and recall defenders
7. Pursue attackers beyond its own holding
8. Reinforce an ally's holding
9. Retreat or evacuate
10. Run convoys and logistics
11. Diplomacy

*Recommendation (P-01):* 1, 3, 4, 5, 6, 9 and 10 within owner-authored standing orders. Item 2 only from an owner-approved list. Never 7, 8 or 11.

**Q7 — Offline losses.** *(Blocking: docs 11, 15)*
Is it acceptable for a player to lose their mother station, or any holding, while offline? If not, which protection do you want: reinforcement timers, vulnerability windows, loss caps, post-attack protection or allied cover?
*Recommendation:* reinforcement through the shield (P-05) plus allied cover.

**Q8 — Deputy while online.** Can a player hand a holding or fleet to the Deputy while they are connected?
*Recommendation:* yes (P-01).

**Q9 — Abandonment.** What happens to a player who stops logging in? Options: the Deputy runs the empire indefinitely, it decays, or after N days it becomes neutral or NPC-owned. With 100 slots, idle empires otherwise occupy space forever.

**Q10 — Losing the mother station, and the name "Nomad".** If the mother station is destroyed, is the commander eliminated, do they respawn, or do they relocate? The repository is called *Nomad*. Is that the working title, and does it mean the mother station can move between systems? A mobile mother station would change the economy, the defence and the Deputy design substantially.

## C. MVP

**Q11 — Does the Universe keep running in the MVP?** *(Blocking: docs 20, 30)*
When the single human closes the client, does the Universe continue, with a headless server and the Deputy taking over, or may it pause?
*Recommendation:* keep running (P-03). Otherwise R-02 and R-03 go untested until after the MVP.

**Q12 — MVP gameplay scope.** *(Blocking: doc 30)*
- Which loops are in: harvesting, refining, building modules, ship production, fleet control, combat, travel between systems, diplomacy with AI players?
- What are the win and lose conditions?
- What session length are you targeting?

**Q13 — Hosting and platforms.** *(Blocking: doc 20)*
- Will the MVP server run locally next to the client, or on a hosted machine (Azure or GCP)?
- Which client platforms: Windows only, Linux and macOS too, or the web (wgpu can target WebGPU)?

**Q14 — Sequencing against phase 1.** Is the phase-1 visual slice still the first deliverable, with the MVP to follow? Or should phase 1 be folded into the MVP milestones?
*Recommendation:* run phase 1 as specified, and design the simulation, Deputy and server in parallel. The fixture/view-model is the seam between them.

**Q15 — The AI players in the MVP.** *(Blocking: doc 13)*
- Are they fixed enemies, potential allies, or free agents with diplomacy?
- Do they play by the human rules with the same information, or are they allowed to cheat?
- Should some of them simulate an offline human, handing over to their Deputy on a schedule, so the MVP exercises human-vs-Deputy fights?

## D. Technical and terminology

**Q16 — Scope of "no World".** Does the rule cover documents and player-facing text only, or also our code identifiers? I have assumed it includes code, with third-party `World` types wrapped. Please confirm.

**Q17 — Rust stack constraints.** Do you have preferences or constraints for any of these?
- ECS: `bevy_ecs` standalone, `hecs`, or our own
- Async runtime and transport: `tokio`, QUIC via `quinn`
- Persistence: SQLite for the MVP, PostgreSQL later
- Serialisation
- Hosting: a cloud you want to standardise on, or one you want to avoid

**Q18 — Working title.** Is the game called *Nomad*? The phase-1 package uses "Voxel RTS".
