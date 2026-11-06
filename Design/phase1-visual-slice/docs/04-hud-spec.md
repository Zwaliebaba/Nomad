# 04 — HUD specification

Target image: `reference/renders/ops-grid-hud@2x.png` (rendered at 2× — halve all measurements you take from it). Rectangles and colours: `assets/layout/hud_layout.json`. The mockup source is `reference/mockup/ops-grid-proposal.html` — open it in a browser and use dev tools to read exact CSS where this spec is silent.

## Common style

- Panel: background `rgba(7,14,22,0.82)`, 1 px border `rgba(56,214,255,0.32)`, 10×10 px L-corner accents (2 px, `#38d6ff`) at top-left and bottom-right.
- Header row: Rajdhani 700, 13 px, letter-spacing 2.5 px, uppercase, accent colour; right side 11 px JetBrains Mono in `#86a3b6`; 1 px divider below.
- Body text: JetBrains Mono 12 px `#cfe3ee`; labels 11 px `#86a3b6` letter-spacing 1 px.
- Bars: 6 px tall, track `rgba(134,163,182,0.18)`.
- Buttons/chips: 1 px accent border at 40–45% alpha, hotkey badge in a 1 px bordered box, accent colour.
- Every interactive element needs hover and pressed states (lighter fill +6% alpha, border to full accent).

## Panels

1. **Top bar** (0,0,1920,52): holding name (Rajdhani 17 px, 3 px spacing) + sector line; separators; mission clock; ALLOY/CRYSTAL with income in green; POWER used/cap with load%; COMMAND used/cap; flexible spacer; amber **alert banner** (warning icon, uppercase, 1 px `#ffc247` border, 10% fill); NETWORK commanders + ping; MENU chip with F10.
2. **System panel** (16,66,468,~250): header "System · Kestrel-7" + state; breadcrumb (current system bright, holding in accent); zoom tabs TACTICAL/SYSTEM/SECTOR/GALAXY (F1–F4, TACTICAL active); left a 214×146 **iso system map** (star with glow, 4 orbits + dotted belt as 2:1 ellipses, holding as a cyan square with a halo box on orbit III, two gates as small ellipses, dashed lanes, relief fleet as a green square, hostile wave as an orange diamond); right "Lane traffic" (3 one-line rows: label coloured by side, detail in label grey, ETA right-aligned) and "System control" (3-part bar you/hostile/free + counts).
3. **Camera widget** (top centre, 836,62): rotate-left chip (Q), compass box (diamond, N needle in amber, "VIEW NE · 1/4"), rotate-right chip (E).
4. **Fleet roster** (1676,66,228): rows with a numbered cyan badge, group name + composition, state tag (PATROL cyan, ENGAGED orange, HAULING green, DOCKED amber); "Hostile tracks" list in orange.
5. **Objectives + Commanders** (1676,384,228): objective rows with a square checkbox (amber = active timer, grey = optional), title, sub-label, right value; then a Commanders section with ally names in green and their status.
6. **Quick chips** (1596,694): `IDLE 1 (.)` amber, `LAST ALERT (Space)` orange, `PING (G)`.
7. **Minimap / Tactical sensor** (1596,732,308,332): see below.
8. **Selection** (16,852,500,212): left column HULL bar, SHIELD GRID bar split into four N/E/S/W segments (weak segment orange), POWER IN, DOCKED, TURRETS; divider; right column BUILD QUEUE with progress bars.
9. **Command card** (532,852,540,212): 4×3 grid of 44 px buttons — label (Rajdhani 12 px bold), sub-line (9.5 px grey: cost or state), hotkey badge top-right. Cooldown: dark overlay over the lower 62% and amber timer. Active toggle: green border and fill.

## Minimap — iso radar with pulse

Drawn in a 300×300 box centred at (150,150) inside the panel.

- Bezel: circle r = 138, accent alpha 0.5.
- Range rings: **2:1 ellipses** rx 46/92/138 (ry = rx/2), alpha 0.2–0.3. Axes: the two iso diagonals (slope ±0.5) through the centre, alpha 0.15.
- Ops-area diamond: `M67.5,150 L150,108.8 L232.5,150 L150,191.2 Z`, faint fill.
- Contacts: friendly 6×6 squares cyan, friendly fighters 2 px dots, hostiles 3.5 px diamonds orange, cruiser 6 px diamond; ally marker green square + name.
- Camera frame: white 1.2 px rectangle (axis-aligned because the minimap is drawn in the same projection as view 0); when the view rotates, the minimap stays north-up and the frame becomes the rotated footprint of the screen.
- North line: dashed amber line from the centre to the bezel top with an "N".
- **Iso pulse** (replaces the rotating sweep): three ellipses with rx 138, ry 69, each animated with scale 0.03 → 1.0 over **3.6 s**, linear; opacity 0.95 → 0.35 at 70% → 0; staggered by 1.2 s so three rings are always visible. Fill: radial gradient that is transparent inside and brightens to a thin rim (`#7fe6ff` 28% at 97%). Stroke 1.6 px constant width (do not scale the stroke with the ring).
- Optional: contacts brighten for 300 ms when a pulse ring passes them.
- Hostile alert ping: two concentric orange 2:1 ellipses on the cruiser.

## World layer (part of the HUD toggle)

Labels for SOL-1, FA-1, MS-01, SY-2, FB-2, AST-1, HSTL-C1; pylon tags (`PYL-N 100%` etc., orange when < 50%); selection brackets around the selected station; lock reticle on HSTL-K7 with text; station-keeping zone ellipse around the hostile group (orange dashed, 5% fill, "STN-KEEP ZONE"); ship stalks; friendly/hostile ship markers (small brackets / diamonds) above each ship.
