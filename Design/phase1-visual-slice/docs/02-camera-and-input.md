# 02 — Camera and input

## Camera model

- Orthographic, pitch fixed at 30°, z scale 0.8165 (see 01 §1).
- **Yaw**: four snap angles 45°, 135°, 225°, 315° (views 0–3). View 0 = the reference renders ("VIEW NE · 1/4").
- Rotation pivots around the point on `z = 0` at the screen centre (initially the deck centre `(-8.5, 9.5, 0)`).
- Transition: 250 ms ease-in-out on yaw. During the turn, render normally (the voxel look survives intermediate angles for a quarter second); input is queued, not dropped.
- Lighting is camera-relative: after a turn, the newly visible faces receive the same top/left/right factors.
- Zoom: discrete steps `0.5, 0.75, 1.0, 1.5, 2.0`; mouse wheel, 150 ms ease, zoom toward the cursor. Keep pixel snapping at integer zoom levels to keep voxel edges crisp.
- Pan: WASD / arrow keys, edge scroll (16 px margin), middle-mouse drag. Clamp to sector bounds (phase 1: ±200 voxels around the pivot).

## Key bindings (phase 1)

| Key | Action |
|---|---|
| Q / E | Rotate view −90° / +90° |
| Mouse wheel | Zoom step |
| WASD, arrows, edge, MMB drag | Pan |
| Home | Reset to view 0, zoom 1, pivot on deck centre |
| H | Toggle HUD (world labels + panels) — matches the mockup's `showHud` switch |
| F1–F4 | Zoom-scale tabs (only TACTICAL active in phase 1; others highlight but do nothing) |
| F10 | Menu button highlight (no menu yet) |
| F11 | Toggle fullscreen |
| F12 | Screenshot to `screenshots/` (used by acceptance tests) |

Command-card hotkeys (R, L, Y, Z, F, V, M, B, Q, W, P, S) only flash the matching button in phase 1. **Conflict to note**: Q is both "rotate" and "Shield surge". Resolution for phase 1: rotation uses Q/E; the command card uses Ctrl+key while the ops grid has focus. Final binding is an open question (07).

## HUD reactions to the camera

- Camera widget shows the view index and compass; its N needle rotates with the yaw.
- Minimap stays north-up; its camera-frame rectangle rotates and moves with the view.
- World labels keep their screen offsets; leader lines re-anchor to the object's projected position each frame.
