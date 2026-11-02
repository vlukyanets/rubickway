# Implementation Plan: Window Management on the Canvas

**Branch**: `worktree-window-management` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261101-220702-window-management/spec.md`

## Summary

Windows become movable, resizable and closable, and applications' move, resize, maximize and
fullscreen requests take effect. All decisions stay in the pure `shell`: the server reports
requests (with serials already checked) and size limits as new `WindowEvent`s, and the shell
answers with a new `WindowCommand` queue (configure size and states, close) that `App` passes
to the server. The shell gains interactive operations (move, resize) next to the client-windows
pointer grab, per-window anchors that decide which point stays fixed when an application
commits a new size, a most-recently-used focus history for Mod+Tab with a view jump, and
maximize/fullscreen geometry with saved state. Fullscreen draws only its window on black.
Maximize and fullscreen requests count only from the focused window; requests made before the
first map go into the initial configure (a new `Created` event, emitted only for such windows
and answered by the shell); the
server tracks `set_parent` so a dialog of a fullscreen window ends fullscreen, while other new
windows open unfocused behind it. Details: [research.md](research.md) W1-W14.

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: unchanged (client-windows set); no new crates

**Storage**: N/A

**Testing**: `cargo test` (debug and release) - unit and `proptest` tests for `shell`
(anchors, clamping, MRU, view fitting, operations); routing tests in `tests/shell.rs`; protocol
tests with in-process clients in `tests/protocol/`; one pixel test for fullscreen in
`tests/render_windows.rs`

**Target Platform**: Linux, nested inside a Wayland session (niri 26.04, Mesa 26)

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: moves and resizes at the display refresh rate (SC-004); one outstanding
size configure per window during a resize (W5)

**Constraints**: never wait for an application (FR-018, Principle VII); never ask for a size
beyond the displayable limit (FR-008); requests without a held press ignored (FR-016)

**Scale/Scope**: one viewer, a handful of windows; ~2.5k lines including tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | xdg-shell handling on `wayland-server`/`wayland-protocols` as before. References: wlroots `wlr_xdg_toplevel.c`, sway `seatop_move_*`/`seatop_resize_*` (serial checks, anchored resize), Smithay `wayland/shell/xdg` (configure state), niri `input/resize_grab.rs` (anchor kept until the final configure). |
| II. Client can't take it down | PASS | Misbehaving inputs listed in research W12, each with a test: stale serials ignored, invalid resize edges a protocol error, contradictory min/max handled, never-acking clients get one outstanding configure, sizes clamped to the displayable limit so case 7 never hits a well-behaved client, state dropped when a window unmaps mid-operation, maximize/fullscreen from unfocused windows ignored (W13), new windows cannot break a fullscreen window unless they are its dialog (W14). |
| III. Test-first, headless | PASS | Shell logic (operations, anchors, MRU, maximize/fullscreen) unit and routing tested; server behaviour via in-process clients; fullscreen rendering by pixel assertion; real-application behaviour in quickstart W1-W14. |
| IV. Backend-independent core | PASS | `Edges`, `Anchor`, `SizeLimits`, `WindowCommand`, `CursorShape` are plain shell types; the server maps them to xdg-shell, `App` maps `CursorShape` to the host `Cursor`. |
| V. Contained unsafe | PASS | No new `unsafe`; the renderer's clear uses the existing GL wrapper. |
| VI. No speculative generality | PASS | No new globals or versions; `wm_capabilities` advertises only what is implemented (maximize, fullscreen); no traits; `Anchor` has three real uses; shortcuts stay fixed (no configuration). |
| VII. Never block the loop | PASS | No blocking work added: operations are in-memory updates; configures are queued writes bounded by the 1 MiB client limit (client-windows R8). |
| VIII. 2D core with plugins | PASS | Core window management; no plugin interface. |

Post-design re-check (after data-model, contracts, quickstart): all PASS, no changes.

## Project Structure

### Documentation (this feature)

```text
specs/20261101-220702-window-management/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (W1-W14)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (W1-W17 manual checks)
|-- contracts/
|   |-- input.md         # gestures, shortcuts, fullscreen exits, cursors
|   `-- protocols.md     # xdg_toplevel behaviour, configure, errors
|-- checklists/
|   |-- requirements.md
|   `-- review.md        # pre-tasks requirements review
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- input/bindings.rs        # + NextWindow, PreviousWindow, CloseWindow
|-- shell/
|   |-- mod.rs               # + WindowEvent variants, WindowCommand queue, operations wiring,
|   |                        #   fullscreen, focus cycling + view jump, cursor(), viewer_configured,
|   |                        #   focus gate for maximize/fullscreen, new windows during fullscreen
|   |-- windows.rs           # + limits, anchor, mode, mru/cycle, move_to, resize_settled
|   |-- route.rs             # + Op (Moving / Resizing) next to Grab
|   `-- geometry.rs          # NEW: Edges, SizeLimits::clamp, 3x3 edge pick, view fitting
|-- server/
|   |-- xdg_shell.rs         # move/resize/maximize/fullscreen requests, wanted configure state,
|   |                        #   coalescing, min/max size, wm_capabilities, close, configured flag,
|   |                        #   parent, initial state + Created (initial configure from the shell)
|   |-- seat.rs              # last button press serial per window
|   `-- mod.rs               # Server::command(&[WindowCommand])
|-- render/gles.rs           # clear to black (fullscreen, no grid)
|-- backend/wayland_host/seat.rs  # Cursor: Move + eight resize shapes
`-- state.rs                 # take_commands -> server, cursor mapping, largest window,
                             #   viewer_configured, fullscreen drawing

tests/
|-- shell.rs                 # + operations, anchors, cycling, maximize/fullscreen, cursors
|-- protocol/
|   |-- wm.rs                # NEW: xdg_toplevel window-management requests and configures
|   `-- misbehaving.rs       # + stale serials, bad edges, never-acking resize
`-- render_windows.rs        # + fullscreen window alone on black
```

**Structure Decision**: same single crate and module direction as client-windows (`canvas` <-
`viewer` <- `shell` <- `state` -> `server`, `backend`, `render`). `shell/geometry.rs` holds the
pure size and edge maths so `windows.rs` and `mod.rs` stay readable.

### Supporting changes

- `AGENTS.md` / `README.md`: the full check also runs `cargo test --release --locked` (done at
  the start of this feature, matching CI).
- `README.md`: a short "Controls" section (none exists yet) listing the canvas gestures from
  nested-canvas and client-windows and the new window gestures and shortcuts.

## Complexity Tracking

No constitution violations to justify.
