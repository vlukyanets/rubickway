---

description: "Task list for window management on the canvas"
---

# Tasks: Window Management on the Canvas

**Input**: Design documents from `specs/20261101-220702-window-management/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md,
checklists/review.md (all items reviewed)

**Tests**: Required by the constitution (Principle III): every user story has test tasks that
are written and seen failing before its implementation tasks.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested
on its own.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1-US5)
- References like (W4) point to research.md; (FR-xxx) and (SC-xxx) to spec.md; "Edge Cases
  <name>" to the named bullet in spec.md Edge Cases

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: module skeleton for the new files

- [X] T001 Create `src/shell/geometry.rs` (module doc: pure size, edge and view maths for window management) declared as `mod geometry; pub use geometry::*;` in `src/shell/mod.rs`, and an empty `tests/protocol/wm.rs` declared as `mod wm;` in `tests/protocol/main.rs`; run *Install deps* and keep the full check from `AGENTS.md` passing

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: size and edge types, the extended window events, the command queue from shell to
server with full configure state, size limits, cursor shapes and the `App` wiring that every
story uses

**CRITICAL**: no user story work can begin until this phase is complete

- [X] T002 [P] Write failing unit tests in `src/shell/geometry.rs` for `Edges { top, bottom, left, right }` (W2, data-model "Edges"): `Edges::from_resize_edge(u32) -> Option<Edges>` maps the xdg values 1, 2, 4, 5, 6, 8, 9, 10 to the matching edges, returns `Some(Edges::NONE)` for 0 and `None` for every other value (3, 7, 11-15, 16, `u32::MAX`); `is_valid()` is true only for "one edge or two adjacent ones"
- [X] T003 [P] Write failing unit and `proptest` tests in `src/shell/geometry.rs` for `SizeLimits { min: Size, max: Size }::clamp(size, largest) -> Size` (W6, FR-008, data-model "SizeLimits"): `min` "0 = none"; `max` "0 = unlimited; a component below `min` counts as `min`"; result per component is `max(min, 1)` raised, then limited by the application's max (if not below min), then by `largest` last, so `largest` wins over `min` and `min` wins over a smaller `max`; property: for any inputs the result is `>= 1` and `<= largest` in each component
- [X] T004 Implement `Edges` and `SizeLimits` in `src/shell/geometry.rs` so T002 and T003 pass
- [X] T005 Extend the shell's event types per data-model "WindowEvent" and "WindowCommand" in `src/shell/mod.rs`: `Mapped { id, size, input, limits, parent: Option<WindowId> }`, `Updated { id, size, input, limits, configured: bool }`, new `Created { id, parent, limits, maximized: bool, fullscreen: bool }`, `MoveRequested { id }`, `ResizeRequested { id, edges }`, `MaximizeRequested { id, on }`, `FullscreenRequested { id, on }`; new `WindowCommand::{Configure { window, size: Option<Size>, states: States }, Close { window }}` and `States { maximized, fullscreen, resizing }`; a command queue with `Shell::take_commands() -> Vec<WindowCommand>`; for now `apply` stores `limits`/`parent` on the window, answers `Created` with `Configure { size: None, states: States::default() }` and ignores the request variants; fill the new fields in `src/server/xdg_shell.rs` (`limits` default, `parent: None`, `configured: false`) and update every construction site in `tests/shell.rs`, `tests/protocol/surfaces.rs` and `tests/protocol/misbehaving.rs` so all existing tests pass unchanged in meaning
- [X] T006 Write failing unit tests in `src/shell/windows.rs` for the extended `Window` (data-model "Window (extended)", "Anchor"): new fields `limits`, `anchor` (default `Anchor::TopLeft`), `mode` (default `Mode::Normal`), `parent`; `Windows::update` applies the anchor to a new size - `TopLeft`: "origin kept"; `Edges { fixed, right, bottom }`: "if `left` is not fixed (the left edge was dragged): `origin.x = right - s.w`; likewise `origin.y = bottom - s.h` for the top edge; other axes keep the origin"; `Centre { point }`: "`origin = point - s / 2`"; `move_to(id, origin)` moves a window and returns whether it exists; `set_anchor(id, anchor)`
- [X] T007 Implement `Anchor`, `Mode` (`Normal`, `Maximized { saved: Option<Rect> }`), the new `Window` fields, anchored `update`, `move_to` and `set_anchor` in `src/shell/windows.rs` so T006 passes
- [X] T008 Write failing protocol tests in `tests/protocol/wm.rs` for configure state (W5, contracts/protocols.md): after `Server::command(&[Configure { window, size: Some(300x200), states: maximized }])` the client gets `xdg_toplevel.configure(300, 200, [activated, maximized])` plus `xdg_surface.configure`; a second `Configure` with another size while the first is unacked is not sent; after the client acks the first, exactly one configure with the newest size follows (coalescing); a focus change re-sends the current wanted size and states with `activated` toggled; after the client acks and commits the shell's configure, the next `Updated` has `configured: true` (emitted even if size and input did not change), while ack+commit of an activation-only configure emits no `Updated`; `Configure { size: None }` sends 0 x 0; `xdg_toplevel.wm_capabilities` on a v6 toplevel lists exactly `maximize` and `fullscreen`; add a helper to the harness for a mapped toplevel that records its configures in `tests/protocol/harness.rs` if needed
- [X] T009 Implement configure state in `src/server/xdg_shell.rs` and `Server::command(&mut self, commands: &[WindowCommand])` in `src/server/mod.rs` (data-model "server (changes)"): per toplevel `wanted { size: Option<(i32, i32)>, maximized, fullscreen, resizing }`, `queued_size`, `newest_serial`, `applied_serial`; `send_configure` sends `wanted` plus `activated`; at most one configure with a new size outstanding; `configured` reported in `Updated` per W4 (only for configures sent for a `WindowCommand::Configure`); `wm_capabilities([maximize, fullscreen])` instead of the empty list; converting the shell's f64 sizes to the protocol's i32 uses checked/saturating conversion, never a plain `as` on unchecked values (Principle II); `Close` is accepted and ignored until US4 so T008 passes
- [X] T010 Write failing protocol tests in `tests/protocol/wm.rs` for size limits (W6): `set_min_size` / `set_max_size` are double-buffered - not visible before the next commit, reported in `Mapped.limits` / `Updated.limits` after it; changing only the limits emits `Updated`; negative sizes remain the `invalid_size` error (existing behaviour); `0` means none/unlimited
- [X] T011 Implement `pending_limits` / `limits` in `src/server/xdg_shell.rs` (applied on root commit, compared with `reported` so a limits-only change emits `Updated`; client values converted with checked arithmetic, Principle II) so T010 passes
- [X] T012 Add `CursorShape::{Default, Grabbing, Move, Resize(Edges)}` and `Shell::cursor()` (Grabbing while the viewer pans, else Default) in `src/shell/mod.rs` with a test in `tests/shell.rs`; add `Cursor::Move` and the eight resize cursors (`ResizeN`, `ResizeNE`, `ResizeE`, `ResizeSE`, `ResizeS`, `ResizeSW`, `ResizeW`, `ResizeNW`, mapped to the `wp_cursor_shape_device_v1` shapes `move`, `n_resize`, `ne_resize`, ...) in `src/backend/wayland_host/seat.rs`; `App::update_cursor` in `src/state.rs` maps `shell.cursor()` to `Cursor` (W11)
- [X] T013 Wire the shell to the server in `src/state.rs` and `src/shell/mod.rs`: after applying window events and after handling input, `self.server.command(&self.shell.take_commands())`; `Shell::viewer_configured(size)` replaces `shell.viewer_mut().configure(size)` (for now it only forwards; US5 adds re-configuring); `Shell::set_largest_window(Size)` stores the limit (unlimited until set, like the server's existing check), set by `App` to `max_texture_side / ceil(scale)` logical pixels when the renderer is created and on every scale change (W6); add a test in `tests/shell.rs` that a `Configure` queued by `apply` comes out of `take_commands` once

**Checkpoint**: foundation ready - user stories can start

---

## Phase 3: User Story 1 - Move windows around the canvas (Priority: P1) - MVP

**Goal**: windows move with their own title bar drag and with Mod + Shift + left-drag; the
grabbed point stays under the pointer at any zoom, also while zooming.

**Independent Test**: quickstart W1-W3: two kittys, one dragged by its title bar, one with
Alt + Shift + left-drag, zooming out during a drag.

### Tests for User Story 1

- [X] T014 [P] [US1] Write failing routing tests in `tests/shell.rs` for the move gesture (FR-002, FR-004, contracts/input.md): Mod + Shift + left on a window focuses and raises it, sends it `PointerLeave` and no `PointerButton`; pointer motion moves the window's `origin` by the pointer delta in canvas units; the last button release ends the move and the pointer re-enters the window under it; a host `PointerLeave` ends it; `Unmapped` of the moved window ends it; `FocusLost` and releasing Mod or Shift do not end it; Mod + Shift + left on the background pans like Mod + left; `cursor()` is `Move` during the move
- [X] T015 [P] [US1] Write failing `proptest` tests in `tests/shell.rs` for SC-001 and FR-003: for zoom in `0.1..=10.0`, any start point inside a window and any pointer path, the grabbed window point stays within 1 screen pixel of the pointer; after Mod + scroll and after a pinch update during the move, the grabbed point is still under the pointer
- [X] T016 [P] [US1] Write failing routing tests in `tests/shell.rs` for Edge Cases "Other input during a move or resize" (W3): further button presses are swallowed (no `ClientInput`, the move continues); scroll without Mod produces nothing; Mod+0 is consumed and leaves the camera unchanged; Mod+Shift+Q still returns `Effect { quit: true }`; a key press and release go to the focused window
- [X] T017 [P] [US1] Write failing routing tests in `tests/shell.rs` for `MoveRequested` (FR-001, FR-016, W2): accepted while the client-windows grab is on that window with a button held - the grab turns into a move with the offset of the current pointer, the window gets `PointerLeave`; ignored with no grab, with the grab on another window, and after the release
- [X] T018 [P] [US1] Write failing protocol tests in `tests/protocol/wm.rs` (W2): `xdg_toplevel.move(seat, serial)` with the serial of the latest `wl_pointer.button` press delivered to that window emits `MoveRequested { id }`; an older button serial, a keyboard serial, or the serial of a press on another window emits nothing and the client stays connected

### Implementation for User Story 1

- [X] T019 [US1] Record `last_press: Option<(WindowId, u32)>` (window and serial of the latest delivered `wl_pointer.button` press) in `src/server/seat.rs`; handle `xdg_toplevel::Request::Move` in `src/server/xdg_shell.rs`: emit `MoveRequested` only if the serial and window match, else ignore (T018)
- [X] T020 [US1] Add `Op::Move { window, offset: Vector }` next to `Grab` in `src/shell/route.rs` (`Router::op: Option<Op>`), and in `src/shell/mod.rs`: start it from Mod + Shift + left on a window (focus, raise, `PointerLeave`) and from an accepted `MoveRequested`; on motion `windows.move_to(window, pointer_canvas - offset)`; after the viewer handles Mod + scroll or pinch during a move, re-apply the same rule; track held buttons so the last release ends it, end it on host `PointerLeave` and on `Unmapped`; apply the other-input rules of W3; `cursor()` returns `Move`; mark dirty on every move (T014-T017)

**Checkpoint**: US1 works on its own - quickstart W1-W3

---

## Phase 4: User Story 2 - Resize windows (Priority: P2)

**Goal**: windows resize from their own borders and with Mod + right-drag; the opposite edge or
corner stays fixed whatever size the application draws.

**Independent Test**: quickstart W4-W5: resize kitty by two corners and with Alt + right-drag
near an edge, a corner and the centre.

### Tests for User Story 2

- [X] T021 [P] [US2] Write failing unit tests in `src/shell/geometry.rs` for `pick_edges(rect, p) -> Edges` (FR-006, W3): cells are half-open `[0, w/3)`, `[w/3, 2w/3)`, `[2w/3, w]` on each axis, so a boundary point belongs to the cell further right or down; corner cells give two edges, edge cells one; the centre cell gives the nearest corner, ties picking bottom / right; and for `resize_size(start: Rect, edges, delta: Vector) -> Size`: dragged right/bottom edges add the delta, dragged left/top edges subtract it, other axes keep the start size
- [X] T022 [P] [US2] Write failing unit and `proptest` tests in `src/shell/windows.rs` for SC-002 and `resize_settled(id)`: with `Anchor::Edges` for any dragged edges and any sizes the application answers, the opposite edges stay within 1 canvas unit of their start position; `resize_settled` turns `Edges` back into `TopLeft`, after which `update` keeps the top-left (US2/AC4)
- [X] T023 [P] [US2] Write failing routing tests in `tests/shell.rs` for the resize gesture and requests (FR-005-FR-009, contracts/input.md): Mod + right on a window focuses and raises it, sends `PointerLeave`, picks edges by `pick_edges`; Mod + Shift + right does the same; Mod + right on the background does nothing; motion queues `Configure { size: Some(clamped resize_size), states.resizing = true }`; after Mod + scroll or a pinch during a resize, the wanted size follows the pointer's current canvas position (the dragged edge stays under the pointer, no jump); the window shown at whatever size `Updated` reports keeps the opposite edges fixed; the last release queues `Configure` with the last wanted size and `resizing = false`; the anchor stays `Edges` until `Updated { configured: true }` after the release, then the shell queues `Configure { size: None }` and the anchor is `TopLeft`; `ResizeRequested` is accepted only during a grab on that window (FR-016) and `Edges::NONE` is ignored; `cursor()` is `Resize(edges)`; requested sizes respect limits and the largest window (FR-008)
- [X] T024 [P] [US2] Write failing protocol tests in `tests/protocol/wm.rs`: `xdg_toplevel.resize(seat, serial, edges)` with a matching serial emits `ResizeRequested { id, edges }`; `edges = none` emits nothing and keeps the client; a value outside the enum (e.g. 3, 7, 16) disconnects only that client with `invalid_resize_edge` while a second client keeps working; configures sent during a resize carry the `resizing` state; a client that never acks gets exactly one outstanding size configure however many `Configure` commands arrive

### Implementation for User Story 2

- [X] T025 [US2] Implement `pick_edges` and `resize_size` in `src/shell/geometry.rs` and `resize_settled` in `src/shell/windows.rs` (T021, T022)
- [X] T026 [US2] Handle `xdg_toplevel::Request::Resize` in `src/server/xdg_shell.rs`: `WEnum::Unknown` -> `invalid_resize_edge`; `none` ignored; otherwise the serial check from T019 and `ResizeRequested { id, edges }` (T024)
- [X] T027 [US2] Add `Op::Resize { window, edges, start_pointer: Point, start_rect: Rect }` in `src/shell/route.rs` and handle it in `src/shell/mod.rs`: start from Mod + right (with or without Shift) and from accepted `ResizeRequested`; set `Anchor::Edges` from the start rect; on motion, and after the viewer zooms during the resize, queue `Configure` with `SizeLimits::clamp(resize_size(start_rect, edges, pointer_canvas_now - start_pointer), largest)` and `resizing`; the f64 -> size conversion saturates (Principle II); on end queue the final configure without `resizing`; on `Updated { configured: true }` after the end call `resize_settled` and queue `Configure { size: None }`; same end conditions and other-input rules as the move; `cursor()` returns `Resize(edges)` (T023)

**Checkpoint**: US1 and US2 work - quickstart W1-W5

---

## Phase 5: User Story 3 - Find windows and switch between them (Priority: P3)

**Goal**: Mod+Tab / Mod+Shift+Tab cycle focus in most-recently-used order and bring the window
into view when it is not fully visible.

**Independent Test**: quickstart W6-W7: three kittys far apart, Alt+Tab repeatedly, Alt+Tab
twice with Alt released in between, Alt+Shift+Tab.

### Tests for User Story 3

- [X] T028 [P] [US3] Write failing unit and `proptest` tests in `src/shell/windows.rs` for the focus history (W9, data-model "Windows (extended)"): `mru` holds "every mapped window exactly once; front = most recently focused"; `map` and `raise_and_focus` move a window to the front; `unmap` removes it; `cycle(step)` with `step` +1 / -1 walks from the front with wrap-around, focusing and raising each window without reordering `mru`; `end_cycle()` moves the reached window to the front; invariant: `mru` is a permutation of the stack's ids
- [X] T029 [P] [US3] Write failing unit tests in `src/shell/geometry.rs` for `reveal(camera, view_size, rect) -> Option<Camera>` (FR-012, W9): `None` when the rect is fully inside the view (no margin); otherwise centred on the rect with the zoom kept if the rect fits, else `zoom = min(current, fit)` where the rect fills at most 90% of the view in each direction (5% margin per side); never below `ZOOM_MIN` (a rect too large even at 10% is centred at 10%)
- [X] T030 [P] [US3] Write failing routing tests in `tests/shell.rs` for cycling (FR-011, FR-012, FR-017, SC-003): with five windows outside the view, each Mod+Tab sends `KeyboardFocus` to the next window in MRU order, raises it and sets the camera per `reveal`; neither the Tab press nor its release reaches an application; releasing Alt commits the order so two Mod+Tab presses with Alt released between them switch back and forth; `FocusLost` while Alt is held also commits; Mod+Shift+Tab walks the other way; zero windows: nothing; one fully visible window: focused and raised, camera unchanged; during a move or resize Mod+Tab and Mod+Shift+Tab are consumed and do nothing

### Implementation for User Story 3

- [X] T031 [P] [US3] Add `Action::NextWindow` (Mod+Tab) and `Action::PreviousWindow` (Mod+Shift+Tab, binding symbol `Tab` at level 0) to `BINDINGS` in `src/input/bindings.rs`, with lookup tests (W10) - written in `tests/keymap.rs`, where the existing binding tests live
- [X] T032 [US3] Implement `mru`, `cycle` and `end_cycle` in `src/shell/windows.rs` and `reveal` in `src/shell/geometry.rs` (T028, T029)
- [X] T033 [US3] Handle `NextWindow` / `PreviousWindow` in `Shell::key` in `src/shell/mod.rs` before the viewer (consumed with their release): step the cycle, emit the focus change, set the camera via `viewer_mut().set_camera(...)` when `reveal` returns one; call `end_cycle` on a `Modifiers` event without Mod and on `FocusLost`; no-op during a move or resize (T030)

**Checkpoint**: US1-US3 work - quickstart W1-W7

---

## Phase 6: User Story 4 - Close windows (Priority: P3)

**Goal**: Mod+Q asks the focused window's application to close it.

**Independent Test**: quickstart W8: Alt+Q closes a kitty; with `sleep 100` running, kitty asks
for confirmation.

### Tests for User Story 4

- [X] T034 [P] [US4] Write failing routing tests in `tests/shell.rs` (FR-010, US4/AC1-AC2): Mod+Q with a focused window queues `WindowCommand::Close { window }` and neither the press nor the release reaches the application; with no focused window nothing is queued; Mod+Shift+Q still quits; during a move Mod+Q closes the focused (moved) window and the move ends when `Unmapped` arrives
- [X] T035 [P] [US4] Write failing protocol tests in `tests/protocol/wm.rs` (US4/AC3): `Server::command(&[Close { window }])` makes the client receive `xdg_toplevel.close`; a client that ignores it stays mapped and connected; `Close` for an unknown window does nothing

### Implementation for User Story 4

- [X] T036 [P] [US4] Add `Action::CloseWindow` (Mod+Q) to `BINDINGS` in `src/input/bindings.rs` with a lookup test (in `tests/keymap.rs`); check that Mod+Shift+Q still maps to `Quit` (exact modifiers)
- [X] T037 [US4] Handle `CloseWindow` in `Shell::key` in `src/shell/mod.rs` (queue `Close` for the focused window) and send `xdg_toplevel.close` for `WindowCommand::Close` in `Server::command` / `src/server/xdg_shell.rs` (T034, T035)

**Checkpoint**: US1-US4 work - quickstart W1-W8

---

## Phase 7: User Story 5 - Maximize and fullscreen requests (Priority: P3)

**Goal**: maximize jumps the view to the window at 100% and fills the viewer; fullscreen shows
only the window on black; both restore what was there before; only the focused window can ask;
initial states and new windows during fullscreen behave as specified.

**Independent Test**: quickstart W9-W12 and W15-W17.

### Tests for User Story 5

- [X] T038 [P] [US5] Write failing tests in `tests/shell.rs` for maximize (FR-013, FR-015, FR-019, W7): `MaximizeRequested { on: true }` from the focused window saves its rect, raises it, sets the camera to zoom 1 centred on the window centre, sets `Anchor::Centre { point }` and queues `Configure { size: Some(clamped viewer size in logical units), maximized }`; answered sizes smaller or larger than the viewer stay centred on `point`; pan and zoom leave it maximized; another window raised later is drawn over it (stack order, US5/AC1); `on: false` restores the saved origin at once and queues `Configure { size: Some(saved size) }` without `maximized`; Mod + right on it restores and the resize continues from the saved rect; a move gesture (or `MoveRequested`) restores the saved size with the grabbed point at `offset.x * saved_w / current_w`, y offset kept; `viewer_configured` with a new size re-queues the viewer size, point kept; repeated `on: true` and `on: false` on a normal window change nothing; a request from a window without focus changes nothing; Mod+Tab to a maximized window outside the view sets the maximize camera (zoom 1, centred) instead of `reveal`
- [X] T039 [P] [US5] Write failing tests in `tests/shell.rs` for fullscreen (FR-014, W8, contracts/input.md "Fullscreen"): `FullscreenRequested { on: true }` from the focused window: `visible()` returns only it, `fullscreen()` names it, camera zoom 1 centred on it, `Configure { viewer size, fullscreen }`, `Anchor::Centre`; each of Mod + left press, Mod + Shift + left on the background, left press on uncovered background, Mod + scroll, scroll over uncovered background, pinch begin and Mod+0 ends fullscreen, restores rect and camera, and is consumed (camera exactly the saved one afterwards); Mod+Tab ends it and sets the camera for the next window directly (computed against the saved view); Mod+Q queues `Close` as usual; `Unmapped` of the fullscreen window restores the camera; a second window's fullscreen ends the first one first, so the camera restored at the end is the pre-fullscreen one; fullscreen over maximized queues both states and leaving fullscreen returns to the maximized rect and state; Mod + Shift + left on the fullscreen window ends it and starts a move at the saved size under the pointer; Mod + right on it ends it and is consumed; `viewer_configured` re-queues the viewer size; a request from a window without focus changes nothing
- [X] T040 [P] [US5] Write failing tests in `tests/shell.rs` for initial state and new windows (FR-019, FR-020, W13, W14): `Created { maximized: true }` queues `Configure { viewer size, maximized }` for that id, and its `Mapped` places it centred in the view and enters maximize with `Mode::Maximized { saved: None }`; leaving it queues `Configure { size: None }` and keeps the centre; `Created { maximized, fullscreen }` queues both states and maps fullscreen over maximized; `Created` while another window is fullscreen with a different parent queues `Configure { size: None, states: default }`; a window `Mapped` during fullscreen is placed centred in the saved camera's view, on top, not focused, second in `mru`, and fullscreen stays; a window `Mapped` whose `parent` is the fullscreen window ends fullscreen first and maps focused as usual; a window answered at `Created` with an initial state that maps after another window went fullscreen (not its parent) maps without focus per FR-020, and the shell queues `Configure { size: None, states: default }` for it (W13 re-check at `Mapped`); `Created { maximized }` before the viewer has been configured asks for the clamped size (at least 1 x 1) and is re-configured by `viewer_configured`
- [X] T041 [P] [US5] Write failing protocol tests in `tests/protocol/wm.rs` (contracts/protocols.md): after map, `set_maximized` / `unset_maximized` / `set_fullscreen(None)` / `unset_fullscreen` emit `MaximizeRequested` / `FullscreenRequested` with `on`; `set_maximized` before the initial commit makes the initial commit emit `Created { maximized: true, .. }` and send no configure until `Server::command` gives one for that id, which then carries the size and `maximized`; a toplevel with no initial state gets its initial configure at once and no `Created`; `Mapped` after `Created` uses the same id; `set_parent(p)` with a mapped `p` is reported in `Mapped.parent`; an unmapped parent counts as none; when the parent unmaps, the child's parent becomes the parent's parent; `set_parent` with itself or a descendant is `invalid_parent`; a configure with both `maximized` and `fullscreen` carries both states
- [X] T042 [P] [US5] Write a failing pixel test in `tests/render_windows.rs` (guarded by `renders_on_llvmpipe`): a frame drawn the way `App` draws fullscreen - the new black clear and one window quad - has black pixels outside the window and the window's colour inside, and no grid line colour anywhere; colours from the `render` constants

### Implementation for User Story 5

- [X] T043 [US5] Handle `SetMaximized`, `UnsetMaximized`, `SetFullscreen`, `UnsetFullscreen` and `SetParent` in `src/server/xdg_shell.rs` (W13, W14, data-model "server (changes)"): before the initial commit, remember `initial { maximized, fullscreen }`; at the initial commit, if any was requested, assign `early_id`, emit `Created` and hold the initial configure until `Server::command` gives a `Configure` for that id; `map` reuses `early_id`; after map, emit the request events; store `parent` with the protocol's rules (unmapped parent = none, re-parent children on unmap, `invalid_parent` for self or a descendant) and report it in `Created` / `Mapped` (T041)
- [X] T044 [US5] Implement maximize in `src/shell/mod.rs` and `src/shell/windows.rs` (W7): the focus gate (FR-019), saved rect, camera, `Anchor::Centre`, restore on leave / resize / move, the move offset scaling, `viewer_configured` re-configuring maximized windows, the maximize camera for Mod+Tab (T038)
- [X] T045 [US5] Implement fullscreen in `src/shell/mod.rs` (W8): `Fullscreen { window, saved_rect, saved_camera }`, `fullscreen()`, `visible()` with only the fullscreen window, the exits that are consumed, Mod+Tab going straight to the next window, restore on unmap, one fullscreen window at a time, fullscreen over maximized, move/resize gestures on it, `viewer_configured` (T039)
- [X] T046 [US5] Implement initial state and new windows during fullscreen in `src/shell/mod.rs` and `src/shell/windows.rs` (W13, W14): answer `Created` per the rules, enter the state on `Mapped` - or, if the window now maps without focus, drop the requested state and queue `Configure { size: None, states: default }` -, `Windows::insert_unfocused(id)` ("top of the stack, second in `mru`, focus unchanged"), placement in the saved camera's view, a dialog of the fullscreen window ending it (T040)
- [X] T047 [US5] Add a plain clear to `GlesRenderer` in `src/render/gles.rs` (black, `render` constant) and draw fullscreen in `App::render` in `src/state.rs`: when `shell.fullscreen()` is `Some`, clear black instead of drawing the grid, then draw only that window's quads (T042)

**Checkpoint**: all stories work - quickstart W1-W17

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T048 [P] Add the server-level misbehaving cases of SC-006 that are not yet covered to `tests/protocol/misbehaving.rs`, each ending with a second client still working: contradictory limits (min above max, negative values already an error) reported as sent; a client that never acks while `Configure` commands keep arriving (one outstanding); a toplevel destroyed while its initial configure is held for `Created`; a client disconnecting while a configure is queued. Shell-level cases (unmapped mid-move / mid-resize / while maximized or fullscreen, requests from unfocused windows, limits above the largest window) stay in `tests/shell.rs` (T014, T023, T038-T040); cross-check that every Edge Cases misbehaving bullet maps to a test in one of the two files
- [X] T049 [P] Add a "Controls" section to `README.md`: canvas gestures from nested-canvas and client-windows (Alt + left-drag pans, Alt + scroll and pinch zoom, Alt+0, Alt+Shift+Q) and the new window gestures and shortcuts from contracts/input.md (Alt + Shift + left-drag moves, Alt + right-drag resizes, Alt+Tab / Alt+Shift+Tab, Alt+Q), matching contracts/input.md exactly (CHK007)
- [X] T050 [P] Update module docs that describe old behaviour: the header of `src/server/xdg_shell.rs` ("Window-management requests are accepted and ignored"), the request match comment in the same file, and the module doc of `src/shell/mod.rs` (routing now includes operations, cycling, maximize and fullscreen)
- [X] T051 Run the full check from `AGENTS.md` (`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked`) and fix everything it reports
- [X] T052 Run the manual checks W1-W17 in `quickstart.md` on a host session (owner; the agent can run a smoke subset with kitty and `grim` screenshots) and record the result here. Agent smoke run 2026-11-02: RubickWay started nested, `kitty --start-as=maximized` connected, mapped (`window 1 opened`), closed and disconnected cleanly, no warnings or errors in the debug log; not checked visually (the RubickWay window was on another workspace). Marked done by the owner on 2026-11-02 as it is.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none
- **Foundational (Phase 2)**: after Setup; blocks all stories
- **US1 (Phase 3)**: after Foundational
- **US2 (Phase 4)**: after Foundational; reuses the serial check (T019) and the operation
  routing (T020) from US1, so in practice after US1
- **US3 (Phase 5)**: after Foundational; independent of US1/US2 except the "during an
  operation" test cases in T030, which need T020
- **US4 (Phase 6)**: after Foundational; the "during a move" case in T034 needs T020
- **US5 (Phase 7)**: after Foundational; the move/resize interactions in T038/T039 need
  US1/US2, the Mod+Tab interactions need US3, the Mod+Q case needs US4
- **Polish (Phase 8)**: after the stories it touches

### Within Each User Story

- Tests are written first and must fail before implementation
- geometry/windows (pure) before shell routing before server and `App` wiring

### Parallel Opportunities

- T002 and T003 (both in `geometry.rs` tests, but separate test functions - write together)
- All test tasks within a story marked [P] touch different files or independent test
  functions and can be written in parallel
- T031 and T036 (bindings) can be done any time after Foundational
- T048, T049, T050 in parallel

## Parallel Example: User Story 1

```text
Task: "T014 routing tests for the move gesture in tests/shell.rs"
Task: "T015 proptest for SC-001 in tests/shell.rs"
Task: "T018 protocol tests for xdg_toplevel.move in tests/protocol/wm.rs"
```

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 + Phase 2
2. Phase 3 (US1): move windows
3. **Stop and validate**: quickstart W1-W3

### Incremental Delivery

1. US1 move -> US2 resize -> US3 cycling -> US4 close -> US5 maximize/fullscreen
2. Run the full check after every story; each story adds value without breaking the earlier
   ones

## Phase 9: Convergence

- [X] T053 End a move or resize on a host pointer leave like on the last release (`end_operation`: a resize sends its last size without `resizing` and settles), with a failing routing test in `tests/shell.rs` first, in `src/shell/mod.rs` per FR-007, FR-015, Edge Cases "Pointer leaves the viewer mid-drag" (partial)
- [X] T054 Make an accepted move or resize request from the fullscreen window's application leave fullscreen first (view and saved size restored, the move continuing under the pointer as for Mod + Shift + left; a resize continuing from the saved rect), with failing routing tests in `tests/shell.rs` first, in `src/shell/mod.rs` per FR-014, FR-001, FR-005 (partial)
- [X] T055 Settle a resize that ended without motion when a configure was sent for it (Mod + right click on a maximized window: push the window to `settling` so `size: None` follows), with a failing routing test in `tests/shell.rs` first, in `src/shell/mod.rs` per plan: research W5 (partial)
