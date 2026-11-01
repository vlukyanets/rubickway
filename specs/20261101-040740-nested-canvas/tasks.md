---

description: "Task list for the nested canvas viewer"
---

# Tasks: Nested Canvas Viewer

**Input**: Design documents from `specs/20261101-040740-nested-canvas/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Required by the constitution (Principle III): every user story has test tasks that
are written and seen failing before its implementation tasks.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested
on its own.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1, US2, US3)
- References like (R4) point to research.md; (FR-xxx) and (SC-xxx) to spec.md

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: dependencies, CI and module skeleton

- [X] T001 Add the dependencies from research R11 to `Cargo.toml`: `wayland-client = "0.31"`, `wayland-backend = { version = "0.3", features = ["client_system"] }`, `wayland-protocols = { version = "0.32", features = ["client", "staging", "unstable"] }`, `wayland-egl = "0.32"`, `khronos-egl = { version = "6", features = ["static"] }`, `glow = "0.18"`, `xkbcommon = "0.9"`, `calloop = { version = "0.14", features = ["signals"] }`, `tracing = "0.1"`, `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }`, `thiserror = "2"`; dev: `proptest = "1"`, `png = "0.18"`; run *Install deps* and the quick check from `AGENTS.md`
- [X] T002 [P] Update `.github/workflows/ci.yml`: before the full check run `sudo apt-get update && sudo apt-get install -y libwayland-dev libxkbcommon-dev libegl-dev libgles-dev libegl-mesa0 xkb-data`; after it add an `actions/upload-artifact` step (latest major, check its releases) with `if: failure()` uploading `target/golden-failures/` (R9)
- [X] T003 [P] Update `README.md` prerequisites with the system libraries from quickstart.md, and replace the "no variables" line in `.env.example` with a commented `RUST_LOG=rubickway=info` entry (contracts/cli.md)
- [X] T004 Create the module skeleton from plan.md: declare `pub mod canvas; pub mod input; pub mod viewer; pub mod render; pub mod backend; pub mod state;` in `src/lib.rs` with empty `mod.rs` files under `src/canvas/`, `src/input/`, `src/viewer/`, `src/render/`, `src/backend/wayland_host/`, and `src/state.rs`; keep the full check passing

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: plain data types every story uses (Principle IV)

**CRITICAL**: no user story work can begin until this phase is complete

- [X] T005 [P] Write failing unit tests in `src/canvas/geometry.rs` for `Point`, `Vector`, `Size` arithmetic (`Point + Vector`, `Point - Point = Vector`, `Vector * f64`, `Vector / f64`, `Size::center()`)
- [X] T006 [P] Write failing unit tests in `src/canvas/camera.rs` for `Camera::default()` (`center = (0, 0)`, `zoom = 1.0`, FR-003) and the conversions `to_canvas(p, size) = center + (p - size/2) / zoom` and `to_logical(p, size) = (p - center) * zoom + size/2`, including round trips at zoom 0.1, 1 and 10
- [X] T007 Implement `Point`, `Vector`, `Size` (`f64` fields; `Size` with `w, h >= 0`) in `src/canvas/geometry.rs` and re-export from `src/canvas/mod.rs`
- [X] T008 Implement `Camera { center: Point, zoom: f64 }` with `ZOOM_MIN = 0.1`, `ZOOM_MAX = 10.0`, `Default`, `to_canvas`, `to_logical` in `src/canvas/camera.rs`
- [X] T009 [P] Define the backend-neutral input types from data-model.md in `src/input/mod.rs`: `InputEvent` (`PointerEnter { pos }`, `PointerMotion { pos }`, `PointerLeave`, `PointerButton { button, pressed }`, `Scroll { source, delta, value120 }`, `PinchBegin`, `PinchUpdate { scale }`, `PinchEnd { cancelled }`, `Key { binding_sym, pressed }`, `Modifiers { mods }`, `FocusLost`), `Button { Left, Right, Middle, Other(u32) }`, `ScrollSource { Wheel, Finger, Continuous }`, `Modifiers { ctrl, alt, shift, logo }` ("locked mods excluded")
- [X] T010 [P] Define `BackendEvent` in `src/backend/mod.rs`: `Input(InputEvent)`, `Configure { size: Size }`, `Scale(f64)`, `FrameDone`, `CloseRequested`

**Checkpoint**: foundation ready - user stories can start

---

## Phase 3: User Story 1 - See the canvas in a nested viewer (Priority: P1) - MVP

**Goal**: `rubickway` opens one window in the host session showing the grid centred on the
origin at 100%; resizing keeps the centre; window close and Mod+Shift+Q exit with status 0;
no session -> message and status 1.

**Independent Test**: start, see the grid, resize, close; exit status 0 (quickstart M1, M2,
M13-M18).

### Tests for User Story 1 (write first, see them fail)

- [X] T011 [P] [US1] Write failing unit tests in `src/render/grid.rs` for `GridParams::new(&Camera, size, scale)`: minor spacing is the smallest `8 * 4^k` canvas units with `8 * 4^k * zoom >= 12` (k = 2 at zoom 0.1, k = -1 at zoom 10), `major_px = 4 * minor_px`, `minor_px >= 12 * scale`, `offset_px` within `[0, major_px)` and computed in f64 (identical fractional result at centre `(1e6, -1e6)` and at the equivalent centre near the origin), `minor_alpha` 0 at 12 and 1 at 24 logical px spacing, `axis_px` `Some` only when the axis is on screen (FR-004, FR-016, FR-017)
- [X] T012 [P] [US1] Write the reference-image harness in `tests/common/golden.rs`: compare an RGBA buffer with `tests/golden/<name>.png` (each channel within 2, at most 0.1% of pixels differ), on mismatch write `target/golden-failures/<name>-actual.png` and `<name>-diff.png`, with `RUBICKWAY_BLESS=1` write the reference instead, and fail with a message naming `RUBICKWAY_BLESS=1` when the reference is missing (R9)
- [X] T013 [US1] Write failing reference-image tests in `tests/render_golden.rs` using `rubickway::render` offscreen rendering on the llvmpipe device, 640x400 logical: `origin_100` (default camera, scale 1), `zoom_10` (zoom 0.1), `zoom_1000` (zoom 10), `far_away` (centre `(1e6 + 37.5, -1e6 - 12.25)`, zoom 1.7), `scale_1_5` (default camera, scale 1.5, buffer 960x600) (FR-002, FR-004, FR-016, FR-017)
- [X] T014 [P] [US1] Write failing viewer tests in `tests/viewer_input.rs`: a new `Viewer` has the default camera and is dirty; `Configure` with a new size keeps `center` and `zoom` and marks dirty (FR-012); a zero width or height is replaced by the default 1280x800; `Scale` must be finite and `> 0` (otherwise ignored); `buffer_size()` = `(round(w * scale), round(h * scale))`, each at least 1 (tiny window edge case)
- [X] T015 [P] [US1] Write failing keymap tests in `tests/keymap.rs`: compile keymaps from RMLVO names (`us` and `us,ua` with the `ua` group active) with xkbcommon; pressing Q with Alt and Shift held yields `binding_sym` = `q` (layout 0, level 0) and `Modifiers { alt, shift }`; Caps Lock / Num Lock never appear in `Modifiers`; `bindings::lookup` maps Mod+Shift+Q to `Action::Quit` and ignores releases and inexact modifier sets (contracts/input.md)
- [X] T016 [US1] Write failing viewer tests in `tests/viewer_input.rs` for quit: `Modifiers { alt, shift }` then `Key { q, pressed: true }` gives `Effect { quit: true }`; the release gives no effect; a second press gives `quit` again (quitting only once is the job of `App`, checked by quickstart M14) (FR-010, edge case "shortcut held down")

### Implementation for User Story 1

- [X] T017 [P] [US1] Implement `GridParams` in `src/render/grid.rs` per data-model.md ("minor spacing ... smallest `8 * 4^k` (integer k) with `8 * 4^k * zoom >= 12`", "`offset_px` ... reduced to `[0, major_px)` in f64 first") with the colours from R6 as constants
- [X] T018 [US1] Implement `src/render/egl.rs` (`#![allow(unsafe_code)]`, `// SAFETY:` on each block): `Egl::software_device()` choosing the device with `EGL_MESA_device_software` via `EGL_EXT_device_enumeration` / `EGL_PLATFORM_DEVICE_EXT` (fallback: surfaceless platform), `Egl::wayland(display_ptr)` via `EGL_PLATFORM_WAYLAND_KHR`, a GLES 3.0 context, window surface creation from a `WlEglSurface`, `eglSwapInterval(0)` (R4, R9); errors as a `thiserror` enum
- [X] T019 [US1] Implement `src/render/gles.rs` and `src/render/shaders/grid.vert`, `grid.frag` (`#![allow(unsafe_code)]`): `GlesRenderer::new(&glow::Context)` compiling the grid program, `draw(&GridParams, buffer_size)` drawing one full-screen triangle with 1-physical-pixel anti-aliased lines, flipping y so row 0 is the top (R5); plus `render::offscreen(params, buffer_size) -> Vec<u8>` (RGBA, top row first) on the software device for tests
- [X] T020 [US1] Run `tests/render_golden.rs` with `RUBICKWAY_BLESS=1`, inspect each PNG in `tests/golden/` (axes at centre, sharp 1-px lines, no solid fill at 10%, lines visible at 1000%, even spacing far away), then run without the variable and confirm all pass
- [X] T021 [US1] Implement `src/input/keyboard.rs`: `Keyboard::from_keymap_string(&str)` with `Keymap::new_from_string`, `update_mask(depressed, latched, locked, group) -> Modifiers` (locked excluded), `binding_sym(keycode) -> Keysym` from layout 0 level 0 (R7)
- [X] T022 [US1] Implement `src/input/bindings.rs`: `Action { ResetView, Quit }`, `Binding { mods, sym, action }`, the fixed table with Mod+Shift+Q -> `Quit` (`Mod` = Alt), `lookup(mods, sym, pressed) -> Option<Action>` matching exact modifiers on press only
- [X] T023 [US1] Implement `Viewer` and `Effect { quit }` in `src/viewer/mod.rs` (redraws only through `dirty` / `take_dirty()`): fields from data-model.md, `handle(&InputEvent) -> Effect` for `Modifiers` and `Key` (via `bindings::lookup`), `configure(size)`, `set_scale(scale)` with validation, `buffer_size()`, `grid_params()`, `take_dirty()`
- [X] T024 [US1] Implement `src/backend/wayland_host/mod.rs`: connect (error text from contracts/cli.md), bind globals with the version ranges in R2 (missing `wl_compositor` or `xdg_wm_base` -> "the Wayland session does not provide <global>"), answer `xdg_wm_base.ping`, and a `calloop::generic::Generic` source on the connection fd that flushes and `dispatch_pending`s before sleeping and does `prepare_read` -> `read` -> `dispatch_pending` when readable (R3); events are queued as `BackendEvent`s; read errors become "lost the connection" errors (FR-014)
- [X] T025 [US1] Implement `src/backend/wayland_host/window.rs`: `wl_surface` + `xdg_surface` + `xdg_toplevel` (title `RubickWay`, app id `rubickway`), server-side decoration request when available, `configure` -> ack + `BackendEvent::Configure` (0 sizes passed through for the viewer to replace), `close` -> `CloseRequested`, `wp_fractional_scale_v1` + `wp_viewporter` (fallback `preferred_buffer_scale`, else 1) -> `Scale`, `wl_egl_window` resize to `buffer_size()` with viewport destination = logical size, `wl_surface.frame` -> `FrameDone` (FR-011, FR-017)
- [X] T026 [US1] Implement the keyboard half of `src/backend/wayland_host/seat.rs`: bind `wl_seat` (v5..=8), `wl_keyboard.keymap` read with `FileExt::read_exact_at` into a string (R7), `key` -> `InputEvent::Key` with `binding_sym` from `input::keyboard::Keyboard` (T021; keycode + 8 for xkb), `modifiers` -> `InputEvent::Modifiers`, `leave` -> `FocusLost`; ignore `repeat_info`
- [X] T027 [US1] Implement `src/state.rs`: `App` owning the host backend, EGL/renderer and `Viewer`; frame scheduling from data-model.md ("ready + dirty -> render, request frame callback, swap -> waiting"); `CloseRequested`, `Effect.quit`, SIGINT and SIGTERM end the loop with success; host errors end it with failure (FR-011, FR-014, FR-015)
- [X] T028 [US1] Rewrite `src/main.rs` and remove the placeholder `version()` and its test from `src/lib.rs`: unknown arguments -> `usage: rubickway`, exit 2; `tracing-subscriber` with `RUST_LOG` (default `rubickway=info`) to stderr; run `App`; print errors as `rubickway: <message>` and exit 1 (contracts/cli.md, FR-013)
- [X] T029 [US1] Run the full check, then run quickstart M1, M2, M14-M16 and M18 on the author's niri session and note results (2026-11-01: all steps passed - agent-run and confirmed by the author on niri)

**Checkpoint**: User Story 1 is functional and testable on its own (MVP)

---

## Phase 4: User Story 2 - Pan and zoom around the canvas (Priority: P2)

**Goal**: drag pans with the grabbed point under the pointer; wheel, Mod+scroll and pinch zoom
around the pointer within 10%..1000%; two-finger scroll pans.

**Independent Test**: drag and scroll in the viewer; the grabbed point and the point under the
pointer stay put (quickstart M3-M8, M10, M19).

### Tests for User Story 2 (write first, see them fail)

- [X] T030 [P] [US2] Write failing unit and `proptest` property tests in `src/canvas/camera.rs`: after `pan_anchor(anchor, p, size)`, `to_logical(anchor, size) == p` within 1e-6 (SC-002); after `zoom_at(f, p, size)` not at a limit, `to_canvas(p, size)` is unchanged within 1e-6 (SC-003); zoom is clamped to `[0.1, 10.0]` and at a limit both zoom and centre stay unchanged (FR-007); `pan_by(d)` moves the centre by `-d / zoom`; all of these hold with centres up to 1e6 from the origin (FR-016)
- [X] T031 [P] [US2] Write failing viewer tests in `tests/viewer_input.rs` for panning: left press then motion keeps the grabbed canvas point under the pointer, also for positions outside the window (negative and beyond size) (FR-005); Mod + left press pans the same way; release, `FocusLost` and `PointerLeave` end the drag and the camera stays; scroll events during a drag are ignored; motion outside the window continues the drag, while `PointerLeave` (the host broke the implicit grab) ends it; a pan marks the viewer dirty
- [X] T032 [US2] Write failing viewer tests in `tests/viewer_input.rs` for zooming: wheel `value120.y = -120` zooms in by 1.1 around the pointer and `+120` zooms out; `value120.y = -60` zooms by `1.1^0.5`; Mod + finger or continuous scroll of 50 logical px is one step; finger and continuous scroll without Mod pan like a scrolled document (`center += delta / zoom`) (FR-008); a horizontal wheel notch `value120.x = 120` pans 50 logical px right; pinch begin/update `scale = 2.0` sets zoom to twice the zoom at begin around the pointer, and end/cancel returns to idle; a zoom request at a limit leaves the viewer clean (`take_dirty()` is false) (FR-006, FR-007, FR-008)

### Implementation for User Story 2

- [X] T033 [US2] Implement `pan_anchor`, `pan_by`, `zoom_at`, `set_zoom_at` in `src/canvas/camera.rs` per data-model.md ("At a limit the zoom and centre stay unchanged (FR-007)") and research R8
- [X] T034 [US2] Implement the gesture state machine in `src/viewer/gesture.rs` per data-model.md (`Idle`, `Panning { anchor }`, `Pinching { zoom_at_begin }`; transitions on press/release/motion/focus lost/pointer leave/pinch events)
- [X] T035 [US2] Extend `Viewer::handle` in `src/viewer/mod.rs` for `PointerEnter`, `PointerMotion`, `PointerLeave`, `PointerButton`, `Scroll` and pinch events using the camera operations and the wheel/scroll/pinch rules from R8; expose `is_panning()` for the cursor
- [X] T036 [US2] Implement the pointer half of `src/backend/wayland_host/seat.rs`: `wl_pointer` enter/leave/motion/button; collect `axis`, `axis_source`, `axis_discrete` (v5-7, as `value120 = discrete * 120`) and `axis_value120` (v8) until `wl_pointer.frame` and emit one `Scroll`; `zwp_pointer_gestures_v1` pinch -> `PinchBegin/Update/End`; set the cursor with `wp_cursor_shape_v1` (default, or grabbing while `is_panning()`) on enter and when panning starts or stops
- [X] T037 [US2] Run the full check, then quickstart M3-M8, M10, M19 on niri and note results (2026-11-01: all steps passed - agent-run and confirmed by the author on niri)

**Checkpoint**: User Stories 1 and 2 work

---

## Phase 5: User Story 3 - Return home (Priority: P3)

**Goal**: Mod+0 returns to the origin at 100%.

**Independent Test**: pan and zoom away, press Mod+0, the view equals the startup view
(quickstart M9).

### Tests for User Story 3 (write first, see them fail)

- [X] T038 [P] [US3] Write a failing unit test in `src/canvas/camera.rs`: after any pan/zoom, `reset()` makes the camera equal `Camera::default()` (FR-009, SC-007)
- [X] T039 [P] [US3] Add failing tests: in `tests/keymap.rs`, Alt+0 maps to `Action::ResetView` with `us` and with `us,ua` (`ua` active); in `tests/viewer_input.rs`, after panning and zooming, `Modifiers { alt }` + `Key { 0, pressed: true }` restores the default camera and marks the viewer dirty, and pressing it twice has the same result as once

### Implementation for User Story 3

- [X] T040 [US3] Implement `Camera::reset()` in `src/canvas/camera.rs`
- [X] T041 [US3] Add Mod+0 -> `Action::ResetView` to the table in `src/input/bindings.rs` and handle it in `Viewer::handle` in `src/viewer/mod.rs`
- [X] T042 [US3] Run the full check, then quickstart M9 (with both keyboard layouts) and note the result (2026-11-01: all steps passed - agent-run and confirmed by the author on niri)

**Checkpoint**: all user stories work

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T043 [P] Add to *Gotchas* in `AGENTS.md`: "**Reference images**: regenerate `tests/golden/` only after an intended visual change, with `RUBICKWAY_BLESS=1`, and inspect the new PNGs before committing - never bless to make a failing test pass."
- [X] T044 Run the remaining quickstart steps (M11, M12, M13, M17) on niri, collect all M1-M19 results for the PR description (2026-11-01: all steps passed - agent-run and confirmed by the author on niri)
- [X] T045 Check that code matches spec.md, plan.md, data-model.md and contracts/; update the documents first wherever the implementation had to deviate
- [X] T046 Run the full check from `AGENTS.md` and confirm it passes

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none; T004 after T001
- **Foundational (Phase 2)**: after Setup; blocks all stories
- **US1 (Phase 3)**: after Foundational
- **US2 (Phase 4)**: after Foundational for its tests and camera/gesture work (T030-T035);
  T036-T037 need the host backend from US1 (T024-T027)
- **US3 (Phase 5)**: after Foundational for T038, T040; T039 and T041 need the keyboard and
  bindings from US1 (T021-T023)
- **Polish (Phase 6)**: after the stories

### Within Each User Story

- Tests first, and they must fail before implementation starts
- Pure modules (camera, grid, keyboard, bindings, viewer) before the backend and `state.rs`
- `src/viewer/mod.rs`, `src/input/bindings.rs`, `src/canvas/camera.rs` and
  `src/backend/wayland_host/seat.rs` are touched by several stories: never in parallel

### Parallel Opportunities

- Setup: T002, T003 alongside T001
- Foundational: T005, T006 together; T009, T010 together
- US1 tests: T011, T012, T014, T015 together (T013 after T012, T016 after T014 - same file); T017 alongside the
  backend work
- US2 tests: T030 alongside T031 (T032 after T031 - same file)
- US3 tests: T038, T039 together

---

## Parallel Example: User Story 1

```text
Task: "Write failing GridParams tests in src/render/grid.rs"           (T011)
Task: "Write the reference-image harness in tests/common/golden.rs"   (T012)
Task: "Write failing viewer tests in tests/viewer_input.rs"           (T014)
Task: "Write failing keymap tests in tests/keymap.rs"                 (T015)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 Setup, Phase 2 Foundational
2. Phase 3 User Story 1
3. **Stop and validate**: full check plus quickstart M1, M2, M14-M16, M18

### Incremental Delivery

1. Setup + Foundational
2. US1 -> a viewer that opens, draws and exits cleanly (MVP)
3. US2 -> pan and zoom
4. US3 -> reset
5. Polish -> remaining manual checks, docs in sync, PR

---

## Notes

- Commit after each task or logical group, only with the full check passing
- `unsafe` only in `src/render/egl.rs` and `src/render/gles.rs`, each block with `// SAFETY:`
- No `unwrap`/`expect`/`panic!` outside tests; exceptions need `#[expect(..., reason = "...")]`

## Phase 7: Convergence

- [X] T047 Hold the host's read guard while the loop sleeps, per wayland-client's integration pattern: in `src/state.rs` `step()` drop any held guard, then loop { dispatch, handle events, render if needed, flush, `prepare_read` } until the guard is taken; `src/backend/wayland_host/mod.rs` keeps the guard (`prepare_read`, `cancel_read`, `read` uses it); never hold it during EGL calls; note Mesa's 10 ms polling when no buffer is free as a known limit in research R4 per plan: research R3 (partial)
- [X] T048 Validate host-supplied sizes without arbitrary limits: in `src/backend/wayland_host/seat.rs` ignore a keymap whose `size` exceeds the fd's real length (`fstat`); in `src/viewer/mod.rs` keep the logical size unlimited, cap buffer sides at the GPU's largest drawable side (queried by `GlesRenderer`, default `i32::MAX`) with one uniform effective scale (`render_scale`); use checked `u32` -> `i32` conversion in `src/backend/wayland_host/window.rs`; add viewer tests for huge sizes and the uniform cap per Constitution II (partial)
- [X] T049 Remove the unused `_globals` and `_qh` parameters from `Seat::set_cursor` in `src/backend/wayland_host/seat.rs` and its caller in `src/backend/wayland_host/mod.rs` per plan: Principle VI (unrequested)
- [X] T050 Replace the reference-image tests with pixel assertions per Constitution III (as amended): add `tests/render_grid.rs` (background, axes, y direction, crisp rows with tiers at 100%, 1000% and scale 1.5, no fill and major spacing at 10%, far-away view equal to the near one); delete `tests/golden/`, `tests/render_golden.rs`, `tests/common/`, the `png` dev-dependency, `RUBICKWAY_BLESS` in `.env.example` and the CI artifact step; update the `AGENTS.md` gotcha, research R9/R11, plan and quickstart per Constitution III (contradicts)

## Phase 8: Convergence

- [X] T051 In `src/input/keyboard.rs` `binding_sym`, add the evdev offset with `checked_add` and return `Keysym::NoSymbol` on overflow, so a host key code near `u32::MAX` cannot panic a debug build; add a test for `u32::MAX` in `tests/keymap.rs` per Constitution II (partial)
- [X] T052 Remove the remaining reference-image wording: `specs/20261101-040740-nested-canvas/plan.md` *Testing* line ("reference-image tests on Mesa's software rasteriser" -> pixel assertions on llvmpipe output) and the `src/render/egl.rs` module doc ("so reference images are identical" -> "so rendered pixels are identical") per Constitution III (partial)
- [X] T053 Sync the Viewer section of `specs/20261101-040740-nested-canvas/data-model.md` with T048: add the `max_buffer_side` field (GPU's largest drawable side, default `i32::MAX`), `render_scale()` (scale reduced uniformly when the longest side would exceed it), and `buffer_size()` = logical size x `render_scale()`, rounded, within `[1, max_buffer_side]` per plan: data-model (partial)
