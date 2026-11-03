# Implementation Plan: Standalone Session on the Hardware

**Branch**: `worktree-drm-backend` | **Date**: 2026-11-02 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261102-143401-drm-backend/spec.md`

## Summary

RubickWay runs directly on the hardware from a text console, and nested mode goes away. A new
thin backend (`src/backend/drm/`) gets the display and input devices through libseat, drives
every connected display with atomic modesetting and one GBM/EGL surface each, reads keyboards,
touchpads and mice through libinput, and watches for display hotplug with udev. It reports
backend-neutral events: relative pointer motion, keys, outputs added and removed, frames
presented, pause and resume. Everything that decides behaviour stays in the pure core and is
unit-tested: a new row layout places the displays and owns the pointer position, the shell
holds one viewer per display (active viewer, home views, per-viewer fullscreen, the window's
viewer), shortcuts become a value built from `config.kdl` (configurable Mod, terminal shortcut,
two-press quit), and the keymap is a fixed US English one. The pointer is drawn by the
renderer. A watchdog thread ends a frozen RubickWay so the session manager restores the
console. Details: [research.md](research.md) S1-S22.

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: added (owner's approval needed, research S1): `libseat` 0.2, `drm`
0.14 (matching `gbm`), `gbm` 0.18, `input` 0.10, `udev` 0.9, `kdl` 6.7; removed: `wayland-egl`;
`wayland-client` moves to dev-dependencies; `wayland-backend`'s `client_system` feature goes.
System libraries: libseat, libinput, libudev, libgbm, EGL/GLES (Mesa), libxkbcommon.

**Storage**: reads `~/.config/rubickway/config.kdl` at startup; Xcursor theme files at startup
if installed

**Testing**: `cargo test` (debug and release) - unit and `proptest` tests for `canvas::layout`,
`config`, bindings and quit confirmation, EDID parsing, CRTC/mode choice, libinput event
translation, watchdog decision; a watchdog subprocess test (real thread and `_exit`); shell
multi-viewer tests in `tests/shell.rs`; fixed-keymap
tests in `tests/keymap.rs`; protocol tests (per-display `wl_output`, enter/leave, pause) in
`tests/protocol/`; pointer drawing pixel test in `tests/render_windows.rs`; hardware behaviour
manually through [quickstart.md](quickstart.md) Q1-Q23

**Target Platform**: Linux with DRM/KMS and logind; the author's laptop (Intel UHD 620 / i915,
1920x1080 eDP + HDMI, Synaptics touchpad, Arch, systemd 262, Mesa 26)

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: canvas on screen within 3 s of start (SC-001); one frame per refresh per
display at most, no tearing (FR-010); console back within 2 s after quit or switch (SC-002,
SC-003); idle CPU under 1% (SC-005)

**Constraints**: never block the loop (spawning on a thread, files read before the loop starts);
nothing above the backend knows DRM, libinput or libseat (Principle IV); `unsafe` only in
`backend`, `render`, `ffi` (Principle V); runs without root

**Scale/Scope**: up to a few displays and input devices, a handful of windows; roughly 4-5k
lines including tests; removes ~1k lines of nested backend

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | No compositor framework. Hardware, session and input go through the system libraries the principle names (libseat, libinput, xkbcommon, drm/gbm, EGL) plus libudev, via thin binding crates (S1). References named: niri `backend/tty.rs`, `input/mod.rs`, `utils/spawning.rs`; DriftWM `backend/udev.rs`, `input/keyboard.rs`; Hyprland `keybinds/Manager.cpp`, aquamarine `Session.cpp` (reading only). |
| II. Client can't take it down | PASS | No new protocol interfaces; new misbehaving cases (activity while paused, stale serials across a pause, removed outputs, cursor images) each get a test (S22, `contracts/protocols.md`). Lints unchanged. Bad configuration never stops RubickWay (FR-026). |
| III. Test-first, headless | PASS | All decisions in pure code with tests written first: layout, multi-viewer shell, config, bindings, quit confirmation, keymap from names, EDID, CRTC/mode choice, input translation, watchdog, pointer pixels. The hardware layer (seat, DRM commits, GBM/EGL surfaces, libinput context, udev monitor) stays thin and is covered by quickstart Q1-Q23. |
| IV. Backend-independent core | PASS | `Layout`, `ViewerId`, `OutputInfo`, `Config`, `Bindings`, `CursorImage` are plain data; the backend emits `BackendEvent`s (S2) and nothing above it imports DRM, GBM, libinput or libseat types. |
| V. Contained unsafe | PASS | `unsafe` in `src/backend/drm/` (libseat, libinput, GBM/EGL interop), `src/render/egl.rs` (GBM platform display), `src/ffi/spawn.rs` (`pre_exec`: signal mask, `setsid`) and the watchdog's `_exit` (`src/backend/watchdog.rs`), each with `// SAFETY:` comments. |
| VI. No speculative generality | PASS | No backend trait (one backend); `wl_output` stays at version 4; configuration has exactly the settings the spec lists; no hardware cursor plane, no explicit modifiers, no layout settings. |
| VII. Never block the loop | PASS | Blocking work named: reading the configuration and cursor theme (before the loop starts), spawning (own thread, S18), libseat's wait for the first `enable` (startup, bounded to 2 s, before the loop), the final "all outputs off" commit on exit (after the loop ended). Every DRM commit while running, modesets included, is `NONBLOCK` with a page-flip event; `EBUSY` is retried (S5). |
| VIII. 2D core with plugins | PASS | Core only; no plugin interface. |

Post-design re-check (after data-model, contracts, quickstart, and the owner's review of the
judgement calls on 2026-11-02): all PASS. Decided in that review: a software pointer for now
(the cursor plane is a later feature, S10), non-blocking modesets (S5), and a debug-only
freeze trigger for the watchdog's hardware check (S19, Complexity Tracking).

## Project Structure

### Documentation (this feature)

```text
specs/20261102-143401-drm-backend/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (S1-S22)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q23 manual checks)
|-- contracts/
|   |-- cli.md           # invocation, messages, exit statuses, started processes
|   |-- config.md        # config.kdl schema and problems
|   |-- input.md         # devices, keys, several displays, pause
|   `-- protocols.md     # wl_output per display, enter/leave, frame callbacks
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- main.rs                  # unchanged usage check; run() standalone
|-- config.rs                # NEW: Config, KeyCombo, parse/load (S16)
|-- watchdog.rs              # NEW: Heartbeat, expired() (pure); thread start lives in backend
|-- canvas/
|   `-- layout.rs            # NEW: Layout, ViewerId, home cameras, pointer clamping (S9, S14)
|-- input/
|   |-- mod.rs               # PointerMoved { delta }, Key { time_ms }
|   |-- bindings.rs          # Bindings value, SpawnTerminal, Mod from config (S15)
|   `-- keyboard.rs          # + Keyboard::us, the fixed keymap (S17)
|-- viewer/mod.rs            # takes &Bindings; no DEFAULT_SIZE / host configure
|-- shell/
|   |-- mod.rs               # viewers + layout, active viewer, per-viewer fullscreen and
|   |                        #   visibility, window's viewer, quit confirmation, spawn effect
|   `-- route.rs             # pan viewer, QuitConfirm
|-- server/
|   |-- output.rs            # one wl_output global per display, enter/leave per viewer
|   `-- mod.rs               # add_output / remove_output, set_window_outputs, paused callbacks
|-- render/
|   |-- egl.rs               # + Egl::gbm, surfaceless current; - Egl::wayland
|   |-- gles.rs              # + cursor quad (drawn last)
|   `-- cursor.rs            # NEW: built-in ASCII-art images, Xcursor theme loading (S10)
|-- backend/
|   |-- mod.rs               # BackendEvent (S2)
|   |-- watchdog.rs          # NEW: watchdog thread, _exit (S19)
|   |-- drm/                 # NEW (S2-S13)
|   |   |-- mod.rs           # DrmBackend: owns everything below, event sources
|   |   |-- session.rs       # libseat
|   |   |-- gpu.rs           # udev: pick the card
|   |   |-- device.rs        # resources, outputs, CRTC/mode choice (pure parts), commits, flips
|   |   |-- edid.rs          # pure EDID parsing
|   |   |-- surface.rs       # GBM surface + EGL surface + framebuffers per output
|   |   |-- input.rs         # libinput context; translate() (pure part)
|   |   |-- keyboard.rs      # xkb state of the US keymap, VT keys, keymap text
|   |   `-- hotplug.rs       # udev monitor
|   `-- wayland_host/        # REMOVED (US6, S21)
|-- ffi/
|   `-- spawn.rs             # NEW: pre_exec (signal mask, setsid), spawn thread (S18)
|-- standalone.rs            # NEW: App on the DRM backend: per-output frame state,
|                            #   estimated vblank timer, pointer drawing, spawner, heartbeat
`-- state.rs                 # AppError (the nested App was removed in US6)

tests/
|-- shell.rs                 # + several viewers, layout routing, home views, window's viewer,
|                            #   per-viewer fullscreen, viewer removal, quit confirmation
|-- viewer_input.rs          # Mod from bindings
|-- keymap.rs                # + the fixed US keymap
|-- config.rs                # NEW: config.kdl parsing and every rejected case
|-- watchdog.rs              # NEW: subprocess test of the real watchdog thread and _exit
|-- panic_exit.rs            # NEW: subprocess test of the one-line panic message
|-- cli.rs                   # NEW: the unchanged command line (usage, exit 2)
|-- render_windows.rs        # + pointer drawn last, above a fullscreen window
`-- protocol/
    |-- outputs.rs           # NEW: wl_output per display, add/remove, enter/leave
    `-- misbehaving.rs       # + activity while paused, stale serial across a pause
```

**Structure Decision**: same single crate and module direction as before (`canvas` <-
`viewer` <- `shell` <- `state` -> `server`, `backend`, `render`). `config` and `watchdog` are
small top-level modules because both `state` and the backend use them; the hardware-facing part
of each lives in `backend`.

### Order of work (by user story)

1. **US1** (P1): config skeleton (Mod only), bindings value, layout with one viewer, the fixed
   keymap, session + GPU + one output + libinput + pointer drawing, two-press quit, signals.
2. **US2** (P1): terminal shortcut and spawner, rest of the configuration, `wl_output` from
   the real display, keymap and repeat to applications.
3. **US4** (P2): pause and resume, VT keys, stale serials across a pause.
4. **US5** (P2): exit paths, SIGHUP, watchdog.
5. **US3** (P2): several viewers, hotplug, per-display outputs and enter/leave.
6. **US6** (P3): remove the nested backend, its dependencies and docs.

The nested backend keeps building until US6, so the tree compiles at every step; `run()`
switches to the DRM backend in US1.

### Supporting changes

- `README.md`: status line (no longer "early skeleton"), prerequisites (libinput, seatd), how
  to start from a console, the configuration file, controls with Super as Mod, Ctrl+Alt+F<n>,
  two-press quit; the nested-mode instructions go.
- `.github/workflows/ci.yml`: system packages per research S1 (touches CI: covered by this
  plan once approved).
- `Cargo.toml`: dependencies per S1.
- `THIRD_PARTY_NOTICES.md`: nothing (dependencies are not vendored).

## Complexity Tracking

No constitution violations. One addition that is not a spec requirement, recorded so it is not
mistaken for speculative generality (Principle VI):

| Addition | Why Needed | Simpler Alternative Rejected Because |
| --- | --- | --- |
| Debug-only SIGUSR1 handler that makes the event loop spin forever (`cfg(debug_assertions)`, absent from release builds) | the only way to check FR-030's whole chain (timer, heartbeat, watchdog, `_exit`, console restored) on real hardware (quickstart Q23) | `kill -STOP` or a debugger also stop the watchdog thread; the automated subprocess test cannot show that the session manager restores the console |
