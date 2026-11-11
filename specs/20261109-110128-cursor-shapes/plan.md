# Implementation Plan: Cursor Shapes

**Branch**: `worktree-cursor-shapes` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261109-110128-cursor-shapes/spec.md`

## Summary

Applications name their pointer shape through `wp_cursor_shape_manager_v1` version 2, and RubickWay
shows it while the pointer is over their window.

- **Which requests count.** A request counts only from the client under the pointer, with the serial
  of its latest enter. This applies to shapes and to `wl_pointer.set_cursor`. At every enter the
  cursor goes back to the arrow (C2).
- **Who decides.** The server keeps what the focused client asked for and tells the shell when it
  changes. The shell puts RubickWay's gestures first (C3).
- **Images.** The image table has one entry per protocol shape plus the window move gesture. Each
  entry looks up its own Xcursor names in the theme, and otherwise uses one of 19 built-in drawings,
  twelve of them new (C5, C10).
- **The theme.** Chosen by the new `cursor { theme }` setting, otherwise `default`. Read once at
  startup, with the log lines of FR-012 (C6-C8).

Details: [research.md](research.md) C1-C12.

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: no new crates.
- `wayland-protocols` 0.32 (`wp::cursor_shape::v1`; the `staging` feature is already on for server
  and client);
- `wayland-server`, `kdl`.

**Storage**: none. `config.kdl` gains the optional `cursor` block (FR-010). Cursor themes are read
from the standard icon directories.

**Testing**: `cargo test` (debug and release).
- Unit: the image table, built-in drawings and hotspots, theme chain and lookup on directories made
  in the test, damaged files, the report's lines, the shape-to-name mapping, `pointer_kind`.
- `tests/config.rs`: the `cursor` block.
- `tests/shell.rs`: the shell's choice of cursor.
- `tests/protocol/`: requests that count or are ignored, errors and misbehaving cases.
- Real applications and real themes are checked by hand through [quickstart.md](quickstart.md)
  Q1-Q14.

**Target Platform**: Linux with DRM/KMS, the author's laptop (Arch, no cursor theme installed, kitty
0.49, mpv)

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: pointer as smooth over kitty's border as over the background (SC-004). At most
one redraw per frame, none when the image does not change (FR-013).

**Constraints**:
- no I/O in protocol handlers; the theme is read only at startup (C12);
- a client can only affect its own cursor;
- no new `unsafe`.

**Scale/Scope**: 37 image entries, 19 drawings, one global; roughly 1.5-2k lines including tests and
art

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | Protocol through `wayland-server` and `wayland-protocols`. No compositor framework. References named in research: wlroots (`wlr_cursor_shape_v1.c`, `wlr_seat_pointer.c`) and Smithay's `cursor_shape`, for reading only. |
| II. Client can't take it down | PASS | The shape is an enum. A value outside 1-36, or a version 2 value on version 1, is a protocol error to that client only (C4). Requests from a client without the pointer, or with a stale serial, are ignored (C2). A destroyed pointer leaves an inert device. No arithmetic on client values. Theme files are local, but parsing keeps today's checks. Theme names cannot leave the icon directories (C6). The spec's 6 misbehaving cases each get a protocol test. |
| III. Test-first, headless | PASS | Every decision is in code that tests reach headless (C3). Test tasks come first in each story. Real applications and themes are checked by hand in the quickstart. |
| IV. Backend-independent core | PASS | `canvas` does not change. The shell gets plain enums (`CursorName`, `AppCursor`), with no Wayland types. `render` gets no Wayland or DRM types. |
| V. Contained unsafe | PASS | No new `unsafe`. |
| VI. No speculative generality | PASS | Version 2, every value of it implemented and tested. One setting (`theme`), which the spec asks for. No size setting. No `XCURSOR_*` for started programs (spec Assumptions). No traits. |
| VII. Never block the loop | PASS | Work that can block, named: reading the theme at startup, before the loop runs, as today and as the configuration (C12). Handlers only compare values. A flood of requests causes events only on change and one redraw per frame (C9). |
| VIII. 2D core with plugins | PASS | Core only. |

Post-design re-check (after data-model, contracts and quickstart): all PASS.

## Project Structure

### Documentation (this feature)

```text
specs/20261109-110128-cursor-shapes/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (C1-C12)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q14 manual checks)
|-- contracts/
|   |-- protocols.md     # wp_cursor_shape_manager_v1 v2, set_cursor change, misbehaving cases
|   |-- config.md        # cursor { theme }
|   `-- cli.md           # log lines
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- shell/
|   |-- cursor.rs            # NEW: CursorName (36 shapes), AppCursor
|   `-- mod.rs               # WindowEvent::CursorChanged (replaces CursorHidden), app_cursor,
|                            # CursorShape::Named, cursor()
|-- server/
|   |-- mod.rs               # creates the new global
|   |-- cursor_shape.rs      # NEW: manager and device, version and value checks, shape_name()
|   `-- seat.rs              # pointer_window, enter_serial, focus_cursor; request_cursor() for
|                            # set_cursor and set_shape (C2, C3)
|-- render/
|   `-- cursor.rs            # CursorKind = Shape | WindowMove; 12 new drawings; PointerImages;
|                            # theme chain, read_dir lookup, ThemeReport, XcursorError
|-- config.rs                # cursor { theme } (C11)
`-- standalone.rs            # PointerImages, pointer_kind(), report lines at startup,
                             # FR-007 first-use line

examples/
`-- cursor_shapes.rs         # NEW: test program for quickstart Q6a: a window of 36 cells, one per
                             # shape; names the shape of the cell under the pointer (wayland-client,
                             # already a dev-dependency; no new crate)

tests/
|-- config.rs                # cursor block
|-- shell.rs                 # cursor choice; the CursorHidden test moves to CursorChanged
`-- protocol/
    |-- main.rs              # + mod cursor_shape
    |-- harness.rs           # binds the cursor shape manager in globals()
    |-- cursor_shape.rs      # NEW: shapes that count or are ignored, errors, resets, lifecycles
    |-- input.rs             # the set_cursor hiding test follows C2 (enter first, its serial)
    `-- misbehaving.rs       # + the cases in contracts/protocols.md
```

**Structure Decision**: same single crate and module direction. The shell owns the shape names, as it
owns `CursorShape`. `render::cursor` keys its images by them.

### Order of work (by user story)

Each step leaves a working desktop.

1. **Foundation (part of US3)**:
   - `CursorName` and `AppCursor` in the shell, and `CursorChanged` in place of `CursorHidden`;
   - the server's focus state with the C2 rule, through `set_cursor` only for now;
   - `CursorKind` keyed by shape, with today's 7 images in their entries.

   Hiding keeps working (with the C2 change). Today's tests stay green, except the one hiding test
   that C2 changes on purpose.
2. **US1 (P1)**:
   - the global and device, with errors;
   - the 12 new drawings and the FR-006/FR-007 table;
   - the debug line and the FR-007 first-use line;
   - the test program `examples/cursor_shapes.rs`.

   Hardware check: Q1-Q6a on the laptop.
3. **US2 (P2)**:
   - the `cursor { theme }` setting;
   - the theme chain read once, per-shape names, damaged-file reasons;
   - the report's lines.

   Hardware check: Q7-Q9.
4. **US3 (P3)**: the rest of the regression checks, Q11-Q14. The tests for gestures, hiding and
   unmapping come with step 1.

### Supporting changes

- `README.md`:
  - under "Running applications": applications show their own pointer shapes, with RubickWay's
    images when no cursor theme is installed;
  - under "Configuration": the `cursor { theme }` block;
  - the reference list gains this feature's `contracts/config.md`.
- `specs/20261102-143401-drm-backend/contracts/protocols.md` is left as it was. This feature's
  contract says what changed.

## Complexity Tracking

No constitution violations and nothing to justify.
