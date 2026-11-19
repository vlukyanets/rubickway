---
description: "Task list for Popups"
---

# Tasks: Popups

**Input**: Design documents from `specs/20261118-130629-popups/`

**Prerequisites**: plan.md, spec.md, research.md (P1-P16), data-model.md, contracts/, quickstart.md

**Tests**: Required by Principle III: each test task is written and seen failing before its
implementation task. No test needs a device.

**Organization**: Tasks are grouped by user story, in the plan's order of work: foundation, US1 (P1),
US2 (P2), US4 (P2), US3 (P3), polish. Each step leaves a working desktop.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US4)

---

## Phase 1: Setup

- [X] T001 Run the full check from `AGENTS.md` in the worktree to confirm a green baseline (`cargo fetch` is done; no new crates, research P1)

---

## Phase 2: Foundational (blocks all stories)

**Purpose**: the placement function, the full positioner state, the new event, command and input
types with no behaviour yet, and `layers()` with only the toplevel. Nothing visible changes; today's
tests stay green (apart from the helpers that build `Mapped`).

### Tests (write first, see them fail)

- [X] T002 [P] Write failing unit tests in `src/shell/positioner.rs` (research P3, data-model "PopupRules", "place"):
  - `Anchor8::from_u32` accepts 0-8 and rejects 9 and `u32::MAX`; `Adjust::from_bits` accepts `0..=0x3f` and rejects `0x40`;
  - step 1 (no area): a table over all 9 anchors x 9 gravities for anchor rectangle `(100, 50, 40, 20)`, size `(30, 10)`, offset `(0, 0)`: anchor point on the named edge or corner (the centre for `None`), popup grown in the gravity's direction and centred on an axis the gravity does not name; then the same with offset `(5, -7)`;
  - `area: None` and `adjust` empty both give exactly step 1;
  - flip: a popup off the area's right edge with `flip_x` flips to the left of the anchor rectangle; if the flipped result is also outside on x, the unflipped result is kept; the same for `flip_y`;
  - slide: off the right edge slides left until inside; larger than the area: aligned to the area's left (top) edge;
  - resize: cut to the intersection; a popup wholly outside keeps its size; width and height stay at least 1;
  - order: with `flip_x | slide_x | resize_x`, flip is tried first, then slide, then resize;
  - saturation: size, anchor rectangle and offset at `i32::MAX` / `i32::MIN` give a result without panic, every field within `i32`;
  - proptest: with `slide_x | slide_y` and a size that fits the area, the result is inside the area; with no adjustment, the result equals step 1; flipping anchor and gravity twice gives the original values.
- [X] T003 [P] Write failing cases in `tests/protocol/misbehaving.rs` for the positioner values (contracts/protocols.md "xdg_positioner"): `set_anchor(9)`, `set_gravity(u32::MAX)` (sent as a raw value), `set_constraint_adjustment(0x40)` and `set_parent_size(-1, 5)` each give `invalid_input` to that client only; a second client keeps working.

### Implementation

- [X] T004 Implement `src/shell/positioner.rs` (`IRect`, `Anchor8`, `Adjust`, `PopupRules` with fields "size (at least 1 x 1), anchor_rect (width and height at least 0), anchor, gravity, adjust, offset, reactive, parent_size", `place(rules, area: Option<IRect>) -> IRect` computing in `i64` and saturating to `i32`), add `mod positioner` and re-exports in `src/shell/mod.rs`, so T002 passes
- [X] T005 In `src/server/xdg_shell.rs`, make `PositionerState` keep every value (size, anchor rectangle, anchor, gravity, adjustment, offset, reactive, parent size, parent configure serial) and build a `PopupRules`; reject out-of-range values with `invalid_input` as contracts/protocols.md says, so T003 passes
- [X] T006 Add the new types with no behaviour yet (data-model "Shell"): `PopupId` in new `src/shell/popups.rs`; `AppId` and `Window.app` in `src/shell/windows.rs`; `app: AppId` on `WindowEvent::Mapped` (the server fills it from `client_number` in `src/server/xdg_shell.rs` `map`); the `WindowEvent` variants `PopupRequested`, `PopupRepositionRequested`, `PopupMapped`, `PopupUpdated`, `PopupUnmapped`; the `WindowCommand` variants `PlacePopup`, `DismissPopup`; `ClientInput::PopupKeyboard` in `src/shell/route.rs`. `Shell::apply` ignores the popup events and the server ignores the new commands and input for now. Update the `Mapped` helpers in `tests/shell.rs` (`mapped`, `mapped_with_parent` and the other places that build `Mapped`) to pass an `AppId`
- [X] T007 Add `layers(state, window) -> Vec<(ObjectId, Point)>` in new `src/server/popup.rs` (returning only the toplevel for now) and move to it every walk over a window's surfaces: `window_parts`, `take_damage`, `presented`, `has_pending_callbacks` in `src/server/mod.rs`; `surface_at`, `relative_to` and keyboard focus in `src/server/seat.rs`; `cross` / `set_window_outputs` in `src/server/output.rs`; `announce_scale` callers. No behaviour change
- [X] T008 Run the quick check (`cargo clippy --all-targets -- -D warnings && cargo test --lib`) and `cargo test`; everything is green

**Checkpoint**: placement exists and is tested; the server and shell know the new types; desktop
unchanged.

---

## Phase 3: User Story 1 - Menus Open and Can Be Used (Priority: P1) MVP

**Goal**: a right-click menu or dropdown appears at the right place, takes the pointer and keyboard,
and closes on a click outside (spec US1).

**Independent Test**: quickstart Q1-Q7 (Firefox, GTK 4, Qt 6).

### Tests for User Story 1 (write first, see them fail)

- [X] T009 [P] [US1] Add helpers at the top of `tests/protocol/popups.rs` (next to the tests that use them; `harness.rs` stays generic): a complete positioner from size, anchor rectangle, anchor, gravity and adjustment; a popup on a parent `xdg_surface` (surface, `xdg_surface`, `xdg_popup`); a press on a window that returns the client's `wl_pointer.button` serial; ack the latest `xdg_surface.configure` and commit a buffer of a given size
- [X] T010 [P] [US1] Write failing tests in new `tests/protocol/popups.rs` (add `mod popups` to `tests/protocol/main.rs`) for the life of a popup (contracts/protocols.md "Life of a popup", research P12):
  - `get_popup` on a mapped window and an initial commit give `WindowEvent::PopupRequested { id, window, parent: None, rules, grab: false }`, with `rules` equal to the positioner's values;
  - `PlacePopup { id, at, token: None }` sends `xdg_popup.configure(at)` then `xdg_surface.configure`;
  - a buffer committed before the ack: `unconfigured_buffer`;
  - ack and a buffer: `PopupMapped { id, at, size, input }`; `window_parts(window)` lists the popup's surface after the window's own surfaces, at `at` in window coordinates;
  - a commit with a new buffer size: `PopupUpdated`; destroying the popup: `PopupUnmapped`;
  - a commit without a buffer: `PopupUnmapped` and `popup_done`; a buffer committed after that gives no event and nothing in `window_parts` (research P12, analyze U1);
  - destroying the popup's `wl_surface` before its `xdg_popup`: `PopupUnmapped`, no protocol error (analyze I3);
  - `DismissPopup { id }`: `popup_done`; later commits with buffers give no event and the popup is not in `window_parts` (FR-012);
  - `presented(time, &[window])` completes the popup surface's frame callbacks; `has_pending_callbacks` sees them;
  - the popup surface gets `wl_surface.enter` for the window's outputs, and a `wp_fractional_scale_v1` made for it gets the preferred scale as the window's surfaces do;
  - no parent: `popup_done` after the initial commit, no `PopupRequested`; a parent window unmapped before the popup's initial commit: `popup_done`; a parent `xdg_surface` without a role: `invalid_popup_parent`;
  - `xdg_surface.destroy` before its `xdg_popup`: `defunct_role_object`.
- [X] T011 [P] [US1] Replace the popup part of `popups_are_dismissed_and_wm_requests_ignored` in `tests/protocol/surfaces.rs`: keep the window-menu and minimize checks; the popup is no longer dismissed at once (now covered by T010)
- [X] T012 [P] [US1] Write failing grab and input tests in `tests/protocol/popups.rs` (research P6, P7, P9; contracts/protocols.md "wl_pointer and wl_keyboard during a grab"):
  - `grab` with the serial of the client's latest button press, before the initial commit: `PopupRequested { grab: true }`; the press serial still counts after the button's release;
  - the serial of the client's latest key press or release counts too;
  - serial 0, a stale serial, or another client's serial: `popup_done` at the initial commit and no `PopupRequested`, no error;
  - `grab` after the initial commit: `invalid_grab`;
  - `ClientInput::PopupKeyboard { window, popup: Some(id) }`: `wl_keyboard.leave` on the root surface, `enter` on the popup surface with the pressed keys, then `modifiers`; no new `xdg_toplevel.configure` (activated unchanged); `Key` goes to the popup surface; `PopupKeyboard { popup: None }` moves it back to the root surface;
  - with a shown grabbing popup and a button held on the window, `PointerMotion` into the popup's area gives `leave` on the root surface and `enter` on the popup surface; without a grab (today), the pointer stays on the pressed surface;
  - with the pointer over the popup surface, `wp_cursor_shape_device_v1.set_shape` with that enter's serial gives `CursorChanged { id: window, .. }` for the popup's window (FR-017, analyze C1).
- [X] T013 [P] [US1] Write failing unit tests in `src/shell/windows.rs` (research P5): `extent()` without popups equals `rect()`; with a shown popup sticking out to the right, it covers both; `Windows::visible` includes a window whose popup alone intersects the view; `hit` returns the window and a point in window coordinates for a point inside a popup outside the window's rect; a popup is hit before the window's own input under it; a higher window covering a lower window's popup wins (stack order, FR-004)
- [X] T014 [P] [US1] Write failing unit tests in `src/shell/popups.rs` for the constraint area (research P4): the viewer under the pointer when it shows part of the window; otherwise the viewer showing the largest part of the window; `None` when no viewer shows it; the area in the parent's coordinates, rounded inwards, for a window at a fractional canvas origin and a parent popup at an offset; at camera zoom 0.5 the area is twice as large in each direction as at zoom 1
- [X] T015 [P] [US1] Write failing tests in `tests/shell.rs` (research P2-P8, contracts/input.md):
  - `PopupRequested` answers `PlacePopup` with `place(rules, area)` and the area of T014; `PopupMapped` puts the popup in its window, marks the viewers dirty, and `visible_in` includes the window when only the popup is in view;
  - the pointer over the popup gives `PointerEnter { window, pos }` with `pos` inside the popup;
  - a grabbing popup: `PopupKeyboard { window, popup: Some(id) }` after `PopupMapped`; keys still go out as `Key { window }`;
  - during the grab, the pointer over another application's window gives `PointerLeave` and no enter; over a second window of the same `AppId` it is delivered;
  - a press outside (the background, another application's window): `DismissPopup` for every popup of the chain, topmost first, then `PopupKeyboard { popup: None }`; the press and its release produce no `PointerButton`, no pan, no focus change (FR-009);
  - a press on the popup, or on the window outside the popups: delivered as usual, no `DismissPopup` (owner events, analyze I1);
  - a press on a second window of the same `AppId`: `PointerButton` is delivered to that window, which is focused and raised; the focus change emits `DismissPopup` for all popups, topmost first, and `PopupKeyboard { popup: None }` before the `KeyboardFocus` of the new window (FR-010, contracts/input.md, analyze M2);
  - during the grab, Mod + left on another application's window: `DismissPopup` for all popups, then the move starts at once (no press swallowed); a plain left press on the background: dismissed and swallowed, no pan; with only a tooltip open (no grab), a left press on the background dismisses the tooltip and pans (contracts/input.md, analyze A1);
  - a window shown on two viewers with its popup in the area both show: `visible_in` of both viewers includes the window (FR-015);
  - a grabbing `PopupRequested` while a move, resize or pan runs: answered with `DismissPopup`;
  - a grabbing popup of another application while a chain is open: the old chain is dismissed first, then the new one is placed;
  - a right press on the window, then a grabbing popup mapped, then motion over the popup with the button held: `PointerMotion` reaches the window with `pos` in the popup; the release is delivered; after the grab ends the pointer is retargeted.

### Implementation for User Story 1

- [X] T016 [US1] Implement the popup life in `src/server/popup.rs` and wire it from `src/server/xdg_shell.rs` (`GetPopup`, `AckConfigure`, `tree_committed` for popup roots, `Dispatch<XdgPopup>`, `xdg_surface` destroy, `command`), data-model "PopupData": parent checks (`invalid_popup_parent`; no parent or unmapped parent: `popup_done` at the initial commit); the initial commit gives an id and `PopupRequested`; `PlacePopup` sends the configure pair and records `(serial, placement)`; an ack selects the placement, applied at the next commit; map, update and unmap report `PopupMapped`, `PopupUpdated`, `PopupUnmapped` only on change; `DismissPopup` sends `popup_done`, sets `dismissed`, and nothing is shown again; a commit without a buffer ends the popup (children dismissed first, then `popup_done` and `PopupUnmapped`, research P12); destroying its `wl_surface` first ends it the same way with no error (through `xdg_shell::surface_destroyed`); `layers()` adds shown, not dismissed popups in creation order; `server.dirty = true` on map, unmap and placement, so T010 and T011 pass
- [X] T017 [US1] Implement the grab request in `src/server/popup.rs` and the seat side in `src/server/seat.rs` (research P6, P9, data-model "SeatState"): `popup_serials` per client, updated on every delivered button press and key press or release and removed when the client goes; `grab` checks (serial, before the initial commit, else `invalid_grab`; a wrong serial marks it refused and dismisses it at the initial commit); `ClientInput::PopupKeyboard`; re-picking the surface among the window's layers while that client has a shown grabbing popup, even with buttons held, so T012 passes
- [X] T018 [US1] Implement popups in the shell in `src/shell/popups.rs` and `src/shell/windows.rs` (data-model "Popup", "Window"): `Window.popups`; `extent()`; hit-testing popups first; the constraint area (research P4); handling `PopupRequested` (place and answer), `PopupMapped`, `PopupUpdated`, `PopupUnmapped` (drop it and its children), with the dirty flag, so T013 and T014 pass
- [X] T019 [US1] Implement the grab in `src/shell/route.rs` and `src/shell/mod.rs` (research P6-P8, data-model "PopupGrab"): accept or dismiss a grabbing popup; `PopupKeyboard` when the chain's top changes and `None` when it ends; `Shell::hit` limited to the grab's `AppId`; `button` dismisses on a press outside and swallows the press and its release; `scroll` outside does nothing; the implicit `router.grab` becomes the popup grab; `dismiss_popups(scope)` pushing `DismissPopup` topmost first, so T015 passes
- [X] T020 [P] [US1] Write the test program `examples/popups.rs` (research P16) with `wayland-client`, in the style of `examples/cursor_shapes.rs`: one window; right-click opens a grabbing menu at the pointer (`flip | slide | resize` on both axes); in the menu, keys open a nested submenu to the right, a menu that allows no adjustment, a tall menu (resize), a non-grabbing tooltip, a reactive popup, and a reposition of the open popup; it prints its keys and each configure it gets
- [X] T021 [US1] Run the quick check; then the owner runs quickstart Q1-Q7 on the laptop (quick check passed 2026-11-18; Q1-Q7 passed on the laptop 2026-11-19)

**Checkpoint**: menus work in real applications.

---

## Phase 4: User Story 2 - Submenus and Menus at the Edge of the View (Priority: P2)

**Goal**: nested menus, reposition and reactive popups; menus kept on the monitor at any zoom (spec
US2).

**Independent Test**: quickstart Q8-Q12.

### Tests for User Story 2 (write first, see them fail)

- [X] T022 [P] [US2] Write failing tests in `tests/protocol/popups.rs` (research P11, P12). The nested-popup and topmost tests passed as soon as they were written: US1 (T016-T019) already built them. Only the reposition tests failed first; an incomplete positioner in `reposition` (`invalid_positioner`) was added to the contract:
  - a grabbing child of a grabbing popup: `PopupRequested { parent: Some(parent_id), grab: true }`; `layers()` and `window_parts` put the child after its parent; the pointer over the overlap enters the child;
  - a grabbing child mapped while its parent is not the topmost grabbing popup: `not_the_topmost_popup`;
  - destroying a popup with a shown child: `not_the_topmost_popup`;
  - a grabbing child of a popup without a grab: `invalid_grab`; a grabbing child of a dismissed grabbing popup: `popup_done`;
  - a parent popup hidden by a commit without a buffer while its child is shown: the child gets `popup_done` first, then the parent; `PopupUnmapped` for both, child first (FR-011, analyze C2);
  - `reposition(positioner, 7)`: `PopupRepositionRequested { id, rules, token: 7 }`; `PlacePopup { token: Some(7) }` sends `repositioned(7)`, `configure`, `xdg_surface.configure`; the old position stays until the commit after the ack, then `PopupUpdated` with the new `at`;
  - 10 000 `reposition` requests in one batch: one `PopupRepositionRequested`, with the newest token;
  - `set_parent_size(200, 100)` appears in the rules as `parent_size`.
- [X] T023 [P] [US2] Write failing tests in `tests/shell.rs` (research P3, P4, P10). Placement, zoom, keyboard and origin tests passed at once (US1 built them; "a press outside dismisses the whole chain" is the US1 test `a_press_outside_the_application_closes_its_menus_and_is_swallowed`); the reactive and reposition tests failed first:
  - a child popup is placed against its parent popup's shown rect, with the area converted into the parent popup's coordinates;
  - the keyboard moves to the child (`PopupKeyboard`) when it maps and back to the parent when the child is unmapped;
  - a press outside dismisses the whole chain, innermost first;
  - a window near a viewer's right edge: a submenu with `flip_x` is placed to the left;
  - at zoom 0.5 the same menu fits where it would not at zoom 1;
  - a window `Updated` with a new size: `PlacePopup` without token for each reactive popup (and its reactive children, after the parent is placed again); non-reactive popups get nothing;
  - `PopupRepositionRequested` answers `PlacePopup` with its token;
  - a window resized from its left edge by the application (its origin moves at `Updated`): a non-reactive popup keeps its position relative to the window, and hit-testing finds it at the new canvas position (FR-005, analyze C3).

### Implementation for User Story 2

- [X] T024 [US2] Implement nested popups, the topmost rules and reposition in `src/server/popup.rs` (keep only the newest waiting reposition), so T022 passes
- [X] T025 [US2] Implement nested placement, the keyboard moving along the chain, reactive re-placement and reposition answers in `src/shell/popups.rs` and `src/shell/mod.rs`, so T023 passes
- [X] T026 [US2] Run the quick check; then the owner runs quickstart Q8-Q12 (quick check passed 2026-11-19; Q8-Q11 passed on the laptop 2026-11-19, log clean; Q12 not run: no second monitor at hand)

---

## Phase 5: User Story 4 - Canvas Actions Close Popups Cleanly (Priority: P2)

**Goal**: every RubickWay action closes all popups first, and windows and displays going away take
their popups with them (spec US4, FR-010, FR-011).

**Independent Test**: quickstart Q16-Q22.

### Tests for User Story 4 (write first, see them fail)

- [X] T027 [P] [US4] Write failing tests in `tests/shell.rs` (research P8, contracts/input.md "Closing popups"). Each opens a grabbing menu with a submenu in window A and a tooltip in window B, then does the action, and checks `DismissPopup` for all three, each chain topmost first, `PopupKeyboard { popup: None }` when there was a grab, and that the action then does what it does without popups:
  - key bindings: Mod+Tab, Mod+Shift+Tab, Mod+Q, Mod+0, a spawn binding, the first Mod+Shift+Q;
  - pointer bindings: move (Mod + left), resize (Mod + right), pan (Mod+Shift + left);
  - Mod + scroll, and a pinch;
  - keyboard focus moving to another window because the focused window unmapped;
  - maximize and fullscreen entered and left, whoever asked: an application's request granted by the shell, and a pan press that leaves fullscreen (analyze I4);
  - `remove_viewer` of the viewer showing the menu;
  - `Unmapped` of window A: its popups go with it without `DismissPopup`, the grab ends, the keyboard goes to the focused window. The tooltip in B then closes too, because the focus moves to B (the focus rule above); the task first said it stays, which contradicted that rule (found 2026-11-19).
  - The bound pan and the pan press leaving fullscreen passed as soon as they were written (US1 built them).
- [X] T028 [P] [US4] Write failing tests in `tests/protocol/popups.rs` (research P13): unmapping or destroying a toplevel with a nested grabbing chain sends `popup_done` to each popup, innermost first, and they leave `window_parts`; a client disconnecting with a grab open leaves no popup in `layers()` (this one passed at once: destroyed objects already end their popups; checked through `window_parts` and a second client that keeps working, as `layers()` is not public)

### Implementation for User Story 4

- [X] T029 [US4] In `src/shell/mod.rs`, call `dismiss_popups(All)` before every binding action (key and pointer; a pointer binding is checked before the grab's press-outside rule and is not swallowed), at the start of a move, resize or pan, in `focus_changed` when the window changes, in every maximize and fullscreen path whoever asked, and in `remove_viewer`; snapshot every viewer's camera at the start of `handle()` and dismiss after it if any changed; drop a window's popups and end its grab at `Unmapped`, so T027 passes
- [X] T030 [US4] In `src/server/popup.rs` and `src/server/xdg_shell.rs` `unmap`, dismiss all popups of a toplevel that unmaps or is destroyed, innermost first, so T028 passes
- [ ] T031 [US4] Run the quick check; then the owner runs quickstart Q16-Q22 (quick check passed 2026-11-19; Q16-Q21 passed on the laptop 2026-11-19, log clean; Q22 not run: no second monitor at hand)

---

## Phase 6: User Story 3 - Tooltips and Other Popups Without a Grab (Priority: P3)

**Goal**: popups without a grab are shown, take no keyboard, and are not closed by clicks elsewhere
(spec US3, FR-014).

**Independent Test**: quickstart Q13-Q15.

### Tests for User Story 3 (write first, see them fail)

- [X] T032 [P] [US3] Write failing tests in `tests/shell.rs` (passed as soon as written: see T034): a popup with `grab: false` is placed and shown; no `PopupKeyboard`; a click elsewhere in its window is delivered and no `DismissPopup` follows; the pointer over it enters its window with `pos` inside it; keys go to the focused window; a reposition moves it
- [X] T033 [P] [US3] Write a failing test in `tests/protocol/popups.rs` (passed as soon as written: see T034): with a tooltip shown, `PointerEnter` at its position enters the tooltip surface; `Key` still goes to the window's root surface

### Implementation for User Story 3

- [X] T034 [US3] Fix whatever T032 and T033 show in `src/shell/mod.rs`, `src/shell/popups.rs` or `src/server/popup.rs` (US1 already handles most of it). If T032 or T033 pass as soon as they are written, record that here, as US1 already built the behaviour (Principle III, analyze P1). Then the owner runs quickstart Q13-Q15. Recorded 2026-11-19: `a_tooltip_takes_no_keyboard_and_survives_clicks_in_its_window` and `a_tooltip_gets_the_pointer_but_not_the_keyboard` passed as soon as they were written - US1 (grab only for grabbing popups) and US2 (reposition) built the behaviour; no code change. Q13-Q15 passed on the laptop 2026-11-19

---

## Phase 7: Polish & Cross-Cutting Concerns

- [X] T035 [P] Add the remaining misbehaving cases of contracts/protocols.md to `tests/protocol/misbehaving.rs` and make them pass: huge positioner values (placed with saturation, the client keeps running), commits after `popup_done`, `ack_configure` of a serial never sent, a client disconnecting with a grab open (other clients keep working). Huge values and the unsent serial are in `misbehaving.rs`; commits after `popup_done` and the disconnect were already covered in `popups.rs` (T010, T028). All passed as soon as written
- [X] T036 [P] Update `README.md`, "Running applications on RubickWay": menus, dropdowns and tooltips work; a click outside a menu closes it and is not passed on; any RubickWay action closes open menus
- [X] T037 Run the full check from `AGENTS.md` (`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked`) (passed 2026-11-19. Seen once in about 100 runs of `cargo test --lib`: `ffi::spawn::tests::no_finished_child_stays_behind` reaps another test's child with `waitpid(-1)`; existed before this feature, `src/ffi` unchanged here)
- [X] T038 The owner runs quickstart Q23 and reports; then `/speckit-converge` (Q23 passed on the laptop 2026-11-19: no slow frame, no `lagging behind`, no leftover menu)

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: none
- **Foundational (Phase 2)**: after Setup; blocks all stories
- **US1 (Phase 3)**: after Foundational; the MVP
- **US2 (Phase 4)**: after US1 (nested popups build on single ones)
- **US4 (Phase 5)**: after US1; independent of US2, but its tests use a submenu, so do it after US2
- **US3 (Phase 6)**: after US1
- **Polish (Phase 7)**: after all stories

### Within each story

- Tests first, seen failing, then implementation (Principle III)
- Server before shell where the shell's tests need server events; pure code (`place`, the area) before
  both

### Parallel opportunities

- Foundational tests: T002, T003
- US1 tests: T009-T015; T020 alongside the implementation
- US2 tests: T022, T023
- US4 tests: T027, T028
- US3 tests: T032, T033
- Polish: T035, T036

### Parallel example: User Story 1

```text
T010 tests/protocol/popups.rs (life of a popup)
T013 src/shell/windows.rs (extent, hit-testing)
T014 src/shell/popups.rs (constraint area)
T015 tests/shell.rs (placement answers, grab routing)
```

## Implementation Strategy

### MVP first (User Story 1)

1. Setup + Foundational: nothing visible changes
2. US1: menus open, take input and close on a click outside; validate with Q1-Q7
3. Stop and let the owner try it

### Incremental delivery

1. US2: submenus, edges, zoom, reposition, reactive
2. US4: every RubickWay action closes popups
3. US3: tooltips
4. Polish: misbehaving cases, README, Q23

Each story is committed only when the owner asks and the full check passes (`AGENTS.md`).

## Phase 8: Convergence

- [X] T039 When a grab ends (the chain empties in `sync_popup_keyboard`, `src/shell/mod.rs`), retarget the pointer at once so the window now under it gets `PointerEnter` (and the old one `PointerLeave`) without waiting for a motion; add the missing assertion of the T015 bullet "after the grab ends the pointer is retargeted" in `tests/shell.rs` (pointer left on another application's window during a grab, the application closes its menu, `PointerEnter` for that window follows) per FR-013 (partial)
- [X] T040 Place a popup's reactive children again when the application changes the popup's size (`PopupUpdated` with a new size in `popup_shown`, `src/shell/mod.rs`), as for a window's reactive popups, with a test in `tests/shell.rs` per FR-005 (partial)
