---
description: "Task list for Cursor Shapes"
---

# Tasks: Cursor Shapes

**Input**: Design documents from `specs/20261109-110128-cursor-shapes/`

**Prerequisites**: plan.md, spec.md, research.md (C1-C12), data-model.md, contracts/, quickstart.md

**Tests**: Required by Principle III: each test task is written and seen failing before its
implementation task. No test needs a device; themes are directories made by the test.

**Organization**: Tasks are grouped by user story. As the plan's order of work says, the foundation
(the new cursor state and the C2 rule, through `set_cursor` only) comes first, so each step leaves a
working desktop.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup

- [X] T001 Run the full check from `AGENTS.md` in the worktree to confirm a green baseline (`cargo fetch` is done; no new crates, research C1)

---

## Phase 2: Foundational (blocks all stories)

**Purpose**: plain shape names in the shell; the server's cursor state per pointer enter with the C2
rule; the image table keyed by shape. Visible behaviour stays as today, except the C2 change to
hiding.

### Tests (write first, see them fail)

- [X] T002 [P] Write failing unit tests in `src/shell/cursor.rs`:
  - `CursorName::ALL` has the 36 shapes in protocol order (data-model "CursorName");
  - `css_name()` is unique per shape and gives the CSS name: `default`, `context-menu`, `vertical-text`, `not-allowed`, `no-drop`, `e-resize`, `nwse-resize`, `col-resize`, `all-scroll`, `zoom-out`, `dnd-ask`, `all-resize`, and so on.
- [X] T003 [P] Write failing tests in `tests/shell.rs`, replacing `the_pointer_is_hidden_only_over_a_window_that_hid_it_and_without_a_gesture` (research C3, data-model "CursorShape"):
  - `CursorChanged { id, cursor: Hidden }` applied after the pointer entered `id` gives `CursorShape::Hidden`.
  - `Shape(Text)` gives `Named(Text)`, only while the entered window is `id`.
  - An event for a window that is not entered changes nothing.
  - Leaving the window and coming back without a new event gives `Default`.
  - A move, resize or pan gesture shows `Move`, `Resize` or `Grabbing` over a named or hidden window. When it ends over the window, the shell emits `ClientInput::PointerEnter` for it and the cursor is `Default` until a new event (FR-005).
  - `Unmapped` of the entered window forgets its cursor.
  - A window shown on two viewers: moving the pointer from one viewer to the other over that window emits no `PointerLeave` or `PointerEnter`, and keeps `Named(Text)` (US1 scenario 6, review CHK004).
- [X] T004 [P] Rewrite `set_cursor_without_a_surface_hides_the_pointer_over_the_clients_windows` in `tests/protocol/input.rs` for the C2 rule, and add failing cases there (contracts/protocols.md "wl_pointer"). The test first delivers `ClientInput::PointerEnter` and reads the serial from the client's `wl_pointer.enter`. Cases:
  - `set_cursor(serial, None)` gives `CursorChanged { id, Hidden }`; the same again gives nothing.
  - `set_cursor(serial, Some(surface))` gives `CursorChanged { id, Shape(Default) }`.
  - `set_cursor` with serial `serial + 1`, before any enter, or after `PointerLeave` gives nothing.
  - A new enter after `Hidden` gives `CursorChanged { id, Shape(Default) }`.
  - A surface with another role is still error `role`.
- [X] T005 [P] Update the unit tests in `src/render/cursor.rs` for the new `CursorKind` (data-model "CursorKind"):
  - every one of the 37 kinds has a 24x24 image with its hotspot inside and something drawn;
  - `texture_key()` is unique per kind and is `u64::MAX - index`;
  - today's 7 images keep their pixels and hotspots under their new kinds: `Shape(Default)`, `Shape(Grabbing)`, `WindowMove`, and the four axis shapes;
  - `theme_names()` of these 7 equal today's lists (US3, review CHK017).
- [X] T006 [P] Write failing unit tests in `src/standalone.rs` for `pointer_kind`'s mapping, as a pure function of `CursorShape` (data-model "App"):
  - `Default`, `Grabbing`, `Move`, `Resize(edges)` map to their kinds, and `Hidden` to `None`;
  - for all eight edge combinations, `resize_name(edges)` gives the same axis as today's `resize_kind`;
  - `Named(n)` maps to `Shape(n)`.

### Implementation

- [X] T007 Implement `src/shell/cursor.rs` (`CursorName`, `AppCursor`) and re-export both from `src/shell/mod.rs`, so T002 passes
- [X] T008 In `src/shell/mod.rs`, so T003 passes:
  - replace `WindowEvent::CursorHidden` by `CursorChanged { id, cursor: AppCursor }`;
  - replace the `cursor_hidden` list by `app_cursor: Option<(WindowId, AppCursor)>`, cleared in `leave()` and at `Unmapped`;
  - add `CursorShape::Named(CursorName)`;
  - write `cursor()` in the order of data-model "CursorShape".
- [X] T009 In `src/server/seat.rs` (data-model "SeatState"), so T004 passes:
  - remove `hidden_cursor` and `cursor_hidden()`;
  - add `pointer_window`, `enter_serial` and `focus_cursor`;
  - make `pointer_enter` take the window, store the serial and reset to `Shape(Default)`, pushing `CursorChanged` only when the cursor was something else;
  - add `request_cursor(state, client, serial, AppCursor)` with the C2 checks, pushing `CursorChanged` only on change and writing the debug lines of contracts/cli.md;
  - make `wl_pointer.set_cursor` go through it.
- [X] T010 In `src/render/cursor.rs`, so T005 passes:
  - change `CursorKind` to `Shape(CursorName) | WindowMove` with `ALL` (37) and `texture_key()`;
  - map today's 7 drawings onto their kinds; every other kind gets the arrow for now;
  - keep today's theme names for those 7, and the theme loading as it is.
- [X] T011 In `src/standalone.rs`, so T006 passes: write `pointer_kind` and `resize_name` for the new kinds, and upload all 37 images
- [X] T012 Run the full check. Hiding works by the C2 rule; everything else is as before. (2026-11-09: fmt, clippy and the debug tests, 596 passed; release tests in T031.)

**Checkpoint**: today's behaviour, with the new cursor state underneath.

---

## Phase 3: User Story 1 - The Pointer Shows What the Application Asks For (Priority: P1) MVP

**Goal**: kitty and other applications name shapes through `wp_cursor_shape_manager_v1` version 2, and
RubickWay shows them with its own images.

**Independent Test**: quickstart Q1-Q6a on the laptop with no theme.

### Tests for User Story 1 (write first, see them fail)

- [X] T013 [P] [US1] Add `mod cursor_shape;` to `tests/protocol/main.rs`, and write failing tests in new `tests/protocol/cursor_shape.rs` (contracts/protocols.md, research C2-C4). The tests bind the manager themselves through a `shape_client` helper; the harness needed no change, as its recorder takes any interface. `globals_and_versions_match_the_contract` in `tests/protocol/surfaces.rs` gains the new global.
  - The global is advertised at version 2.
  - `get_pointer` then `set_shape(enter serial, text)` gives `CursorChanged { id, Shape(Text) }`; the same again gives nothing.
  - Each value 1-36 gives its `CursorName`, in protocol order.
  - Ignored:
    - a stale serial;
    - a request from a second client while the pointer is over the first client's window (the first client's cursor stays);
    - a request after `PointerLeave`;
    - a request after the device's `wl_pointer` was destroyed, with no error.
  - Lifecycles:
    - `device.destroy` changes nothing;
    - a device from a destroyed manager still works;
    - with two devices for one pointer, the latest request of either counts;
    - with `set_cursor` and `set_shape` mixed, the latest counts (FR-004).
  - Entering a subsurface of the same window resets to `Shape(Default)` with one `CursorChanged`.
- [X] T014 [P] [US1] Write failing cases in `tests/protocol/misbehaving.rs` (contracts/protocols.md "Misbehaving applications"):
  - shape 0, 37 and `u32::MAX` give `invalid_shape` to that client only; a second client's cursor and connection are unaffected;
  - on a manager bound at version 1, shapes 35 and 36 give `invalid_shape`, while 34 works;
  - 10 000 `set_shape` requests in one batch: alternating shapes give one `CursorChanged` per change, repeats give none;
  - a client that disconnects while its shape is shown: its window is unmapped, and nothing is left behind for the next client.
- [X] T015 [P] [US1] Write failing unit tests for the built-in drawings in `src/render/cursor.rs` (research C5, C10, spec FR-006 - FR-008):
  - the 19 drawings differ from each other pixel by pixel, including the mirrored and transposed ones;
  - all art rows are 12 characters of `#`, `.` or space;
  - entries that share a drawing have identical images: `e`/`w`/`ew`/`col` resize, `n`/`s`/`ns`/`row`, `ne`/`sw`/`nesw`, `nw`/`se`/`nwse`; `no_drop` and `not_allowed`; `all_scroll`, `all_resize` and `WindowMove`; and the five FR-007 arrow shapes with `default`;
  - `has_own_art` is false exactly for `context_menu`, `alias`, `copy`, `move` and `dnd_ask`;
  - hotspots:
    - (0, 0) for the arrow, help, progress and the arrow stand-ins;
    - a drawn pixel in the top two rows for the pointing hand;
    - the lens centre, inside the lens outline, for zoom in and out;
    - (12, 12) for the rest.
- [X] T016 [P] [US1] Write failing unit tests in `src/standalone.rs` for the FR-007 first-use line, a pure function of the noted set and the image entry:
  - it gives the contracts/cli.md line once per shape per run;
  - it gives nothing for shapes with their own drawing or with a theme image.

### Implementation for User Story 1

- [X] T017 [US1] Implement new `src/server/cursor_shape.rs`, so T013 and T014 pass (research C1, C4; data-model "Cursor shape objects"):
  - the manager global at version 2, created from `src/server/mod.rs`;
  - a device keeping its `WlPointer`;
  - `set_shape` with the `WEnum::Unknown` and since-2 checks, then `request_cursor` if the pointer is alive;
  - a pure `shape_name()`.
- [X] T018 [US1] Draw the 12 new 12x12 drawings in `src/render/cursor.rs`: pointer, help, progress, wait, cell, crosshair, text, vertical text (the I-beam transposed), not allowed, open hand, zoom in, zoom out (zoom in without its vertical stroke). Add `Art`, `art_of`, `has_own_art` and the hotspots, so T015 passes.
- [X] T019 [US1] Replace `Vec<(CursorKind, CursorImage)>` with `PointerImages`, recording `Theme` or `Builtin` per entry, in `src/render/cursor.rs` and `src/standalone.rs` (data-model "PointerImages"). Add `arrow_noted`, and log the FR-007 line from `mark_dirty` when the drawn kind changes, so T016 passes.
- [X] T020 [P] [US1] Write the test program `examples/cursor_shapes.rs` (quickstart Q6a), using `wayland-client` and `wayland-protocols` from the dev-dependencies:
  - an `xdg_toplevel` of 6x6 cells drawn in shared memory, each cell with a different shade and a thin border;
  - on `wl_pointer.enter` and `motion`, it names the shape of the cell under the pointer with the latest enter serial, and prints that shape's name when it changes;
  - it handles errors without `unwrap` (crate lints apply to examples).
- [X] T021 [US1] Run the full check; the owner runs quickstart Q1-Q6a on the laptop (2026-11-11: full check passed, 629 tests in debug and in release; Q1-Q4, Q6, Q6a passed, Q5 not run - see T032)

**Checkpoint**: application shapes with the built-in images; MVP.

---

## Phase 4: User Story 2 - An Installed Cursor Theme Takes Priority (Priority: P2)

**Goal**: a theme chosen by `cursor { theme }` or `default` gives every shape its theme image, with the
built-in images and log lines for whatever it lacks.

**Independent Test**: quickstart Q7-Q9.

### Tests for User Story 2 (write first, see them fail)

- [X] T022 [P] [US2] Write failing tests in `tests/config.rs` for the `cursor` block (contracts/config.md):
  - `theme "Adwaita"` gives `cursor_theme == Some("Adwaita")`;
  - no block, or a block without `theme`, gives `None` with no warning;
  - each of these is a warning with its line and gives `None`: an empty string, `"."`, `".."`, `"a/b"`, a number, two values, an unknown child `size`.
- [X] T023 [P] [US2] Write failing unit tests in `src/render/cursor.rs` on theme directories made in a temporary directory (research C6-C8, data-model "ThemeReport", contracts/cli.md). (2026-11-09: written right after T025's code rather than before it, so they were not seen failing; each checks a case T025 handles.)
  - **The chain**: `Inherits=` breadth first, at most 8 themes, cycles stopped; an inherited name with `/` or `..` is skipped.
  - **Names, in the C5 table's order**:
    - the theme's `e-resize` is used for `e_resize`; without it, the theme's `ew-resize`;
    - `no_drop` uses the theme's `not-allowed` when it has no `no-drop`;
    - the window move gesture keeps today's names.
  - **A configured theme**:
    - not installed: `default` is used and `configured_missing` is set;
    - not installed, and no `default` either: built-in images and the combined line;
    - installed with no images (it inherits only themes that are not installed): built-in images, not `default`, and the line names the missing inherited themes (the laptop's case).
  - **Missing shapes**: `missing` names exactly the entries the theme has no file for, in one line.
  - **Damaged files**:
    - the first existing name decides: a damaged `text` before a good `xterm` gives the built-in I-beam;
    - a damaged file is reported with its reason, and a file used by two entries is reported once.
  - **`parse_xcursor`** returns `NotACursorFile`, `CutShort`, `NoImage` or `TooLarge` for the matching broken files.
  - **`ThemeReport::lines()`**:
    - gives exactly the contracts/cli.md lines for each case;
    - with no theme it gives exactly one line (SC-003).

### Implementation for User Story 2

- [X] T024 [US2] Add `pub fn valid_theme_name` to `src/render/cursor.rs` (research C6), and parse the `cursor` block into `Config.cursor_theme` in `src/config.rs` with that check (research C11), so T022 passes
- [X] T025 [US2] In `src/render/cursor.rs`, so T023 passes (research C5-C7):
  - make `parse_xcursor` return `Result<CursorImage, XcursorError>`;
  - resolve the theme chain once;
  - list each `cursors/` directory once;
  - give each of the 37 kinds its full `theme_names()` list from C5;
  - write `load(icon_dirs, configured) -> (PointerImages, ThemeReport)` and `ThemeReport::lines()`.
- [X] T026 [US2] In `src/standalone.rs`, pass `config.cursor_theme` to `load` and log the report's lines at startup, at the levels in contracts/cli.md (info for the theme used, warn for fallbacks)
- [X] T027 [US2] Run the full check; the owner runs quickstart Q7-Q9 (2026-11-11: full check passed; Q7-Q9 passed)

**Checkpoint**: theme images where installed, built-in otherwise, every fallback in the log.

---

## Phase 5: User Story 3 - Nothing Else Changes (Priority: P3)

**Goal**: gestures, hiding and closing windows behave as before (with the C2 rule).

**Independent Test**: quickstart Q11-Q14. The automated part comes with T003-T006 and T013-T014.

- [X] T028 [US3] The owner runs quickstart Q11-Q14 (gestures over a named window, mpv hiding, closing a window under the pointer, 100 border crossings) (2026-11-11: all passed. Q13 first showed the window below getting its shape only after a motion: the shell did not enter it when the window under the pointer unmapped. Fixed in `src/shell/mod.rs`, test `unmapping_the_window_under_the_pointer_enters_the_one_below_without_motion` in `tests/shell.rs`; passed on the repeat.)

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T029 [P] Update `README.md` (plan "Supporting changes"):
  - under "Running applications": applications show their own pointer shapes, with RubickWay's images when no cursor theme is installed;
  - under "Configuration": the `cursor { theme }` block;
  - this feature's `contracts/config.md` in the reference list.
- [X] T030 [P] Compare the log lines in code with contracts/cli.md, and the protocol behaviour with contracts/protocols.md. Update the contracts first if anything had to differ. (2026-11-09: contracts/cli.md gained the line for a missing configured theme when `default` has no images, and the note that the info line follows the "not installed" warning.)
- [X] T031 Run the full check from `AGENTS.md`, debug and release. (2026-11-09: 628 passed in each, clippy clean.)
- [X] T032 Record the quickstart results (Q1-Q14) in this file. Mention in the PR body that it also carries the `drm >=0.15` Dependabot ignore rule in `.github/dependabot.yml`. (2026-11-11, results recorded; the PR body (#8) mentions the rule. On the laptop, owner:
  - **Passed**: Q1-Q4, Q6, Q6a, Q7-Q9, Q11-Q14.
  - **Not run**: Q5, no second monitor at hand (its automated part is `crossing_displays_over_the_same_window_keeps_its_shape`); Q10, no GTK 4 or Qt 6 program installed.
  - **Fixed on the way**: Q6a's test program opened its picture file write-only, so the server could not map it (`wl_shm` "cannot map the pool: Permission denied"); Q13, see T028; the Q9 expected line in quickstart.md named the wrong one of the two contracts/cli.md lines for the laptop, where `default` still exists.)

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: none
- **Foundational (Phase 2)**: after Setup; blocks all stories
- **US1 (Phase 3)**: after Foundational; the MVP
- **US2 (Phase 4)**: after US1 (it fills the `PointerImages` entries that US1 introduces)
- **US3 (Phase 5)**: after US1 and US2 (the manual checks need both)
- **Polish (Phase 6)**: after all stories

### Within each story

- Tests first, seen failing, then implementation (Principle III)
- Pure code (names, mapping, drawings, theme reading) before the protocol objects and the app wiring

### Parallel opportunities

- Foundational tests: T002-T006 (different files)
- US1 tests: T013, T014, T015, T016; and T020 alongside the implementation
- US2 tests: T022, T023
- Polish: T029, T030

## Implementation Strategy

### MVP first (User Story 1)

1. Setup + Foundational: nothing visible changes except the C2 hiding rule
2. US1: kitty's borders and text with the built-in images; validate with Q1-Q6a
3. Stop and let the owner try it

### Incremental delivery

1. US2: themes, the `cursor` setting, the log lines
2. US3: the regression checks by hand
3. Polish: README, contracts

Each story is committed only when the owner asks and the full check passes (`AGENTS.md`).
