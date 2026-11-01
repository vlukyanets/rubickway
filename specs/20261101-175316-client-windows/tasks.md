---

description: "Task list for client windows on the canvas"
---

# Tasks: Client Windows on the Canvas

**Input**: Design documents from `specs/20261101-175316-client-windows/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Required by the constitution (Principle III): every user story has test tasks that
are written and seen failing before its implementation tasks.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested
on its own.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story the task belongs to (US1, US2, US3)
- References like (R4) point to research.md; (FR-xxx) and (SC-xxx) to spec.md; "case N" to
  the numbered misbehaving-application cases in spec.md Edge Cases

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: dependencies and module skeleton

- [X] T001 Add the dependencies from research R14 to `Cargo.toml`: `wayland-server = "0.31"`, `libc = "0.2"`, and `"server"` in the `wayland-protocols` features list; change the comment on the `unsafe_code` lint to name "FFI, backend, render and plugin loader modules"; run *Install deps* and the quick check from `AGENTS.md`
- [X] T002 Create the module skeleton from plan.md: declare `pub mod shell; pub mod server; pub mod ffi;` in `src/lib.rs` with empty `src/shell/mod.rs`, `src/server/mod.rs`, `src/ffi/mod.rs` (the last with `#![allow(unsafe_code)]` only once it contains unsafe); keep the full check passing
- [X] T003 [P] Add a "Running applications on RubickWay" section to `README.md`: the startup line with the socket name, `WAYLAND_DISPLAY=<name> LIBGL_ALWAYS_SOFTWARE=1 kitty`, shared-memory clients only for now (contracts/cli.md, quickstart.md)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: geometry, input-type extensions, the shared-memory FFI layer and the server core
that every story uses

**CRITICAL**: no user story work can begin until this phase is complete

- [X] T004 [P] Write failing unit tests in `src/canvas/geometry.rs` for `Rect { origin: Point, size: Size }`: `contains(Point)` (half-open: left/top edge inside, right/bottom edge outside), `intersects(Rect)`, `translate(Vector)`, `bounding(&[Rect]) -> Option<Rect>`
- [X] T005 Implement `Rect` in `src/canvas/geometry.rs` and re-export it from `src/canvas/mod.rs`
- [X] T006 Extend the input types per data-model.md "input (changes)" in `src/input/mod.rs`: `Key { keycode: u32, binding_sym, pressed }`, `Modifiers { mods, raw: RawModifiers { depressed, latched, locked, group } }`, `PointerButton { button, code: u32, pressed }`, new `FocusGained`; fill the new fields in `src/backend/wayland_host/seat.rs` and emit `FocusGained` on the host's `wl_keyboard.enter` (the viewer ignores it); update `src/viewer/mod.rs`, `tests/viewer_input.rs` and `tests/keymap.rs` so all existing tests still pass unchanged in meaning
- [X] T007 Add `BackendEvent::Keymap(String)` and `BackendEvent::RepeatInfo { rate: i32, delay: i32 }` in `src/backend/mod.rs`; in `src/backend/wayland_host/seat.rs` push `Keymap` with the keymap text after it compiles and `RepeatInfo` from `wl_keyboard.repeat_info` (R11)
- [X] T008 [P] Write failing tests in `src/ffi/shm_map.rs` (unit tests) for the FFI layer (R4): mapping a memfd of 4096 bytes and reading it through the guarded access returns its bytes; after `ftruncate` to 0 the guarded read completes, yields zeros and reports a fault (the SIGBUS path) while the process keeps running; two threads reading two different pools at the same time, one of them truncated, each get their own result (the per-thread access record); an access outside the mapping is refused; growing the mapping keeps the old bytes; `sealed_memfd(text)` returns an fd whose content equals the text and whose seals include `F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE`
- [X] T009 Implement `src/ffi/shm_map.rs` (`#![allow(unsafe_code)]`, `// SAFETY:` on every block, `libc` only): `ShmMap::new(fd, size)` (`mmap` `PROT_READ`, `MAP_SHARED`), `grow(new_size)` (`mremap`), `with_access(offset, len, |&[u8]|) -> Result<T, AccessError>` using the once-installed `SA_SIGINFO` SIGBUS handler that looks up the faulting thread's access record (`thread_local!` with a `const` initialiser) and remaps anonymous zero pages over the accessed range with `MAP_FIXED` (no allocation or locks, async-signal-safe) and otherwise restores the previous handler; `sealed_memfd(&str) -> OwnedFd`; references: libwayland `wayland-shm.c`, Smithay `wayland/shm/pool.rs`
- [X] T010 Write the protocol test harness in `tests/protocol/main.rs` and `tests/protocol/harness.rs`: start a `Server` in-process (no host, no GL), connect `wayland-client` test clients through `UnixStream::pair()` via `Server::insert_client`, a `roundtrip` helper that dispatches server and client alternately until both are idle, a helper creating an shm pool + buffer filled with a solid ARGB colour, and a helper reporting whether a client was disconnected and with which protocol error; crate-level `#![expect(..., reason = "...")]` for helpers per `AGENTS.md`
- [X] T011 Implement the server core in `src/server/mod.rs` (R1, R3, R8): `Server` owning `Display<ServerState>` with `set_default_max_buffer_size(1 << 20)`, `ServerState`, `ClientState` (numeric id; logs `application <id> connected` / `disconnected` with the reason, contracts/cli.md), `insert_client(UnixStream)`, `dispatch()`, `flush()`, `poll_fd()`; `listen()` with `ListeningSocket::bind_auto("wayland", 1..33)` returning the socket and its name; no globals yet
- [X] T012 [P] Write a failing test in `tests/socket.rs` (one test function, sets `XDG_RUNTIME_DIR` to a temp dir at its start): `Server::listen` picks `wayland-1`, a second `listen` picks `wayland-2`, both socket files and `.lock` files exist, and dropping a listener removes its files (FR-001, FR-002); then make it pass

**Checkpoint**: foundation ready - user stories can start

---

## Phase 3: User Story 1 - An application's window appears on the canvas (Priority: P1) - MVP

**Goal**: kitty (software rendering) started against RubickWay's socket shows its window
centred in the view, moving and scaling with the canvas; closing or crashing removes it;
misbehaving applications only disconnect themselves.

**Independent Test**: start RubickWay, start kitty with the printed socket name, see the
prompt, pan and zoom, exit kitty (quickstart M1-M5, M14, M17-M20).

### Tests for User Story 1 (write first, see them fail)

- [X] T013 [P] [US1] Write failing unit and `proptest` tests in `src/shell/windows.rs` per data-model.md "Windows": `map` places the geometry centred on the camera centre (any centre within 1e6, any zoom) and puts the window on top; `update` with a new size keeps `origin` (clarification 2); `unmap` removes it; `hit` returns the topmost window whose input rects contain the point and the window-local point `canvas - origin` within 1e-6, skips a window whose input rects miss the point and returns the window below, returns `None` on the background; and in `tests/shell.rs`: `Shell::apply` of `Mapped` / `Updated` / `Unmapped` changes the stack and marks the view dirty, `Shell::visible(view)` lists partly visible and covered windows back to front and omits windows fully outside the view (FR-006)
- [X] T014 [P] [US1] Write failing protocol tests in `tests/protocol/surfaces.rs` for globals and core surfaces: the client sees exactly the globals and versions of `contracts/protocols.md`; `wl_surface` state is double-buffered (attach without commit changes nothing); `set_buffer_scale(0)` and an invalid transform are protocol errors; a buffer not divisible by its scale without a viewport is `invalid_size`; viewport destination sets the logical size and a source outside the buffer is `out_of_buffer`; subsurfaces: synchronised state applies on the parent's commit, desync applies at once, `place_above` / `place_below` order, a parent that is its own descendant is `bad_parent`
- [X] T015 [US1] Write failing protocol tests in `tests/protocol/surfaces.rs` for xdg-shell and output (R9, R12): the first configure has size 0x0 and `activated`; committing a buffer before acking is `unconfigured_buffer`; after ack + buffer commit the server emits `WindowEvent::Mapped` with the size of `set_window_geometry` if set, else the surface tree's bounding box (centring is checked in T013); a later commit with a larger buffer emits `Updated` with the new size; attaching null, destroying the toplevel and disconnecting emit `Unmapped`; `get_popup` gets `popup_done`; `set_maximized`, `set_fullscreen`, `set_minimized`, `move`, `resize`, `show_window_menu` cause no error; `wl_output` reports name `RUBICKWAY-1`, the viewer buffer size as mode and ceil(scale), and re-sends after a scale change, together with `preferred_buffer_scale` and fractional `preferred_scale` = round(scale x 120) (FR-017)
- [X] T016 [US1] Write failing protocol tests in `tests/protocol/surfaces.rs` for the clipboard stub (R10): data sources and devices can be created and destroyed, `set_selection` is accepted, no `data_offer` or `selection` event is ever sent, `start_drag` sends `cancelled` to its source
- [X] T017 [US1] Write failing protocol tests in `tests/protocol/surfaces.rs` for frame pacing (FR-006, SC-002, R12): a commit with new content marks the server dirty and the window's surface parts then carry that buffer for the next frame; frame callbacks complete only after `presented(ids)` and only for the window ids passed (the visibility decision itself - partly visible counts, covered counts, outside does not - is tested on `Shell::visible` in `tests/shell.rs` within T013's scope); with no presentation (viewer hidden) none complete
- [X] T018 [US1] Write failing tests in `tests/protocol/misbehaving.rs` for cases 1, 2, 3 (basic), 4, 5, 6, 7 (SC-004), each asserting that only the offending client is disconnected and a second, well-behaved client still gets a frame callback afterwards: (1) `create_buffer` with negative size, overflowing `stride * height`, offset beyond the pool, unknown format; (2) `ftruncate` of the pool file after the buffer was committed, then a server-side read of the buffer -> `invalid_fd`; `wl_shm_pool.resize` to a smaller size -> error; (3) a client that disconnects while mapped and mid-commit -> `Unmapped` is emitted and its surfaces and buffers are released (focus handling is tested in T035); (4) a client that never reads its socket while the server sends it events until more than 1 MiB is pending -> disconnected; (5) a client that sends 100 000 `wl_surface.damage` requests in a burst -> the other client's frame callback still arrives in the same dispatch round; (6) a client that creates a toplevel and never commits a buffer -> no window, others unaffected; (7) a buffer one pixel wider than the texture limit given to the server (`Server::set_max_texture_side`) -> `wl_surface` `invalid_size` error
- [X] T019 [P] [US1] Write failing pixel tests in `tests/render_windows.rs` (offscreen llvmpipe, as `tests/render_grid.rs`): a solid-colour window covers exactly its expected rectangle at zoom 1 and 2 with the grid around it; two overlapping windows show the top one; a 50%-alpha premultiplied window blends over the grid; an `xrgb8888` buffer with alpha 0 bytes is drawn opaque; a `flipped-180` transform (a vertical flip) puts the buffer's top-left texel at the bottom-left; a part (subsurface) placed below its opaque parent is hidden where they overlap; a window of one-pixel white lines every fourth column reads as even dark grey (each channel 64 +-12) at 25% zoom, and the test fails with mipmaps disabled (FR-005)

### Implementation for User Story 1

- [X] T020 [US1] Implement `Window`, `Windows` and `WindowId` in `src/shell/windows.rs` per data-model.md ("Placement on map: `origin = camera.center - size / 2`", "Resize: `size` replaced, `origin` unchanged"), `WindowEvent`, and a `Shell` in `src/shell/mod.rs` that owns the `Viewer` and `Windows`, with `apply(WindowEvent)`, `visible(view)`, and `handle(&InputEvent) -> Effect` forwarding to the viewer (routing comes in US2)
- [X] T021 [US1] Implement `src/server/compositor.rs` (R6): `wl_compositor` v6, `wl_region`, `wl_surface` with pending/current state (buffer + offset, damage, frame callbacks, input region, scale, transform, viewport), `commit`, role tracking, logical size, the protocol errors listed in data-model.md "Surface", surface-tree traversal (bounding box, input shape); register the global in `src/server/mod.rs`
- [X] T022 [US1] Implement `src/server/subcompositor.rs`: `wl_subcompositor` v1, `wl_subsurface` with position, `place_above` / `place_below`, sync/desync caching per R6 (reference: Smithay `wayland/compositor`, wlroots `wlr_subcompositor.c`)
- [X] T023 [US1] Implement `src/server/shm.rs` (R4, data-model.md "ShmPool / ShmBuffer"): `wl_shm` v1 advertising `argb8888` and `xrgb8888`, pools on `ffi::shm_map::ShmMap` (grow only), buffers validated with checked arithmetic ("`offset + stride * height <= pool size`"), `ShmBuffer::read(|bytes, width, height, stride, format|)` through the guarded access, disconnecting the client with `invalid_fd` on a fault; `wl_buffer.release` after the content has been copied
- [X] T024 [US1] Implement `src/server/viewporter.rs`: `wp_viewporter` v1 (source, destination, the protocol's errors) and `wp_fractional_scale_manager_v1` v1 sending `preferred_scale` = round(scale x 120) on creation and on scale changes
- [X] T025 [US1] Implement `src/server/output.rs`: one `wl_output` v4 per client bind (geometry, mode = viewer buffer size, scale = ceil(viewer scale), name `RUBICKWAY-1`, description, done), `Server::set_viewer(size, scale)` re-sending to outputs, `preferred_buffer_scale` and fractional scale; `wl_surface.enter` for surfaces of mapped windows
- [X] T026 [US1] Implement `src/server/xdg_shell.rs` (R9): `xdg_wm_base` v6 with ping/pong, `xdg_surface` (configure serials, `set_window_geometry`), `xdg_toplevel` (initial configure 0x0 + `activated`, `unconfigured_buffer` check, `WindowEvent::Mapped` on the first buffer, `Updated` on a size or input-shape change, `Unmapped` on null buffer / destroy / disconnect (the server never touches the shell), title / app_id / min / max stored and logged as `window <id> opened: <app_id> "<title>"` / `closed`, window-management requests ignored), `xdg_popup` answered with `popup_done`
- [X] T027 [US1] Implement `src/server/data_device.rs` (R10): `wl_data_device_manager` v3, data sources kept until destroyed, data devices that never send offers, `set_selection` ignored, `start_drag` -> `cancelled`
- [X] T028 [US1] Implement the seat global skeleton in `src/server/seat.rs`: `wl_seat` v8 named `seat0` with pointer + keyboard capabilities, `get_pointer` / `get_keyboard` objects (no input delivery yet; keymap and repeat info are sent once T042 lands), `wl_pointer.set_cursor` accepted (cursor role, not shown)
- [X] T029 [US1] Implement the drawing interface and frame pacing in `src/server/mod.rs` (R7 data flow, R12): `Server::take_window_events() -> Vec<WindowEvent>`; `Server::window_parts(id) -> Vec<SurfacePart>` (surface tree in drawing order, each with its rect in window coordinates, transform and texture or pending upload); `Server::take_dirty()`; `Server::presented(time, &[WindowId])` completing frame callbacks of those windows' surfaces; `Server::has_pending_callbacks(&[WindowId])`; `Server::set_max_texture_side(n)` and the case-7 check on commit
- [X] T030 [US1] Extend `src/render/gles.rs` and add `src/render/shaders/window.vert`, `window.frag` (R5): `GlesRenderer::max_texture_side()` from `GL_MAX_TEXTURE_SIZE`; `Texture` upload from shm bytes (`glTexImage2D` `GL_RGBA` / `GL_UNSIGNED_BYTE`, BGRA swizzle in the shader, alpha 1 for `xrgb8888`, `glGenerateMipmap`, `LINEAR_MIPMAP_LINEAR` / `LINEAR`); `draw_windows(&[WindowQuad])` with quad positions computed in f64 on the CPU, premultiplied blending (`ONE, ONE_MINUS_SRC_ALPHA`) and the 8 buffer transforms; offscreen variant for `tests/render_windows.rs`
- [X] T031 [US1] Integrate the server into `src/state.rs` (R3, R5, R7, R12): `App` replaces its `Viewer` with the `Shell` and gains `Server` and the `ListeningSocket`; apply `take_window_events()` to the shell each step; draw the shell's `visible(view)` windows back to front using `window_parts`, and pass those ids to `presented` / `has_pending_callbacks`; calloop sources for the listener (`accept` -> `insert_client`) and the display fd (`dispatch`); print `applications can connect with WAYLAND_DISPLAY=<name>` at info level after the window is created; in `step()` after `cancel_read`: upload pending buffers with the context current (never inside server dispatch), render grid + windows, `presented` on `FrameDone`, request a host frame with an empty commit when only client callbacks are pending, `flush()` the server before sleeping; pass the viewer size/scale to `Server::set_viewer` and the texture limit to `set_max_texture_side`
- [X] T032 [US1] Add `AppError::Socket` with message `cannot open a socket for applications: <reason>` (contracts/cli.md) in `src/state.rs`; make sure the listener (and so the socket and lock files) is dropped on every exit path
- [X] T033 [US1] Run the full check, then quickstart M1-M5, M14, M17, M18, M19 (if a second monitor scale is available) and M20 with kitty on niri and note the results - full check passed; M1, M3, M14 (kill) and M20 passed in an agent smoke run; the owner ran a subset of the rest with no failures

**Checkpoint**: User Story 1 is functional and testable on its own (MVP)

---

## Phase 4: User Story 2 - Use an application on the canvas (Priority: P2)

**Goal**: a new window gets the keyboard; clicking focuses and raises; pointer input reaches the
window under the pointer at the right position at any zoom; shortcuts stay with RubickWay.

**Independent Test**: two kittys - type, click between them, scroll, select text at 50% and 200%
(quickstart M4, M6-M8, M10-M14).

### Tests for User Story 2 (write first, see them fail)

- [X] T034 [P] [US2] Write failing routing tests in `tests/shell.rs` (`InputEvent` sequences -> `ClientInput`s, data-model.md): motion over a window gives `PointerEnter` + `PointerMotion` in window coordinates at zoom 0.5, 1 and 2 (SC-003); leaving it gives `PointerLeave`; a left/right/middle press on an unfocused window gives `KeyboardFocus`, raises it and delivers the button (FR-009); a press grabs: motion, buttons and scroll keep going to it outside it and over another window until the last release (FR-010), and a host `PointerLeave` during the grab ends it; a press outside a window's input rects goes to the window below or the background; keys go to the focused window, Mod+0 and Mod+Shift+Q are consumed together with their releases (FR-011); `Modifiers` carry the raw masks
- [X] T035 [US2] Write failing focus tests in `tests/shell.rs`: a newly mapped window gets focus even while keys are held for another window (FR-008); unmapping the focused window focuses the topmost remaining one or none (FR-014); presses on the background (any button) never change focus (FR-013); right/middle presses on the background produce nothing; `FocusLost` from the host gives `KeyboardFocus(None)` and `FocusGained` gives `KeyboardFocus(Some(previous))`; unmapping the grabbing window ends the grab and drops its held keys (case 3)
- [X] T036 [P] [US2] Write failing protocol tests in `tests/protocol/input.rs`: `wl_keyboard` receives the keymap as a sealed fd of the right size and format `xkb_v1`, repeat info from the host (25/600 by default), `enter` with the currently pressed keys, `key` with raw keycodes, `modifiers` with the layout group, `leave` on focus change; `wl_pointer` receives `enter` / `motion` with the expected surface-local coordinates (also inside a subsurface), `button`, `axis` + `axis_source` + `axis_value120` (v8) or `axis_discrete` (v5-7), each group ended by `frame` (no `axis_stop`, see research R11); the toplevel's configure toggles `activated` with focus; a client disconnecting mid-grab or with a held key leaves the shell free and focus moved (case 3)

### Implementation for User Story 2

- [X] T037 [US2] Implement the pointer routing state and key routing in `src/shell/route.rs` per data-model.md ("Free / Grabbed", consumed shortcuts and their releases, `FocusLost` handling)
- [X] T038 [US2] Extend `Shell` in `src/shell/mod.rs`: `handle(&InputEvent) -> (Effect, Vec<ClientInput>)` delegating canvas gestures to the viewer and window input to `route`, focus rules from FR-008, FR-009, FR-013, FR-014, dirty on any window change; `ClientInput` per data-model.md
- [X] T039 [US2] In `src/state.rs`, forward the `ClientInput`s from `Shell::handle` to the server, keeping `Effect.quit` and the cursor update from `is_panning`
- [X] T040 [US2] Implement input delivery in `src/server/seat.rs` (R11): map `ClientInput` to `wl_pointer` / `wl_keyboard` events of the window's client, pick the surface (root or subsurface) and surface-local position from window coordinates, serials, monotonic millisecond times, axis events per seat version, `frame` grouping, `enter` with pressed keys, `activated` configure on focus change via `src/server/xdg_shell.rs`
- [X] T041 [US2] Clear input state on disconnect in `src/server/mod.rs` and `src/shell/mod.rs`: unmapping a window that holds the grab or focus ends the grab, drops its held keys and moves focus (case 3)
- [X] T042 [US2] Keymap and repeat info for applications in `src/server/seat.rs`: on `BackendEvent::Keymap` write the text to `ffi::shm_map::sealed_memfd` and send it to every keyboard (and to new keyboards), on `RepeatInfo` forward rate and delay (default 25/600)
- [X] T043 [US2] Run the full check, then quickstart M4, M6-M8 and M10-M14 with two kittys on niri and note the results - full check passed; the owner ran a subset with no failures

**Checkpoint**: User Stories 1 and 2 work

---

## Phase 5: User Story 3 - Navigate the canvas over windows (Priority: P3)

**Goal**: Mod + left-drag, Mod + scroll and pinch move the canvas anywhere, including over
windows, without the application receiving anything.

**Independent Test**: zoom in until a window fills the viewer, pan away with Alt + left-drag and
zoom out with Alt + scroll over the window (quickstart M9, M10, M16).

### Tests for User Story 3 (write first, see them fail)

- [X] T044 [P] [US3] Write failing routing tests in `tests/shell.rs`: Mod + left press and drag over a window pans exactly like on the background and produces no `ClientInput` (no button, motion, enter/leave) and no focus change or raise (FR-012); Mod + scroll over a window zooms around the pointer with no `Scroll` delivered; a pinch over a window zooms the canvas; releasing Mod mid-drag keeps the pan until the button is released; plain drag and scroll on the background behave as in nested-canvas (FR-013)

### Implementation for User Story 3

- [X] T045 [US3] Extend `src/shell/route.rs` and `src/shell/mod.rs` so Mod + left, Mod + scroll and pinch go to the viewer anywhere, suppress enter/leave to windows during a canvas pan, and resume pointer focus on the window under the pointer when the pan ends
- [X] T046 [US3] Run the full check, then quickstart M9 and M16 on niri and note the results - full check passed; the owner ran a subset with no failures

**Checkpoint**: all user stories work

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T047 [P] Add to *Gotchas* in `AGENTS.md`: "**Protocol tests** run the server in-process with `wayland-client` test clients (`tests/protocol/harness.rs`); every new global needs its version in the feature's `contracts/protocols.md`." and update the unsafe-modules gotcha if `src/ffi/` needs mentioning
- [X] T048 Run quickstart M15 (five idle kittys, SC-005) and collect all M1-M20 results for the PR description - collected: not every M check was run by hand; the PR description lists which were
- [X] T049 Check that code matches spec.md, plan.md, research.md, data-model.md and contracts/; update the documents first wherever the implementation had to deviate
- [X] T050 Run the full check from `AGENTS.md` and confirm it passes

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none; T002 after T001
- **Foundational (Phase 2)**: after Setup; blocks all stories. T005 after T004, T009 after T008,
  T007 after T006, T010 and T012 after T011 (T010 can be written alongside T011)
- **US1 (Phase 3)**: after Foundational
- **US2 (Phase 4)**: after US1 (needs mapped windows, the seat skeleton and the server in `App`)
- **US3 (Phase 5)**: after US2 (extends the routing in `src/shell/route.rs`)
- **Polish (Phase 6)**: after the stories

### Within Each User Story

- Tests first, and they must fail before implementation starts
- Pure `shell` code before the server, the server before `state.rs`
- `src/server/mod.rs`, `src/server/seat.rs`, `src/shell/mod.rs`, `src/shell/route.rs`,
  `src/state.rs` and `tests/shell.rs` are touched by several tasks: never in parallel
- US1 implementation order: T020, T021, T022, T023, T024, T025, T026, T027, T028, T029, T030,
  T031, T032

### Parallel Opportunities

- Setup: T003 alongside T001/T002
- Foundational: T004, T008 together; T012 once T011 is done
- US1 tests: T013, T014 and T019 together (T015-T017 follow T014 in the same file); T018 after
  T010
- US2 tests: T034 and T036 together (T035 follows T034 in the same file)
- Polish: T047 alongside T048

---

## Parallel Example: User Story 1

```text
Task: "Write failing Windows tests in src/shell/windows.rs"             (T013)
Task: "Write failing protocol tests in tests/protocol/surfaces.rs"      (T014)
Task: "Write failing pixel tests in tests/render_windows.rs"            (T019)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 Setup, Phase 2 Foundational
2. Phase 3 User Story 1
3. **Stop and validate**: full check plus quickstart M1-M5, M14, M17-M20

### Incremental Delivery

1. Setup + Foundational
2. US1 -> applications show windows on the canvas (MVP)
3. US2 -> typing and clicking in them
4. US3 -> navigating over them
5. Polish -> remaining manual checks, docs in sync, PR

---

## Notes

- Commit after each task or logical group, only with the full check passing
- `unsafe` only in `src/ffi/shm_map.rs` and `src/render/`, each block with `// SAFETY:`
- No `unwrap`/`expect`/`panic!` outside tests; exceptions need `#[expect(..., reason = "...")]`
- Every client-supplied number goes through checked arithmetic before it sizes memory

## Phase 7: Convergence

- [X] T051 Make the `DisconnectReason::ConnectionClosed` log line in `src/server/mod.rs` say why an application left as far as RubickWay can tell (`application <id> disconnected: connection closed or messages unread`), because wayland-backend reports a full outgoing buffer (case 4) the same way as a normal exit; record this in research R8 and `contracts/cli.md` per Edge Cases "logs which application it disconnected and why" (partial)
