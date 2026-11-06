# 12 — Offline play and the Deputy

Status: **Proposed (round 2).** Nothing here is agreed until it is recorded in `03-decision-log.md`. This document addresses W-01 (offline asymmetry) and W-02 (Deputy competence paradox) from `01-game-concept.md`. It is written for the hardest case, PvP (Q5). If the hostiles turn out to be NPC-only, the same rules still work, and some of them could be simplified.

## 1. The problem in one paragraph

Players are offline about 85–95% of the time, attackers choose when to strike, and today a siege is decided in minutes. If nothing changes, the best strategy is to attack sleeping players, and the outcome of the game depends on how good the Deputy AI is. Making the Deputy stronger only moves the problem: once the Deputy defends better than a human does, being offline becomes an *advantage*. A fix therefore has to say **what an attacker can achieve, and when**, without that answer depending on the Deputy's skill.

## 2. Principles

**PR-1: Protection depends on the clock, never on connection status.** *(Proposed.)*
Whether a holding can fall depends on the time of day relative to its owner's chosen window. It never depends on whether the owner is logged in. Any rule of the form "protected while the Deputy is in control" invites players to log off when attacked so they become protected; this is the classic combat-logging exploit. **This withdraws P-05 in `01-game-concept.md`,** which had exactly that flaw.

**PR-2: Stakes are tiered.** *(Proposed.)*
At any moment, what a player can lose is bounded. Fleets and assets outside the shield are always at risk, and the player controls that exposure through doctrine. A holding's core can only be lost in a scheduled, time-limited battle.

**PR-3: Only humans take the initiative.** *(Proposed; refines P-01.)*
Attacking, expanding, claiming, diplomacy and changing doctrine are human-only actions. Offensive orders lapse when the attacking commander goes offline, so an attack needs its attacker to be present, at the start and at the decisive moment.

**PR-4: The Deputy is always on.** *(Proposed; refines R-03, needs the owner's approval, see Q22.)*
The Deputy is not a mode that switches on at disconnect. It is the layer that always executes the commander's doctrine. Being online adds direct control on top of it. Going offline only removes that direct control. This avoids a handover discontinuity, lets players see and tune the Deputy every session, and means the offline experience is the same as the online one minus the human.

**PR-5: Being offline must never be advantageous.** *(Derived from PR-1 and PR-4.)*
Two consequences follow:
- The Deputy's tactical competence is capped below that of an engaged human: reaction latency and command rate are bounded (§5.4).
- No mechanic grants anything for being disconnected.

## 3. The stakes rule

> **A holding's core can only be lost during its owner's exposure window, and only after at least 12 hours' notice.**

| Target | Outside the owner's exposure window | Inside the owner's exposure window |
|---|---|---|
| Ships and fleets away from the dome; convoys; harvesters | Can be destroyed in real time | Same |
| Peripheral structures outside the dome (for example SOL-1, mining outposts) | Can be **disabled**: knocked offline, not destroyed. They restore automatically 10 min after the last hostile leaves, or sooner if repaired. | Can be destroyed |
| Shield sectors | Can be worn down. A sector at 0 triggers **Reinforced** (§4). | Same; there is no immediate siege either. |
| Core structures inside the dome | Cannot be attacked: the dome is intact or Reinforced | Can be attacked only during a **Siege** that follows a Reinforced period |

**What attackers can always gain** is economic and positional pressure: raiding fleets that stray beyond their leash, intercepting convoys, blockading lanes (the phase-1 "ORE CONVOY · lane unsafe · HELD" is exactly this), disabling power, and scheduling a siege. These losses are real but recoverable, and they keep the Universe active around the clock.

**What attackers can never do** is take a core while the owner sleeps, or without warning.

The disabled-not-destroyed rule keeps the phase-1 reason for defending SOL-1 (D-003): losing its power weakens the shield sectors. At the same time it prevents an offline player from losing their infrastructure outright.

## 4. The shield lifecycle

This builds directly on the phase-1 sectored shield (D-003). Phase 1 itself does not change: the siege it depicts is the **Siege** state below. Its "Hold MS-01 for relief" objective timer is the siege clock.

```mermaid
stateDiagram-v2
    [*] --> Nominal
    Nominal --> Weakened: a sector drops below 50%
    Weakened --> Nominal: sectors regenerate
    Weakened --> Reinforced: a sector reaches 0 (breach)
    Reinforced --> Siege: exit time reached
    Siege --> Held: siege clock runs out and the core stands
    Siege --> Held: no attacking commander online at siege start
    Siege --> Fallen: core destroyed before the clock runs out
    Held --> Nominal: shield restored, then 48 h immunity from a new Reinforced state
    Fallen --> [*]: consequence per Q21
```

**Reinforced**
- The dome hardens and is immune to hostile fire.
- The commander's own ships can still launch and dock, and production continues.
- The exit time is the start of the owner's first exposure window that begins **at least 12 h after the breach**. It is announced at once to the owner, their allies and the attackers.
- A breach that happens inside the window still leads to Reinforced. Every decisive battle is scheduled; none is ever impromptu.

**Siege**
- The breached sector stays open and cannot regenerate. Every other sector behaves normally.
- The attackers must destroy the core within the **siege clock** (60 min).
- The defender and any allies, who have had at least 12 h to prepare and send relief, must hold.
- If no attacking commander is online when the siege starts, the siege fails at once (PR-3).

**Held**
- The shield restores.
- The holding cannot enter Reinforced again for 48 h. Raids and sector damage still work during that time, but nobody can keep a holding permanently locked down.

## 5. The Deputy

### 5.1 Authority

This is the proposed answer to Q6.

| Action | Deputy |
|---|---|
| Continue build queues and convoy routes | Yes |
| Rebuild losses from an owner-approved replacement list (for example "keep CAP WING at 3 × VX"), within a daily budget cap | Yes |
| Repair; route power according to the doctrine's alert profile | Yes |
| Use defensive abilities (SHLD SURGE, FLAK SCREEN) on doctrine triggers | Yes |
| Launch, recall and retreat defenders; evacuate harvesters and hold convoys at the gate | Yes |
| Engage hostiles inside the defended zone, and anything that fires on our assets, up to the fleet's leash | Yes |
| Start new projects; spend beyond the budget cap; dismantle | **No** |
| Act outside its own holdings, including reinforcing an ally's holding | **No** |
| Attack, expand, claim, trade, run diplomacy, change doctrine | **No** (PR-3) |

The "anything that fires on us, up to the leash" rule closes a loophole. Without it, an attacker could stand just outside the defended radius with longer-range weapons and shoot in safely.

### 5.2 Doctrine (standing orders)

The player writes doctrine. It applies whether the player is online or offline (PR-4).

- **Holding doctrine:**
  - The defended zone radius.
  - Rules of engagement: weapons free in the zone, return fire only, or hold.
  - An alert power profile, for example shields first.
  - Ability triggers, for example SHLD SURGE when a sector falls below 30%.
  - Evacuation thresholds.
  - Repair priorities.
- **Fleet doctrine:**
  - A role: garrison, patrol route, escort or reserve.
  - A leash distance.
  - A retreat threshold, for example dock at 40% hull.
  - Rearm and repair at dock.
- **Economy doctrine:** continue queues, the replacement list with its budget cap, and convoy routes.
- **Presets:** *Fortress*, *Balanced* (the default) and *Trader*, so a new player is reasonably protected without touching anything.

Writing good doctrine becomes a strategic skill that pays off even while the player is offline. Offline outcomes then follow from the player's decisions, not from the quality of the AI.

### 5.3 Connection rules

- **Disconnect grace (120 s).** Direct orders stay valid during a network blip. After the grace period the orders lapse, and the units fall back to their fleet doctrine.
- **An offline attacker** cannot vanish: their ships stay in the Universe and remain vulnerable. Their offensive orders lapse, so the fleet withdraws or holds, according to its doctrine.
- **Notifications:** a breach that triggers Reinforced must reach the owner outside the game client, or the 12 h notice is meaningless. That needs a notification channel; the MVP can start in-client only.
- **Away report:** on login, an after-action report replaces "my station was gone". It lists raids repelled, losses, disabled structures, scheduled Reinforced exits, and decisions the Deputy deferred ("replacement budget exhausted").

### 5.4 Competence target

The Deputy should be **competent and readable, not optimal**. It works as a utility or behaviour-tree executor of doctrine, with no strategic lookahead. Its reaction latency and command rate are capped at human levels, for example 300 ms reaction time and a bounded number of commands per second. This puts it below an engaged human at micro (PR-5). The same cap becomes the difficulty knob for the MVP's AI commanders (P-02).

## 6. Why this resolves the paradox

The Deputy's quality only affects peripheral skirmishes. Peripheral losses are bounded by the player's own doctrine, and cores are decided in scheduled fights. A good Deputy is pleasant, a mediocre one is a tuning issue, and neither decides the game. Being online is worth something specific:

- **initiative:** only humans attack, expand and do diplomacy;
- **direct control** in every fight, against a Deputy that is capped below human micro;
- **presence at the decisive sieges**, which take place inside the defender's own window;
- **economic optimisation**, especially if storage caps are adopted (W-08).

## 7. Stress test of this proposal

1. **Defender advantage and stagnation.** A well-organised defender with a good window and online allies may be nearly impossible to take. *Mitigation:* windows long enough to plan against; constant economic pressure; seasons (Q2) so the board resets. Watch this in playtests.
2. **Time zones.** For some attackers, some targets fall in the middle of their night. That is accepted: the attacker chose the target. Expect time-zone clustering among allies.
3. **Timer bureaucracy.** Games with scheduled vulnerability can turn into calendar management. At 100 players with a few holdings each, the timer load stays small. The away report and HUD must make timers obvious without becoming an event log, which phase 1 removed from the HUD.
4. **Dogpiling.** Several holdings of one commander can come out of Reinforced at the start of the same window. *Option:* spread their exit times across the window. This is open, part of Q20.
5. **Doctrine UX is new UI work.** Nothing in the phase-1 HUD covers writing doctrine except the PWR ROUTE toggle. Good presets matter more than a powerful editor.
6. **Predictable Deputy behaviour can be learned and exploited.** That is accepted, because only peripheral, bounded assets are exposed.
7. **The MVP has one human with irregular sessions.** A 4 h daily window and 12 h of notice are impractical to test in real time. All timings must be configuration values, with a time-compression setting per Universe for test runs.
8. **Vacation.** A player who is away for days is defended only by the Deputy, including during their sieges. *Option:* a limited vacation mode (Q23).

## 8. Proposed defaults (all configurable, to be tuned)

| Parameter | Value | Reason |
|---|---|---|
| Exposure window | 4 h per day, contiguous, chosen per commander | Attendable by the defender; plannable for attackers |
| Window change delay | 48 h | Prevents dodging an attack that is already known |
| Minimum notice from breach to siege | 12 h | Covers a night or a working day |
| Siege clock | 60 min | One play session; enough time for relief to arrive |
| Immunity after Held | 48 h | No permanent lockdown |
| Disconnect grace | 120 s | Network blips do not cancel orders |
| Disabled peripheral restore | 10 min after the last hostile leaves | Raids hurt, then heal |
| Deputy reaction / command rate | 300 ms / human-level cap | PR-5 |

## 9. Effect on other documents

- **Phase 1:** no change. The HUD will later need a window indicator, a Reinforced countdown, the siege clock, Deputy and doctrine status, and the away report. These are not phase-1 scope.
- **`01-game-concept.md`:** P-05 is withdrawn and P-01 is refined by PR-3 and PR-4.
- **`02-open-questions.md`:** this document gives proposed answers to Q6, Q7 and Q8, and raises Q19–Q23.
- **Assumption:** Q1 is answered with continuous real time. A strategic-tick model would still work with these rules, but the timings would change.
