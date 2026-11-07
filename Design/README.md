# Nomad — Design

This folder holds every design document for the game. Nothing outside `Design/` is a design source of truth.

**Status:** the MVP design is complete. All questions Q1–Q267 are answered (D-001 to D-278). Round 18 (doc 21) agreed the browser as the player's client, the server in Docker Desktop and the MP-1 milestone (D-235 to D-237), and its questions became D-238 to D-251: a Rust/wasm client with a Leptos HTML UI, one WebSocket, `axum` serving the client, a Host and Origin guard, a compose file, and MP-1 after δ. Round 19 decided the β client screens, the view-model contract for C7 and pylons outside the dome, and placed S9 and K1 (D-255 to D-278; `plans/U-beta-ui.md`, `plans/C7-view-model-inventory.md`). Building follows `30-mvp-scope-and-milestones.md`: three parallel streams toward milestones α–δ, with MVP acceptance at γ (D-094 to D-097), in the order of doc 30 §4 as D-205 amended it. Built so far (doc 30 §2): S1–S5, S6a and S8 in the simulation; N1–N3 on the server side; phase-1 M1–M4 in the client. Next towards α: S6b, S7 and A1a, then A1b, A2 and A3.

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
| 12 | [`12-offline-play-and-deputy.md`](12-offline-play-and-deputy.md) | Offline protection (stakes rule, shield lifecycle, absence states) and the Deputy (authority, doctrine, competence cap) | Agreed |
| 13 | [`13-ai-commanders.md`](13-ai-commanders.md) | AI stack (Deputy, Strategist, raider director), Deputy rules, Strategist goals, AI schedules, difficulty, doctrine presets, AI acceptance criteria | Agreed |
| 14 | [`14-economy.md`](14-economy.md) | Resources, pooled stockpile with convoys, deposits, harvesting and refining, power, command cap, build slots and queues, costs, repair, start kit, pacing targets | Agreed |
| 15 | [`15-combat-and-defence.md`](15-combat-and-defence.md) | Ships, orders, weapons and damage, dome and power, siege and capture mechanics, outposts, gates, raider limits, TTK targets, starting values | Agreed |
| 16 | [`16-information-model.md`](16-information-model.md) | Information classes, sensors and detection, transit visibility, last-known memory, what the server sends, AI fairness via `CommanderView` | Agreed |
| 17 | [`17-strategic-views-and-ui.md`](17-strategic-views-and-ui.md) | SYSTEM, SECTOR and GALAXY views, orders in strategic views, UI beyond phase 1, hotkey scheme | Agreed |
| 20 | [`20-technical-architecture.md`](20-technical-architecture.md) | Processes, crates, threads, protocol, replication, persistence, lifecycle, client architecture, security, testing | Agreed; §2, §5, §8, §9 changing with doc 21 |
| 21 | [`21-browser-client-and-local-container.md`](21-browser-client-and-local-container.md) | The browser as the player's client, the server in Docker Desktop, WebSocket transport, security without a login, the HTML UI, and what MP-1 needs kept open | Partly agreed (D-235 to D-237); Q230–Q243 open |
| 30 | [`30-mvp-scope-and-milestones.md`](30-mvp-scope-and-milestones.md) | MVP definition, work packages, milestones α–δ, order of work, acceptance, out of scope | Agreed |
| — | [`phase1-visual-slice/`](phase1-visual-slice/README.md) | Owner's phase-1 package: client look, camera, HUD, renderer architecture, voxel assets | Agreed (owner) |

Every document the MVP needs now exists (see `30-mvp-scope-and-milestones.md` for the build plan).

## Work-package plans

Implementation plans for packages from `30-mvp-scope-and-milestones.md` §2. Each plan is reviewed before its package starts, and it doubles as the brief for whoever builds it.

| Package | Plan | Status |
|---|---|---|
| S3 | [`plans/S3-movement-and-orders.md`](plans/S3-movement-and-orders.md) | Built (D-101 to D-105; gaps decided as D-112 to D-115) |
| N2 | [`plans/N2-server-shell.md`](plans/N2-server-shell.md) | Built (D-106, D-107, D-109, D-110; season end D-116, sleep catch-up D-117) |
| N3 | [`plans/N3-transport-and-sessions.md`](plans/N3-transport-and-sessions.md) | Built (D-120 to D-125, D-150, D-151) |
| S4 | [`plans/S4-economy.md`](plans/S4-economy.md) | Built (D-126 to D-138; core-kit price D-189 and diminishing yield D-217, both built) |
| S5 | [`plans/S5-combat.md`](plans/S5-combat.md) | Built, S5a and S5b (D-139 to D-149; S5a details D-188; S5b readings D-227; slot geometry D-228 follows with S6b) |
| S7 | [`plans/S7-sensors.md`](plans/S7-sensors.md) | Built (D-173 to D-181) |
| N4 | [`plans/N4-replication.md`](plans/N4-replication.md) | Planned and decided (D-182 to D-187) |
| S6 | [`plans/S6-offline-and-sieges.md`](plans/S6-offline-and-sieges.md) | Built (D-154 to D-172; S6b with D-224, D-228, D-229) |
| S8 | [`plans/S8-season-end.md`](plans/S8-season-end.md) | Built (D-191 to D-198) |
| A1 | [`plans/A1-deputy.md`](plans/A1-deputy.md) | Planned and decided (D-199 to D-211) (also the shared AI architecture for A1–A3) |
| A2 | [`plans/A2-strategist-economy.md`](plans/A2-strategist-economy.md) | Planned and decided (D-212 to D-218) |
| A3 | [`plans/A3-raider-director.md`](plans/A3-raider-director.md) | Planned and decided (D-219 to D-225) (also the α acceptance runs) |
| C7 | [`plans/C7-view-model-inventory.md`](plans/C7-view-model-inventory.md) | Inventory decided (D-264 to D-274); amends N4 §4.5 |
| W2, M6, U1–U3 | [`plans/U-beta-ui.md`](plans/U-beta-ui.md) | β screens decided (D-256 to D-263, D-275, D-276) |

## Conventions

- **The total game space is the Universe** (D-008). Use *Universe* for the whole persistent game space in documents, player-facing text and domain types. *World* remains fine as a technical term, for example renderer world space or an ECS `World`.
- Engineering conventions are not design documents. They live at the repository root: `AGENTS.md`, `CONTRIBUTING.md`, `CODING_STANDARDS.md`.
- Every requirement, decision and question has a stable ID (`R-nn`, `D-nnn`, `Qnn`). Refer to the ID, not the wording.
- Each statement in a document is one of three things. **Stated** means the owner said it. **Derived** means it follows from stated requirements and leaves no real choice. **Proposed** means it is a recommendation that needs the owner's approval. Nothing proposed counts as decided until it is in `03-decision-log.md`.
- Phase-1 decisions in `phase1-visual-slice/docs/00-brief.md` stay in force unless a later decision overrides them.
