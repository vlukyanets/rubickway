---
description: "Task list for Clipboard"
---

# Tasks: Clipboard

**Input**: Design documents from `specs/20261119-142759-clipboard/`

**Prerequisites**: plan.md, spec.md, research.md (C1-C14), data-model.md, contracts/, quickstart.md

**Tests**: Required by Principle III: each test task is written and seen failing before its
implementation task. No test needs a device.

**Organization**: Tasks are grouped by user story, in the plan's order of work: setup, foundation,
US1 (P1), US2 (P2), US3 (P3), polish. Each step leaves a working desktop.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US3)

---

## Phase 1: Setup

- [X] T001 Run the full check from `AGENTS.md` in the worktree to confirm a green baseline
- [X] T002 Add `wayland-protocols-wlr = { version = "0.3", features = ["server"] }` to `[dependencies]` and `wayland-protocols-wlr = { version = "0.3", features = ["client", "server"] }` to `[dev-dependencies]` in `Cargo.toml` (research C1; approved by the owner 2026-11-19), with a comment on why (the older data-control protocol); `cargo fetch` updates `Cargo.lock`; `cargo build` is green

---

## Phase 2: Foundational (blocks all stories)

**Purpose**: the selection model, the serial ring, the option and the new globals with no behaviour
yet. Nothing visible changes; today's tests stay green apart from the globals list.

### Tests (write first, see them fail)

- [X] T003 [P] Write failing unit tests in new `src/server/selection.rs` (data-model "SourceData", "Slot"):
  - `SourceData::add_type`: keeps types in the order offered, a repeat is ignored, the 257th distinct type is ignored ("at most 256"), and after `used` is set nothing is added;
  - `Slot`: every change (set, clear) bumps `generation`; an `OfferData` with an older generation is stale.
- [X] T004 [P] Write failing unit tests for the serial ring (data-model "SeatState.input_serials", research C4) beside it in `src/server/seat.rs` (a small `SerialRing` type): it holds the 16 latest serials; a serial pushed 16 events ago is still in, one pushed 17 events ago is not; an empty ring holds nothing.
- [X] T005 [P] Write failing tests in `tests/config.rs` for `primary-selection` (contracts/config.md): absent, `on` and `#true` give on; `off` and `#false` give off (a bare `false` is a KDL 2.0 syntax error, found while implementing; `on`/`off` chosen with the owner); no value, a string, two values, or children give one warning naming the line and on; given twice: the last counts, with a warning.
- [X] T006 [P] Update the globals-and-versions test in `tests/protocol/surfaces.rs` to expect `zwp_primary_selection_device_manager_v1` 1, `ext_data_control_manager_v1` 1 and `zwlr_data_control_manager_v1` 2 (contracts/protocols.md), and add a test that after `h.server.set_primary_selection(false)` (on a fresh `Harness::new()`, before any client connects) the primary selection global is absent (see it fail).

### Implementation

- [X] T007 Create `src/server/selection.rs` (data-model "SelectionState", "Slot", "Source", "Device", "SourceData" with "mime_types: distinct, in the order offered, at most 256" and "used", "OfferData", `Which`) and add `selection: SelectionState` to `ServerState` in `src/server/mod.rs`; `announce` and `focus_changed` exist and do nothing yet, so T003 passes
- [X] T008 In `src/server/seat.rs`, add `SerialRing` and `input_serials: HashMap<ClientId, SerialRing>`, fed at every key press and release, button press and release, and keyboard enter (window and popup), pruned with `popup_serials` when a client's input objects are gone; add `selection_serial_ok(state, client, serial)`, so T004 passes
- [X] T009 Add `Config.primary_selection` and the top-level `primary-selection` node in `src/config.rs`, and `Server::set_primary_selection(enabled)` in `src/server/mod.rs` (research C10: called before applications connect, like `set_gpu_offer`; for now it only records the switch); call it from `src/standalone.rs` with the configured value, so T005 passes
- [X] T010 Create the three globals (contracts/protocols.md): new `src/server/primary_selection.rs` (created in `Server::new()`; `set_primary_selection(false)` removes it again) and new `src/server/data_control.rs` (ext version 1, wlr version 2). Managers create devices and sources (registered in `SelectionState`, sources with `SourceData`); nothing is offered yet. So T006 passes
- [X] T011 Run the quick check (`cargo clippy --all-targets -- -D warnings && cargo test --lib`) and `cargo test`; everything is green (2026-11-19: all tests green; clippy reports only dead code of the model that US1 starts calling - it is green again at T017, without any `allow`)

**Checkpoint**: the model, the serial ring, the option and the globals exist; the clipboard still
transfers nothing.

---

## Phase 3: User Story 1 - Copy in One Application, Paste in Another (Priority: P1) MVP

**Goal**: the clipboard through `wl_data_device` (spec US1, FR-001-FR-005, FR-009-FR-011, FR-013).

**Independent Test**: quickstart Q1-Q6.

### Tests for User Story 1 (write first, see them fail)

- [X] T012 [US1] Write failing tests in new `tests/protocol/clipboard.rs` (add `mod clipboard` in `tests/protocol/main.rs`). Helpers at the top: a client with a seat, keyboard, pointer, data device and a mapped window; giving it the keyboard focus (`ClientInput::KeyboardFocus`); a key press that returns its serial; a source that writes a given text on `send` and closes the descriptor; pasting through `std::io::pipe` and reading to the end. Tests:
  - the first focused client gets `selection(null)`; after A sets the clipboard with its key serial and B gets the focus, B gets `data_offer`, one `offer` per type A offered, then `selection(offer)`, all **before** B's `wl_keyboard.enter` (research C3);
  - B pastes `text/plain`: A gets `send("text/plain", fd)`; B reads exactly A's text (FR-003);
  - a client that never had the focus gets no offer (FR-002);
  - while B has the focus, A sets new content (with a key serial from before the focus moved): accepted, B is told at once, A's previous source gets `cancelled` (FR-004, FR-005, spec Clarifications);
  - `set_selection` with a serial never sent, one from another client's input, and one 17 input events old: ignored, the source gets `cancelled`, the clipboard keeps its content, no error;
  - `set_selection(null)`: B gets `selection(null)`, the old source `cancelled`; A destroying its source, and A disconnecting: B gets `selection(null)`;
  - a paste on a stale offer (made before the content changed) and with a type the source did not offer: B reads end of file at once; A gets no `send`;
  - a data device created while its client has the focus gets the current clipboard at once;
  - the keyboard moving to a popup of the focused window (`ClientInput::PopupKeyboard`) sends no new offer;
  - a console switch (the shell sends `KeyboardFocus(None)` on `FocusLost`, then the focus again on `FocusGained`): after A set the clipboard and the focus goes to nothing and then to B, B is offered A's content and pastes A's text; A's source got no `cancelled` (FR-009; unplugging a monitor does not change the keyboard focus, so the clipboard sees nothing of it);
  - 10 000 `set_selection` calls from A in a row: B is told at most once per dispatch (a few times: the requests arrive over a few socket reads; found while implementing), the last time with the newest source; A's other sources get `cancelled` (research C11);
  - 300 `offer` calls with 200 distinct types: the offer lists the 200 once each; 300 distinct types: the first 256 (FR-011);
  - no redraw (FR-013): with B already focused and `take_damage()` emptied, A setting the clipboard (B is offered it) and B pasting leave `take_damage()` equal to `Damage { all: false, windows: vec![] }` (the focus change itself is outside the measurement: it changes the windows' activated state, which is not the clipboard's doing).
- [X] T013 [P] [US1] Replace the stub's test in `tests/protocol/surfaces.rs` (a data device that is accepted and never offered anything) with: `start_drag` still gets `cancelled` at once, and that source used again in `set_selection` is `used_source` (FR-010, research C8, C9).

### Implementation for User Story 1

- [X] T014 [US1] In `src/server/selection.rs`, implement the clipboard slot: `set` (replace, cancel the previous source, bump the generation), `clear`, `source_destroyed`, `focus_changed` (announce at once to the newly focused client's data devices), `announce` (once per batch: the focused client if its view is stale, research C3), `receive` (current offer and offered type: `send` to the source; the descriptor is dropped in every case, research C5)
- [X] T015 [US1] Rewrite `src/server/data_device.rs` on top of `selection.rs` (contracts/protocols.md "wl_data_device_manager"): sources keep `SourceData` (`offer` adds types until used); devices are registered and forgotten on `release` or destruction; `set_selection` checks the serial with `selection_serial_ok` and `used`; offers carry `OfferData`; `receive`, `accept` (ignored), `finish` (`invalid_finish`), `set_actions` (`invalid_offer`); `start_drag` cancels and marks the source used
- [X] T016 [US1] In `src/server/seat.rs` `keyboard_focus`, call `selection::focus_changed` before sending `wl_keyboard.enter`; in `src/server/mod.rs`, call `selection::announce` after dispatching client requests and after delivering `ClientInput`, so T012 and T013 pass
- [X] T017 [US1] Run the quick check; then the owner runs quickstart Q1-Q6 on the laptop (2026-11-19: all tests green; clippy green from T025 on, once US2 and US3 used the whole model. Q1-Q6 passed after T029; the first run found T029. Q5: the owner was not sure the content was gone after closing - most likely a window closed while the application kept running (Firefox with another window, kitty's single process), which keeps it the owner, as designed; not a defect)

**Checkpoint**: copy and paste work between applications.

---

## Phase 4: User Story 2 - Clipboard Tools and Managers (Priority: P2)

**Goal**: both data-control protocols (spec US2, FR-007, FR-008).

**Independent Test**: quickstart Q7-Q9.

### Tests for User Story 2 (write first, see them fail)

- [X] T018 [US2] Write failing tests (2026-11-19: four of eight passed as soon as written - the new device, reading, watching and the old wlr version - because `announce` from US1 already tells every device; setting by a tool, the errors and quitting failed first) in `tests/protocol/clipboard.rs`, each run once with `ext_data_control_manager_v1` and once with `zwlr_data_control_manager_v1` version 2:
  - a new device gets the current clipboard and primary selection at once (`data_offer`, types, `selection`, `primary_selection`);
  - a tool sets the clipboard with no focus and no serial: the focused application is offered it; the previous source gets `cancelled`; the application pastes the tool's text (US2 scenario 1);
  - a tool reads an application's clipboard through `receive` without any focus (scenario 2);
  - every change by applications is announced to every data-control device; 10 000 changes in a row are announced at most once per dispatch, the last time with the newest (scenario 3, research C11);
  - the owner application disconnects: the tool is told the clipboard is empty; the tool sets it again with the same content and the focused application can paste it (scenario 4, FR-008);
  - a source used twice: `used_source`; `offer` after use: `invalid_offer`;
  - the tool disconnects while owning the clipboard: the focused application gets `selection(null)`;
  - wlr bound at version 1: no `primary_selection` events.

### Implementation for User Story 2

- [X] T019 [US2] Implement `src/server/data_control.rs` for both protocols with shared functions over `Source`/`Device` (contracts/protocols.md "ext_data_control_manager_v1 and zwlr_data_control_manager_v1"), and extend `selection::announce` to tell data-control devices of every change once per batch, so T018 passes
- [X] T020 [US2] Run the quick check; then the owner runs quickstart Q7-Q9 (passed on the laptop 2026-11-19, checked with Q1-Q14)

**Checkpoint**: wl-copy, wl-paste, cliphist and wl-clip-persist work.

---

## Phase 5: User Story 3 - Select and Paste with the Middle Button (Priority: P3)

**Goal**: the primary selection and its switch (spec US3, FR-006).

**Independent Test**: quickstart Q10-Q11.

### Tests for User Story 3 (write first, see them fail)

- [X] T021 [US3] Write failing tests (2026-11-19: the "switched off" case passed as soon as written, since T019 already ignores `set_primary_selection` then; the other four failed first) in `tests/protocol/clipboard.rs`:
  - an application sets the primary selection with a button-release serial; the focused application's primary device gets the offer before its keyboard enter and can paste it;
  - the clipboard and the primary selection are independent: setting one leaves the other's content and offers unchanged (US3 scenario 2);
  - the serial, replacement, cancel and disconnect rules of T012 hold for the primary selection;
  - a primary source used a second time: no error, it stays the owner (changed 2026-11-19 with T029; first: ignored and cancelled);
  - a data-control tool sees and sets the primary selection;
  - after `h.server.set_primary_selection(false)`: no primary global; data-control devices get no `primary_selection` events; `set_primary_selection` from a tool is ignored and its source gets `cancelled`.

### Implementation for User Story 3

- [X] T022 [US3] Implement `src/server/primary_selection.rs` on top of the primary slot in `selection.rs` (contracts/protocols.md "zwp_primary_selection_device_manager_v1"), and the switch in `data_control.rs`, so T021 passes
- [X] T023 [US3] Run the quick check; then the owner runs quickstart Q10-Q11 (passed on the laptop 2026-11-19)

**Checkpoint**: middle-click paste works and can be switched off.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T024 [P] Add the contract's misbehaving rows not yet covered (2026-11-19: all four tests passed as soon as written - US1-US3 already built the behaviour) to `tests/protocol/misbehaving.rs`, each with a second client that keeps working: 10 000 `offer` calls with duplicates; an owner that never writes (the reader's end stays open, the server keeps dispatching, another client copies and pastes); a reader that never reads; an owner disconnecting with both slots; `finish` and `set_actions` on a selection offer
- [X] T025 Add the logging of research C12 in `src/server/selection.rs`: one debug line per announced change (slot, owner application, number of types; never the types or content) and at most one debug line per application per minute for ignored `set_selection` requests. The text comes from a small pure function; write its unit test first: the line names the slot, the application (`application N (pid, name)`) and the number of types, and contains none of the type names, even for a source offering `text/plain` and `x-kde-passwordManagerHint`
- [X] T026 [P] Update `README.md`: "Running applications on RubickWay" (copy and paste, middle-click paste and how to switch it off, clipboard tools; drag-and-drop not yet) and the defaults block under "Configuration" (`primary-selection`)
- [X] T027 Run the full check from `AGENTS.md` (`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked`)
- [X] T028 The owner runs quickstart Q12-Q14 and reports; then `/speckit-converge` (passed on the laptop 2026-11-19; the debug log of the second run is clean - no protocol error, no slow frame, no `lagging behind`; the first run's log was overwritten by the restart)
- [X] T029 Fix found on the laptop (2026-11-19, first run of Q1): Firefox puts the same `wl_data_source` into the clipboard again on Copy, and the `used_source` error of T015 disconnected it (Firefox then showed its crash reporter). Test first in `tests/protocol/clipboard.rs` (`an_application_setting_the_same_source_again_keeps_working`), the drag test in `tests/protocol/surfaces.rs` and the primary case of T021; then no `used_source` for `wl_data_device` or the primary selection (`selection::set` ignores the current owner set again). Clipboard tools keep the error their protocols require. Contract, research C8, data-model and spec updated

---

## Dependencies & Execution Order

### Phase dependencies

- Setup (T001-T002) -> Foundational (T003-T011) -> US1 (T012-T017) -> US2 (T018-T020) -> US3
  (T021-T023) -> Polish (T024-T028).
- US2 and US3 build on the slot logic of US1 (T014) and could run in either order after it; the
  plan's order is kept (priority).

### Within each phase

- Test tasks before their implementation tasks; implementation is done when its tests pass.
- `tests/protocol/clipboard.rs` is shared by T012, T018 and T021: sequential, in that order.

### Parallel opportunities

- Foundational tests: T003, T004, T005, T006 (different files).
- US1: T013 beside T012.
- Polish: T024 and T026 beside T025.

## Implementation Strategy

### MVP first

1. Setup and Foundational.
2. US1: copy and paste between applications - the essential need; stop and run Q1-Q6.

### Incremental delivery

3. US2: clipboard tools (Q7-Q9).
4. US3: primary selection (Q10-Q11).
5. Polish: misbehaving cases, logging, README, Q12-Q14, converge.

Each story is committed only when the owner asks and the full check passes (`AGENTS.md`).
