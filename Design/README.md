# Nomad — Design

This folder holds every design document for the game. Nothing outside `Design/` is a design source of truth.

**Status:** design phase, round 1. The owner's requirements are captured; most game-level design is still open and waiting on the questions in `02-open-questions.md`.

## Documents

| # | File | What it is | Status |
|---|------|------------|--------|
| 00 | [`00-glossary.md`](00-glossary.md) | Vocabulary and the Universe hierarchy. Read this first. | Draft |
| 01 | [`01-game-concept.md`](01-game-concept.md) | Stated requirements, what they force, stress test, MVP framing | Draft |
| 02 | [`02-open-questions.md`](02-open-questions.md) | Questions for the owner, each with its impact and a recommendation | Open |
| 03 | [`03-decision-log.md`](03-decision-log.md) | Decisions taken, with date and reason | Living |
| — | [`phase1-visual-slice/`](phase1-visual-slice/README.md) | Owner's phase-1 package: client look, camera, HUD, renderer architecture, voxel assets | Agreed (owner) |

Planned once the round-1 questions are answered (numbers reserved):

| # | Planned document | Depends on |
|---|------------------|------------|
| 10 | Universe structure: topology, scale, holdings, travel | Q1–Q4 |
| 11 | Time and simulation: clocks, tick, offline behaviour, time scales | Q1, Q2, Q7 |
| 12 | Deputy: authority, standing orders, defensive repertoire | Q6–Q9 |
| 13 | AI commanders | Q12, Q15 |
| 14 | Economy | Q1, Q12 |
| 15 | Combat and defence | Q5, Q7 |
| 20 | Technical architecture: server, client, persistence, networking | Q11, Q13, Q17 |
| 30 | MVP scope, milestones and acceptance criteria | Q11–Q15 |

## Conventions

- **The total game space is the Universe** (D-008). Use *Universe* for the whole persistent game space in documents, player-facing text and domain types. *World* remains fine as a technical term, for example renderer world space or an ECS `World`.
- Engineering conventions are not design documents. They live at the repository root: `AGENTS.md`, `CONTRIBUTING.md`, `CODING_STANDARDS.md`.
- Every requirement, decision and question has a stable ID (`R-nn`, `D-nnn`, `Qnn`). Refer to the ID, not the wording.
- Each statement in a document is one of three things. **Stated** means the owner said it. **Derived** means it follows from stated requirements and leaves no real choice. **Proposed** means it is a recommendation that needs the owner's approval. Nothing proposed counts as decided until it is in `03-decision-log.md`.
- Phase-1 decisions in `phase1-visual-slice/docs/00-brief.md` stay in force unless a later decision overrides them.
