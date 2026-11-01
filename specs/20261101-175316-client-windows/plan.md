# Implementation Plan: Client Windows on the Canvas

**Branch**: `worktree-client-windows` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261101-175316-client-windows/spec.md`

## Summary

RubickWay starts serving Wayland applications: a `wayland-server` display on its own socket
joins the existing `calloop` loop, offers the globals foot and other shared-memory clients need
(compositor, subcompositor, shm, xdg-shell, seat, output, viewporter, fractional scale and a
clipboard stub), and turns mapped toplevels into windows on the canvas. A new pure `shell`
module owns windows (top-left canvas position, size, stacking, focus) and routes input: Mod
gestures and the background go to the existing viewer, everything else to the window under the
pointer or with keyboard focus, as backend-neutral `ClientInput`s that the server turns into
protocol events. Buffers are uploaded to GL textures (with a SIGBUS-safe shared-memory reader)
and drawn as quads above the grid; frame callbacks follow what the host actually shows.
Everything except the host backend and real devices is tested headless, including every
misbehaving-client case. Details: [research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: existing (nested-canvas) plus `wayland-server` 0.31, `libc` 0.2,
and the `server` feature of `wayland-protocols` 0.32 (research R14)

**Storage**: N/A

**Testing**: `cargo test` - unit and `proptest` tests for `shell`; protocol integration tests
with in-process `wayland-client` test clients over socket pairs; pixel assertions on llvmpipe
output (research R13)

**Target Platform**: Linux, nested inside a Wayland session (developed on niri 26.04, Mesa 26)

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: typed characters shown within one refresh after the application draws
(SC-002); smooth pan/zoom with five windows (SC-006); terminal visible < 1 s after start (SC-001)

**Constraints**: never wait for an application (FR-016, Principle VII); no frames while idle or
hidden, idle CPU < 1% with five idle terminals (FR-018, SC-005); a misbehaving application can
only disconnect itself (FR-015, Principle II); 1 MiB outgoing buffer per client (R8)

**Scale/Scope**: one viewer, a handful of windows; ~5k lines including tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | Protocol code on `wayland-server` / `wayland-protocols`, as the principle names; no framework. References: libwayland `wayland-shm.c` and Smithay `wayland/shm/pool.rs` (SIGBUS handling, R4); Smithay `wayland/compositor` and wlroots `wlr_compositor.c` / `wlr_subcompositor.c` (commit and subsurface caching, R6). |
| II. Client can't take it down | PASS | The spec lists the misbehaving cases; each has a mechanism and a test (R4, R5, R8, R13, contracts/protocols.md): protocol errors per request with checked arithmetic on every client value, SIGBUS guard on pool reads, oversized buffers disconnected, 1 MiB outgoing limit, disconnect at any point cleans all state through object destructors. No `unwrap`/`expect`/`panic` outside tests (lints). |
| III. Test-first, headless | PASS | `shell` is pure and unit/property tested; the server is driven by in-process test clients over socket pairs (no host, no GL); windows are checked by pixel assertions on llvmpipe; host-only behaviour is in quickstart M1-M20, with kitty (software rendering) as the acceptance application. |
| IV. Backend-independent core | PASS | `shell` (windows, focus, routing) uses only `canvas` and `input` types; `WindowId` is a plain `u64`; the server maps it to protocol objects. Nothing in `shell` or `viewer` knows Wayland. |
| V. Contained unsafe | PASS | New `unsafe` only in `ffi::shm_map` (mmap, SIGBUS handler, memfd for the keymap - FFI) and `render` (texture upload, window shader). Each block has `// SAFETY:`; modules opt in with `#![allow(unsafe_code)]`. |
| VI. No speculative generality | PASS | Each global at the version implemented and tested (R2); not offered: decorations, cursor shape, gestures, presentation, dmabuf. The clipboard global is a stub only because common shm clients such as foot refuse to start without it (spec Assumptions), and every request is handled and tested (R10). No traits: the server is tested without GL because uploads happen in `App`, not behind an abstraction. Damage-based partial uploads deferred. |
| VII. Never block the loop | PASS | Named blocking risks: client sockets (non-blocking; full sockets buffer up to the limit, R8); `accept` (non-blocking listener); pool reads and texture uploads (memory copies; SIGBUS cannot stall, R4; size bounded by the GPU texture limit, R5); keymap memfd write (memory, only when the host keymap changes); `eglSwapBuffers` as in nested-canvas R4. |
| VIII. 2D core with plugins | PASS | Everything here is core 2D compositor functionality; no plugin interface is introduced (it comes with the first plugin). |

Post-design re-check (after data-model, contracts, quickstart): all PASS, no changes.

## Project Structure

### Documentation (this feature)

```text
specs/20261101-175316-client-windows/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1
|-- contracts/
|   |-- cli.md           # socket name, new error, log lines
|   |-- input.md         # routing between canvas and windows
|   `-- protocols.md     # globals, versions, disconnect errors, frame callbacks
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- main.rs                  # unchanged except the socket error path
|-- lib.rs                   # + pub mod shell, server, ffi
|-- canvas/                  # + Rect (geometry.rs)
|-- input/                   # Key/Modifiers/PointerButton gain raw codes (data-model)
|-- viewer/                  # unchanged API; used by shell for canvas gestures
|-- shell/                   # NEW, pure core (Principle IV)
|   |-- mod.rs               # Shell: viewer + windows + pointer routing -> ClientInput
|   |-- windows.rs           # Window, Windows: placement, stacking, focus, hit-test
|   `-- route.rs             # Free / Grabbed pointer state, key routing, consumed shortcuts
|-- server/                  # NEW, Wayland protocol (no unsafe)
|   |-- mod.rs               # Display, ListeningSocket, globals, ClientState, ServerState
|   |-- compositor.rs        # wl_compositor, wl_surface, wl_region, commit, surface tree
|   |-- subcompositor.rs     # wl_subcompositor, wl_subsurface (sync/desync cache)
|   |-- shm.rs               # wl_shm, pools, buffers (validation; reads via ffi::shm_map)
|   |-- xdg_shell.rs         # xdg_wm_base, xdg_surface, xdg_toplevel, popup dismissal
|   |-- seat.rs              # wl_seat, wl_pointer, wl_keyboard; ClientInput -> events
|   |-- output.rs            # wl_output for the viewer
|   |-- viewporter.rs        # wp_viewporter, wp_fractional_scale
|   `-- data_device.rs       # clipboard stub
|-- ffi/                     # NEW, unsafe (Principle V)
|   |-- mod.rs
|   `-- shm_map.rs           # mmap, guarded access + SIGBUS handler, sealed memfd
|-- render/
|   |-- gles.rs              # + textures, window quads, max texture size
|   `-- shaders/             # + window.vert, window.frag (swizzle, xrgb, transform, alpha)
|-- backend/wayland_host/    # seat: raw keycodes/masks, keymap text, repeat info
`-- state.rs                 # + server sources, uploads after cancel_read, frame callbacks

tests/
|-- shell.rs                 # windows + routing (InputEvent -> ClientInput)
|-- protocol/                # in-process server + wayland-client test clients
|   |-- main.rs
|   |-- harness.rs           # socket pair, dispatch both sides, shm helpers
|   |-- surfaces.rs          # compositor, subcompositor, xdg-shell, output, viewport
|   |-- input.rs             # focus, pointer and keyboard delivery
|   `-- misbehaving.rs       # every spec edge case (SC-004)
|-- socket.rs                # socket naming and cleanup (own binary: changes the environment)
`-- render_windows.rs        # pixel assertions with windows
```

**Structure Decision**: same single crate. Module direction: `canvas` <- `viewer` <- `shell`
<- `state` -> `server`, `backend`, `render`; `server` depends on `shell` types (`WindowId`,
`ClientInput`) but `shell` never on `server`; `ffi` is used by `server::shm` and the keymap.

### Supporting changes

- `Cargo.toml`: dependencies from R14; the lint comment on `unsafe_code` names the plugin loader
  too (constitution V as amended).
- `README.md`: how to run applications on RubickWay (`WAYLAND_DISPLAY`), shm-only note.
- `AGENTS.md` *Gotchas*: protocol tests use in-process clients (`tests/protocol/harness.rs`);
  every new global needs a version entry in `contracts/protocols.md`.
- CI: no new system packages (the server is pure Rust; tests need no host session).

## Complexity Tracking

No constitution violations to justify.
