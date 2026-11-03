# Research: Standalone Session on the Hardware

Builds on nested-canvas (`specs/20261101-040740-nested-canvas/research.md`), client-windows
(R1-R14) and window-management (W1-W14). Decisions here are numbered S1-S22. References,
reading only (Principle I), sources checked out on 2026-11-02:

- niri `src/backend/tty.rs` (session, device and connector handling, atomic modeset,
  estimated-vblank timer), `src/input/mod.rs` (`find_bind`: hardcoded VT keys,
  `release_held_inputs`), `src/utils/spawning.rs` (spawn thread, unblocked signal mask,
  double fork), niri-config (`input { keyboard { xkb { ... } } }`, `binds`).
- DriftWM `src/backend/udev.rs`, `src/input/keyboard.rs` (VT keys before everything, also
  while locked).
- Hyprland `src/keybinds/Manager.cpp` (`handleVT`), aquamarine `src/backend/Session.cpp`
  (`libseat_switch_session`), `start/src/core/Instance.cpp` (crash restarter, no hang
  detection).

## S1. Dependencies (needs the owner's approval)

- **Decision**: add these crates; each binds a system library the constitution names
  (Principle I) or is the configuration parser chosen in the spec:

  | Crate | Version | Licence | System library | For |
  | --- | --- | --- | --- | --- |
  | `libseat` | 0.2 | MIT | libseat (Arch: `seatd`) | session, device access, console switching (S3) |
  | `drm` | 0.14 (the version `gbm` 0.18 is built against, so buffer types match) | MIT | kernel DRM ioctls (no C library) | connectors, CRTCs, atomic modesetting, page flips (S5) |
  | `gbm` | 0.18, `default-features = false`, feature `drm-support` | MIT | libgbm (Mesa) | scanout buffers for EGL (S6) |
  | `input` | 0.10, `default-features = false`, features `udev`, `libinput_1_21` | MIT | libinput | keyboard, touchpad, mice (S8) |
  | `udev` | 0.9 | MIT | libudev (systemd) | finding the GPU, display hotplug (S4, S13) |
  | `kdl` | 6.7, `default-features = false`, feature `span` | Apache-2.0 | - | `config.kdl` (S16) |

  `drm`, `gbm` and `udev` live in the Smithay GitHub organisation but are plain bindings used
  by many projects, not the `smithay` compositor framework that Principle I forbids. Removed:
  `wayland-egl`, `wayland-backend`'s `client_system` feature, and `wayland-client` moves to
  `[dev-dependencies]` (the protocol tests' in-process clients still use it), all with the
  nested backend (S21).
- **System packages**: Arch `libinput`, `seatd` (provides libseat; with the logind backend the
  seatd daemon is not needed), `mesa` (libgbm, already there). CI (Ubuntu) adds
  `libinput-dev libseat-dev libgbm-dev libudev-dev` and drops `libwayland-dev`: with
  `wayland-egl` gone nothing links libwayland (the server and the test clients use the
  pure-Rust backend) - confirmed by a CI run when the workflow changes.
- **Confirmed on 2026-11-02** (task T001, the author's laptop): libinput 1.32.0, libseat 0.9.3
  (its library contains the logind backend; the seatd service is inactive), libgbm 26.2.6,
  libudev 262, kitty installed.
- **Alternatives considered**: hand-written FFI to libdrm and logind over D-Bus (more unsafe
  code to own for no learning gain on the parts that matter; a D-Bus crate such as `zbus` is a
  larger dependency than `libseat`); `nix` for signals and process groups (only two libc calls
  are needed, S18).

## S2. Module layout and the backend interface

- **Decision**: a new `src/backend/drm/` module replaces `src/backend/wayland_host/`:
  `session.rs` (libseat), `gpu.rs` (udev lookup), `device.rs` (DRM resources, outputs,
  commits, page-flip events), `surface.rs` (GBM + EGL surface per output), `input.rs` (libinput
  to `InputEvent`), `keyboard.rs` (xkb state from the configuration, VT keys), `hotplug.rs`
  (udev monitor). It is the only module with hardware types and the only one allowed `unsafe`
  besides `ffi` and `render` (Principle V). `BackendEvent` drops the nested-only variants
  (`Configure`, `Scale`, `CloseRequested`, `FrameDone`) and gains
  `OutputAdded { id, info }`, `OutputRemoved { id }`, `Presented { id, time_ms }`,
  `Paused`, `Resumed`; `Input`, `Keymap` and `RepeatInfo` stay. No backend trait: there is one
  backend (Principle VI).
- **Rationale**: keeps the hardware layer thin and the rest backend-neutral (Principles III,
  IV); `App` (`state.rs`) keeps orchestrating shell, server and renderer as today.
- **Alternatives considered**: keeping a `Backend` trait for a future backend (speculative,
  Principle VI).

## S3. Session: libseat

- **Decision**: open the seat with libseat at startup and dispatch its fd on the event loop.
  The `enable` callback grants access; `disable` (console switch away, or the session becoming
  inactive) is acknowledged with `libseat_disable_seat` after the backend has stopped
  committing and suspended libinput. Device fds (the DRM card, input devices) come from
  `libseat_open_device`. Console switching calls `libseat_switch_session(vt)`. RubickWay never
  touches the VT ioctls: the session manager switches the console to graphics mode when it
  hands over control and back to text when RubickWay releases it or dies - which is what makes
  FR-012's "killed without a chance to clean up" recover (logind notices the controller's
  connection closing).
- **Startup checks (FR-002, US6)**: opening the seat fails over SSH or without a login session;
  RubickWay waits up to 2 seconds for the first `enable` and fails if it does not come (a console
  that is not being shown); taking the session fails when a desktop already controls it
  (started from inside a running desktop). libseat tries every backend and then reports any
  failure as `ENOSYS` (seen over SSH, Q12, 2026-11-03), so the desktop case is told apart by
  the environment: `XDG_SESSION_TYPE` `wayland`/`x11` or a non-empty `WAYLAND_DISPLAY` (or
  `EBUSY`, should a backend pass it on). Each case has its own one-line message
  (`contracts/cli.md`).
- **Alternatives considered**: logind directly over D-Bus (S1); seatd only (libseat picks the
  available backend itself).

## S4. Which GPU

- **Decision**: enumerate `drm` devices named `card*` on `seat0` with udev; take the one whose
  PCI parent has `boot_vga=1`, otherwise the first one with at least one connector. Only that
  card is opened; other GPUs (the laptop's NVIDIA, which drives no connector) are left alone
  (FR-016). The same fd is used for modesetting and, through GBM, for rendering.
- **Alternatives considered**: render on one GPU and scan out on another (out of scope).

## S5. Modesetting: atomic, primary plane only

- **Decision**: atomic modesetting (`DRM_CLIENT_CAP_ATOMIC`, `UNIVERSAL_PLANES`). For each
  connected connector: its preferred mode (the mode flagged preferred, else the first), a free
  CRTC the connector's encoders allow, and that CRTC's primary plane. The first commit for an
  output (and every commit after `Resumed`, S12) uses `ALLOW_MODESET` with `MODE_ID`,
  `ACTIVE`, `CRTC_ID`, `FB_ID` and the source/destination rectangles; later commits only swap
  `FB_ID`. Every commit, modesets included, is `NONBLOCK | PAGE_FLIP_EVENT`: the event loop
  never waits for the kernel, and the output stays `WaitingForFlip` until its event (S7).
  Turning an output off (disconnect, exit) is a non-blocking commit too; when a pending flip
  refuses it (`EBUSY`), the display is kept aside and turned off right after that flip's
  event, never with a blocking commit (`after_refused_off`); only the final
  "everything off" commit on exit may block, the loop being over by then. A commit refused with
  `EBUSY` (another display's modeset still running) leaves the output `Queued` and is retried
  at the next page-flip event of any output, or after 100 ms if none is pending. Page-flip
  events are read from the DRM fd on the event loop and become `Presented`. Any other failed
  commit is retried once as a full modeset on the next frame; if that retry fails too, the
  display is turned off without waiting and stays dark (a warning saying so, `OutputRemoved`)
  until it is tried again at the next display hotplug event or resume, and when no display is
  left the device counts as lost (`after_failed_commit`); `ENODEV` (device gone) is fatal
  (FR-012). A connector whose old output still waits for its off-commit is not added again
  until that commit is done; the rescan after it picks up a monitor plugged back meanwhile. CRTC assignment and mode choice are pure functions over the device's resource
  lists, unit-tested with made-up resources.
- **Rationale**: atomic is what i915 supports best and makes multi-output commits independent;
  without a hardware cursor (S10) only the primary plane is needed. A modeset takes from about
  50 ms to several hundred (link training, HDMI); blocking would freeze input and every display
  on hotplug, which Principle VII forbids.
- **Alternatives considered**: blocking modesets (niri's `ALLOW_MODESET` commit in `tty.rs` has
  no `NONBLOCK`; simpler, but stalls the loop on hotplug and resume); a commit thread
  (aquamarine `CommitThread.cpp`; threading around DRM for little gain here); legacy
  `drmModeSetCrtc`/`PageFlip` (no clean path to planes later); a hardware cursor plane (S10).

## S6. Rendering: one EGL context, one GBM surface per output

- **Decision**: `Egl::gbm(&gbm::Device)` on `EGL_PLATFORM_GBM_KHR`, choosing the config whose
  native visual is `XRGB8888`. One GLES context for all outputs (the renderer's textures are
  shared); each output has a `gbm_surface` (`SCANOUT | RENDERING`, implicit modifiers) and an
  EGL window surface on it. A frame: make that surface current, draw, `eglSwapBuffers`, lock the
  front buffer, wrap it in a DRM framebuffer (cached per buffer object), commit, and release
  the previous buffer at the page-flip event. With no output, the context is made current
  without a surface (`EGL_KHR_surfaceless_context`) so uploads still work. The existing
  `Egl::software_device` stays for the rendering tests.
- **Alternatives considered**: explicit modifiers (later, with direct scanout); dumb buffers
  with CPU drawing (no GPU).

## S7. Frame scheduling per output

- **Decision**: each output has `Idle`, `Queued` (dirty, waiting to draw, or a commit refused
  with `EBUSY`, S5) and `WaitingForFlip` (after any commit, modesets included) states. `App`
  draws an output when it is dirty and not waiting for a flip; the flip event returns it to `Idle` and reports the windows shown to the server
  (`presented`), as the host's frame callback did. When nothing is dirty but applications on an
  output wait for a frame callback, an estimated-vblank timer (one refresh interval after the
  last presentation, as niri) completes them without drawing. Dirtiness: a viewer's camera or
  size change marks that output; window changes and pointer moves mark the outputs that show
  them (S10); while paused nothing is drawn.
- **Rationale**: one frame per refresh per output at most, no tearing (page flips), a slow
  output never blocks another, no drawing while idle (FR-010).

## S8. Input: libinput

- **Decision**: a libinput udev context on `seat0` whose `open_restricted` /
  `close_restricted` go through libseat. Translation to `InputEvent` (pure function over the
  extracted values, unit-tested):
  - pointer motion: the accelerated delta, as a new `PointerMoved { delta }` (S9);
  - buttons: Linux codes as today; `seat_button_count` drops duplicates from a second mouse;
  - scrolling: `SCROLL_WHEEL` with `value120`, `SCROLL_FINGER` and `SCROLL_CONTINUOUS` with
    their deltas, matching the existing `Scroll { source, delta, value120 }`;
  - pinch: begin, update (`scale`), end (`cancelled`), as today;
  - keys: evdev code and state, with `seat_key_count` so two keyboards holding a key give one
    press; the event time in milliseconds is added to `Key` (`time_ms`, S15).
  Touchpads get tap-to-click enabled when added; everything else keeps libinput's defaults
  (FR-006, Assumptions). Device add/remove needs nothing extra: libinput reports it, and the
  release of a removed device's held keys comes as normal key releases.

## S9. Pointer and layout in the core

- **Decision**: the pointer position is core state, not backend state. A new pure
  `canvas::layout` module places viewers in a row (FR-018: built-in first, then connection
  order, top edges aligned; displays present at startup in the card's connector order),
  integrates relative motion, clamps x to the row, takes the display whose horizontal range
  contains x and clamps y to that display's height (so the pointer crosses to a shorter display
  with its height clamped; a "nearest point of the union" rule, as wlroots'
  `wlr_output_layout_closest_point`, would instead keep it on the taller display, against the
  spec), and maps a layout position
  to `(viewer, logical position)`. The shell turns each `PointerMoved` into the existing
  per-viewer enter / motion / leave handling, so routing, grabs and operations keep working;
  during a pan, positions keep being given relative to the viewer the pan started on (FR-019),
  the way the nested host kept its implicit grab.
- **Rationale**: all of FR-009, FR-018, FR-019, FR-021 and FR-023 become unit-testable
  (Principle III); the backend only knows deltas.
- **Alternatives considered**: absolute positions computed in the backend (puts layout logic in
  the untested layer).

## S10. Drawing the pointer

- **Decision**: a software cursor: the renderer draws the pointer image as the last quad of the
  frame on the output it is on (above a fullscreen window too, FR-008). Pointer motion marks
  that output (and the one it left) dirty; drawing is limited to one frame per refresh by S7.
  Images: the installed Xcursor theme `default` (following `Inherits`, size 24) when one is
  found at startup, otherwise built-in 24 px images defined as 12 px ASCII art in the source
  and doubled (arrow, closed hand, move, two double arrows plus their transpose and mirror for
  the four resize directions, mapped to the eight resize shapes), uploaded once as textures and
  drawn as ordinary window quads, 1:1, so no new GL code is needed. Cursor images set by applications stay unshown (the default arrow is drawn), but
  `set_cursor` without a surface hides the pointer while it is over that client's window and no
  RubickWay gesture runs (niri `cursor.rs` and DriftWM `render/cursor.rs` treat
  `CursorImageStatus::Hidden` the same way): the server records a per-client "hidden" flag and
  the shell's `cursor()` returns `Hidden` when the pointer focus belongs to such a client.
- **Rationale**: one drawing path, testable by pixel assertions headless (Principle III); the
  author's laptop has no cursor theme, so the built-in images are what gets used.
- **Trade-off accepted (owner, 2026-11-02)**: every pointer move redraws that display (at most
  once per refresh), costing GPU work and battery while the pointer moves, and up to about one
  frame of extra pointer latency compared with a hardware cursor. niri, Hyprland and DriftWM
  all use the cursor plane by default with software drawing as a fallback (niri's
  `disable-cursor-plane`, Hyprland's `cursor:no_hardware_cursors`); the plane is a later,
  self-contained feature, measured against quickstart Q21.
- **Baseline (Q21, release build, the author's laptop, eDP-1 1920x1080@60, 2026-11-03)**: idle
  CPU under 1% (below `top` itself); while moving the pointer, panning and dragging windows
  about 0.4% of the whole CPU in `btop` (8 threads, so about 3% of one core; `top` showed peaks
  up to 5% of one core), and up to 1.5% of the whole CPU (about 12% of one core) while resizing
  a kitty. No tearing seen. The cursor-plane feature compares against these numbers.
- **Alternatives considered**: a hardware cursor plane now, with or without a software fallback
  (no redraw on motion, but a second path with its own commits and size limits, about 300-400
  more lines); procedural drawing (harder to read than ASCII art).

## S11. Console switching (FR-011)

- **Decision**: the backend's keyboard checks each pressed key's symbol *with* modifiers
  applied: `XF86Switch_VT_1..12` (what the default XKB rules give for Ctrl+Alt+F1..F12) calls
  `libseat_switch_session`, and the press and its later release are swallowed - before any
  other shortcut and never configurable (as niri, DriftWM, Hyprland). Switching to the console
  RubickWay is on does nothing (as Hyprland).
- **Note**: the shell's shortcuts match the level-0 symbol (nested-canvas research), where
  Ctrl+Alt+F1 is just `F1`; that is why this check lives in the backend's keyboard, which has
  the full xkb state.

## S12. Pause and resume (FR-011, FR-029)

- **Decision**: on `disable`, the backend stops committing, suspends libinput, acknowledges,
  and sends `Paused` after `Input(PointerLeave)` and `Input(FocusLost)`: the shell's existing
  handling ends grabs, moves, resizes and pans, and applications get key and pointer leave
  (window-management already does this for a lost host focus). Application frame callbacks
  wait (no presentation while paused). On `enable`: resume libinput, re-read the connectors
  (a monitor may have changed while away, S13), commit every output with a full modeset, mark
  everything dirty, then `Resumed` and `Input(FocusGained)`. Suspend and resume of the machine
  need nothing more: the kernel and libinput report removed and added devices, and a failed
  first commit after resume is retried as a modeset (S5).

## S13. Display hotplug

- **Decision**: a udev monitor on the `drm` subsystem; every change event re-reads the
  connectors of the card. A newly connected connector becomes an output (`OutputAdded`, then
  its first modeset); a disconnected one is turned off in a commit and becomes `OutputRemoved`.
  Changes are applied in event order, and every rescan compares against the current state, so
  fast plug/unplug sequences end in the right state (edge case). Output ids are never reused.
- **Output info** (FR-015): connector name (`eDP-1`, `HDMI-A-1`: type name plus type index),
  physical size from the connector, make and model from the EDID blob (PNP manufacturer id and
  the monitor name descriptor; `Unknown` when absent) - EDID parsing is pure and unit-tested;
  internal = connector type eDP, LVDS or DSI.

## S14. Several viewers in the shell

- **Decision**: the shell keeps a list of viewers keyed by `ViewerId` (the backend's output
  id), each with its `Viewer` (camera, size, gesture), its home camera and its fullscreen
  window. The single `viewer` field becomes "the active viewer" (the one under the pointer,
  FR-019) for keyboard shortcuts, new-window placement and focus cycling (FR-022). Home views
  (FR-020) are computed by `canvas::layout` from the row: the row's bounding box centred on the
  canvas origin at 100%, each viewer's camera centred on its part; a viewer added later
  continues the row to the right. `visible()` becomes per viewer; the union drives frame
  callbacks, and per-window viewer sets drive `wl_surface.enter`/`leave`. Fullscreen state
  moves from one `Option` to one per viewer; "the later request wins" applies per viewer.
  Removing a viewer ends its fullscreen (the window gets its previous rect back), keeps
  maximized windows as they are, ends a pan, move or resize that involved it (as a broken grab),
  and moves the pointer to the centre of the leftmost viewer; pointers on other viewers keep
  their position on their viewer when the row re-flows. Fullscreen requests naming an output
  use that output's viewer (niri `handlers/xdg_shell.rs` `fullscreen_request` and DriftWM
  `handlers/xdg_shell.rs` resolve the requested output the same way). Focus cycling only moves
  and leaves fullscreen on the active viewer. The shell with zero viewers keeps working: new
  windows open centred on the canvas origin, focus cycling moves no view, maximize and
  fullscreen requests are ignored, nothing is drawn and frame callbacks wait.
- **Rationale**: the core stays plain data (Principle IV); every multi-viewer rule is a unit
  test. Existing single-viewer tests keep their expectations with one viewer of the same size.

## S15. Shortcuts: configurable Mod, terminal, two-press quit

- **Decision**: `input::bindings` stops being a constant table: a `Bindings` value is built
  from the configuration (Mod, terminal combination and command) and owned by the shell; the
  built-in shortcuts are the same as before with `Mod` substituted. New actions:
  `SpawnTerminal` (the shell reports it in `Effect`, `App` hands the command to the spawner,
  S18) and the quit confirmation: the first Mod+Shift+Q records its `time_ms`; a second press
  within 2000 ms quits; any other non-modifier key press, a console switch (`FocusLost`) or the
  window passing cancels it, while modifier presses and releases do not (FR-031). Standalone
  input has no key repeat events, so a held combination never counts twice.

## S16. Configuration file

- **Decision**: `$XDG_CONFIG_HOME/rubickway/config.kdl` (default `~/.config/rubickway/`, also
  when the variable is empty or relative, per the XDG base directory specification), read once
  at startup before the event loop runs, parsed with `kdl` into a document and
  walked by hand into a plain `Config` (pure, unit-tested from strings). Unknown nodes and bad
  values are warnings with the file, line (from the node's span) and reason; each falls back to
  its default (FR-026). A syntax error makes the whole file fall back to the defaults, with one
  warning naming the line. A node given twice keeps the last one, with a warning. Schema:
  `contracts/config.md`. Key combinations parse with xkb's
  keysym names (case-insensitive retry, letters lowered so `Mod+T` matches the level-0 `t`);
  combinations equal to a built-in shortcut, Ctrl+Alt+F1..F12, or with unknown names are
  rejected.
- **Alternatives considered**: serde mapping (KDL's arguments/properties/children do not map
  cleanly; hand walking gives better messages).

## S17. Keyboard layout

- **Decision** (revised 2026-11-03, owner): the backend's keyboard compiles one fixed keymap,
  `evdev`, `pc105`, `us`, no variant, no options (`Keyboard::us`), with
  `xkb::CONTEXT_NO_ENVIRONMENT_NAMES`, so the environment never changes it (FR-007). The
  configuration has no keyboard settings: the earlier `input > keyboard > xkb` block (raw RMLVO
  names, as niri) was removed, because the owner wants users to choose input languages, not
  XKB names; that comes as a later feature (a language list switched by a binding, one keymap
  per language, input methods for Chinese). The keymap text goes to applications through
  `Keymap`, repeat info is fixed at 25/s after 600 ms (`RepeatInfo`). The shell's
  `binding_sym` (layout 0, level 0) and modifier tracking stay as they are, ready for several
  layouts.

## S18. Starting the terminal (FR-028)

- **Decision**: a spawn runs on its own short-lived thread: `std::process::Command` in `$HOME`
  with `WAYLAND_DISPLAY` set and the rest of RubickWay's environment kept, stdin `/dev/null`
  (RubickWay's stdin is the console it was started from, which is not the program's to read;
  stdout and stderr stay RubickWay's, so a program's messages land in the same log), a
  `pre_exec` that unblocks all signals (the event loop's signal source blocks SIGINT, SIGTERM
  and SIGHUP, and children inherit the mask) and calls `setsid`; the thread then waits for the
  child, so no finished child stays behind. Spawn errors and unsuccessful exit statuses come
  back to the loop through a channel and are logged (`contracts/cli.md`). The `pre_exec` code
  is in `src/ffi/spawn.rs` (it needs `unsafe`, Principle V).
- **Rationale and reference**: niri `src/utils/spawning.rs` (spawn thread, unblocked mask).
- **Alternatives considered**: double fork (no waiting thread, but more unsafe code).

## S19. Watchdog (FR-030)

- **Decision**: the watchdog thread starts right after the configuration is read and runs until
  the process ends. Startup refreshes the heartbeat after each step (seat, GPU, outputs, input,
  socket); the event loop stores it (an `AtomicU64` of milliseconds on the monotonic clock)
  after every dispatch, and a 1-second calloop timer guarantees a dispatch even when idle;
  shutdown refreshes it once when the loop ends, so a shutdown hanging for longer than the
  timeout is ended too. A watchdog thread checks it every 250 ms (so the end comes at most a quarter second late); when it is older than the
  configured timeout it writes one line to stderr and calls `_exit(1)` (no destructors: the loop may hold
  anything; the session manager restores the console as after a kill). Rust's `Instant` uses
  `CLOCK_MONOTONIC`, which does not advance during suspend, so sleeping never counts (FR-030).
  Verification on three levels:
  - the decision is a pure function (last heartbeat, now, timeout), unit-tested;
  - a subprocess test: the test binary re-runs itself in a watchdog-only mode (selected by an
    environment variable read only by that test's helper) with a 1-second timeout and a stalled
    heartbeat; the parent checks exit status 1 and the stderr line within 2 seconds, so the
    real thread and `_exit` run, headless;
  - a debug-only freeze trigger for the hardware check: in debug builds
    (`cfg(debug_assertions)`) SIGUSR1 makes the event loop spin forever, so
    `kill -USR1 <pid>` from another console or SSH gives a real freeze; quickstart Q23 checks
    the console comes back within the timeout. Release builds have no SIGUSR1 handling, so
    its default action ends the process at once (plan, Complexity Tracking).
  `kill -STOP` and a debugger cannot fake a freeze: they stop the watchdog thread too.
- **Note**: none of the references has one (niri, DriftWM, Hyprland); a freeze inside a GPU
  driver call may delay even `_exit` until the call returns, like `kill -9`.

## S20. Signals and exit

- **Decision**: SIGHUP joins SIGINT and SIGTERM (FR-012). Every orderly exit (two-press quit,
  signals, fatal errors, a panic unwinding through `run`) drops the backend in order: outputs
  off, GBM and EGL objects, devices closed, seat closed; the session manager then shows the
  text console. A panic hook records the message and location without printing; `main` catches
  the unwind (`catch_unwind` around `run`), so the backend is dropped first, then prints
  `rubickway: internal error: <message> at <file>:<line>` as the last line and exits with 1
  (FR-017). Exit statuses and messages: `contracts/cli.md`. Log output stays on stderr
  (FR-013): the kernel console does not draw while in graphics mode, so lines written meanwhile
  appear only after RubickWay exits.
- **Signal mask** (found by Q23, 2026-11-03): the handled signals (SIGINT, SIGTERM, SIGHUP, and
  SIGUSR1 in debug builds) are blocked in the main thread first thing in `run`, before the
  watchdog, Mesa or a spawn starts a thread, so every thread inherits the mask and only the
  event loop's signalfd receives them. Blocking them only when the loop's signal source is
  created is too late: a signal delivered to an earlier thread takes its default action and
  ends the process at once, with no orderly exit. Spawned programs still get an empty mask
  (S18).

## S21. Removing nested mode (FR-001, US6)

- **Decision**: delete `src/backend/wayland_host/`, `Egl::wayland` and the host-specific parts
  of `state.rs` (read guard, host frame callbacks, host cursor shapes); `CursorShape` is now
  mapped to the built-in or theme images (S10). The viewer's default size and the
  host-configure path go; a viewer's size is its display's mode. Done last (US6), after the
  standalone backend works, so the tree builds at every step.

## S23. Nested-era decisions, reviewed for the hardware (owner's request, 2026-11-02)

Every decision from nested-canvas, client-windows and window-management that assumed a host
desktop, checked against the standalone backend:

| Decision (nested) | Standalone | Action |
| --- | --- | --- |
| The shell routes keys to windows only after the host gives the viewer the keyboard (`FocusGained`) | nobody sent it at startup: keys reached no window until a console switch | **fixed**: the DRM backend sends `FocusGained` at startup |
| A pointer leave means the host broke the grab: grabs, moves, resizes and pans end | the backend sends it on a pause (FR-011 wants exactly that) | keep |
| `FocusLost` clears modifiers and held keys, commits the focus order | sent on a pause | keep |
| Frame callbacks complete when the host presents the frame | page flips and the estimated vblank (S7) | keep, wording only |
| Keymap and key repeat come from the host | from the fixed US English keymap; repeat 25/600 sent by the backend | keep, wording only |
| Scroll deltas "as the host delivered them, natural scrolling applied" | libinput deltas, natural scrolling off | keep, wording only |
| One `wl_output` `RUBICKWAY-1` with the viewer's buffer size | one per display with its real description, none without a display | replaced by T045 (US2) and T071 (US3); the placeholder kept until the first display went in US6 (T075) |
| Viewer default size 1280x800 and "zero means your choice" | the size is always the display mode | remove in US6 (T075) |
| Buffer downscaling when the viewer exceeds the GPU framebuffer limit ("the host scales up") | a scanout surface is always the full mode, far below the limit | remove in US6 (T075) |
| `disable_vsync` (frames paced by host callbacks) | GBM swaps do not wait; pacing is the page flip | remove with `Egl::wayland` in US6 |
| `Bindings::default()` = Mod Alt, used by `Shell::new()` and the tests | standalone builds bindings from the configuration | in US6: default Super, tests updated |
| Software rendering needed by GL clients (`LIBGL_ALWAYS_SOFTWARE`) | Mesa falls back by itself when no GPU-buffer protocol is offered (confirmed on the laptop) | dropped from the docs |
| Cursor images from applications not shown; host cursor shapes | drawn by RubickWay (S10) | keep |
| Fullscreen leaves the host window as it is | fills the display | keep |
| Clipboard stub (`wl_data_device_manager`) | still no clipboard; with no host there is nothing to share with | keep |

## S22. Misbehaving applications (Principle II)

- **Decision**: the protocol surface is unchanged apart from one `wl_output` global per display
  appearing and disappearing (`contracts/protocols.md`). New cases from the spec, each with an
  automated test in `tests/protocol/`: frame callbacks are held while paused and the latest
  content per window replaces older content (no growth); windows created, mapped or destroyed
  while paused are shown correctly after `Resumed`; move/resize requests with a serial from
  before a pause are ignored (the pause ends the press, so the serial no longer matches);
  binding a removed output, or an output global that disappeared, gets no error (the server
  keeps the global's resources inert until the client destroys them); cursor images stay
  ignored.

## S24. Pointer bindings (owner's request, 2026-11-03; FR-032)

- **Decision**: the Mod pointer gestures become entries of the existing `binds` block, written
  like key combinations with `MouseLeft`, `MouseRight` or `MouseMiddle` as the last part, and
  one action node: `move-window`, `resize-window` or `pan-canvas`. `Bindings` keeps a pointer
  table next to the shortcuts: by default Mod + left -> move, Mod+Shift + left -> pan, Mod +
  right and Mod+Shift + right -> resize (window-management's "Shift ignored" for resize stays
  through the fourth entry). The two left-button defaults are the other way round from
  window-management: after trying both, the owner chose move on Mod + left (as niri and most
  window managers), 2026-11-03. Lookups match the modifiers exactly, like key bindings; a press
  matching nothing is an ordinary click. Each action keeps its window-management behaviour,
  including the background (move pans, resize does nothing) and fullscreen (resize and pan
  leave it). One valid pointer binding replaces the pointer defaults; spawn and pointer
  defaults are replaced independently, so binding only `Mod+MouseLeft` keeps Mod+Return.
- **Pan by any button**: the viewer used to start a pan on every left press itself. The shell
  now starts and ends pans (`Viewer::begin_pan` / `end_pan`) and remembers the button that
  started one, so `pan-canvas` works on any button and only that button's release ends it.
- **Rejected**: a pointer combination without modifiers (it would take plain clicks from every
  application; niri likewise only binds pointer gestures with modifiers); `spawn` on pointer
  combinations and pointer actions on keys (no use yet); scroll and pinch as bindings (zoom has
  no other action to choose from).
- **Alternatives considered**: a separate `pointer-binds` block - rejected, the owner wanted the
  same `binds` block, and one block gives one duplicate check; hard-coded gestures with only a
  "swap" setting - too narrow.
