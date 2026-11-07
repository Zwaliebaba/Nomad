# β client UI: screens, layout and decisions

Status: **Agreed (round 19).** The owner reviewed the β boards on his Design canvas ("Nomad · Bravo client UI") and decided Q256–Q265 as D-256 to D-263, D-275 and D-276; the data each board needs is in `C7-view-model-inventory.md` (Q244–Q254, D-264 to D-274). This plan is the written record of the boards, so the canvas is not the only place they exist, and the brief for W2, M6, U1–U3, U7 and β's start screen. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

*Derived* from doc 30 §3 (β), doc 17 and doc 21 §7.

The β screens: the TACTICAL HUD (M6) at the 1920×1080 reference and in its compact layout; SYSTEM and SECTOR (U1, U2); the season view on F4 (D-261); the holding drawer with build and economy (U3); the start and resume screens (D-263). The doctrine editor (U4), the away report and the window panel (U5) and the rest of U6 stay in γ.

## 2. Layers

*Agreed* (D-238 to D-240, D-260); the pointer rule is *Derived*.

| Layer | Contents | Technology |
|---|---|---|
| L4 | Alert banner and toasts; browser notifications in the background (D-249) | HTML, Leptos |
| L3 | Holding drawer (build, economy, blueprints), menu, jump confirmation; they dim the scene and leave it running | HTML, Leptos |
| L2 | HUD panels, anchored to the corners | HTML, Leptos |
| L1 | SYSTEM, SECTOR and the F4 season view (D-276); opaque, and the canvas stops rendering while one is open (D-260) | HTML, SVG |
| L0 | The scene: voxels, grid, shield, effects, world labels, markers, selection brackets and order lines (D-240) | wgpu, wasm |

HTML layers take pointer events only on their own panels, so clicks in the gaps reach the scene. The view-model store (C7) feeds the scene every frame and Leptos signals on change.

## 3. TACTICAL HUD (M6)

*Agreed* where a decision is named; panel positions follow phase-1 doc 04 and `hud_layout.json`.

- **Top bar:** the holding and its system; season day and local time; alloy and crystal with cap, income and "full in" (D-269); power of the selected holding; command; the Deputy chip (preset and fleets under direct control, D-013, D-257); the alert banner (D-271); the exposure-window chip; server status with the clock check (D-257); MENU.
- **System panel (top left):** breadcrumb, the F1–F4 tabs, the mini system map, lane traffic, the control bar.
- **Camera widget (top centre):** rotation on Alt+Q and Alt+E (D-093), the view compass, and the ELEV REF toggle (D-258).
- **Fleets (top right):** fleets with label (D-267), composition and state; hostile tracks with observable state only (moving, stationary, firing; orders are private, D-075); last-known tracks faded with their age (D-076).
- **Holding panel (right, D-256):** the exposure window and where the day is in it, the four sectors, Reinforced and siege timers, the Deputy's deferred decisions (with A1).
- **Quick chips:** IDLE and LAST ALERT. PING is gone: there are no allies in the MVP.
- **Minimap (bottom right):** the iso radar with pulse (phase-1 doc 04).
- **Selection (bottom left)** and **command card (bottom centre):** the positional cards of D-258; an unaffordable item stays enabled and shows its shortfall (D-259).

**Compact layout** (*Agreed*, D-275): below about 1600 px wide the top bar condenses, the system panel folds to its tabs, the fleet, hostile and holding sections fold to one line each, the minimap shrinks to about 200 px, and selection and card share one bottom dock. Text stays at 11 px or more and buttons at 44 px.

## 4. Strategic views

*Agreed* (D-090, D-091, D-260, D-261); contents follow doc 17 §4–§5.

- **SYSTEM (F2):** star, orbits and belt as 2:1 ellipses; sites by owner (squares for yours, diamonds for rivals, circles for free) with deposits and richness; gates with lane traffic; own, sensed and last-known fleets; overlays for sensor coverage, defended zones (with A1's doctrine) and convoy routes. Rival exposure windows are public (D-075) and may be shown. The selected fleet's order preview shows its target and an estimated arrival, marked as an estimate.
- **SECTOR (F3):** the nine systems as iso discs by control state, lanes with transit times, fleets in transit, observed departures, breach and siege markers for your own holdings (D-254). A jump shows its route from the shared route search (D-270), each leg's arrival and contested gates (D-055), and is confirmed with Enter or a second right-click; right-click on empty space or Backspace cancels (D-262).
- **F4:** the season view until δ (D-261): standings, early-win progress, standings history, near-win warning.

## 5. Holding drawer (U3)

*Agreed* where named; the layout is from the boards.

- **Build:** the slot diagram of the holding (pylon, inner, dock and peripheral slots of the template, D-130, D-131, D-255), the catalogue filtered by the selected slot with cost, time, power and hull from the rules (D-265), the queue per lane with progress and completion (D-230), reordering (D-273) and cancelling with the refund shown (D-068), and blueprints (save, apply).
- **Economy:** pool and cap with "full in" (D-269); cap breakdown; command by class; the diminishing yield (D-217); income per holding (D-268); buffers and convoys with their routes; power per holding with its profile.

## 6. Start and resume (D-263)

*Agreed.* Served by the server over D-272's API before any session exists:

- **Start:** server status, the Universes in the volume with their state (running, shut down, ended), resume with the catch-up it needs, ended standings read-only, and a create form (name, seed, AI count fixed at 3 for stage 1, one difficulty for all AI until A5, season length, window start).
- **Resume:** catch-up progress from `CatchUp` ticks with an estimate, the note that the Deputy is in charge and commands wait until live, Play (enters fullscreen, D-248) enabled when live, and Shut down. No event counts during catch-up; the away report is U5's.

## 7. Done when

*Proposed:* each screen is built in W2 against fixtures first, then on C7's live store; M6 meets phase-1 doc 06's ±4 px at 1920×1080 in a browser screenshot test (D-240, D-250); the compact layout is checked at 1280×720; every binding of §3–§4 passes U7's browser tests.
