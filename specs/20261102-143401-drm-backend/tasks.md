---

description: "Task list for the standalone session on the hardware"
---

# Tasks: Standalone Session on the Hardware

**Input**: Design documents from `specs/20261102-143401-drm-backend/`

**Prerequisites**: plan.md, spec.md, research.md (S1-S22), data-model.md, contracts/,
quickstart.md, checklists/review.md (all items answered in the spec; ticking is the owner's)

**Tests**: Required by the constitution (Principle III): every user story has test tasks that
are written and seen failing before its implementation tasks. Hardware-only behaviour is
checked through quickstart.md (Q1-Q23).

**Organization**: Tasks are grouped by user story, in the plan's order of work: US1, US2, US4,
US5, US3, US6. The nested backend keeps building until US6, so the tree compiles at every step.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1-US6)
- References like (S9) point to research.md; (FR-xxx) and (SC-xxx) to spec.md; "Edge Cases
  <name>" to the named bullet in spec.md Edge Cases; Qn to quickstart.md

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: system packages, dependencies, module skeletons

- [X] T001 Confirm the prerequisites on the laptop and record the result in `specs/20261102-143401-drm-backend/research.md` S1 ("Confirmed on <date>: ..."): the owner installs `libinput seatd kitty` (`sudo pacman -S libinput seatd kitty`); `pkg-config --modversion libinput libseat gbm libudev` succeeds; libseat's logind backend is present without the seatd daemon running (spec Assumptions; checklist CHK049). If libseat lacks the logind backend, stop and ask the owner before continuing
- [X] T002 Add the dependencies from research S1 to `Cargo.toml`: `libseat = "0.2"`, `drm = "0.14"` (the version `gbm` 0.18 is built against), `gbm = { version = "0.18", default-features = false, features = ["drm-support"] }`, `input = { version = "0.10", default-features = false, features = ["udev", "libinput_1_21"] }`, `udev = "0.9"`, `kdl = { version = "6.7", default-features = false, features = ["span"] }`; run *Install deps*; commit the updated `Cargo.lock` with it; keep the full check passing (nested code untouched)
- [X] T003 Create the module skeletons with module docs only, declared in their parents: `src/config.rs` and `src/watchdog.rs` (in `src/lib.rs`), `src/canvas/layout.rs` (in `src/canvas/mod.rs`, `pub use layout::*`), `src/render/cursor.rs` (in `src/render/mod.rs`), `src/backend/drm/mod.rs` with empty `session.rs`, `gpu.rs`, `device.rs`, `edid.rs`, `surface.rs`, `input.rs`, `keyboard.rs`, `hotplug.rs` and `src/backend/watchdog.rs` (in `src/backend/mod.rs`), `src/ffi/spawn.rs` (in `src/ffi/mod.rs`); `#![allow(unsafe_code)]` only in `src/backend/drm/mod.rs`, `src/backend/watchdog.rs` and `src/ffi/spawn.rs` (Principle V); empty `tests/config.rs` and `tests/protocol/outputs.rs` (declared in `tests/protocol/main.rs`)
- [X] T004 Add `libinput-dev libseat-dev libgbm-dev libudev-dev` to the apt line in `.github/workflows/ci.yml` (S1; covered by the approved plan); keep `libwayland-dev` until US6

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: the backend-neutral types every story uses: relative pointer motion, key times,
the row layout, viewers keyed by id in the shell, the `Bindings` value, the new backend events

**CRITICAL**: no user story work can begin until this phase is complete

- [X] T005 Write failing unit and `proptest` tests in `src/canvas/layout.rs` for `Layout` and `ViewerId(u32)` (S9, data-model "Layout"): entries ordered "internal entries first, then by `connected_seq`" (FR-018); entry `x` is the sum of the widths before it, `y` is 0; the first entry added puts the pointer at its centre (FR-009); `move_by(delta)` keeps the pointer inside one entry, crosses shared edges, and "a point outside every entry moves to the nearest point of the union of the entries' rectangles, so crossing to a shorter entry clamps y to its bottom edge"; `locate(point) -> Option<(ViewerId, Point)>`; `relative_to(id, point)` also outside the entry; `remove(id)` with the pointer on it moves it "to the centre of the first entry", with the pointer on another entry keeps its position on that entry (FR-021); with no entries `locate` is `None`; property: after any sequence of `add`/`remove`/`move_by` with at least one entry, `locate(pointer)` is `Some`
- [X] T006 Write failing unit tests in `src/canvas/layout.rs` for home cameras (FR-020, S14): one entry -> camera centre `(0, 0)`, zoom 1; two entries 1920x1080 + 2560x1440 -> the bounding box (4480 x 1440) centred on the origin, each camera centred on its entry's part of that box (the shorter one's centre higher); `home_for_new(entry)` continues the rightmost entry's *home* view to the right whatever its current camera
- [X] T007 Implement `Layout`, `LayoutEntry { id, size, internal, connected_seq }`, `ViewerId`, `home_cameras()` and `home_for_new()` in `src/canvas/layout.rs` so T005 and T006 pass (no Wayland, GL or device types, Principle IV)
- [X] T008 Change `InputEvent` in `src/input/mod.rs` (data-model "Input"): add `PointerMoved { delta: Vector }` and `time_ms: u32` on `Key`; keep `PointerEnter`/`PointerMotion`/`PointerLeave` as per-viewer events; give `Key` a `time_ms` at every construction site (`src/backend/wayland_host/seat.rs` uses the host's key time; `tests/shell.rs`, `tests/viewer_input.rs`, `tests/protocol/*.rs`) so all existing tests pass unchanged in meaning
- [X] T009 Write failing tests in `tests/shell.rs` for viewers keyed by id (S14, data-model "Shell"): `Shell::new()` has no viewers; `add_viewer(id, size, internal)` gives that viewer its home camera from the layout; `viewer_of(id)`, `viewers()`, `active()` (the viewer under the pointer), `pointer()`; `remove_viewer(id)`; `PointerMoved` is turned into the existing per-viewer enter/motion/leave handling through the layout, so a window under the pointer gets `wl_pointer` enter and motion as before, and a press before any motion still pans; helper `shell_with_viewer()` (one viewer, id 1). Existing tests keep using a viewer-less shell, whose `viewer()`/`viewer_mut()` is a spare viewer driven by absolute positions, so their expectations stay unchanged
- [X] T010 Implement the viewer list in `src/shell/mod.rs`: `viewers: Vec<ViewerSlot { id, viewer }>` (home views live in the layout), a `spare` viewer used while there is none, `layout: Layout`, `active`, `pan_viewer` (a pan keeps the viewer it started on, FR-019); every `self.viewer` use becomes the active viewer; Mod+0 sets the active viewer's home view; `place_home_views()`, `visible_in(id)`, `visible_any()`, `take_dirty_viewers()`, `pointer() -> Option<(ViewerId, Point)>`; absolute `PointerEnter`/`PointerMotion` (nested, tests) keep the layout pointer in step, so the nested `App` needs no change; fullscreen stays one shared value until T070 makes it per viewer; T009 and all existing tests pass
- [X] T011 Write failing unit tests in `src/input/bindings.rs` for the `Bindings` value (S15, data-model "Bindings"): `Bindings::new(mod_key: ModKey, terminal: TerminalBind)` with `ModKey::{Super, Alt, Ctrl}`; the built-in table (Mod+0 ResetView, Mod+Shift+Q Quit, Mod+Q CloseWindow, Mod+Tab NextWindow, Mod+Shift+Tab PreviousWindow) uses the given Mod; `lookup(mods, sym, pressed)` matches only presses with exactly those modifiers; `mod_held(mods)`; the terminal binding returns `Action::SpawnTerminal`
- [X] T012 Implement `Bindings`, `ModKey`, `KeyCombo`, `TerminalBind { combo, command }` and `Action::SpawnTerminal` in `src/input/bindings.rs`; the shell owns a `Bindings` (`Shell::with_bindings(bindings)`; `Shell::new()` uses `Bindings::default()`) and gives every viewer a copy (`Viewer::set_bindings`) instead of the constant table; nested builds it with `ModKey::Alt` so nested behaviour is unchanged; T011 and existing tests pass
- [X] T013 Add the standalone `BackendEvent` variants next to the nested ones in `src/backend/mod.rs` (S2, data-model "Backend events"): `OutputAdded { id: ViewerId, info: OutputInfo }`, `OutputRemoved { id }`, `Presented { id, time_ms }`, `Paused`, `Resumed`; `OutputInfo { name, make, model, physical_mm: (u32, u32), mode: Mode { width, height, refresh_mhz }, internal }`; `App` ignores them for now

**Checkpoint**: foundation ready - user stories can start

---

## Phase 3: User Story 1 - Start on a bare console and get back to it (Priority: P1) MVP

**Goal**: from tty1, RubickWay shows the canvas on the built-in display with keyboard and
touchpad, draws its pointer, and gives the console back on a two-press quit or a signal

**Independent Test**: quickstart Q1-Q4

### Tests for User Story 1 (write first, see them fail)

- [X] T014 [P] [US1] Write failing tests in `tests/config.rs` for the configuration framework and `mod-key` (S16, contracts/config.md): `Config::parse("")` gives the defaults ("`mod_key` `Super`", "`terminal` `Mod+Return`, `["kitty"]`", "`watchdog_timeout` 10 s", xkb "`evdev`, `pc105`, `us`, empty, empty"); `input { mod-key "Alt"; }` and `"alt"` (case-insensitive) give Alt; `mod-key "Hyper"` gives Super and one warning with line 1; a KDL syntax error gives all defaults and one warning naming its line; an unknown top-level node is ignored with a warning; a node given twice keeps the last with a warning; `Config::load(path)` on a missing file gives defaults and no warning, on an unreadable path defaults and one warning without a line; the config path is `$XDG_CONFIG_HOME/rubickway/config.kdl`, else `~/.config/rubickway/config.kdl` "also when `XDG_CONFIG_HOME` is empty or not an absolute path" (FR-024)
- [X] T015 [P] [US1] Write failing tests in `tests/shell.rs` for the two-press quit (FR-031, data-model "QuitConfirm"): Mod+Shift+Q at `t` gives no quit and is not forwarded; a second press at `t + 2000` quits, at `t + 2001` does not (it starts a new pending press); a Shift or Mod press or release in between does not cancel; any other key press, or `FocusLost`, cancels; wrapping `time_ms` near `u32::MAX` works; during a move (window-management) the same rule applies
- [X] T016 [P] [US1] Write failing tests in `tests/keymap.rs` for `Keyboard::from_names(rules, model, layout, variant, options)` (S17, FR-007): empty names compile to a US keymap whose `binding_sym` for evdev `KEY_Q` is `q`; `layout "us,ua"` alone compiles with two layouts and `binding_sym(KEY_Q)` is still `q` (first layout, level 0); `layout "nonexistent"` and a name containing a NUL byte return `None`; the keymap text round-trips. Environment independence is not tested in-process (setting a variable needs `unsafe` in Rust 2024, not allowed in tests); it rests on `CONTEXT_NO_ENVIRONMENT_NAMES` in the code
- [X] T017 [P] [US1] Write failing unit tests in `src/backend/drm/device.rs` for the pure resource logic (S5): `choose_mode(modes)` picks the mode flagged preferred, else the first; `assign_crtcs(connectors, encoders, crtcs)` gives each connected connector a CRTC its encoders allow, never one CRTC twice, connectors in the order given; a connector with no free CRTC is skipped with a reason; `connector_name(type, index)` gives `eDP-1`, `HDMI-A-1`, `DP-2`; `is_internal(type)` true for eDP, LVDS, DSI
- [X] T018 [P] [US1] Write failing unit tests in `src/backend/drm/edid.rs` for `parse(bytes) -> EdidInfo { make, model }` (S13): a valid 128-byte block gives the 3-letter PNP id from bytes 8-9 and the monitor name from the `0xFC` descriptor, trimmed at `0x0A`; a bad header, a short blob, or no name descriptor gives `Unknown` for the missing part; never panics on any input (`proptest` with random bytes)
- [X] T019 [P] [US1] Write failing unit tests in `src/backend/drm/input.rs` for `translate(...)` (S8): pointer motion -> `PointerMoved { delta }` with the accelerated delta; buttons with `seat_button_count` 2 on press are dropped; wheel with v120 -> `Scroll { source: Wheel, value120: Some(..) }`; finger and continuous scroll -> `Scroll` with their deltas and `value120: None`; pinch begin/update/end -> `PinchBegin`, `PinchUpdate { scale }`, `PinchEnd { cancelled }`; keys with `seat_key_count` != 1 on press (or != 0 on release) are dropped; key time in ms is kept in `time_ms`
- [X] T020 [P] [US1] Write failing tests in `src/render/cursor.rs` for the built-in images (S10, data-model "Cursor images"): every `CursorShape` (default, grabbing, move, eight resize directions) maps to an image of 24 x 24 with a hotspot inside it; the ASCII art has only the characters ` `, `#`, `.`; `#` is opaque black, `.` opaque white, ` ` transparent, premultiplied; theme loading from a directory without themes returns `None`; a minimal Xcursor file built in the test (one 24 px image) is parsed with its hotspot
- [X] T021 [P] [US1] Write a failing pixel test in `tests/render_windows.rs` (guarded by `renders_on_llvmpipe`): with a cursor image at pointer position `(100, 50)` the frame has the image's black outline pixels at `(100, 50) - hotspot + offset` above the grid, and above a fullscreen window's content (FR-008); colours from the `render` constants

### Implementation for User Story 1

- [X] T022 [US1] Implement `Config`, `ConfigWarning { line: Option<usize>, message }`, `Config::parse` (walking the `kdl` document by hand, lines from node spans) and `Config::load` with the path rules in `src/config.rs`, for `input > mod-key` and the warning rules of contracts/config.md "Problems"; other settings keep their defaults until US2/US5 so T014 passes
- [X] T023 [US1] Implement the quit confirmation in `src/shell/route.rs` and `src/shell/mod.rs`: `QuitConfirm::{Idle, Pending { since }}` with the transitions in data-model "QuitConfirm"; `Effect.quit` only on the confirming press; window-management's "Mod+Shift+Q quits" during an operation now goes through the same rule so T015 passes
- [X] T024 [US1] Implement `Keyboard::from_names` in `src/input/keyboard.rs` with `xkb::CONTEXT_NO_ENVIRONMENT_NAMES` and per-field defaults (rules `evdev`, model `pc105`, layout `us`) so T016 passes; plus `keymap_text`, `layout_count`, `update_key`, `modified_sym` and `modifiers()` (effective and raw masks) for the DRM keyboard
- [X] T025 [US1] Implement `choose_mode`, `assign_crtcs`, `connector_name`, `is_internal` in `src/backend/drm/device.rs` and `edid::parse` in `src/backend/drm/edid.rs` so T017 and T018 pass (pure: plain structs, no `drm` types in their signatures)
- [X] T026 [US1] Implement `translate` in `src/backend/drm/input.rs` over plain extracted values so T019 passes
- [X] T027 [US1] Implement the built-in cursor images (12 px art, doubled to 24 px) and Xcursor theme loading (theme `default` following `Inherits` in `~/.local/share/icons`, `~/.icons`, `/usr/share/icons`, `/usr/share/pixmaps`; size 24; read at startup only) in `src/render/cursor.rs`, with `CursorImage::quad(kind, pos)` giving the quad that `App` draws last with the existing `draw_windows` (no new GL code), so T020 and T021 pass
- [X] T028 [US1] Implement the session in `src/backend/drm/session.rs` (S3): open the seat with `libseat`, dispatch its fd, wait for the first `enable` for at most 2 s before the loop (else "this console is not active"), `open_device` / `close_device`, `switch_session(vt)`, `enable`/`disable` callbacks recorded as pending events; the three startup messages of contracts/cli.md (no seat, not active, another program controls the display - "busy" when taking the session)
- [X] T029 [US1] Implement GPU selection in `src/backend/drm/gpu.rs` (S4): udev enumeration of `drm` `card*` devices on `seat0`, prefer the PCI parent with `boot_vga=1`, else the first with a connector; error "no graphics device with displays found"
- [X] T030 [US1] Implement the device in `src/backend/drm/device.rs` (S5, S7): open the card through the session, set `UNIVERSAL_PLANES` and `ATOMIC`, read connectors, encoders, CRTCs, planes and properties, build outputs for connected connectors with `choose_mode`/`assign_crtcs`, `OutputInfo` from the connector and `edid::parse` of its `EDID` blob; every commit `NONBLOCK | PAGE_FLIP_EVENT` (modesets with `ALLOW_MODESET`); read page-flip events into `Presented { id, time_ms }`; `EBUSY` leaves the output queued for a retry at the next flip of any output or after 100 ms; another failure marks `needs_modeset` for one retry; `ENODEV` is fatal "lost the graphics device"
- [X] T031 [US1] Implement `Egl::gbm(&gbm::Device)` in `src/render/egl.rs` (`EGL_PLATFORM_GBM_KHR`, config with native visual `XRGB8888`, surfaceless `make_current` when no output exists) and the per-output surface in `src/backend/drm/surface.rs` (S6): `gbm_surface` (`SCANOUT | RENDERING`), EGL window surface, front-buffer lock after swap, DRM framebuffer cache per buffer object, release of the previous buffer at the flip, all `unsafe` blocks with `// SAFETY:` comments
- [X] T032 [US1] Implement the input context in `src/backend/drm/input.rs`: a libinput udev context on `seat0` whose `open_restricted`/`close_restricted` use the session; tap-to-click enabled on touchpads when added; events through `translate`; and the keyboard in `src/backend/drm/keyboard.rs`: xkb state from `Config.xkb` via `Keyboard::from_names` (falling back to the default keymap with a warning), `update_key` per key event, `Modifiers { mods, raw }` events, `Keymap(text)` and `RepeatInfo { rate: 25, delay: 600 }` at startup
- [X] T033 [US1] Implement `DrmBackend` in `src/backend/drm/mod.rs`: owns session, device, surfaces, input and keyboard; registers their fds on the calloop loop; `take_events() -> Vec<BackendEvent>` (`OutputAdded` for each output at startup, `Input`, `Presented`, `Keymap`, `RepeatInfo`); `render_output(id, |gl| ...)` makes the output's surface current, lets `App` draw, swaps and commits
- [X] T034 [US1] Write the standalone `App` and `run()` in a new `src/standalone.rs`, called from `src/main.rs` (the nested `App` stays in `src/state.rs`, compiled but no longer called, until US6): load `Config` before the loop and log its warnings; `Shell::new(Bindings::new(config.mod_key, ..))`; `OutputAdded` -> `shell.add_viewer` + per-output frame state `Idle`/`Queued`/`WaitingForFlip` (data-model "FrameState"); draw every `Queued` output in `before_sleep` (grid or fullscreen, windows, then the pointer if it is on that output, S10); `Presented` -> `Idle` + `server.presented`; the estimated-vblank timer (one refresh interval after the last presentation) when apps wait and nothing is dirty (S7); pointer motion marks the outputs it was and is on dirty; the startup line per display (contracts/cli.md); each output's presentation interval logged at debug level (`frame <name> <ms> ms`), so quickstart Q21 can check SC-005
- [X] T035 [US1] First write a regression test in `tests/cli.rs` for the unchanged command line (FR-027, contracts/cli.md "Invocation"): it runs the built binary (`env!("CARGO_BIN_EXE_rubickway")`) with `foo`, with `--help` and with two arguments, and asserts exit status 2, stderr exactly `usage: rubickway\n`, empty stdout, and that the run returns within 1 s (it must not try to open the seat); it passes against the existing check in `src/main.rs` and keeps guarding it while `main.rs` changes in T063. Then add SIGHUP to the signal source in `src/state.rs` (FR-012, S20); exit statuses per contracts/cli.md; the orderly drop order "outputs off, GBM and EGL objects, devices closed, seat closed" in `Drop for App`, with the final all-off commit allowed to block after the loop has ended
- [X] T036 [US1] (passed 2026-11-03, Q11 after the T090 signal fix) Run quickstart Q1-Q4 and Q11-Q13 on the laptop (manual; the owner runs them on tty1 and reports); fix what fails before moving on

**Checkpoint**: User Story 1 works on the laptop: canvas, pointer, touchpad and keyboard
navigation, two-press quit, console back

---

## Phase 4: User Story 2 - Run applications on the standalone canvas (Priority: P1)

**Goal**: Mod+Return starts kitty; applications see the real display and the configured
keymap; the configuration covers the spawn bindings and the keyboard layout; applications can
hide the pointer

**Changed during implementation (owner, 2026-11-02)**: the single "terminal shortcut" became
any number of spawn bindings (`SpawnBind`, `Action::Spawn(n)`, `Effect.spawn`, `Config.spawns`;
a combination bound twice keeps the first). T037/T038/T042-T044 are done in that form (spec
FR-014, FR-024, FR-025; contracts/config.md)

**Independent Test**: quickstart Q5-Q8, Q21b

### Tests for User Story 2 (write first, see them fail)

- [X] T037 [P] [US2] Write failing tests in `tests/config.rs` for `binds`, `xkb` and key combinations (FR-024, FR-025, contracts/config.md): `binds { Mod+T { spawn "kitty" "--single-instance"; } }` gives that combination and command; `Mod+t` equals `Mod+T`; `binds { F12 { spawn "foot"; } }` (no modifier) is accepted; `Mod+Ctrl+Q` is accepted; `Mod+Q`, `Mod+Shift+Q`, `Mod+0`, `Mod+Tab`, `Mod+Shift+Tab`, `Ctrl+Alt+F3` are rejected; unknown modifier, unknown key, no key, two keys rejected; `binds` with two children uses the first valid one with a warning; a binding without `spawn` or with an empty `spawn` is rejected; `binds {}` gives the default; `input { keyboard { xkb { layout "us,ua"; options "grp:win_space_toggle"; } } }` sets those fields and keeps the others' defaults
- [X] T038 [P] [US2] Write failing tests in `tests/shell.rs`: the terminal combination gives `Effect.spawn_terminal` and is not forwarded with its release; a combination that is not the terminal's reaches the focused window; with `ModKey::Ctrl`, Ctrl+Tab cycles windows
- [X] T039 [P] [US2] Write failing protocol tests in `tests/protocol/outputs.rs` (contracts/protocols.md "wl_output"): after `Server::add_output(id, info, position)` a client binding `wl_output` v4 gets `geometry` (position, physical mm, make, model), `mode` (current + preferred, width, height, refresh mHz), `scale` 1, `name` (e.g. `eDP-1`), `description` `<make> <model> (<name>)`, `done`; `preferred_buffer_scale` is 1 and `wp_fractional_scale` 120; binding the output many times gives the full description each time
- [X] T040 [P] [US2] Write failing tests for hidden pointers (FR-008, contracts/protocols.md "wl_pointer"): in `tests/protocol/input.rs`, `set_cursor` without a surface makes the server report a "cursor hidden" window event for that client's windows, and `set_cursor` with a surface reports "not hidden"; in `tests/shell.rs`, with that event applied, `Shell::cursor()` returns `CursorShape::Hidden` while the pointer is over that window, the gesture's shape during a pan, move or resize, and `Default` over another client's window
- [X] T041 [P] [US2] Write a failing test in `src/ffi/spawn.rs` (unit, headless): spawning `["sh", "-c", "exit 3"]` reports `exited with status 3` back over the channel; spawning a non-existent program reports `cannot start`; spawning `["true"]` reports nothing; afterwards no zombie child of the test process remains (`waitpid(-1, WNOHANG)` returns `ECHILD`); the child's working directory is `$HOME` and it sees `WAYLAND_DISPLAY`

### Implementation for User Story 2

- [X] T042 [US2] Extend `Config::parse` in `src/config.rs` with `input > keyboard > xkb`, `binds` and key-combination parsing (`KeyCombo` per data-model, xkb keysym names with a case-insensitive retry, letters lowered, rejection after resolving `Mod`) so T037 passes
- [X] T043 [US2] Add `Effect.spawn_terminal` and the terminal binding handling in `src/shell/mod.rs` and `src/viewer/mod.rs` so T038 passes
- [X] T044 [US2] Implement the spawner in `src/ffi/spawn.rs` (S18): a short-lived thread per command, `Command` in `$HOME` with `WAYLAND_DISPLAY`, `pre_exec` unblocking all signals and calling `setsid` (`// SAFETY:` comments), the thread waits for the child and sends `SpawnResult` over a calloop channel; `App` in `src/state.rs` hands `config.terminal.command` to it on `Effect.spawn_terminal` and logs results per contracts/cli.md so T041 passes
- [X] T045 [US2] (the nested-style `RUBICKWAY-1` output stays as a placeholder until the first display is added, then is removed; it goes with US6) Implement per-display outputs in `src/server/output.rs` and `src/server/mod.rs`: `add_output(id, info, position)` creates a `wl_output` v4 global with the info of contracts/protocols.md; `preferred_buffer_scale` 1; `App` calls it on `OutputAdded` so T039 passes (single display; removal and re-flow in US3)
- [X] T046 [US2] Record `set_cursor` without a surface as a per-client "hidden" flag in `src/server/seat.rs`, report it to the shell as a window event (`WindowEvent::CursorHidden { client_windows, hidden }` or per window), and return `CursorShape::Hidden` from `Shell::cursor()` per T040; `App` draws no pointer for `Hidden`
- [X] T047 [US2] (covered by the existing `keyboards_get_the_host_keymap_and_repeat_info` protocol test; the DRM keyboard sends its keymap and 25/600 at startup) Send the configured keymap and repeat info to applications (`BackendEvent::Keymap`/`RepeatInfo` from the DRM keyboard reach `Server::set_keymap`/`set_repeat` as before) and check with a protocol test in `tests/protocol/input.rs` that a new client gets `repeat_info(25, 600)`
- [X] T048 [US2] (passed 2026-11-03; Q21b's expectation corrected to kitty's `mouse_hide_wait`) Run quickstart Q5-Q8 and Q21b on the laptop (manual, default configuration, no `LIBGL_ALWAYS_SOFTWARE`); fix what fails

**Checkpoint**: Stories 1 and 2 give a usable single-display desktop on the laptop

---

## Phase 5: User Story 4 - Switch to another console and back (Priority: P2)

**Goal**: Ctrl+Alt+F<n> switches consoles; RubickWay pauses and resumes with everything kept

**Independent Test**: quickstart Q9, Q10, Q20

### Tests for User Story 4 (write first, see them fail)

- [X] T049 [P] [US4] (done with US1, so the first run on tty1 can switch consoles) Write failing unit tests in `src/backend/drm/keyboard.rs` for the console-switch check (S11): with the default keymap, Ctrl+Alt+F1 .. F12 give `Switch(1..12)` for the press and swallow both press and release; F1 alone or Ctrl+F1 give normal key events; switching to the current VT gives nothing (FR-011); with `options "srvrkeys:none"` Ctrl+Alt+F2 is an ordinary key (spec Assumptions)
- [X] T050 [P] [US4] Write failing tests in `tests/shell.rs` for pause and resume (S12, contracts/input.md "Console switching and pause"): `PointerLeave` + `FocusLost` end a pan, a move and a resize where they are and cancel a pending quit; applications get pointer and keyboard leave; after `FocusGained` keyboard focus returns to the focused window and the next `PointerMoved` re-enters where the pointer was
- [X] T051 [P] [US4] (the stale-serial test is in `tests/protocol/wm.rs`; it found that the server kept the last press after its release or a pointer leave, against window-management FR-016 - fixed in `src/server/seat.rs`) Write failing protocol tests in `tests/protocol/misbehaving.rs` (S22, spec Edge Cases "Misbehaving applications"): while the server is paused (`Server::set_paused(true)`) a client committing 100 buffers keeps only the latest content per surface and gets no `frame` done; windows mapped, resized or destroyed while paused are reported correctly after `set_paused(false)`; an `xdg_toplevel.move` with the serial of a press from before the pause is ignored; surfaces get no `wl_surface.leave` while paused

### Implementation for User Story 4

- [X] T052 [US4] (done with US1) Implement the console-switch check in `src/backend/drm/keyboard.rs` (modified keysym in `XF86Switch_VT_1..12`, swallowed press and release, `session.switch_session(vt)`, "switched to console <n>" log) so T049 passes
- [X] T053 [US4] Implement pause and resume in `src/backend/drm/mod.rs` (S12): on `disable` stop committing, suspend libinput, acknowledge, push `Input(PointerLeave)`, `Input(FocusLost)`, `Paused`; on `enable` resume libinput, rescan connectors, mark every output `needs_modeset`, push `Resumed`, `Input(FocusGained)`; a `disable` arriving during startup (after the first access, before the first frame) is handled as an ordinary pause, and T028's 2-second first-access check applies only once (spec Edge Cases "Console switched away during startup"); `App` stops drawing and holds frame callbacks while paused (`Server::set_paused`), and on `Resumed` marks all outputs dirty; the shell side so T050 passes
- [X] T054 [US4] Implement `Server::set_paused` in `src/server/mod.rs` (callbacks held, no leave events, latest content kept) so T051 passes
- [X] T055 [US4] (passed 2026-11-03) Run quickstart Q9, Q10 and Q20 on the laptop (manual); fix what fails

**Checkpoint**: console switching and suspend work without losing anything

---

## Phase 6: User Story 5 - Never lose the machine to a crash (Priority: P2)

**Goal**: every way of ending gives the console back; a frozen RubickWay ends itself

**Independent Test**: quickstart Q11, Q23 and the automated watchdog tests

### Tests for User Story 5 (write first, see them fail)

- [X] T056 [P] [US5] Write failing unit tests in `src/watchdog.rs` for `expired(last_ms, now_ms, timeout) -> bool` (S19): false at exactly `timeout`, true one millisecond later; `Heartbeat` stores and loads monotonic milliseconds
- [X] T057 [P] [US5] Write a failing subprocess test in `tests/watchdog.rs` (S19): the test re-runs its own binary with `RUBICKWAY_WATCHDOG_TEST=1`, which starts the real watchdog thread with a 1-second timeout and never refreshes the heartbeat; the parent asserts exit status 1 within 2 s and the stderr line `rubickway: the event loop did not respond for 1 seconds, exiting` (the variable is read only by this test's helper, never by `rubickway`)
- [X] T058 [P] [US5] Write failing tests in `tests/config.rs` for `watchdog-timeout-sec`: `5` gives 5 s; `0`, `601`, `-1`, `2.5`, `"10"` are rejected with a warning and keep 10 s (FR-024: "whole seconds, 1..=600")
- [X] T059 [P] [US5] Write a failing test in `tests/panic_exit.rs` for a new library function `rubickway::run_reporting_panics(f: impl FnOnce() -> Result<(), AppError>) -> ExitCode` (used by `src/main.rs`): a subprocess (the test binary re-run with an environment variable read only by this test's helper) whose `f` panics reports one stderr line `rubickway: internal error: <message> at <file>:<line>` as its last line and exit status 1 (FR-017, S20)

### Implementation for User Story 5

- [X] T060 [US5] Implement `Heartbeat` and `expired` in `src/watchdog.rs` and the watchdog thread in `src/backend/watchdog.rs` (checks every 250 ms, so a 1-second timeout fires within 2 s; on expiry one line to stderr and `libc::_exit(1)` with a `// SAFETY:` comment) so T056 and T057 pass
- [X] T061 [US5] (done in `src/standalone.rs`, the standalone App's home since T030) Wire the watchdog in `src/state.rs` and `src/main.rs`: started right after the configuration is read; heartbeat refreshed after each startup step, after every loop dispatch (plus a 1-second calloop timer), and once when the loop ends (FR-030); `watchdog-timeout-sec` parsed in `src/config.rs` so T058 passes
- [X] T062 [US5] Add the debug-only freeze trigger (plan Complexity Tracking): under `cfg(debug_assertions)` SIGUSR1 joins the signal source and its handler loops forever; release builds do not handle SIGUSR1
- [X] T063 [US5] Implement `run_reporting_panics` in `src/lib.rs` and call it from `src/main.rs`: a panic hook that records message and location without printing; `catch_unwind` around `run()` so the backend is dropped first; then the one-line message and exit code 1, so T059 passes
- [X] T064 [US5] (OpenGL ES setup failure now reports `cannot use the graphics device <path>: <reason>`; errors are printed after `run()` returned, so the backend was dropped and the console given back first) Make every error path in `src/state.rs` and `src/backend/drm/` end with its contracts/cli.md message as the last line written (FR-013); check the startup failure after the first modeset gives the console back (FR-002)
- [X] T065 [US5] (passed 2026-11-03 after T090: Q23 froze and the watchdog ended it; no automatic restart, by design) Run quickstart Q11 and Q23 on the laptop (manual); fix what fails

**Checkpoint**: no way of ending leaves the laptop without a console

---

## Phase 7: User Story 3 - Every monitor is a viewer (Priority: P2)

**Goal**: every connected display is its own viewer in a row; hotplug adds and removes viewers

**Independent Test**: quickstart Q14-Q19

### Tests for User Story 3 (write first, see them fail)

- [X] T066 [P] [US3] (the single-viewer test helpers `shell()` and `spawn_shell` now use a display-backed viewer of the same size; the nested-only test of an initial state before the first configure became `an_initial_state_follows_a_resized_viewer`) Write failing tests in `tests/shell.rs` for several viewers (FR-018 - FR-023, spec Edge Cases): two viewers side by side; pan, zoom and Mod+0 act only on the viewer under the pointer; a pan started on viewer A keeps acting on A after the pointer crosses to B; a window moved across the edge keeps the grabbed point under the pointer in B's coordinates (it jumps to B's region); new windows open centred in the active viewer; focus cycling to a window fully inside B moves no view, to a window outside every viewer moves the active viewer
- [X] T067 [P] [US3] Write failing tests in `tests/shell.rs` for maximize and fullscreen with several viewers (FR-022, Edge Cases "Fullscreen on one viewer, actions on another"): the window's viewer rule (active viewer if it shows part of the window, else the largest share, else active); fullscreen on A plus fullscreen on B both stay; pan, zoom and Mod+0 on B leave A's fullscreen; focus cycling ends fullscreen only on the active viewer and reaching A's fullscreen window moves no view; a fullscreen request naming B's output uses B; maximize moves only that viewer's camera; window-management's new-window-during-fullscreen rule applies only when the active viewer has a fullscreen window
- [X] T068 [P] [US3] Write failing tests in `tests/shell.rs` for viewer removal and zero viewers (FR-021, Edge Cases "A viewer removed during an operation", "No display at all"): removing a viewer with a fullscreen window restores the window's previous rect; a maximized window stays; a pan, move or resize involving it ends with the window where it was; the pointer moves to the centre of the leftmost viewer; with no viewers new windows open centred on the origin, focus cycling moves no view, maximize and fullscreen requests are ignored, `visible_any()` is empty
- [X] T069 [P] [US3] (plus subsurfaces entering and leaving with their window, contracts/protocols.md "wl_surface") Write failing protocol tests in `tests/protocol/outputs.rs`: `remove_output(id)` removes the global; a client still holding that `wl_output` may use it without error, and `set_fullscreen` with it counts as no output; `set_window_outputs(window, &[ids])` sends `enter`/`leave` diffs; a re-flow sends a new `geometry` with the position, then `done`; a frame callback of a window on two outputs completes on the first presentation of either output after it was requested

### Implementation for User Story 3

- [X] T070 [US3] (`Shell::remove_viewer` returns the pointer leave for applications; until US6 the nested App registers its host window as viewer 1) Implement the several-viewer rules in `src/shell/mod.rs` and `src/shell/route.rs` (S14) so T066, T067 and T068 pass; the server reports a fullscreen request's output as `Option<ViewerId>` in `WindowEvent::FullscreenRequested`
- [X] T071 [US3] (`App` passes every window's displays each step from `Shell::visible_in`; the server sends only the changes) Implement `remove_output`, `set_window_outputs`, geometry updates on re-flow and inert removed outputs in `src/server/output.rs` and `src/server/mod.rs`; `App` computes each window's viewers from `Shell::window_viewers` once per frame and passes diffs, so T069 passes
- [X] T072 [US3] (a refused non-blocking off commit, a flip still pending, is repeated blocking; the rescan also runs on resume) Implement hotplug in `src/backend/drm/hotplug.rs` and `src/backend/drm/device.rs` (S13): a udev monitor on `drm`, a connector rescan per change event compared with the current outputs, `OutputAdded` with a new never-reused id and its first modeset, `OutputRemoved` after a non-blocking off commit; startup outputs in the card's connector order (FR-018)
- [X] T073 [US3] (done in `src/standalone.rs`; a connected display logs the startup line, a disconnected one `display <name> disconnected`, contracts/cli.md) Handle `OutputAdded`/`OutputRemoved` in `src/state.rs`: per-output frame state, surface and wl_output created and destroyed; `shell.add_viewer`/`remove_viewer`; the pointer drawn on whichever output it is on; "display <name> connected/disconnected" log lines
- [X] T074 [US3] (owner, 2026-11-03/04, at home with the HDMI monitor: works; not every step was run. Follow-up idea from it: fullscreen per monitor, a later feature) Run quickstart Q14-Q19 on the laptop with the HDMI monitor (manual); fix what fails

**Checkpoint**: the laptop display and an HDMI monitor are two viewers of one canvas

---

## Phase 8: User Story 6 - Nested mode is gone (Priority: P3)

**Goal**: only the standalone backend remains; tests still run headless

**Independent Test**: full check; quickstart Q22 where a desktop is available

- [X] T075 [US6] (also removed: the placeholder `wl_output` `RUBICKWAY-1`, so no display means no output; `Viewer::new()` starts 1 x 1 until its display sets the size. Nested-only tests removed: `zero_configure_sizes_fall_back_to_the_default`, `buffer_is_scaled_down_uniformly_to_the_gpu_maximum`, the placeholder parts of `output_reports_the_viewer_and_follows_scale_changes`, now `surfaces_follow_scale_changes`) Remove `src/backend/wayland_host/` (approved by the plan, S21), the nested `App` paths and host cursor mapping in `src/state.rs`, `BackendEvent::{Configure, Scale, FrameDone, CloseRequested}` in `src/backend/mod.rs`, `Egl::wayland` in `src/render/egl.rs`, `DEFAULT_SIZE`, the zero-size configure fallback and the buffer downscaling (`max_buffer_side`, `render_scale` below the scale) in `src/viewer/mod.rs`, `disable_vsync` in `src/render/egl.rs`; `Bindings::default()` becomes Mod Super with the tests updated; doc comments that still say "host" in `src/server/`, `src/input/`, `src/shell/` and `src/viewer/` are reworded for the hardware (research S23); tests that only covered nested behaviour are removed with a note in the commit body, every other test keeps passing
- [X] T076 [US6] (the binary links no libwayland; CI still to be confirmed once the branch is pushed) Update `Cargo.toml` (remove `wayland-egl` and `wayland-backend`'s `client_system` feature; move `wayland-client` to `[dev-dependencies]`) and `Cargo.lock`; drop `libwayland-dev` from `.github/workflows/ci.yml` (S1) and confirm CI passes on the branch
- [X] T077 [US6] (Q22 not run: no machine with a desktop here) Write a test in `src/backend/drm/session.rs` that the "busy" error from taking the session maps to `rubickway: another program controls the display; start RubickWay from a text console, not inside a desktop` (contracts/cli.md); run quickstart Q22 if a machine with a desktop is available

**Checkpoint**: one backend, one mode

---

## Phase 8b: Pointer bindings (owner's request, 2026-11-03; FR-032, research S24)

**Goal**: the Mod pointer gestures are configurable in `binds`; defaults unchanged

**Independent Test**: quickstart Q24

- [X] T083 [P] [US2] Write failing unit tests in `src/input/bindings.rs` for the pointer table: the defaults with each `ModKey` (Mod + left pan, Mod+Shift + left move, Mod + right and Mod+Shift + right resize), exact matching (Mod+Ctrl + left matches nothing), a configured table replacing the defaults
- [X] T084 [P] [US2] Write failing tests in `tests/config.rs` (contracts/config.md "Pointer combinations"): `Mod+MouseLeft { move-window; }` and `Mod+Shift+MouseLeft { pan-canvas; }` parse and replace the four pointer defaults while Mod+Return stays; a spawn binding alone keeps the pointer defaults; rejected with one warning each: `MouseLeft` without a modifier, `Mod+MouseFoo`, `Mod+MouseLeft { spawn "kitty"; }`, `Mod+T { move-window; }`, `Mod+MouseLeft { pan-canvas 1; }`, the same pointer combination twice (the first stays)
- [X] T085 [P] [US2] Write failing tests in `tests/shell.rs` with a shell built from custom bindings: Mod + left moves a window (focus and raise) and Mod+Shift + left pans without focus; `pan-canvas` on the middle button pans and ends only on the middle release; an unbound Mod + right press on a window reaches the window; `resize-window` over the background does nothing; `pan-canvas` with a fullscreen window on the active viewer leaves fullscreen
- [X] T086 [US2] (during a pan, presses of other buttons are swallowed, as during a move or resize) Implement `PointerAction`, `PointerCombo`, `PointerBind`, the default pointer table and `Bindings::pointer_lookup`/`with_pointer` in `src/input/bindings.rs`; parse pointer combinations and actions in `src/config.rs` (`Config::pointer`, defaults replaced independently of spawns); route presses through the pointer table in `src/shell/mod.rs`, with `Viewer::begin_pan`/`end_pan` in `src/viewer/mod.rs` and the pan button kept in `src/shell/route.rs`, so T083-T085 pass and every existing test keeps passing
- [X] T088 [US2] Swap the left-button defaults (owner, 2026-11-03): Mod + left `move-window`, Mod+Shift + left `pan-canvas`, in `src/input/bindings.rs` and `src/config.rs`; update spec FR-032, contracts, research S24, quickstart Q6/Q24, README, and the tests that used the old defaults; log the touchpad's tap finger count in `src/backend/drm/input.rs` (three-finger tap = middle only if the pad reports three fingers)
- [X] T089 [US2] Remove the keyboard layout setting (owner, 2026-11-03; input languages are a later feature): spec FR-007/FR-024 and Session 2026-11-03, contracts, data-model, research S17, plan, quickstart Q7, README; `Config::xkb`/`XkbNames` and the `input > keyboard` parsing in `src/config.rs`, with its tests (none replaces them, as if the setting never existed); `Keyboard::from_names` -> `Keyboard::us` in `src/input/keyboard.rs`; `DrmKeyboard::new()` without names in `src/backend/drm/keyboard.rs`; tests in `tests/config.rs`, `tests/keymap.rs`
- [X] T087 [US2] (README done; Q24 and the Q6 defaults passed on the laptop, 2026-11-03; the touchpad reports 3 tap fingers) Update README (Configuration, Controls) and run quickstart Q24 on the laptop (manual)

- [X] T090 [US5] Fix signals ending RubickWay without an orderly exit (Q23: `pkill -USR1` killed a debug build at once): `src/ffi/signals.rs` blocks the handled signals for the calling thread, with a unit test that a thread started afterwards has them blocked; `run` in `src/standalone.rs` calls it before starting the watchdog (research S20)

---

## Phase 9: Polish & Cross-Cutting Concerns

- [X] T078 [P] Rewrite `README.md` (plan "Supporting changes"): status line, prerequisites (`libinput`, `seatd`, `kitty`, a scalable monospace font such as `ttf-dejavu`; no `LIBGL_ALWAYS_SOFTWARE`), starting from a console, the configuration file with the example from contracts/config.md, controls with Super as Mod, Mod+Return, Ctrl+Alt+F<n>, the two-press quit; nested-mode text removed
- [X] T079 [P] Add a *Gotchas* line to `AGENTS.md` for the hardware layer if implementation found something every agent would otherwise rediscover (for example running hardware checks only through quickstart.md); skip if nothing
- [X] T080 (2026-11-03: CPU numbers in S10, no tearing seen; the frame-time log was not kept) Run quickstart Q21 (frame-time log, idle CPU, pointer-move baseline) and record the baseline numbers in `specs/20261102-143401-drm-backend/research.md` S10
- [X] T081 Remove the Sync Impact Report comment at the top of `.specify/memory/constitution.md` before the constitution change is committed
- [X] T082 (2026-11-04: full check passes, 427 tests in debug and release; the quickstart was run again by the owner except Q22 (no desktop machine) and some Q14-Q19 steps; spec, research and contracts were updated with every deviation, Phases 10-11) Run the full check from `AGENTS.md` and the whole quickstart once more; update spec, plan or contracts first wherever implementation had to deviate

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 first (the owner installs packages); T002-T004 after it
- **Foundational (Phase 2)**: after Setup; blocks every story
- **US1 (Phase 3)**: after Foundational - the MVP
- **US2 (Phase 4)**: after US1 (needs the DRM backend and `App` on it)
- **US4 (Phase 5)** and **US5 (Phase 6)**: after US1; independent of each other and of US2
- **US3 (Phase 7)**: after US1 and US2 (needs per-display outputs from T045)
- **US6 (Phase 8)**: after US1-US5 and US3 (nested is removed last)
- **Polish (Phase 9)**: after everything

### Within Each Story

- Test tasks before their implementation tasks, seen failing first
- Pure logic (config, layout, shell, edid, translate) before the hardware layer that uses it
- The manual quickstart task closes each story

### Parallel Opportunities

- Setup: T003 and T004 in parallel after T002
- Foundational: T005/T006 (layout), T008 (input), T011 (bindings) in parallel; T009/T010 after T007 and T008
- US1: T014-T021 (all tests, different files) in parallel; T022, T024, T025, T026, T027 in parallel; T028-T033 mostly sequential (shared backend)
- US2: T037-T041 in parallel; T042-T046 touch different files and can run in parallel
- US4 and US5 can be worked on in parallel after US1

## Parallel Example: User Story 1 tests

```text
T014 tests/config.rs          T015 tests/shell.rs           T016 tests/keymap.rs
T017 src/backend/drm/device.rs T018 src/backend/drm/edid.rs T019 src/backend/drm/input.rs
T020 src/render/cursor.rs     T021 tests/render_windows.rs
```

## Implementation Strategy

### MVP First (User Story 1)

1. Setup + Foundational
2. US1: the canvas on the built-in display with the two-press quit - validate with Q1-Q4
3. Stop and let the owner try it on tty1

### Incremental Delivery

1. US2: applications -> a usable desktop on one display
2. US4 + US5: console switching and crash safety (development on the only screen gets safe)
3. US3: the HDMI monitor as a second viewer
4. US6: nested mode removed
5. Polish: README and baseline numbers

Each story is committed separately once the full check passes (AGENTS.md "Git").

## Phase 10: Convergence

- [X] T091 CRITICAL: turn a removed display off without a blocking commit on the event loop in `src/backend/drm/mod.rs` (`remove_gone`): when the non-blocking off-commit is refused because a flip is pending, keep the output in a turning-off state and retry the non-blocking commit after its page-flip event, never `device::disable(.., true)` while running per Constitution VII and research S5 (contradicts)
- [X] T092 Keep windows' outputs unchanged while paused: skip `update_window_outputs` in `src/standalone.rs` (or make `Server::set_window_outputs` a no-op) while paused and bring them up to date on resume, with a protocol test in `tests/protocol/outputs.rs` that a window resized while paused gets no `leave` per FR-015 (partial)
- [X] T093 Start the pointer at the centre of the leftmost display: after the displays present at startup are added (`place_home_views` in `src/shell/mod.rs`, or a `Layout` method in `src/canvas/layout.rs`), put the pointer at the centre of the first entry in row order, with a unit test adding an external display before the built-in one per FR-009 and FR-018 (partial)
- [X] T094 Make the active viewer follow the pointer when a pan ends: in `end_pan` (`src/shell/mod.rs`) set `active` from `layout.locate(pointer)` before re-targeting, so Mod+0, zoom and clicks right after a pan that crossed displays act on the viewer under the pointer, with a test in `tests/shell.rs` per FR-019 (partial)
- [X] T095 (a later `wl_output` bind was already covered) Send `wl_surface.enter` for the displays a window is on to a subsurface added after the window entered them (and to a `wl_output` bound later), in `src/server/subcompositor.rs` / `src/server/output.rs`, with a protocol test per FR-015 (partial)
- [X] T096 (refined: a display whose retry fails too stays dark and is reported removed; the device counts as lost only when no display is left; research S5 and contracts/cli.md updated) Retry a failed commit (other than `EBUSY`/`ENODEV`) once as a full modeset and end with `rubickway: lost the graphics device: <reason>` when that retry fails too, in `src/backend/drm/mod.rs`, with the decision in a pure function and a unit test, per research S5 and contracts/cli.md (partial)
- [X] T097 (libseat reports `ENOSYS` for every failure; the desktop is recognised by `XDG_SESSION_TYPE` or `WAYLAND_DISPLAY`; research S3, contracts/cli.md; Q22 checks it for real) Find out which errno `libseat_open_seat` gives when another program already controls the seat (inside a desktop) and make `src/backend/drm/session.rs` print `rubickway: another program controls the display ...` in that case (for example by checking the logind session's controller, or `WAYLAND_DISPLAY`/`DISPLAY` set when the seat cannot be opened), with unit tests for the decision, per US6/AC1 and FR-002 (contradicts)
- [X] T098 Count lines in configuration warnings by bytes (KDL spans are byte offsets) in `Parser::line_of` in `src/config.rs`, with a test in `tests/config.rs` whose file has Cyrillic text before the bad line, per FR-026 and SC-010 (partial)
- [X] T099 (removed: the viewer's key and button handling with its four tests (quit and Mod+0 are the shell's, tested there), `viewer_configured` (its three tests now call `add_viewer`), `window_viewers`, `DrmBackend::is_active`, the `client` feature outside `[dev-dependencies]`; kept, as the tests' view into real state: `Shell::visible`, `view_rect`, `fullscreen`, `take_dirty`, and `Keyboard::from_keymap_string`, `update_mask`, `layout_count`, which test first-layout shortcut matching across layouts that the input-languages feature builds on) Remove code that only tests reach, or record why it stays, per FR-001: the `Viewer`'s own key and button handling and `camera.reset` path (`src/viewer/mod.rs`, `tests/viewer_input.rs`), `Shell::viewer_configured`, `visible`, `view_rect`, `fullscreen`, `take_dirty` and the unused `window_viewers` (`src/shell/mod.rs`), `Keyboard::from_keymap_string`, `update_mask`, `layout_count` (`src/input/keyboard.rs`, `tests/keymap.rs`), `DrmBackend::is_active`, and the `client` feature of `wayland-protocols` in `[dependencies]` (move it to `[dev-dependencies]`) (unrequested)
- [X] T100 Apply only the last of a top-level node given twice (`binds`, `input`, as the warning says) in `src/config.rs`, with a test in `tests/config.rs`, per FR-026 and contracts/config.md (contradicts)
- [X] T101 Reject `spawn ""` (an empty program) and a binding with more than one `spawn` with one warning each in `src/config.rs`, with tests in `tests/config.rs`, per spec Edge Cases ("An empty command is rejected") and contracts/config.md (`binds > <key combo>`: one child `spawn`) (partial)
- [X] T102 (`Server::set_viewer` became `set_scale`; outputs and surfaces hear only about a scale change) Stop re-sending every `wl_output` description when the active viewer changes: drop the nested-era `Server::set_viewer` buffer size from `src/server/mod.rs` and its call in `src/standalone.rs` (scale stays 1), so outputs are re-sent only when the row re-flows, per FR-015 and contracts/protocols.md (unrequested)
- [X] T103 (`Server::take_damage`: windows with new content, or everything when a window or subsurface came or went; a display redraws for a window it shows now or showed one step ago) Mark only the displays showing a changed window dirty (not every display) in `src/standalone.rs` `mark_dirty`, per FR-010 and research S7 (partial)
- [X] T104 (10 failed reads in a row: `cannot read input devices` or the new `lost the login session`, contracts/cli.md) Make a read error on the seat or libinput fd that repeats fatal (or remove the source) instead of only logging it from a level-triggered source, in `src/backend/drm/session.rs`, `src/backend/drm/input.rs` and `src/standalone.rs`, per FR-012 ("event loop failure") (partial)
- [X] T105 Dispatch the session once right after its fd source is registered in `src/standalone.rs`, so an enable or disable read by libseat during startup is handled at once, per spec Edge Cases ("console switched away during startup") (partial)
- [X] T106 (also spec FR-028 and the touchpad log line) Update the docs to the code: contracts/cli.md log list (`display <name> (<make> <model>) <w>x<h>@<hz> Hz` rather than `display <name> connected`, plus `starting <command>`), and research S18 / FR-028 noting that spawned programs get `/dev/null` as stdin, per FR-028 and contracts/cli.md (partial)

## Phase 11: Convergence

- [X] T107 (`ServerState::scales` is now the constant 1 / 120; the viewer's own scale stays, the renderer reads it) Remove the nested-era scale-change path that only tests reach since T102: `Server::set_scale`, `output::announce_all`, `compositor::announce_scale_all` and the `scale` field in `src/server/` (applications are told scale 1 when they bind or create a surface), with `surfaces_follow_scale_changes` (tests/protocol/surfaces.rs) and `outputs_are_described_again_only_when_the_scale_changes` (tests/protocol/outputs.rs); keep `surfaces_prefer_scale_1_on_the_hardware`, per FR-001 (unrequested)
- [X] T108 Send `wl_surface.leave` for the window's displays to a subsurface's tree when its `wl_subsurface` is destroyed (`src/server/subcompositor.rs`, using `output.rs` like `enter_subtree`), so a re-made subsurface never gets two `enter`s in a row, with a protocol test in `tests/protocol/outputs.rs`, per FR-015 (missing)
- [X] T109 (the warning itself says when it is tried again) Turn a display whose modeset retry failed off with a non-blocking commit before dropping it (`CommitNext::GiveUp` in `src/backend/drm/mod.rs`), and state in research S5 and contracts/cli.md that it is tried again on the next display hotplug event or resume, per research S5 and T096 (partial)
- [X] T110 Do not add a connector whose old output still waits in `retiring` (`add_outputs` in `src/backend/drm/mod.rs`), and rescan with `add_outputs` once `retire` has turned it off, so a quick unplug and replug is never turned off by the late off-commit, per research S5 and T091 (contradicts)
- [X] T111 (`Effect` moved to `shell`, the only producer) Remove what nothing outside tests uses after T099: make `Viewer::handle` return nothing (its `Effect` is always empty; the shell builds its own) in `src/viewer/mod.rs` and `src/shell/mod.rs`, and remove `Camera::reset` with its unit test in `src/canvas/camera.rs`, per FR-001 (unrequested)
