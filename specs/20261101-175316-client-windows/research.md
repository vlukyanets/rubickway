# Research: Client Windows on the Canvas

Decisions made while planning `client-windows`. Each entry: decision, rationale, alternatives.
Crate versions are the latest on crates.io on 2026-11-01. "Reference" names reading material
only (constitution I).

## R1. Serving applications

- **Decision**: `wayland-server` 0.31 with `wayland-backend`'s pure-Rust server implementation
  (the default; no `server_system`), and the `server` feature of `wayland-protocols` 0.32. The
  socket comes from `wayland_server::ListeningSocket::bind_auto("wayland", 1..33)`: the first
  free `wayland-N` in `$XDG_RUNTIME_DIR`, guarded by a lock file, so it never collides with the
  host's socket or another RubickWay (FR-002). `$XDG_RUNTIME_DIR` is mode 0700, so only the same
  user can connect (FR-001). The socket and lock file are removed when RubickWay exits.
- **Rationale**: constitution I names `wayland-server` / `wayland-protocols` for protocol code.
  The pure-Rust backend needs no C library, and libwayland-server is only needed for the legacy
  `wl_drm` / `EGL_WL_bind_wayland_display` path, which we will not use (dmabuf comes later).
- **Alternatives**: `server_system` (libwayland-server underneath) - no benefit now; a
  hand-rolled socket - `bind_auto` already implements the lock-file convention.

## R2. Globals offered

| Global | Version | Why |
| --- | --- | --- |
| `wl_compositor` | 6 | surfaces and regions; v5 `offset`, v6 `preferred_buffer_scale` |
| `wl_subcompositor` | 1 | foot (and GTK) refuse to start without it |
| `wl_shm` | 1 | shared-memory buffers, formats `argb8888` and `xrgb8888` (the mandatory two) |
| `xdg_wm_base` | 6 | top-level windows; popups are dismissed at once (R9) |
| `wl_seat` | 8 | pointer and keyboard; v8 `axis_value120` (same as the host seat we bind) |
| `wl_output` | 4 | the viewer as the one display; foot refuses to start without an output |
| `wl_data_device_manager` | 3 | stub: foot refuses to start without it; no selection is ever offered (R10) |
| `wp_viewporter` | 1 | needed by fractional scale (destination size) |
| `wp_fractional_scale_manager_v1` | 1 | FR-017 at fractional host scales |

- **Decision**: offer exactly these, each at the version listed and no higher (constitution VI).
  Not offered: `zxdg_decoration_manager_v1` (applications draw their own decorations, as the
  spec assumes), `wp_cursor_shape_manager_v1` and client cursor surfaces (the host's default
  arrow is shown over windows; cursors come with window management), `zwp_pointer_gestures_v1`,
  `wp_presentation`, primary selection, text input, `linux-dmabuf`.
- **Rationale**: the acceptance application is kitty (spec Assumptions). Its GLFW Wayland
  backend (verified in `kitty/glfw/wl_init.c`, 2026-11-01) requires only `xdg_wm_base` and
  `wl_shm` and uses `wl_compositor` unconditionally; subcompositor, output, seat, viewporter,
  fractional scale, data device and decorations are optional. kitty renders with OpenGL through
  EGL; until `linux-dmabuf` exists it runs with `LIBGL_ALWAYS_SOFTWARE=1`, where Mesa's
  Wayland EGL platform draws with llvmpipe into `wl_shm` buffers. foot, the strictest common
  shm client, requires (verified in `foot/wayland.c`, 2026-11-01) `wl_compositor` >= 4,
  `wl_subcompositor`, `wl_shm`, `xdg_wm_base`, `wl_data_device_manager` >= 3, `wl_seat` >= 5
  and `wl_output` >= 2; offering foot's set covers kitty and simpler clients too.
  weston-terminal's requirements were not verified (not part of acceptance).
- **Alternatives**: also offering decorations / cursor shape now - not needed by the spec.

## R3. Event loop

- **Decision**: two more `calloop` sources on the existing loop: the listening socket
  (readable -> `accept`, `Display::handle().insert_client(stream, ClientState)`) and the
  `Display` poll fd (readable -> `display.dispatch_clients(&mut server)`). `step()` runs
  `display.flush_clients()` before sleeping. Client sockets are non-blocking; a full socket
  leaves messages in the per-client buffer (R8).
- **Rationale**: Principle VII - one loop. `Display` and the server state are separate fields of
  `App`, so dispatch borrows them independently.
- **Alternatives**: a server thread - violates the single-loop design and makes the renderer
  shared state.

## R4. Shared memory and SIGBUS

- **Decision**: pools are mapped read-only (`MAP_SHARED`, `PROT_READ`) in a new FFI module
  `src/ffi/shm_map.rs` (`libc` 0.2). `wl_shm_pool.resize` may only grow (protocol); the mapping
  is replaced. Every read of pool memory goes through one guarded access function. A process
  SIGBUS handler (installed once, `SA_SIGINFO`) checks whether the faulting address lies in the
  pool currently being accessed by the faulting thread (a per-thread record: `thread_local!`
  with a `const` initialiser, so reading it in the handler needs no allocation or lock - as
  Smithay does; parallel test threads therefore do not disturb each other). The record and the
  fault flag are atomics, and `compiler_fence(SeqCst)` brackets the guarded read: the handler
  runs on the same thread invisibly to the optimiser, which otherwise drops the record and
  folds the fault check to "no fault" in release builds. If the address is in the pool, it maps
  anonymous zero pages over that range with `MAP_FIXED`, records the fault and returns, so the
  read finishes on zeros. Otherwise it restores the previous handler and returns, so the signal
  is re-raised with default behaviour. After the access, a recorded fault disconnects the client
  with `wl_shm` error `invalid_fd`. Every access is guarded, also for pools sealed against
  shrinking: the guard costs two thread-local writes, and one code path is simpler to test.
- **Rationale**: an application can truncate the file behind a pool at any time; the next read
  would kill RubickWay with SIGBUS (spec edge case, FR-015). This is how libwayland-server
  (`wayland-shm.c`, `wl_shm_buffer_begin_access`) and Smithay (`wayland/shm/pool.rs`) solve it -
  references. GL texture uploads read pool memory inside the driver; the handler covers that too,
  because it is process-wide and the upload copies synchronously (`glTexImage2D` copies before
  returning).
- **Alternatives**: copying with `pread` from the fd (no mmap, no SIGBUS) - works only for
  regular files and memfds and costs a syscall per frame per window, but would avoid the
  handler; kept as the fallback if the handler proves fragile. Requiring sealed memfds - most
  clients do not seal.

## R5. Drawing windows

- **Decision**: one GL texture per surface with content. Buffers are uploaded with
  `glTexImage2D` (full buffer, `GL_RGBA` / `GL_UNSIGNED_BYTE`; the shader swizzles the
  little-endian `ARGB8888` bytes B,G,R,A and forces alpha 1 for `XRGB8888`), then
  `glGenerateMipmap`; sampling uses `LINEAR_MIPMAP_LINEAR` / `LINEAR`, so zoomed-out windows do
  not shimmer and zoomed-in ones are smooth (FR-005, clarification 5). The buffer is released
  (`wl_buffer.release`) right after the upload, so the client can reuse it. Windows are drawn
  after the grid, back to front, each surface tree in order (subsurfaces below/above per their
  stacking) as textured quads with premultiplied-alpha blending (`ONE, ONE_MINUS_SRC_ALPHA`).
  Quad positions are computed on the CPU in `f64` (camera -> physical pixels) and passed as small
  floats, as for the grid (no jitter far from the origin). `wl_surface.set_buffer_transform` is
  honoured with a texture-coordinate transform (8 cases). Uploads happen in `step()` after the
  host read guard is dropped, never inside server dispatch (the guard rule from nested-canvas R3).
- **Max size**: the renderer reads `GL_MAX_TEXTURE_SIZE`; a committed buffer wider or taller
  disconnects the client (clarification 3) with the `wl_surface` `invalid_size` error naming the
  limit (wayland-server's pure-Rust backend has no API to send `wl_display` errors, and
  `Client::kill` closes the connection without telling the client why).
- **Rationale**: full uploads are simple and fast enough for terminals (a 1000x700 window is
  2.8 MB); damage-based partial uploads are an optimisation for later (constitution VI).
- **Alternatives**: `wl_shm` -> CPU compositing (throwaway once dmabuf arrives); `EXT_texture_
  format_BGRA8888` (not guaranteed on all drivers; the swizzle is one line).

## R6. Surface state and commit

- **Decision**: per surface, double-buffered pending/current state: buffer (+ offset), damage
  (recorded, not used for partial uploads yet), frame callbacks, input region, opaque region
  (ignored), buffer scale, buffer transform, viewport source/destination. Subsurfaces use the
  protocol's sync/desync caching (state applied on the parent's commit when synchronised).
  Surface logical size = viewport destination, else (viewport source size, else buffer size
  after transform) / scale; protocol errors per the spec of each request (non-positive scale,
  buffer size not divisible by scale without a viewport, viewport source outside the buffer,
  bad subsurface parent). `wl_surface.offset` (and a non-zero attach offset before v5) is
  accepted but not applied: it only matters for cursor and drag-icon surfaces, which are not
  shown yet. Reference: Smithay `wayland/compositor` (cached state) and wlroots
  `types/wlr_compositor.c` / `wlr_subcompositor.c`.
- **Rationale**: subsurfaces are required by foot (R2) and used for CSD by many toolkits;
  correct commit semantics are what every later feature builds on.

## R7. Windows on the canvas (core model)

- **Decision**: a pure `shell` module (no Wayland types, Principle IV) holds windows by
  `WindowId`: the window geometry's top-left canvas position, its size in canvas units, the
  stacking order and keyboard focus. Geometry = `xdg_surface.set_window_geometry` when set
  (excludes CSD shadows), else the bounding box of the surface tree. On map, the top-left is
  chosen so the geometry is centred on the view centre (FR-004). When the geometry size changes,
  the top-left stays (clarification 2). 1 canvas unit = 1 logical pixel of the application, so
  coordinates the application sees are `canvas point - window origin`, independent of zoom
  (FR-010, SC-003). Hit-testing: the server gives the shell each window's input shape (rects of
  the surface tree's input regions in window coordinates); the shell finds the topmost window
  under a canvas point and returns window-local coordinates; the server then picks the surface
  and surface-local position.
- **Data flow**: the server never touches the shell. It queues `WindowEvent`s (mapped, updated,
  unmapped) that `App` applies to the shell, the same way host events are queued as
  `BackendEvent`s. For drawing, `App` takes the stack, positions and visibility from the shell
  and each visible window's surface parts (window coordinates, textures) from the server; after
  presenting, it passes the visible window ids back so the server completes their frame
  callbacks. This keeps the server testable without a shell and the shell without a server.

## R8. Clients that do not read, or flood

- **Decision**: `Display::set_default_max_buffer_size(1 MiB)`. wayland-backend buffers outgoing
  messages per client and kills the client (`ConnectionClosed`) when a message does not fit
  after a non-blocking flush (verified in `wayland-backend` 0.3.17, `rs/socket.rs`
  `write_message`, `rs/server_impl/client.rs`). Requests are read per client in bounded chunks
  by `dispatch_clients`, so a flooding client gets its turn like the others; an integration test
  checks that a second client keeps receiving frame callbacks while one floods. Because the
  disconnect reason is the same `ConnectionClosed` as a normal exit, the log line cannot tell
  the two apart and names both (`connection closed or messages unread`, contracts/cli.md).
- **Rationale**: 1 MiB holds thousands of input events for a briefly busy client, but bounds the
  memory a stuck one can pin. FR-015, FR-016.
- **Known limit**: the number of objects a client creates is not limited (as in libwayland
  compositors); it costs the client's own requests but RubickWay memory. Accepted for now.

## R9. xdg-shell behaviour

- **Decision**: the first configure has size 0x0 (the application chooses) and state
  `activated` when the window will get focus (new windows always do, FR-008). Focus changes send
  a configure with or without `activated`. `set_title` / `set_app_id` are stored for logging.
  `move`, `resize`, `set_maximized`, `set_fullscreen`, `set_minimized`, `show_window_menu` are
  accepted and ignored (no window management in this feature); `set_min/max_size` are stored.
  `get_popup`: the popup is created and immediately sent `popup_done` (popups are not shown).
  A toplevel maps when its surface commits its first buffer after the initial configure ack;
  committing a buffer before the first ack is the protocol error `unconfigured_buffer`.
- **Rationale**: valid protocol responses keep applications running even though the features
  they request do not exist yet.

## R10. Clipboard stub

- **Decision**: `wl_data_device_manager` v3 creates data sources and data devices; sources are
  kept until destroyed, `set_selection` is accepted and ignored, no `data_offer` /
  `selection` events are ever sent, drag-and-drop `start_drag` sends `cancelled` to the source.
- **Rationale**: foot requires the global (R2). The spec's "no clipboard" stays true: nothing is
  transferred. Every request is handled and covered by a test (constitution VI).

## R11. Input to applications

- **Decision**:
  - The shell routes each `InputEvent`: Mod + left-drag and Mod + scroll (anywhere), plain
    drag/scroll and pinch on the background, and pinch anywhere (no gestures are offered to
    applications) go to the canvas viewer; a Mod + left press on a window neither focuses nor
    raises it (FR-012). Right and middle presses on the background are ignored. Other pointer
    input goes to the window under the pointer (input regions respected; outside them the next
    window below or the background). A button pressed on a window grabs the pointer for that
    window until all buttons are released: motion, buttons and scrolling keep going to it, also
    outside it or over another window. A press on a window without focus focuses and raises it,
    and is delivered too (FR-009).
  - When the grabbing or focused application disconnects, the grab ends (the pointer targets
    whatever is under it at the next event), its held keys are dropped, and focus moves per
    FR-014. When the viewer loses keyboard focus in the host (`FocusLost`), the focused
    application gets `wl_keyboard.leave`; `enter` follows when the host gives the viewer the
    keyboard again.
  - `wl_pointer`: enter/leave with serials, motion, button, axis + `axis_source` +
    `axis_value120` (v8) + `axis_discrete` (v5-7 clients), grouped by `wl_pointer.frame`.
    `axis_stop` is not sent: the host backend does not pass the host's stop on as an input
    event, and no kinetic scrolling depends on it yet. While a button is held, the pointer
    stays on the surface where it was pressed (implicit grab), also when it moves over a
    subsurface. Times come from the compositor's monotonic clock (ms since startup).
  - `wl_keyboard`: the host keymap text is written to a sealed memfd (`F_SEAL_SHRINK | GROW |
    WRITE`) and sent to every keyboard (format `xkb_v1`, mapped `MAP_PRIVATE` by v7+ clients);
    each send reopens it through `/proc/self/fd/N`, so every client gets its own file offset;
    repeat info is forwarded from the host (25/600 if the host sent none); `enter` (with the
    currently pressed keys), `leave`, `key` and `modifiers` carry the host's raw keycodes and
    modifier masks, including the layout group. A key press that triggers a compositor shortcut
    is consumed, and so is its release.
  - Known limit: `enter` is followed by the last modifier state delivered to a window; if the
    modifiers changed while no window had focus, the newly focused window sees the old state
    until the next modifier change.
  - The nested backend therefore adds raw keycodes and raw modifier masks to its input events,
    and reports the keymap text and repeat info.
- **Rationale**: FR-010-FR-013; forwarding the host's keymap keeps layouts and layout switching
  identical (spec edge case "Keyboard layout").

## R12. Output, scale and frame pacing

- **Decision**: one `wl_output` ("RUBICKWAY-1"): mode = viewer buffer size, integer `scale` =
  ceil(viewer scale), sent again when the viewer changes. Mapped surfaces get `wl_surface.enter`
  for it, `preferred_buffer_scale` = ceil(scale) (v6) and, with fractional scale, `preferred_scale`
  = round(scale * 120); all three are sent again whenever the viewer's scale changes (FR-017).
  Frame callbacks of surfaces of windows that at least partly intersect the view (occlusion by
  other windows is not tracked) are completed when the host's frame callback for the frame that
  showed them arrives (FR-006); windows outside the view or a hidden viewer get none. A commit
  with new content marks the view dirty, so it is drawn in the next frame (SC-002). When only callbacks are pending (no new
  content), RubickWay requests a host frame callback with an empty commit instead of redrawing.
- **Rationale**: pacing applications by what is actually shown keeps idle and hidden CPU at zero
  (FR-018) and matches nested-canvas's pacing.

## R13. Testing (Principle III)

- **Decision**:
  - `shell`: unit and property tests - placement centred on the view, top-left fixed on resize,
    stacking, focus rules (new, click, background, removal), hit-testing at zoom 0.1-10 within
    1e-6 (SC-003), routing of `InputEvent` sequences (Mod, grabs, consumed shortcuts).
  - Protocol: integration tests run the server in-process without a host or GL; test clients
    use `wayland-client` over `UnixStream::pair()` and the test drives both dispatch loops. They
    cover globals and versions, surface/subsurface commit semantics, xdg-shell configure/map,
    focus and input delivery (enter/motion/key with expected coordinates), frame callbacks only
    for visible windows, the clipboard stub, and every misbehaving case from the spec (SC-004):
    invalid requests, shrinking a pool's file (SIGBUS path), disconnect at any point, a client
    that never reads (buffer limit), a flooding client, never drawing, an oversized buffer
    (one pixel above the limit the renderer reports, 16384 on llvmpipe). A test also checks
    that a commit with new content is shown in the next frame (SC-002) and that frame callbacks
    follow the scale and visibility rules.
  - Rendering: pixel assertions on llvmpipe output - a solid-colour window at its expected
    rectangle at zoom 1 and 2, stacking order, alpha blending, `xrgb` alpha, a buffer transform,
    and a window of one-pixel white lines every fourth column that reads as even dark grey
    (about 64, within a tolerance) at 25% zoom (FR-005: mipmapped, no moire). A checkerboard at
    50% was rejected: plain bilinear filtering also averages it, so it cannot tell whether
    mipmaps are used (verified by disabling them).
  - Manual (quickstart): kitty with software rendering on niri - typing, focus, scrolling,
    selection, layouts, fractional scale, idle CPU, crash/kill of a client.
- **Rationale**: the in-process client/server pair needs no host and exercises the real protocol
  code, so only the host backend and real-device behaviour remain manual.

## R14. Dependency summary (to approve with this plan)

Runtime, new: `wayland-server` 0.31, `libc` 0.2; changed: `wayland-protocols` 0.32 gains the
`server` feature. Dev: none new (`wayland-client` is already a dependency).
