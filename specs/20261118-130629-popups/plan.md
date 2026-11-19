# Implementation Plan: Popups

**Branch**: `worktree-popups` | **Date**: 2026-11-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261118-130629-popups/spec.md`

## Summary

Applications' popups (menus, dropdowns, submenus, tooltips) are shown instead of dismissed at once,
through the popup parts of `xdg_wm_base` version 6, which RubickWay already advertises.

- **Who decides.** The server keeps the protocol: objects, the positioner's rules, configures and
  acks, errors, the grab's serial. The shell places popups, routes input during a grab and decides
  every dismissal (P2).
- **Placement.** A pure function applies anchor, gravity, offset and the allowed flip, slide and
  resize against the canvas area of one viewer: the one under the pointer, else the one showing most
  of the window (P3, P4). It works at any zoom because the area is in canvas units.
- **Part of the window.** A shown popup is drawn, hit-tested, damaged and given frame callbacks as
  part of its window, above the window's own surfaces and at the window's place in the stack (P5).
  The renderer does not change.
- **Grab.** Owner-events pointer for the grabbing application, keyboard on the topmost grabbing
  popup, a click outside closes and is swallowed (P6, P7, P9).
- **Dismissals.** Before any binding, gesture, view change, focus change, any maximize or fullscreen
  change, and display removal, all popups close, topmost first (P8). A click on the grabbing
  application's own window goes to it; a Mod gesture during a grab acts at once (P6).

Details: [research.md](research.md) P1-P16.

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: no new crates. `wayland-server` and `wayland-protocols` 0.32 (`xdg::shell`,
already used); `wayland-client` (dev-dependency) for tests and the example.

**Storage**: none. No configuration change.

**Testing**: `cargo test` (debug and release).
- Unit: `place()` per anchor, gravity and adjustment, with proptest properties (P3); the constraint
  area choice (P4); `Window::extent` and popup hit-testing.
- `tests/shell.rs`: placement answers, stacking, grab routing (pointer and keyboard), the swallowed
  click, every FR-010 dismissal, reactive re-placement, windows unmapping with popups.
- `tests/protocol/popups.rs` (new): life of a popup, configure and ack, reposition, grab rules,
  dismiss order, keyboard and pointer during a grab, frame callbacks and outputs for popup surfaces;
  `tests/protocol/misbehaving.rs`: the contract's cases.
- No new rendering test: the renderer does not change, and it already draws `window_parts` in order
  (`tests/render_windows.rs` `later_quads_are_on_top`). That popups are in `window_parts`, after the
  window's own surfaces and at their offset, is checked in `tests/protocol/popups.rs`.
- Real applications are checked by hand through [quickstart.md](quickstart.md) Q1-Q23.

**Target Platform**: Linux with DRM/KMS, the author's laptop (Arch); Firefox, GTK 4, Qt 6 for the
manual checks.

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: opening and closing a menu 100 times leaves no slow frame and no input lag
line (SC-003); at most one redraw per display frame (FR-016).

**Constraints**:
- no I/O and no waiting in protocol handlers (Principle VII); placement is arithmetic only;
- a client can only affect its own popups and its own windows' place in the stack;
- no new `unsafe`.

**Scale/Scope**: about 2.5-3.5k lines including tests and the example program; no globals added.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | Protocol through `wayland-server` and `wayland-protocols`. No compositor framework. References named in research: wlroots (`wlr_xdg_positioner.c`, `wlr_xdg_popup.c`), sway, mutter (`meta-wayland-popup.c`), KWin (`popup_input_filter.cpp`), Smithay (`PopupManager`, `PopupPointerGrab`), for reading only. |
| II. Client can't take it down | PASS | Positioner values checked when set (`invalid_input`); placement in `i64` with saturation to `i32` (P3). Each protocol violation is an error to that client only; where the protocol allows, a dismissal instead (P12). A popup is drawn only in its own window's group. The spec's 12 misbehaving cases and the contract's 20 rows each get a test. |
| III. Test-first, headless | PASS | Placement, grab and dismissals are in the shell and tested headless; the protocol through in-process clients, including popups' place in `window_parts`; the renderer does not change. Test tasks come first in each story. Real applications in the quickstart. |
| IV. Backend-independent core | PASS | `PopupRules`, `place`, `Popup`, `PopupGrab` are plain data in `src/shell/` with no Wayland or GL types. `canvas` does not change. The renderer gets nothing new. |
| V. Contained unsafe | PASS | No new `unsafe`. |
| VI. No speculative generality | PASS | No new global or version: version 6's popup parts are implemented and tested because version 6 is already advertised (P1). `set_parent_size` and `set_parent_configure` are accepted, checked and not used, which the protocol allows ("may"; research P10). No traits. No configuration. |
| VII. Never block the loop | PASS | No work that can block: handlers copy values; placement and hit-testing are arithmetic over a handful of popups. A flood of repositions keeps only the newest (P11); redraws stay one per frame (P14). |
| VIII. 2D core with plugins | PASS | Core only. |

Post-design re-check (after data-model, contracts and quickstart): all PASS.

## Project Structure

### Documentation (this feature)

```text
specs/20261118-130629-popups/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (P1-P16)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q23 manual checks)
|-- contracts/
|   |-- protocols.md     # xdg_positioner, xdg_popup, grab, misbehaving cases
|   `-- input.md         # input during a grab, what closes popups
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- shell/
|   |-- positioner.rs        # NEW: PopupRules, Anchor8, Adjust, IRect, place() (P3)
|   |-- popups.rs            # NEW: PopupId, Popup, Shown; constraint area choice (P4);
|   |                        # placing, re-placing reactive children, dismiss order (P8, P10)
|   |-- windows.rs           # Window.app, Window.popups, extent(); hit-testing popups first (P5)
|   |-- route.rs             # PopupGrab; ClientInput::PopupKeyboard (P6, P7)
|   `-- mod.rs               # WindowEvent Popup* and Mapped.app; WindowCommand PlacePopup,
|                            # DismissPopup; grab routing in button/pointer_moved/scroll/key;
|                            # dismiss_popups() at every FR-010 point; camera snapshot (P8)
|-- server/
|   |-- popup.rs             # NEW: PopupData, layers(), configure/ack/map, reposition,
|   |                        # grab checks, dismiss, topmost rules (P6, P11, P12, P13)
|   |-- xdg_shell.rs         # full PositionerState; get_popup and xdg_popup dispatch call popup.rs;
|   |                        # tree_committed routes popup commits; unmap dismisses popups
|   |-- seat.rs              # popup_serials (P9); PopupKeyboard; re-pick within the app while a
|   |                        # grab chain is open (P6); surface_at/relative_to through layers()
|   |-- compositor.rs        # nothing new beyond callers moving to layers()
|   |-- output.rs            # enter/leave for popup surfaces through layers()
|   `-- mod.rs               # window_parts, take_damage, presented, has_pending_callbacks use
|                            # layers(); WindowCommand Popup* to popup.rs; Mapped.app
`-- standalone.rs            # nothing beyond the type changes (rendering already uses window_parts)

examples/
`-- popups.rs                # NEW: test program for quickstart Q11 (P16)

tests/
|-- shell.rs                 # popups in the shell
`-- protocol/
    |-- main.rs              # + mod popups
    |-- harness.rs           # helpers: a positioner, a popup, a grab with a press serial
    |-- popups.rs            # NEW
    |-- surfaces.rs          # the "popup is dismissed at once" test changes on purpose
    `-- misbehaving.rs       # + the contract's cases
```

**Structure Decision**: same single crate and module direction. The shell owns placement and
decisions, as it owns window placement; the server owns the protocol, as for toplevels.

### Order of work (by user story)

Each step leaves a working desktop.

1. **Foundation**:
   - `positioner.rs` with `place()` and its tests (P3);
   - the full positioner state in the server;
   - `Mapped.app`, `PopupId`, the new events and commands with empty handling;
   - `layers()` with only the toplevel, and every caller moved to it (no behaviour change; today's
     tests stay green).
2. **US1 (P1)**: menus open and can be used.
   - Server: popup life (configure, ack, map, unmap, destroy), the grab's serial and errors,
     `PopupKeyboard`, re-picking with a button held.
   - Shell: placement against the chosen viewer, popups in windows (extent, hit-testing), the grab
     (pointer, keyboard, swallowed click).
   - The test program `examples/popups.rs`.
   - Hardware check: Q1-Q7.
3. **US2 (P2)**: nested popups, the topmost rules, reposition, reactive popups, zoom.
   Hardware check: Q8-Q12.
4. **US4 (P2)**: every FR-010 dismissal, windows and displays going away. Hardware check: Q16-Q22.
5. **US3 (P3)**: popups without a grab: no keyboard, not closed by clicks. Hardware check: Q13-Q15.
6. **Polish**: misbehaving cases not yet covered, Q23, README.

### Supporting changes

- `README.md`, under "Running applications": menus, dropdowns and tooltips work; a click outside a
  menu closes it and is not passed on; any RubickWay action closes open menus.
- `tests/protocol/surfaces.rs`: the test that expects `popup_done` right after `get_popup` changes to
  the new life of a popup (named in tasks).
- Earlier features' contracts are left as they were. This feature's contracts say what changed.

## Complexity Tracking

No constitution violations and nothing to justify.
