---
description: "Task list for Pointer Lock"
---

# Tasks: Pointer Lock

**Input**: Design documents from `specs/20261119-161011-pointer-lock/`

**Prerequisites**: plan.md, spec.md, research.md (L1-L14), data-model.md, contracts/, quickstart.md

**Tests**: Required by Principle III: each test task is written and seen failing before its
implementation task. No test needs a device.

**Organization**: Tasks are grouped by user story, in the plan's order of work: setup, foundation,
US1 (P1), US2 (P2), US3 (P3), polish. Each step leaves a working desktop.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US3)

---

## Phase 1: Setup

- [X] T001 Run the full check from `AGENTS.md` in the worktree to confirm a green baseline (no new dependency: `wayland-protocols` 0.32 with `unstable` and `input` 0.10 already give everything, plan Technical Context)

---

## Phase 2: Foundational (blocks all stories)

**Purpose**: the new input fields, the crate types, Mod+Escape and the two globals with inert
objects. Nothing visible changes; today's tests stay green apart from the globals list.

### Tests (write first, see them fail)

- [X] T002 [P] Write a failing unit test beside `translate` in `src/backend/drm/input.rs`: `RawEvent::Motion { dx, dy, dx_unaccel, dy_unaccel, time_us }` becomes `InputEvent::PointerMoved { delta, unaccel, time_us }` with each value carried unchanged (data-model "Backend", research L4)
- [X] T003 [P] Write failing unit tests (2026-11-20: the config refusal is tested in `tests/config.rs`, where the other refused combinations are) in `src/input/bindings.rs`: for each Mod key, Mod+Escape in `builtin_table` gives `Action::ReleasePointer`; a configured spawn binding on Mod+Escape is refused with the existing warning "the combination is a built-in shortcut" (with its line) and the other configured bindings still work (research L7, contracts/input.md "New shortcut"); and a failing test in `tests/shell.rs`: Mod+Escape with a focused window gives no `ClientInput` at all (no `Key` reaches the window) and changes nothing else (FR-008, spec Edge Cases)
- [X] T004 [P] Update the globals-and-versions test in `tests/protocol/surfaces.rs` to expect `zwp_relative_pointer_manager_v1` 1 and `zwp_pointer_constraints_v1` 1 (contracts/protocols.md); see it fail

### Implementation

- [X] T005 Add `unaccel: Vector` and `time_us: u64` to `InputEvent::PointerMoved` in `src/input/mod.rs`, and `dx_unaccel`, `dy_unaccel`, `time_us` to `RawEvent::Motion` in `src/backend/drm/input.rs`, read from libinput (`dx_unaccelerated`, `dy_unaccelerated`, `time_usec`); update every other place that builds or matches `PointerMoved` (`src/shell/mod.rs`, `src/viewer/mod.rs`, `tests/shell.rs`) without changing behaviour, so T002 passes
- [X] T006 Add `Action::ReleasePointer` and Mod+Escape to `builtin_table` in `src/input/bindings.rs`; in `src/shell/mod.rs` the action is consumed and does nothing yet (the key never reaches the application), so T003 passes
- [X] T007 Add the crate types (data-model "Shared types", contracts/input.md "Crate API"), with the shell and the server ignoring them for now (2026-11-20: `ConstraintToken` is a newtype over `u64`; the constraint types live in new `src/shell/constraint.rs`, re-exported from `rubickway::shell`):
  - in `src/shell/mod.rs`: `ConstraintToken` (`u64`, never reused), `ConstraintKind` (`Lock` / `Confine`), `ConstraintInfo { token, kind, persistent: bool ("false: single-use (oneshot)"), area: Option<Vec<Rect>> ("window coordinates; None: the whole window; any number of rectangles"), hint: Option<Point> ("locks only: the latest committed release point, window coordinates") }`, `WindowEvent::ConstraintChanged { id: WindowId, constraint: Option<ConstraintInfo> }`;
  - in `src/shell/route.rs`: `ClientInput::RelativeMotion { window, delta, unaccel, time_us }`, `ClientInput::Constraint { token, change }`, `ConstraintChange::{Start, Stop(ConstraintEnd)}`, `ConstraintEnd::{Focus, PointerLeft, Shortcut, WindowGone, Resize}`;
  - `deliver` in `src/server/seat.rs` ignores the two new `ClientInput`s; `Shell::apply` ignores `ConstraintChanged`
- [X] T008 Create the two globals with inert objects (2026-11-20: the objects' user data is their token; `ConstraintData` lives in `ServerState.constraints.by_token`, data-model updated) (contracts/protocols.md), registered in `src/server/mod.rs` like `cursor_shape.rs`:
  - new `src/server/relative_pointer.rs`: the manager; `get_relative_pointer` registers the object in `SeatState.relative_pointers: HashMap<ClientId, Vec<ZwpRelativePointerV1>>`, pruned on destroy and with the client; the manager's `destroy` leaves existing relative pointers alone;
  - new `src/server/pointer_constraints.rs`: the manager; `lock_pointer` and `confine_pointer` create inert objects with `ConstraintData` (data-model "Server state") in a `Mutex`, tokens from `ServerState.next_constraint_token`; no lifetime handling, no `already_constrained` check (both come with their tests in T013 and T017) and nothing is reported yet;
  - so T004 passes
- [X] T009 Run the quick check (`cargo clippy --all-targets -- -D warnings && cargo test --lib`) and `cargo test`; everything is green (2026-11-20: all tests green; clippy reports only the `ConstraintData` fields US1 starts reading - green again at T017, without any `allow`)

**Checkpoint**: the types, Mod+Escape and the globals exist; applications still get no relative
motion and no lock.

---

## Phase 3: User Story 1 - Look Around in a Game or a 3D View (Priority: P1) MVP

**Goal**: relative motion end to end, locks, the release point, Mod+Escape and click to capture
again, focus changes (spec US1, FR-001-FR-004, FR-006-FR-008, FR-010, FR-011).

**Independent Test**: quickstart Q1-Q7.

### Tests for User Story 1 (write first, see them fail)

- [X] T010 [P] [US1] Write failing unit tests in new `src/shell/constraint.rs` (declared in `src/shell/mod.rs`) for the effective area (data-model "Effective area", research L5, L10): the constraint's area (or the whole window when `None`) intersected with the window's input shape and with the part of the window shown on the pointer's viewer, in window coordinates; an empty result means the constraint cannot apply; an area or input shape of more than `MAX_CONFINE_RECTS = 256` rectangles counts as its bounding box, and so does a result of more than 256; areas at the `i32` limits do not overflow (all arithmetic in `f64` from `i32`)
- [X] T011 [US1] Write failing shell tests for relative motion (2026-11-20: the background and gesture case and the absolute position passed as soon as written - nothing produced relative motion yet) in new `tests/pointer_constraints.rs` (helpers at the top, modelled on `tests/shell.rs`: a shell with one or two viewers, a mapped and focused window, `PointerMoved` with given `delta`, `unaccel`, `time_us`; a helper that sends `WindowEvent::ConstraintChanged`) (research L4, FR-001-FR-003, SC-006):
  - over a window: `RelativeMotion { window, delta, unaccel, time_us }`, then `PointerMotion`, then `PointerFrame`, exactly one `RelativeMotion` per `PointerMoved`;
  - with a button held on a window and the pointer outside it: the held window gets it;
  - at zoom 50% and 200%: `delta` is the device delta divided by the zoom, `unaccel` unchanged;
  - at a screen edge the pointer stops but `RelativeMotion` carries the full delta;
  - over the background, a title bar or a border, and during a pan, a move or a resize: no `RelativeMotion` (FR-002);
  - an absolute `PointerMotion` input event produces no `RelativeMotion`
- [X] T012 [US1] Write failing shell tests for locks in `tests/pointer_constraints.rs` (research L5-L7, L9, data-model "ConstraintState", contracts/input.md) (2026-11-20: the tooltip case passed as soon as written - nothing stopped a lock yet; the replacing-token case changed with I1: no decision for the old token, research L3):
  - a lock on the focused window with the pointer over its area: `Constraint { token, Start }`; afterwards `PointerMoved` leaves `shell.pointer()` unchanged and gives `RelativeMotion` + `PointerFrame` only, no `PointerMotion`; buttons and scroll still reach the window (FR-004);
  - a lock on a window without the focus, or with the pointer outside its area: nothing until both hold (US1 scenario 8);
  - focus moves away (Mod+Tab, a new window, the window closing): `Stop(Focus)` (closing: `Stop(WindowGone)`); a single-use one is forgotten; a lasting one starts again when the focus comes back with the pointer over the window, without a click (US1 scenarios 5, 6);
  - Mod+Escape while a lock applies: `Stop(Shortcut)`, no key event reaches the window; a lasting one then stays released after a focus change and return, and after a Mod + click (a press taken by a binding), and starts again after a button press delivered to the window (FR-008, US1 scenario 4); a single-use one is gone; Mod+Escape with nothing applying: no output, the key does not reach the window;
  - release point: when a lock stops and the hint is inside the effective area on the pointer's viewer, the output is `Constraint { token, Stop(..) }` first, then a `PointerMotion` to the hint, with no `RelativeMotion`; a hint outside it, or none: the pointer stays (FR-007); when the application removes an applying lock (`ConstraintChanged { constraint: None }`), there is no `Constraint` output (the server logs that stop itself, T017), only the `PointerMotion` to the hint if it is inside;
  - a grabbing popup of the window takes the keyboard: `Stop(Focus)`, a single-use one ends, a lasting one starts again once the popup closes; a non-grabbing popup of the same window under the pointer does not stop it (FR-006, spec Edge Cases);
  - `Unmapped` while it applies: `Stop(WindowGone)`, forgotten; a `ConstraintChanged` for it after `Mapped` again starts a lasting one once the conditions hold (spec Edge Cases, research L12);
  - a view jump under the still pointer (Mod+0): `Stop(PointerLeft)` if the pointer is no longer over the area, and a lasting one starts again when it is; the window going maximized or fullscreen under a still lock: it goes on if the pointer is still over the area, now holding the window point under it, else `Stop(PointerLeft)` (spec Edge Cases, research L6);
  - `FocusLost` (console switch): `Stop(Focus)`; on `FocusGained` a lasting one starts again when the conditions hold;
  - the viewer with the locked pointer is removed (`Shell::remove_viewer`, a monitor unplugged): its output has `Stop(PointerLeft)` (the pointer is now on another viewer); a single-use one is forgotten, a lasting one starts again when the conditions hold (spec Edge Cases);
  - a `ConstraintChanged` with a new token replacing an applying one: the old one counts as removed (no decision for its token), then the new one is judged by itself (research L3)
- [X] T013 [P] [US1] Write failing protocol tests (2026-11-20: the surface-that-is-not-a-window case passed as soon as written - nothing was reported yet; an unknown lifetime value cannot be sent from `wayland-client`, so it is a unit test in `src/server/pointer_constraints.rs`) in new `tests/protocol/pointer_constraints.rs` (add `mod pointer_constraints` in `tests/protocol/main.rs`) (contracts/protocols.md, research L11, L12):
  - relative pointers: the shell's `RelativeMotion` arrives as `relative_motion` on every live relative pointer of the window's client, `utime_hi`/`utime_lo` the split of `time_us`, `dx, dy, dx_unaccel, dy_unaccel` as `wl_fixed`; destroying the manager keeps them working; a destroyed one gets nothing;
  - `lock_pointer` on a mapped window: the shell gets `ConstraintChanged` with a token, `kind: Lock`, `persistent` from the lifetime (an unknown value: `false`), `area: None` for a null region, or the region's rectangles in window coordinates (minus the window geometry origin) copied at creation;
  - `set_cursor_position_hint` and `set_region` change nothing until `wl_surface.commit`, then a new `ConstraintChanged`; a window geometry change reports again;
  - a lock created before the window is shown is reported only after `Mapped` (M4); a lock on a popup, a sub-surface, a cursor surface or a role-less surface is never reported (FR-011, M3);
  - `Constraint { token, Start }` sends `locked`; `Stop(..)` sends `unlocked`; a `oneshot` lock is then defunct: its requests are ignored, it gets no more events, and a new lock on the surface is accepted (M2); a decision for a stale token is ignored (research L3);
  - destroying the lock or the surface: `ConstraintChanged { constraint: None }` (M5);
  - a second lock on the same surface: `already_constrained`, only that client disconnected (M1);
  - regions and release points at the `i32` limits: no panic, the reported area and hint are correct in window coordinates, a hint outside the window is dropped (M7)

### Implementation for User Story 1

- [X] T014 [US1] Implement the effective area in `src/shell/constraint.rs` (`Constraint { info, state, locked_at }`, `ConstraintState::{Waiting, Active, Released}`, `MAX_CONFINE_RECTS`), computed again only when the area, the input shape or the view changes, so T010 passes
- [X] T015 [US1] Implement the shell side in `src/shell/mod.rs` (research L4-L7, L9), so T011 and T012 pass (2026-11-20: no stored `locked_at` - the layout pointer does not move while locked, data-model updated; a stop because the pointer left, the window went away or a resize does not move the pointer to the release point, spec FR-007 and research L9 updated):
  - `Shell.constraints: HashMap<WindowId, Constraint>` and `Shell.applying: Option<WindowId>`; `apply(ConstraintChanged)` stores, replaces or removes;
  - in `handle(PointerMoved)`: `RelativeMotion` from the device delta before the layout clamps the pointer (delta / zoom of the pointer's viewer, `unaccel` unchanged), to the entered window or the window of a held-button grab, not during a pan, a move or a resize; while a lock applies the layout pointer does not move and no `PointerMotion` is sent;
  - `update_constraint` at the end of every `handle`, `apply` and `remove_viewer` (the entry points that return `ClientInput`s; a view change by `add_viewer` or `place_home_views` is checked on the next `handle`): checks the conditions of research L5 for the focused window and the previously applying one only (bounded work, FR-012), emits `Start` and `Stop(reason)`, forgets a stopped single-use one, sends a lasting one back to `Waiting`, moves the pointer to the release point when a lock stops (FR-007; the `Stop` comes before the `PointerMotion`), updates `locked_at` when the window changes size or place under a still lock; the shell is the only guard against `PointerMotion` while a lock applies (the server does not filter it, research L9);
  - `Action::ReleasePointer`: `Stop(Shortcut)`, a lasting one becomes `Released`; a button press delivered to its window (not one taken by a binding) makes it `Waiting` again
- [X] T016 [P] [US1] Implement relative motion in `src/server/relative_pointer.rs` and `deliver` in `src/server/seat.rs`: `RelativeMotion` goes to the client's live relative pointers, at most 32 per client - the first 32 live ones in creation order, so one created later gets events once an earlier one is destroyed (more are kept but inert, with one debug line per client, research L11), so the relative-pointer part of T013 passes
- [X] T017 [US1] Implement locks in `src/server/pointer_constraints.rs`, `src/server/compositor.rs` and `deliver` in `src/server/seat.rs` (after T016, same file) (research L12), so T013 passes:
  - the lifetime (an unknown value counts as `oneshot`) and `already_constrained` (1) for a surface with a live, not defunct constraint in `ServerState.constraints`;
  - `commit` makes the pending area and hint current; for a window root (once `Mapped`), `ConstraintChanged` when anything reported changed, also on a window geometry change, and again after `Mapped` for one created earlier; areas and hints converted from surface to window coordinates in `f64` from `i32`, so the `i32` limits cannot overflow (Principle II);
  - `ClientInput::Constraint` in `deliver`: `Start` sends `locked` and marks it applying, `Stop` sends `unlocked`, clears applying and makes a `oneshot` one defunct (removed from `ServerState.constraints`); a stale or defunct token is ignored;
  - destroying the lock or its surface reports `ConstraintChanged { constraint: None }`; if it was applying, the server itself records the stop for the log (reason `application` when the client destroyed the lock, `window gone` when the surface or the client went away), since the shell sends no decision for a removed constraint (research L13); requests on a defunct one are ignored
- [X] T018 [US1] Run the quick check and `cargo test`; then the owner runs quickstart Q1-Q7 on the laptop (2026-11-20: quick check and all tests green. 2026-11-22: Q1 and Q2 in Firefox after the fixes T035 and T036; Q3-Q5 and Q7 with a small SDL3 test client instead of a game and Blender (lock, Mod+Escape, click, focus change, release point through `set_cursor_position_hint`) - passed; Q6 skipped: gamescope 3.17 does not start in RubickWay at all, see spec Assumptions)
- [X] T035 [US1] Fix found in Q1 (2026-11-20): Firefox locked the pointer but the arrow stayed, as GTK 3 hides it with a cursor surface without a buffer (research L15, contracts/protocols.md "wl_pointer"). Test first in `tests/protocol/input.rs` (`a_cursor_surface_without_an_image_hides_the_pointer`; the older set_cursor test now gives its cursor surface an image); then `src/server/seat.rs` remembers the cursor surface and `src/server/compositor.rs` checks it on each commit that takes effect
- [X] T036 [US1] Fix found in Q2 (2026-11-20, owner's choice): Firefox ignores `unlocked` after Mod+Escape and keeps the cursor hidden; until the click the window receives, a hidden cursor shows as the arrow over it (spec FR-008, research L16, contracts/input.md). Test first in `tests/pointer_constraints.rs` (`after_mod_escape_the_pointer_shows_over_the_window_until_a_click`); then `Shell::cursor` in `src/shell/mod.rs`

**Checkpoint**: mouse-look works in Firefox, games, gamescope and Blender; Mod+Escape frees the
pointer.

---

## Phase 4: User Story 2 - Keep the Pointer Inside a Window (Priority: P2)

**Goal**: confinement (spec US2, FR-005, FR-014).

**Independent Test**: quickstart Q8-Q9 (Q9a optional).

### Tests for User Story 2 (write first, see them fail)

- [X] T019 [P] [US2] Write failing unit tests for `confine(area: &[Rect], from: Point, to: Point) -> Point` in `src/shell/constraint.rs` (research L8):
  - `to` inside the area: `to`;
  - `to` outside: clamped into the rectangle that contains `from`, sliding along its edge (a diagonal move into a wall keeps the movement along the wall), kept a small margin inside the half-open rectangle as the layout does (`INSIDE` in `src/canvas/layout.rs`);
  - an area of several rectangles: crossing from one rectangle into a touching one is allowed;
  - `from` in no rectangle (the area changed): the nearest point of the area to `to`;
  - a proptest: for random areas of 1-8 rectangles, `from` inside and random moves, the result is always inside the area
- [X] T020 [US2] Write failing shell tests for confinement in `tests/pointer_constraints.rs` (FR-005, FR-014, US2 scenarios) (2026-11-20: the starting, empty-area, bounding-box and release cases passed as soon as written - US1's activation rules already cover them; the four that hold or move the pointer failed first; the window partly off the viewer is covered with two viewers in T026):
  - a confinement starts only once the pointer is inside the area (scenario 4);
  - fast moves toward every edge and corner: the pointer stays inside, `PointerMotion` reports the clamped position, `RelativeMotion` the full movement (scenario 1, SC-003 at zoom 50%, 100% and 200%);
  - an area of part of the window, and of several rectangles (scenario 2);
  - the area changes while it applies with the pointer outside the new one: the pointer moves to the nearest point inside, with a `PointerMotion` and no `RelativeMotion` (scenario 3);
  - an empty area never applies; the window partly off the viewer: the pointer stays in the shown part;
  - an area of 10,000 rectangles: the bounding box is used (FR-012);
  - Mod+Escape and a focus change stop it as for a lock (scenario 5)
- [X] T021 [P] [US2] Write failing protocol tests (2026-11-20: both passed as soon as written - T017 implemented locks and confinements together) in `tests/protocol/pointer_constraints.rs` for `confine_pointer`: reported with `kind: Confine`; `confined` / `unconfined` from the shell's decisions with the `oneshot` defunct rule; `set_region` applied on commit (null: the whole surface); destroying it frees the pointer with no move (contracts/protocols.md "zwp_confined_pointer_v1")

### Implementation for User Story 2

- [X] T022 [US2] Implement `confine` in `src/shell/constraint.rs`, so T019 passes
- [X] T023 [US2] Implement confinement in `src/shell/mod.rs`: while a confinement applies, the new pointer position goes through `confine` on the effective area (converted between layout and window coordinates through the viewer's camera); on an area change while it applies, move a pointer left outside to the nearest point inside; so T020 passes
- [X] T024 [US2] Implement the confined pointer in `src/server/pointer_constraints.rs` (`confined` / `unconfined`, the rest shared with the lock), so T021 passes (2026-11-20: done with T017, which handles both kinds)
- [ ] T025 [US2] Run the quick check and `cargo test`; then the owner runs quickstart Q8-Q9 (and Q9a if a remote desktop client is at hand) (2026-11-20: quick check and all tests green. 2026-11-22: Q8-Q9 passed with the SDL3 test client, confining to the window and to a smaller area; Q9a skipped, no remote desktop client at hand)

**Checkpoint**: confining applications keep the cursor inside their area.

---

## Phase 5: User Story 3 - The Canvas Stays in the User's Hands (Priority: P3)

**Goal**: pans, moves and resizes suspend a constraint silently; zoom and two viewers (spec US3,
FR-009, FR-013).

**Independent Test**: quickstart Q10-Q13.

### Tests for User Story 3 (write first, see them fail)

- [X] T026 [US3] Write failing shell tests in `tests/pointer_constraints.rs` (research L6, contracts/input.md "Other input while a constraint applies") (2026-11-20: all but the resize reason passed as soon as written - US1's `update_constraint` already does nothing while a gesture runs, and the confinement already uses the pointer's viewer; the window leaving the monitor is shown by moving the camera, since a pan carries the window with the pointer):
  - Mod + left-drag pan while a lock or confinement applies: no `Constraint` output and no `RelativeMotion` or `PointerMotion` during the pan (the `PointerLeave` a pan or operation sends must not stop it); afterwards it still applies, and the pointer is on the same window point within 1 logical pixel of the screen at zoom 50%, 100% and 200% (US3 scenario 1, SC-004);
  - Mod+Shift + left-drag move: the same; afterwards it applies again (scenario 3);
  - Mod + right-drag resize of a confined window: suspended; afterwards it goes on if the pointer is over the area, else `Stop(Resize)` (scenario 4);
  - Mod + scroll and pinch zoom: no suspension; the pointer keeps its window point and the lock or confinement keeps applying to the window as now shown (scenario 2);
  - a confined window on the second viewer at 50% zoom: the pointer stays in the area as shown on that viewer and never crosses to the other one; a window visible on both viewers: a lock holds the pointer where it is, a confinement uses the viewer the pointer is on (scenario 5, FR-013);
  - the window panned fully off the pointer's viewer: a confinement pauses, and applies again once the pointer is inside the area;
  - while a lock applies, Mod+Tab, Mod+Q, Mod+0, a spawn binding and quit work as usual (scenario 6);
  - the cursor during the pan, move or resize is the gesture's own (`Grabbing`, move, resize), and the application's hidden cursor is back after it (spec Edge Cases)

### Implementation for User Story 3

- [X] T027 [US3] Implement suspension in `src/shell/mod.rs` (2026-11-20: the suspension came with T015; this task added `ConstraintEnd::Resize` through `finish_operation`): an `Active` constraint is suspended while `router.operation` or a pan runs ("Suspended" is not a stored state, data-model); `update_constraint` emits nothing and the lock does not hold the pointer while suspended; when a pan or move ends the pointer is on the same window point and it applies on; when a resize ends the conditions are checked again with `ConstraintEnd::Resize`; a zoom only recomputes the effective area; so T026 passes
- [X] T028 [US3] Run the quick check and `cargo test`; then the owner runs quickstart Q10-Q13 (2026-11-20: quick check and all tests green. 2026-11-22: Q10, Q11 and Q13 passed with the SDL3 test client; Q12 skipped: the owner's external monitor is on the HDMI port of the NVIDIA GPU, which RubickWay does not open (one card only, drm-backend research) - covered by the two-viewer shell tests)

**Checkpoint**: the canvas stays usable while an application holds the pointer.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T029 [P] Add the contract's misbehaving rows (2026-11-20: M11, run end to end with a real `Shell`, found that a single-use lock asked for again at once captured the pointer right back after Mod+Escape; test first in `tests/pointer_constraints.rs`, then the shell keeps the window released until it receives a click - spec FR-008, data-model and contracts/input.md updated. M10 needs the stuck client to send a request now and then, or it never notices its closed socket) to `tests/protocol/misbehaving.rs`, each with a second, well-behaved client that keeps working (contracts/protocols.md M1-M11). M1-M5 and M7 are already tested in `tests/protocol/pointer_constraints.rs` (T013): add one test here per row that repeats the misbehaviour and checks only that a second client with a mapped window still gets input and commits. New here: M6 a 10,000-rectangle confinement area and window input shape (bounding box, no error); M8 10,000 lock/destroy pairs and 10,000 `set_region` + commit in one batch; M9 10,000 relative pointers (32 get events); M10 a client that never reads while getting relative motion (only it is disconnected); M11 a lock asked for again at once after every `unlocked` (Mod+Escape frees the pointer, a persistent one waits for a click); and SC-005: one client floods 10,000 constraints, a 10,000-rectangle area and 10,000 relative pointers in one batch, and a second client's input sent in the same step reaches it in that step
- [X] T030 Add the debug log of research L13 and FR-015 in `src/server/pointer_constraints.rs` (2026-11-20: the process description is read once at connect and kept in `ClientState`, so a lock reads nothing; research L13 updated): one line when a constraint starts and stops applying (from the shell's decisions, and from the stops the server records itself in T017), from a small pure function; write its unit test first: the line names the application (`application N (pid, name)` from `describe_process`), the kind (`locked` / `confined`), the lifetime (single-use or lasting) and, on stop, the reason (`application`, `release shortcut`, `focus`, `pointer left`, `window gone`, `resize`), and contains no coordinates
- [X] T031 [P] Update `README.md` (FR-016, plan "Supporting changes"): Mod+Escape in the controls table; under running applications, a paragraph on games and 3D tools capturing the pointer and taking it back with Mod+Escape (a lasting capture returns on a click); a spawn binding on Mod+Escape is refused
- [X] T032 [P] If implementation turned up something an agent would otherwise rediscover, propose a one-line gotcha for `AGENTS.md` to the owner (do not add it unasked) (2026-11-20: proposed - a paused test client that sends nothing never sees the server close its socket)
- [X] T033 Run the full check from `AGENTS.md` (2026-11-20: green) (`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked`)
- [X] T034 (waits for the owner) The owner runs quickstart Q14-Q15 (and the log grep from its Prerequisites) and reports; then `/speckit-converge` (2026-11-22: one line per start and stop, reasons `application`, `release shortcut`, `pointer left`, `window gone`, no coordinates; no warning or protocol error; one libinput "lagging behind by 72ms" right at the return from a console switch, events queued during the pause, not a stalled loop. Q15: Firefox and SDL both ask for lasting constraints; single-use ones are covered by the automated tests only)

---

## Dependencies & Execution Order

### Phase dependencies

- Setup (T001) -> Foundational (T002-T009) -> US1 (T010-T018) -> US2 (T019-T025) -> US3
  (T026-T028) -> Polish (T029-T034).
- US2 uses US1's effective area (T014), activation rules (T015) and server objects (T017); US3
  suspends what US1 and US2 apply. The order is the plan's and the priority's.

### Within each phase

- Test tasks before their implementation tasks; implementation is done when its tests pass.
- `tests/pointer_constraints.rs` is shared by T011, T012, T020 and T026: sequential, in that order.
- `tests/protocol/pointer_constraints.rs` is shared by T013 and T021; `src/shell/constraint.rs` by
  T010, T014, T019 and T022; `src/shell/mod.rs` by T015, T023 and T027; `src/server/pointer_constraints.rs`
  by T017, T024 and T030: each sequential.
- T016 (relative pointers) needs only the Foundational phase and can run beside T014-T015; T017
  comes after it, as both change `deliver` in `src/server/seat.rs`.

### Parallel opportunities

- Foundational tests: T002, T003, T004 (different files).
- US1: T010 and T013 beside the shell tests T011-T012; T016 beside T014-T015.
- US2: T019 and T021 beside T020.
- Polish: T029, T031 and T032 beside T030.

## Parallel Example: User Story 1

```text
Task: "T010 effective-area unit tests in src/shell/constraint.rs"
Task: "T011 relative-motion shell tests in tests/pointer_constraints.rs"
Task: "T013 protocol tests in tests/protocol/pointer_constraints.rs"
```

## Implementation Strategy

### MVP first

1. Setup and Foundational.
2. US1: relative motion, locks and Mod+Escape - what games, 3D tools and remote desktops need first;
   stop and run Q1-Q7.

### Incremental delivery

3. US2: confinement (Q8-Q9a).
4. US3: the canvas while the pointer is held (Q10-Q13).
5. Polish: misbehaving cases and SC-005, logging, README, Q14-Q15, converge.

If implementation has to deviate from the spec, plan or contracts, update them first (AGENTS.md).
Each story is committed only when the owner asks and the full check passes (`AGENTS.md`).
