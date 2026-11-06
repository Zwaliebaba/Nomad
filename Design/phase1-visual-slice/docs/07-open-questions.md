# 07 — Open questions (not blocking phase 1)

1. **Hotkey conflict**: Q/E rotate the camera, but Q is also "Shield surge" on the command card. Phase 1 uses Ctrl+key for command-card hotkeys; the final scheme (for example a grid layout on QWER/ASDF/ZXCV with rotation on mouse buttons 4/5) needs a decision.
2. **Ship altitude vs labels**: the mockup's hostile cruiser label still says "ELEV +26". The phase 1 rule is that all ships fly on z = 16, so the label shows range only. Update the mockup or revisit the rule.
3. **Mockup vs data**: the proposal artboard is the visual source; the voxel data was reconstructed from it. If the artboard changes, re-export the data rather than hand-editing JSON.
4. **Ships are placeholders**: the mockup ships were 2D-baked and could not be exported. Final ship art needs a 3D voxel pipeline (MagicaVoxel `.vox` import is a natural next step).
5. **Rotating minimap**: north-up is chosen; revisit after playtests if players prefer a view-aligned minimap.
6. **Elevation tiers for building placement**: phase 2 must define which tiers a building type may use and how trusses are auto-generated.
7. **Zoom-scale tabs** (System/Sector/Galaxy): visual only in phase 1; their views are a separate design.
8. **True cubes vs z scale 0.8165**: kept behind a config flag; decide after seeing both in motion.
9. **Top bar label (resolved, Design D-022)**: Kestrel-7 is a system. The fixture now reads `SYSTEM KESTREL-7 · OPS GRID 0`; `reference/renders/ops-grid-hud@2x.png` still shows `SECTOR KESTREL-7`. The spec wins (README ground rule 1).
