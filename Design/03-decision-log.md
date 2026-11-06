# 03 — Decision log

Newest entries at the bottom. A decision is changed by adding a new entry that supersedes the old one, never by editing the old one. Status is one of **Agreed** (the owner has decided), **Superseded**, or **Provisional** (assumed so work can continue, and confirmation is pending).

| ID | Date | Decision | Reason | Status |
|---|---|---|---|---|
| D-001 | 2026-11-06 | The persistent game space is the **Universe**. *World* is not used in documents, player-facing text or our code identifiers. In the imported phase-1 package: holding-local coordinates became **grid coordinates** and the label layer became the **scene layer**. JSON keys were renamed (`world_labels`→`scene_labels`, `pivot_world`→`pivot_grid`, `screen_*_per_world_*`→`screen_*_per_grid_*`, `world_axes`→`grid_axes`), and format versions stay at `@1` because nothing consumes them yet. | Owner instruction. The scope for code identifiers is still open in Q16. | Agreed (code scope Provisional) |
| D-002 | 2026-11-06 | All design documents live in `Design/`. The owner's phase-1 package is imported as `Design/phase1-visual-slice/`, with docs, assets and reference renders kept together. | Owner instruction. Keeping the package intact preserves its internal references. | Agreed |
| D-003 | 2026-11-06 | Every decision in `phase1-visual-slice/docs/00-brief.md` → "Decisions already taken" stays in force: sectored shield, solar array outside the dome, core spindle, elevation tiers and a single ship plane, fixed-pitch camera with 4 snap angles, emissive budget, HUD layout, north-up minimap. | Owner's phase-1 package. | Agreed |
| D-004 | 2026-11-06 | Implementation language is Rust. The client renderer is custom `wgpu` + `winit`. | R-01; phase-1 doc 05. | Agreed |
| D-005 | 2026-11-06 | The Universe targets up to 100 players. | R-04. What "player" means here is Q3. | Agreed |
| D-006 | 2026-11-06 | The MVP is playable by one human with up to 9 AI players. | R-05. Scope is Q11–Q15. | Agreed |
| D-007 | 2026-11-06 | A Deputy takes over a disconnected player's empire. It continues standard actions and defends assets. | R-03. Its authority is Q6. | Agreed |
