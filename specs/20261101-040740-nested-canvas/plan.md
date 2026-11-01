# Implementation Plan: Nested Canvas Viewer

**Branch**: `worktree-nested-canvas` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261101-040740-nested-canvas/spec.md`

## Summary

`rubickway` connects to the host Wayland session as an ordinary client, opens one
`xdg_toplevel` window - the first viewer - and draws the endless canvas as an adaptive grid with
OpenGL ES. Pointer, touchpad and keyboard input from the host is translated into
backend-neutral `InputEvent`s; a pure `Viewer` turns them into camera changes (pan by anchor,
zoom about the pointer, reset), a dirty flag and quit effects. Everything runs on one `calloop` loop;
frames are drawn only when something changed and the host's frame callback allows it. The
canvas/camera math, gestures, bindings and rendering are tested headless; the thin host backend
is verified manually through [quickstart.md](quickstart.md). Details: [research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: `wayland-client` 0.31, `wayland-backend` 0.3 (`client_system`),
`wayland-protocols` 0.32, `wayland-egl` 0.32, `khronos-egl` 6 (`static`), `glow` 0.18,
`xkbcommon` 0.9, `calloop` 0.14 (`signals`), `tracing` 0.1, `tracing-subscriber` 0.3,
`thiserror` 2; dev: `proptest` 1 (see research R11)

**Storage**: N/A

**Testing**: `cargo test` - unit, property (`proptest`) and integration tests; pixel
assertions on Mesa software rasteriser (llvmpipe) output (research R9)

**Target Platform**: Linux, nested inside a Wayland session (developed on niri, Mesa 26)

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: frames at the host's refresh rate during interaction (SC-004); launch to
first frame < 1 s (SC-001)

**Constraints**: no frames while idle or hidden, idle CPU < 1% (FR-015, SC-005); no jitter
within 1e6 canvas units (FR-016); the event loop never blocks (Principle VII)

**Scale/Scope**: one viewer, no clients; ~2.5k lines including tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | No framework crates; raw `wayland-client` instead of winit/SCTK (R1). Reference reading: Smithay `backend/winit` and niri's nested backend for host-window handling; anvil's input handling for pointer gestures. |
| II. Client can't take it down | PASS | No clients yet; the spec states "Misbehaving clients: none". Host failures are errors with exit 1, never panics (contracts/cli.md). Clippy lints already deny `unwrap`/`expect`/`panic`. Values from the host (sizes, scale) are validated: zero/negative sizes replaced by defaults, scale must be > 0 and finite. |
| III. Test-first, headless | PASS | Tests for each story come before implementation: camera + property tests, viewer input integration tests, keymap tests, pixel assertions on llvmpipe output (R9). Hardware/host-only behaviour has manual steps M1-M19 in quickstart.md. The host layer stays thin: it only maps protocol events to `InputEvent`/`BackendEvent`. |
| IV. Backend-independent core | PASS | `canvas` uses only `f64` geometry. `Viewer` consumes `InputEvent` and knows nothing about Wayland. The renderer draws `GridParams` into whatever EGL surface it is given. |
| V. Contained unsafe | PASS | `unsafe` only in `render::egl` (EGL window surface from the `wl_egl_window` pointer) and `render::gles` (`glow` calls), each module opting in with `#![allow(unsafe_code)]`. The keymap is read with a safe positional read and compiled from a string (R7). |
| VI. No speculative generality | PASS | No `Renderer` trait: the real GLES renderer runs headless on llvmpipe, so a test double is not justified. Host globals bound only up to the versions handled (R2). No config file; fixed bindings and colours. |
| VII. Never block the loop | PASS | Named blocking risks and their handling: `eglSwapBuffers` - swap interval 0, paced by frame callbacks (R4); keymap fd read - memory-backed fd, positional copy (R7); stderr logging - info level only by default (R10); EGL/shader setup - before the loop starts. |

Post-design re-check (after data-model, contracts, quickstart): all PASS, no changes.

## Project Structure

### Documentation (this feature)

```text
specs/20261101-040740-nested-canvas/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1
|-- contracts/
|   |-- cli.md           # invocation, exit codes, messages, window
|   `-- input.md         # bindings and gestures
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- main.rs                  # args, logging, run, exit codes
|-- lib.rs
|-- canvas/
|   |-- mod.rs
|   |-- geometry.rs          # Point, Vector, Size
|   `-- camera.rs            # Camera + unit/property tests
|-- input/
|   |-- mod.rs               # InputEvent, Modifiers, Button, ScrollSource
|   |-- keyboard.rs          # xkb keymap/state -> binding keysym + modifiers
|   `-- bindings.rs          # fixed binding table -> Action
|-- viewer/
|   |-- mod.rs               # Viewer, Effect
|   `-- gesture.rs           # Idle / Panning / Pinching
|-- render/
|   |-- mod.rs
|   |-- grid.rs              # GridParams (pure)
|   |-- egl.rs               # EGL display/context: Wayland + software device (unsafe)
|   |-- gles.rs              # GlesRenderer: grid shader, draw (unsafe)
|   `-- shaders/             # grid.vert, grid.frag
|-- backend/
|   |-- mod.rs               # BackendEvent
|   `-- wayland_host/
|       |-- mod.rs           # connection, globals, calloop source
|       |-- window.rs        # surface, xdg toplevel, scale, viewport, decorations, frames
|       `-- seat.rs          # pointer, keyboard, gestures, cursor shape -> InputEvent
`-- state.rs                 # ties host, viewer and renderer together; frame scheduling

tests/
|-- viewer_input.rs          # InputEvent sequences -> camera / effects
|-- keymap.rs                # us, us,ua keymaps -> bindings
`-- render_grid.rs           # offscreen llvmpipe renders checked by pixel assertions
```

**Structure Decision**: single crate with a library (everything testable) and a thin binary.
Module boundaries follow Principle IV: `canvas` <- `viewer` <- `state` -> `backend`, `render`;
`input` types are shared by `backend` (producer) and `viewer` (consumer).

### Supporting changes

- `Cargo.toml`: the dependencies above.
- `.github/workflows/ci.yml`: install `libwayland-dev libxkbcommon-dev libegl-dev libgles-dev
  libegl-mesa0 xkb-data` before the full check (R9).
- `README.md`: system library prerequisites; `.env.example`: `RUST_LOG`.
- `AGENTS.md` *Gotchas*: rendering tests assert pixel facts; never commit reference images.

## Complexity Tracking

No constitution violations to justify.
