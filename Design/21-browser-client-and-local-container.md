# 21 — Browser client and the local container

Status: **Agreed (round 18).** The owner's direction is D-235 to D-237; the questions Q230–Q243 (§11) were decided as D-238 to D-251, all as recommended except Q243 (MP-1 comes after δ, D-251). Statements below marked *Proposed* with a question number are agreed through that question's decision. Builds on docs 17 and 20, the N3 and N4 plans, and phase-1 doc 05. Labels: *Stated* means the owner said it, *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document changes how the MVP is delivered to the player, not how the game plays:

- the browser becomes the player's client (D-235);
- the server runs in Docker Desktop on the player's PC (D-236);
- a multiplayer milestone, MP-1, follows MVP acceptance (D-237).

It covers the process model, the transport, security without a login, porting the client, the HTML UI, build and tests, and what MP-1 needs this design to keep open. MP-1 itself (hosting, accounts, more than one human) gets its own plan when it is next.

## 2. What the owner stated

*Stated* (2026-11-07, while weighing a web-server game with an HTML5 front end):

- **Goals:** "multiplayer sooner and easy use by players".
- **Audience now:** "no internet and one player, so no login required".
- **Where the server runs:** "Docker Desktop".
- **Order:** single player first. γ stays MVP acceptance; multiplayer comes after it.
- **First multiplayer season:** up to 4 humans, which fits the stage-1 sector (doc 10 §4.4).
- **Clients:** the browser is the only client for players. The native build stays only for development (PIX, the GPU tests).
- **Milestone:** multiplayer, with up to 4 humans on a hosted server, is named in doc 30 as a milestone after γ.

These are recorded as **D-235** (the client), **D-236** (the process model and the audience) and **D-237** (the milestones).

*Derived:* the two goals and the near-term choices pull against each other. Multiplayer is not sooner in milestone terms, because MP-1 still follows γ. And Docker Desktop is not easy use for anyone but the owner (§12 item 1). What the choices buy is a client and transport that are ready for the web from the start, so MP-1 becomes hosting and identity work, not a client rewrite.

## 3. What does not change

*Derived.* The game is unaffected:

- **The simulation and the AI:** `nomad-sim`, the rules file, every S and A package, determinism (D-049) and the harness. α is unaffected.
- **The messages:** `nomad-protocol`'s message enums, framing and version rule (D-087's encoding part), the session rules built in N3 (handshake, `client_seq`, rate limits, replacement, D-124), and N4's replication (D-182 to D-187).
- **The server:** persistence, the journal, recovery and catch-up (D-047, D-117), and the season end (D-116, D-234).
- **The look:** phase-1 docs 01–04 and the wgpu renderer of M1–M4.
- **γ:** its acceptance (D-097), including the 14-day playtest. The playtest now runs in the browser against the container.

## 4. Process model: the server in Docker Desktop

### 4.1 Compose and restart

*Proposed (Q237).* A `compose.yaml` in the repository runs the `nomad-server` image with:

- `restart: unless-stopped`, so the server keeps running when the browser closes (D-026) and comes back after a crash or a Docker restart;
- the port published on the host's loopback only: `127.0.0.1:31790:31790`, now TCP (§5.1). The container binds 0.0.0.0 inside, as before (D-150);
- the Universe data in a named volume (§4.2).

Docker Desktop's start-at-sign-in setting, with the restart policy, replaces "start with Windows" (D-085). It stays opt-in, as D-085 asked, but it now lives in Docker Desktop's settings, not the game's.

Installing for the player means: install Docker Desktop, run `docker compose up -d`, and open `http://localhost:31790`. The README covers this (doc 30 §5 item 5).

### 4.2 The Universe's data

*Derived.* SQLite's write-ahead log, which D-088 uses, needs a local file system with working locks and shared memory. SQLite documents that WAL does not work over network file systems. Docker Desktop shares Windows folders with the Linux VM through a file-sharing layer. So:

- the Universe lives in a **named Docker volume**, which sits on the Linux VM's own disk, never in a bind mount of a Windows folder;
- backups are a documented export of that volume, made while the server is stopped or from a snapshot (doc 20 §6).

### 4.3 Start screen, create, continue, shut down

*Proposed (Q237).* A browser cannot start a process, and with `unless-stopped` a server that exits is restarted. The server process therefore always runs, and it owns the start screen (doc 17 §6):

- **With no Universe running** it serves the start screen. Create a Universe (seed, number of AI commanders, difficulty, D-231) or continue one from the volume. Today's `nomad-server create` arguments become a form.
- **"Shut down the Universe"** stops the simulation inside the process: a final snapshot, every session closed, then the start screen with *Resume*. Resuming catches up exactly as a restart does (D-047). The Universe does not advance while it is shut down, the same as when the PC is off, so this is no pause button.
- **The season end** (D-116) shows the final standings read-only. A new season is a new Universe from the start screen (D-020).
- **`docker compose stop`** still works. The server writes its final snapshot on SIGTERM (doc 20 §7).

### 4.4 The clock after sleep

*Derived risk.* Docker Desktop runs Linux in a WSL2 VM, and WSL2 has a history of its clock lagging behind the host's after the host sleeps or hibernates. Inside the container, D-117 compares against that clock:

- **While the VM's clock lags,** the Universe runs behind real UTC. Deadlines the client shows in local time (siege notices, windows; D-015) are then off by the lag.
- **When the clock jumps forward,** D-117 sees the gap and catches up, as designed.

*Proposed* (part of Q237):
- **Detection:** the client compares its own clock, which is the host's, with the tick-to-UTC mapping in `Welcome` and the latest view. It shows a warning when they differ by more than 5 s. This needs no protocol change.
- **Test:** β's 3-day playtest includes sleeping the PC, and checks the server's time against the host's.

## 5. Transport and serving

### 5.1 WebSocket

*Derived:* a browser cannot open a raw QUIC connection, so `quinn` (D-087) cannot reach a browser. The options are WebSocket over TCP, or WebTransport over HTTP/3.

*Proposed (Q233):* **one WebSocket per session**, at `ws://localhost:31790/ws`:

- **Streams:** each binary message carries one N1 frame (`u32` length plus `postcard` body), behind a one-byte tag for N3's control or state stream (§4.3 of the N3 plan). The frame decoder, size limits, handshake, rate limits and session rules stay as built.
- **Ordering:** TCP orders everything, so the two streams share one order, as they would on a single QUIC connection with head-of-line blocking. On loopback this does not matter (doc 20 §5).
- **Keep-alive:** the protocol's `Ping`/`Pong` stays, for RTT and liveness. WebSocket pings are not used.
- **The native development build** uses the same WebSocket, so the server has one endpoint.

*Why not WebTransport now:* it needs HTTP/3 and a certificate. A self-signed one must be pinned by hash and be valid for at most 14 days, so it would need rotating, all for no gain on loopback. Revisit at MP-1, where UDP being blocked on many networks also argues for WebSocket.

### 5.2 One port for the page and the socket

*Proposed (Q235):*

- **One port:** `nomad-server` serves the web client's files and the WebSocket on the same port, through an HTTP server (`axum`, MIT, with its WebSocket support and `tower-http` for static files; justified when it lands).
- **Built together:** the image's build stage builds the client for `wasm32-unknown-unknown` and copies the files in, so the server and the client always come from one commit.

*Derived consequence:* this makes D-087's rule that client and server ship together hold by construction. A browser holding a stale page still fails `Hello` with `Bye { VersionMismatch }`, and the page then reloads. File names carry the build ID (N2 plan §4.6), and `index.html` is served with `no-cache`.

### 5.3 The QUIC endpoint

*Proposed (Q234):* retire N3's `quinn` endpoint once the WebSocket endpoint passes the integration test. What goes:
- `quinn`, `rustls` and `rcgen`;
- the persistent certificate (D-151's certificate part);
- the `nomad://` import string.

What stays:
- `ring`, for tokens;
- `server.json`, which still gives the address and token to a local process;
- `connect-info` and `rotate-token`, for the token.

This is the one piece of built work this document makes obsolete.

## 6. Security without a login

*Stated:* no login in the MVP. *Derived:* a server on loopback still has two reachable attack paths from web pages:

1. **Cross-site WebSocket hijacking.** Any page open in the same browser can open a WebSocket to `localhost`. WebSockets are not covered by the browser's cross-origin rules (CORS).
2. **DNS rebinding.** A hostile domain that resolves to 127.0.0.1 makes its page same-origin with a server on loopback, as far as the browser is concerned.

*Proposed (Q236):*

- **Host check.** Every HTTP request and WebSocket upgrade must carry a `Host` of `localhost`, `127.0.0.1` or `[::1]` with the published port. Anything else is refused. This stops DNS rebinding. The published port is configured (`NOMAD_PUBLIC_PORT`, set by the compose file), because the host side of `-p` may differ from 31790.
- **Origin check.** Browsers always send `Origin` on a WebSocket upgrade. An upgrade with an `Origin` must match the same allowlist (`http://` scheme). This stops hijacking.
- **No token for browser sessions.** A session whose upgrade passed the Origin check skips `Auth`'s token check. A session without an `Origin` (the native build, tests) must still present D-151's token.
- **No CORS headers**, and a strict content security policy on the page (scripts and wasm from the same origin only).
- **The trust boundary is the player's Windows account.** Anything running as that user can already read the Docker volume, so local processes are trusted, as the token in `server.json` already assumes (doc 20 §9).
- **At MP-1** the same `Auth` slot carries a ticket from a login service (§10).

## 7. The client

### 7.1 Technology

*Proposed (Q230):* the 3D scene stays the Rust `wgpu` + `winit` client of M1–M4, compiled to WebAssembly. D-004 stands. The UI is HTML (§7.2).

- **GPU APIs:** wgpu runs on WebGPU where the browser offers it, and on WebGL2 otherwise.
- **Why this is cheap:** the client already asks only for WebGL2-level limits (`crates/nomad-client/src/render/context.rs:99`), and keeps its palette in a texture for that reason (phase-1 doc 07 item 17).

*Alternatives:* a TypeScript rewrite (three.js or Babylon.js), which throws away M1–M4 and the shared protocol types and supersedes D-004; or the whole UI in wgpu in the browser too, which keeps phase-1 doc 05 but leaves doc 30 §7's UI-volume risk where it is.

### 7.2 The HTML UI layer

*Derived:* most of doc 17 §6 is forms and lists: the doctrine editor, the build menu and queue, the economy, fleet and season panels, the away report and the start screen. Those are cheap in HTML and expensive in a custom wgpu text renderer. Phase-1 ground rule 3 (data comes from the view-model, never from literals) maps directly onto binding HTML elements to the view-model.

*Proposed (Q231):* the view-model store (C7) is Rust, shared by the browser and the native build. The HTML UI is written in Rust with Leptos (MIT), compiled into the same wasm module, so it reads the view-model's own types. That avoids a serialisation bridge, a second language, and an npm supply chain that `cargo deny` cannot check.

*Proposed (Q232):* the phase-1 HUD (M6) is built in HTML over the canvas, against `hud_layout.json` and the ±4 px acceptance of phase-1 doc 06. World labels and markers stay in wgpu (phase-1 doc 05 frame order, step 9), because they follow the camera every frame. This supersedes phase-1 doc 05's "the game HUD is custom-drawn" for the HUD panels. The golden images then cover the canvas only, and a browser screenshot test covers the HUD (§8).

### 7.3 What the port changes

*Derived* from the code at the time of writing:

| Today | In the browser |
|---|---|
| Fonts, voxels and configs read with `std::fs` (`assets/fonts.rs`, `assets/voxel.rs`, `config/*.rs`) | Fetched over HTTP at startup, behind one asset-source interface the native build implements with files |
| `pollster::block_on` for the adapter and device (`render/context.rs:77`, `:94`) | Async startup; the browser's main thread cannot block |
| `std::time::Instant` (`app.rs:5`, `:80`, `:243`), which panics on `wasm32-unknown-unknown` | `web-time` (MIT/Apache-2.0), a drop-in that winit already uses |
| The screenshot thread (`app.rs:299`); no threads on the web without cross-origin isolation | A screenshot is a file download |
| `tracing-subscriber` to standard output | `tracing` to the browser console |
| Command-line options (`cli.rs`) | URL query parameters for the development options |
| Borderless fullscreen at start (D-108) | Fullscreen needs a click or key press first, so it starts on *Play* (Q240) |
| `nomad-link`'s `quinn` client (D-125) | A browser WebSocket behind C7's link interface; the native build uses `tokio-tungstenite` (§5.1) |
| wgpu built with DX12, Vulkan and GLES | Plus the `webgpu` and `webgl` features for the wasm target |

`unsafe` stays denied. winit, wgpu and `web-sys` offer safe APIs, and the wasm entry point is `main`. If a `#[wasm_bindgen]` export turns out to trip `unsafe_code`, that needs the owner's agreement under `AGENTS.md`.

### 7.4 Browsers

*Proposed (Q239):*
- **Reference browsers:** Edge and Chrome on Windows, the platforms the owner plays on.
- **Firefox:** best effort.
- **Rendering:** WebGPU first, with WebGL2 as the fallback and both tested.
- **Desktop only.** Touch controls for an isometric RTS would be a separate UI design.
- **Minimum viewport:** 1280×720, scaled from the 1920×1080 reference (phase-1 ground rule 2).

### 7.5 Keys and fullscreen

*Derived:* a browser reserves keys that doc 17 §7 uses. Among them are Ctrl+1–9 (switch tabs), mouse buttons 4 and 5 (back and forward), F11 (the browser's fullscreen), F12 (developer tools), and Esc, which always leaves fullscreen.

*Proposed (Q240):* keep doc 17 §7 except for keys a browser reserves. U7 tests every binding on Edge, Chrome and Firefox, in a window and in fullscreen, and proposes a replacement for each binding that cannot be captured reliably. Fullscreen uses the Fullscreen API from the game's own control, after the *Play* click. This supersedes D-108's fullscreen at start for the browser.

### 7.6 Fonts

*Derived:* phase-1 ground rule 4 keeps JetBrains Mono and Rajdhani out of the repository, and the mockup loads them from Google Fonts. With "no internet" (D-236) the page cannot rely on Google Fonts. *Proposed (Q238):* the image's build stage downloads both fonts and serves them, with their OFL licence files. The OFL allows that. The repository still does not commit them, which keeps ground rule 4 for the source tree.

### 7.7 Notifications

*Proposed (Q241):* the browser's Notification API replaces the Windows toast of doc 17 §6. Localhost counts as a secure context, so the API works without TLS.
- **Coverage:** notifications arrive while a game tab is open, even in the background. That matches the native client's "running but minimised".
- **Permission:** asked the first time the player turns alerts on, never at page load.
- **Away from the PC:** notifications that reach the player outside the browser remain hosting work (doc 30 §6, MP-1).

## 8. Build, CI and tests

*Proposed (Q242):*

- **CI:** add a `wasm32-unknown-unknown` build and clippy of `nomad-client`, Debug (D-010). This amends D-011, which names Linux and Windows. The other jobs are unchanged.
- **The image:** builds the wasm client and the server in one Dockerfile.
  - Today's image builds Debug (D-010).
  - A Debug wasm client is large and slow in a browser, so the player's image builds the client in release.
  - CI still builds Debug only; the image build is not CI.
- **Local tests** (`#[ignore]`, D-190):
  - the client–server integration test over the WebSocket, with the native build as the test client;
  - a browser smoke test with Playwright and Chromium: load the page, connect, render a frame, send a command;
  - the GPU tests on lavapipe for the renderer (native), as now.

## 9. Work packages and milestones

*Proposed (Q243 for MP-1's place; the packages follow Q230–Q242).*

| Package | Content | Size |
|---|---|---|
| **N3** ✅ | Sessions, hub, limits and token, as built. Its `quinn` endpoint retires with N6 (Q234) | — |
| **N5** (changed) | The Docker process model: compose file, restart policy, named volume, backups, the start screen's server side, shut down and resume (§4) | M |
| **N6** (new) | Web endpoint: HTTP serving of the client, WebSocket transport for N3's sessions, Host and Origin checks (§5, §6) | M |
| **W1** (new) | The web build of the client: wasm target, async startup, assets over HTTP, the browser link for C7, keys and fullscreen, notifications, the image's client stage, wasm in CI (§7.3–§7.7, §8) | M |
| **W2** (new) | The HTML UI layer: Leptos, the view-model binding, and the M6 HUD in HTML (§7.2). U1–U7 then build on it | M |
| **M6** (changed) | Built in W2's HTML instead of in wgpu (Q232) | — |
| **U1–U7** (changed) | Built in HTML (Q231); sizes unchanged | — |

**Order:** W1 needs no server work, so it can start now against the phase-1 fixtures. W2 follows W1. N6 follows N4 or runs alongside it. N5 follows N6. C7 builds on W1 and N6.

**Milestones** (doc 30 §3):

| Milestone | Change |
|---|---|
| **β** | A human plays from the browser against the server in Docker Desktop. Contents add N6, W1 and W2. The 3-day playtest adds PC sleep (§4.4). |
| **γ** | Unchanged (D-097, D-237). |
| **MP-1** (new, D-237) | Up to 4 humans in the stage-1 sector on a hosted server, AI filling empty seats, for a full season. Contents decided in its own plan (§10). |
| **δ** | Unchanged; after MP-1 if Q243 is accepted. |

## 10. MP-1: what this design keeps open

*Derived* from D-237. *Proposed* where marked. MP-1's plan decides the rest.

**Already ready:**
- the WebSocket works unchanged behind a TLS-terminating reverse proxy (`wss://`);
- `Auth` carries a token that becomes a ticket from a login service;
- the Host and Origin allowlist is configuration;
- sessions are per commander (N3);
- replication is per client (N4).

**Known limits to lift:**
- `create` writes one human commander (`stage1_roster` in `crates/nomad-server/src/create.rs`);
- the server accepts at most 4 connections (N3), exactly the MP-1 maximum, which leaves no spare for a reconnect;
- N4 extracts views on the simulation thread (D-182), which is fine for 4 clients, and its plan names the path for more.

**Rules that come back in** (D-096 keeps them out of the MVP):
- **Derelict holdings (D-023):** a human who stops playing would otherwise block the sector;
- **notifications outside the client:** another human schedules sieges in your window;
- **hosting and identity:** accounts and real certificates.

Late joiners (D-045) can stay out if every human joins before the season starts. Vacation (D-024), alliances and chat can stay out.

**The AI:** with 4 humans, no Strategist is needed to have opponents; the Deputy and the raiders remain. AI seat fillers are only as good as γ's AI, which is one reason MP-1 follows γ.

**Open for MP-1's plan, not asked now:** where it is hosted (a cloud VM, or a player's own PC reachable from the internet), the login provider, the lobby, and the database for accounts (D-030's PostgreSQL, or SQLite at this scale).

## 11. Questions

Numbered Q230–Q243 (round 18). Each has a recommendation and alternatives.

- **✅ Q230 (D-238) — The client's technology** (§7.1).
  *Recommendation:* the Rust `wgpu` + `winit` client compiled to WebAssembly for the scene (D-004 stands), with the UI in HTML. *Alternatives:* a TypeScript rewrite of the client (supersedes D-004; loses M1–M4 and the shared protocol types); the whole UI in wgpu in the browser (keeps phase-1 doc 05; keeps the UI-volume risk).

- **✅ Q231 (D-239) — The language of the HTML UI** (§7.2).
  *Recommendation:* Rust with Leptos in the same wasm module, reading the Rust view-model directly. *Alternatives:* TypeScript with a bridge from the wasm view-model (familiar web tooling, but two languages, generated types and an npm supply chain outside `cargo deny`); Rust with plain `web-sys` DOM calls (no framework, much more code).

- **✅ Q232 (D-240) — Where the phase-1 HUD (M6) is drawn** (§7.2).
  *Recommendation:* in HTML over the canvas, held to phase-1 doc 06's ±4 px; world labels and markers stay in wgpu. Supersedes phase-1 doc 05's custom-drawn HUD. *Alternatives:* M6 in wgpu as phase-1 doc 05 says, and only the doc 17 §6 panels in HTML (two UI systems on one screen); everything in wgpu (Q230's last alternative).

- **✅ Q233 (D-241) — The transport** (§5.1).
  *Recommendation:* one WebSocket per session on TCP 31790, carrying N1 frames behind a one-byte stream tag; the native build uses the same. Supersedes D-087's QUIC transport and D-150's UDP port; D-087's encoding and version rule stay. *Alternatives:* two WebSockets per session, one per stream; WebTransport (needs HTTP/3 and a rotating certificate, no gain on loopback).

- **✅ Q234 (D-242) — The QUIC endpoint** (§5.3).
  *Recommendation:* retire it once the WebSocket endpoint passes the integration test, with `quinn`, `rustls`, `rcgen`, the certificate (D-151's certificate part) and the `nomad://` import string. *Alternatives:* keep it for the native build (two endpoints to maintain and test); keep it until MP-1 decides on WebTransport.

- **✅ Q235 (D-243) — Serving the client** (§5.2).
  *Recommendation:* `nomad-server` serves the client's files and the WebSocket on one port with `axum`, the client built into the image from the same commit, file names carrying the build ID. *Alternatives:* a second container (for example Caddy) serving the files and proxying the socket (one more moving part, and the Host and Origin checks must then be in both); the client embedded in the server binary (couples every server build to a wasm build).

- **✅ Q236 (D-244) — The guard without a login** (§6).
  *Recommendation:* Host and Origin checks against a loopback allowlist; browser sessions skip the token; sessions without an `Origin` keep D-151's token; the player's Windows account is the trust boundary. *Alternatives:* no token at all on loopback; a token delivered in a `SameSite=Strict` cookie (no stronger, since anything that can load the page gets the cookie); a pairing code from `docker logs` on first visit (a small login, which D-236 rules out).

- **✅ Q237 (D-245) — The process model in Docker Desktop** (§4).
  *Recommendation:* a compose file with `restart: unless-stopped`, a named volume and loopback-only publishing; Docker Desktop's start at sign-in replaces "start with Windows"; the server always runs and owns the start screen; "Shut down the Universe" stops the simulation inside the process and *Resume* catches up as a restart does; the client warns when its clock and the server's differ by more than 5 s. Supersedes D-085 and D-152. *Alternatives:* `restart: on-failure` with "shut down" exiting the process (then nothing restarts it at sign-in, and resuming needs Docker Desktop or the command line); no start screen in the browser, with `create` staying a command (not easy use).

- **✅ Q238 (D-246) — Fonts** (§7.6).
  *Recommendation:* the image's build stage downloads JetBrains Mono and Rajdhani and serves them with their OFL licence files; the repository still does not commit them. *Alternatives:* load them from Google Fonts (fails without internet); commit them to the repository (supersedes phase-1 ground rule 4).

- **✅ Q239 (D-247) — Browsers** (§7.4).
  *Recommendation:* Edge and Chrome on Windows as the reference, Firefox best effort, WebGPU first with WebGL2 as the tested fallback, desktop only, a 1280×720 minimum viewport. *Alternatives:* Chromium only (no Firefox testing); include tablets (a touch design first).

- **✅ Q240 (D-248) — Keys and fullscreen in the browser** (§7.5).
  *Recommendation:* doc 17 §7 stays except for keys a browser reserves; U7 tests every binding on Edge, Chrome and Firefox and proposes replacements for those that fail; fullscreen starts from the game's own control after *Play*. Supersedes D-108's fullscreen at start for the browser. *Alternatives:* use Chromium's Keyboard Lock API in fullscreen to capture more keys (Chromium only, fullscreen only); redesign the hotkey scheme for the browser now, before U7.

- **✅ Q241 (D-249) — Notifications** (§7.7).
  *Recommendation:* the browser's Notification API replaces the Windows toast, with permission asked when the player turns alerts on; outside the browser remains hosting work. *Alternatives:* the in-page banner only; Web Push already in the MVP (needs a push service on the internet, which D-236 rules out).

- **✅ Q242 (D-250) — Build, CI and tests** (§8).
  *Recommendation:* a wasm32 build and clippy of the client in CI, Debug, amending D-011; the player's image builds the wasm client in release; the WebSocket integration test and a Playwright browser smoke test run locally (D-190); GPU tests stay native. *Alternatives:* no wasm job in CI (the wasm build breaks unnoticed between local runs); the browser smoke test in CI (a network test, which D-190 keeps out).

- **✅ Q243 (D-251) — Where MP-1 sits** (§9).
  *Recommendation:* directly after γ and before δ, because multiplayer is the goal and δ (10 AI commanders in two sectors) does not serve it. *Alternatives:* after δ, as the last milestone; in parallel with δ.

## 12. Stress test

1. **Docker Desktop is not easy use.** It needs WSL2 or Hyper-V, admin rights to install, a few GB of disk, and a paid subscription in larger companies. That is acceptable while the only player is the owner (D-236). For anyone else it fails the "easy use" goal, which MP-1 meets by hosting instead. A native Windows server without Docker would be the easier local install. D-085 had exactly that, and D-236 replaces it.
2. **The browser is a hostile neighbour.** Every page the player opens can reach loopback. §6 handles that, but it is a new attack surface. The native client never had it. The Host and Origin checks need their own tests, including an upgrade with a forged `Host`.
3. **The QUIC work is sunk.** N3's endpoint, certificate and import string (D-150, D-151) were built for a model D-235 replaces. The session logic, which is most of N3, survives.
4. **A Debug wasm client may be too slow to judge visuals.** D-010 keeps CI on Debug. The owner judges visuals in the player's image, which builds release (Q242).
5. **WebGL2 is the floor, not the target.** Bloom, MSAA and the shield's two-layer blend must hold on WebGL2, because the reference browsers fall back to it on some GPUs. The client was written to WebGL2 limits, but nothing has run on a real WebGL2 context yet. W1's first task is to render M1–M4 in a browser and compare against the reference renders.
6. **Two clients in one codebase.** The native build stays for development only (D-235). If it falls behind the browser build, its value as a debugging tool goes. The CI wasm job and the native GPU tests keep both building; nothing keeps the native build's UI complete, and that is accepted, since it shows the scene, not the HTML UI.
7. **The clock after sleep** (§4.4) can shift siege deadlines against the player's wall clock while the WSL2 clock lags. The warning makes it visible, but does not fix it. The β playtest measures how often it happens.

## 13. Effect on other documents

- **`03-decision-log.md`:** D-235 to D-237 are recorded now. Answers to Q230–Q243 add superseding entries for D-085, D-087, D-108, D-150, D-151 (certificate part), D-152 and D-011, and amend D-094 (MP-1).
- **`02-open-questions.md`:** round 18 lists Q230–Q243.
- **Doc 20:** §2 (processes), §5 (transport), §8 (client architecture), §9 (security), §11 (testing) and §12 (dependencies) follow the answers. Notes now point here.
- **Doc 17:** §6 (start screen and notifications) and §7 (hotkeys) follow Q237, Q240 and Q241. Notes now point here.
- **Doc 30:** §2 gains N6, W1 and W2 and changes N5; §3 gains MP-1 (D-237) and changes β's wording; §6 notes that hosting returns at MP-1.
- **Phase-1 package:** doc 05's custom-drawn HUD (Q232), ground rule 4 for the image (Q238) and doc 06's M6 acceptance method (Q232) follow the answers.
- **`AGENTS.md` and the root `README.md`:** the client and process-model lines follow D-235 and D-236; installing and running follow N5.
