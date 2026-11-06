# Nomad — Design

This folder holds every design document for the game. Nothing outside `Design/` is a design source of truth.

**Status:** design and build run in parallel (D-051). All questions Q1–Q70 are answered (D-001 to D-076). Docs 10, 11, 12, 14, 15 and 16 are agreed. Remaining design: 13 (AI commanders), 17 (strategic map views), 20 (architecture), 30 (MVP scope and milestones). Being built: phase-1 M2 and the Universe structure in `nomad-sim`.

## Documents

| # | File | What it is | Status |
|---|------|------------|--------|
| 00 | [`00-glossary.md`](00-glossary.md) | Vocabulary and the Universe hierarchy. Read this first. | Draft (decisions applied) |
| 01 | [`01-game-concept.md`](01-game-concept.md) | Stated requirements, what they force, stress test, MVP framing | Draft (decisions applied) |
| 02 | [`02-open-questions.md`](02-open-questions.md) | Questions for the owner, with answers and a summary table | All answered |
| 03 | [`03-decision-log.md`](03-decision-log.md) | Decisions taken, with date and reason | Living |
| 04 | [`04-design-review.md`](04-design-review.md) | Ranked risk register for the agreed design (RK-01 to RK-11) | Decided |
| 10 | [`10-universe-structure.md`](10-universe-structure.md) | Hierarchy, scale, topology and travel, sites and holdings, system control, season start and late joiners, coordinate frames | Agreed |
| 11 | [`11-time-and-simulation.md`](11-time-and-simulation.md) | Clocks, timer layers, tick phases, determinism, journal, catch-up after downtime, persistence, crate boundary | Agreed |
| 14 | [`14-economy.md`](14-economy.md) | Resources, pooled stockpile with convoys, deposits, harvesting and refining, power, command cap, build slots and queues, costs, repair, start kit, pacing targets | Agreed |
| 16 | [`16-information-model.md`](16-information-model.md) | Information classes, sensors and detection, transit visibility, last-known memory, what the server sends, AI fairness via `CommanderView` | Agreed |
| 15 | [`15-combat-and-defence.md`](15-combat-and-defence.md) | Ships, orders, weapons and damage, dome and power, siege and capture mechanics, outposts, gates, raider limits, TTK targets, starting values | Agreed |
| 12 | [`12-offline-play-and-deputy.md`](12-offline-play-and-deputy.md) | Offline protection (stakes rule, shield lifecycle, absence states) and the Deputy (authority, doctrine, competence cap) | Agreed |
| — | [`phase1-visual-slice/`](phase1-visual-slice/README.md) | Owner's phase-1 package: client look, camera, HUD, renderer architecture, voxel assets | Agreed (owner) |

Planned documents (numbers reserved). Their inputs are now decided:

| # | Planned document | Inputs |
|---|------------------|--------|
| 13 | AI commanders: Strategist on top of the Deputy (P-02), schedules, difficulty | D-013, D-016, D-027 |
| 17 | Strategic map views: SYSTEM and SECTOR views, fleet travel commands (phase-1 doc 07, item 7) | D-022, D-026 |
| 20 | Technical architecture: server, client, protocol, persistence, crates | D-026, D-029, D-030 |
| 30 | MVP scope, milestones and acceptance criteria, including the `nomad-sim` track | D-026 to D-028, D-037, D-038 |

## Conventions

- **The total game space is the Universe** (D-008). Use *Universe* for the whole persistent game space in documents, player-facing text and domain types. *World* remains fine as a technical term, for example renderer world space or an ECS `World`.
- Engineering conventions are not design documents. They live at the repository root: `AGENTS.md`, `CONTRIBUTING.md`, `CODING_STANDARDS.md`.
- Every requirement, decision and question has a stable ID (`R-nn`, `D-nnn`, `Qnn`). Refer to the ID, not the wording.
- Each statement in a document is one of three things. **Stated** means the owner said it. **Derived** means it follows from stated requirements and leaves no real choice. **Proposed** means it is a recommendation that needs the owner's approval. Nothing proposed counts as decided until it is in `03-decision-log.md`.
- Phase-1 decisions in `phase1-visual-slice/docs/00-brief.md` stay in force unless a later decision overrides them.
