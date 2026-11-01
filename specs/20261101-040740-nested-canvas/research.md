# Research: Nested Canvas Viewer

Decisions made while planning `nested-canvas`. Each entry: decision, rationale, alternatives.
Crate versions are the latest on crates.io on 2026-11-01.

## R1. Talking to the host compositor

- **Decision**: Use `wayland-client` 0.31 directly, with `wayland-backend` 0.3 (feature
  `client_system`, i.e. libwayland-client underneath) and `wayland-protocols` 0.32 (features
  `client`, `staging`, `unstable`). No windowing toolkit.
- **Rationale**: Principle I (own implementation) and the learning goal. Talking raw protocol
  gives direct access to everything the spec needs - fractional scale, pointer gestures, axis
  sources, cursor shapes - and the connection fd plugs into our own event loop (R3).
  `client_system` is required because EGL needs libwayland-client's `wl_display` pointer (R4).
- **Alternatives**: `winit` - owns its event loop, which fights the single `calloop` loop the
  compositor will need once it also serves clients; hides axis sources and gestures.
  `smithay-client-toolkit` - convenient, but it is a Smithay-project framework layer; writing
  the ~10 objects we need by hand is small and is the point of the project.

## R2. Host protocols bound (client side)

| Global | Version bound | Required? | Used for |
| --- | --- | --- | --- |
| `wl_compositor` | 4..=6 | yes | surfaces; v4 `damage_buffer`; v6 `preferred_buffer_scale` fallback |
| `xdg_wm_base` | 1..=5 | yes | the toplevel window, ping/pong, close |
| `wl_seat` | 5..=8 | no (warn) | pointer + keyboard; v5 axis source/frame, v8 `axis_value120` |
| `wp_viewporter` | 1 | no | needed together with fractional scale |
| `wp_fractional_scale_manager_v1` | 1 | no | fractional scale (FR-017) |
| `zxdg_decoration_manager_v1` | 1 | no | ask the host for server-side decorations |
| `zwp_pointer_gestures_v1` | 1 | no | pinch zoom (FR-008) |
| `wp_cursor_shape_manager_v1` | 1 | no | default / grabbing cursor |

- **Decision**: bind each at the highest version in the range the host offers; never above
  what we handle (Principle VI). Missing required globals -> FR-013 error. Missing optional
  ones degrade as follows: no fractional scale -> integer `preferred_buffer_scale` (or 1);
  no gestures -> pinch unavailable (Mod+scroll still zooms); no cursor-shape -> cursor left
  to the host; no decoration manager -> no decorations; no seat -> viewer works without input.
- **Alternatives**: requiring everything (fails on hosts like GNOME without some globals);
  `wayland-cursor` + xcursor themes as cursor fallback (extra dependency for little value).

## R3. Event loop

- **Decision**: `calloop` 0.14 with the `signals` feature. The host connection is a
  `calloop::generic::Generic` source on the connection fd, following wayland-client's
  integration pattern: before sleeping, dispatch pending events and handle them, draw if
  needed, flush, then take a read guard (`prepare_read`); if that fails, events arrived
  meanwhile (for example read by EGL during `eglSwapBuffers`), so handle them and try again.
  The guard is held while the loop sleeps; on readable, `guard.read()` -> `dispatch_pending`;
  when something else woke the loop the guard is dropped. The guard is never held during an EGL
  call: EGL may read the socket itself, and libwayland would make that read wait for ours on
  the same thread (deadlock). SIGINT and SIGTERM exit cleanly through a signal source.
- **Rationale**: Principle VII - one loop, nothing blocks it. The next feature adds the
  compositor's own client socket to the same loop. A hand-written source avoids the extra
  `calloop-wayland-source` crate and is ~40 lines.
- **Alternatives**: blocking `blocking_dispatch` (cannot add the server socket later, cannot
  handle signals); a thread per source (violates the single-loop design).

## R4. Rendering stack

- **Decision**: EGL + OpenGL ES 3.0 through `khronos-egl` 6 (feature `static`, links libEGL)
  and `glow` 0.18; `wayland-egl` 0.32 for the window surface. `eglSwapInterval(0)`; frames are
  paced by our own `wl_surface.frame` callbacks.
- **Rationale**: GLES 3 is available on every Mesa and NVIDIA driver, and the later client
  buffer import (dmabuf via `EGL_EXT_image_dma_buf_import`) uses the same stack. Swap interval
  0 matters for Principle VII: with interval 1, Mesa's `eglSwapBuffers` blocks waiting for the
  host's frame callback, stalling the loop.
- **Known limit**: when none of its buffers is free, Mesa with swap interval 0 polls the host
  inside `eglSwapBuffers` (sync requests every 10 ms) until one is released, briefly blocking
  the loop. Pacing by frame callbacks keeps this rare. Removing it needs our own buffer
  management (GBM + dmabuf instead of `wl_egl_window`), which belongs with the DRM backend.
- **Buffer size**: the renderer reads the GPU's largest drawable side
  (`min(GL_MAX_RENDERBUFFER_SIZE, GL_MAX_VIEWPORT_DIMS)`); a window larger than that gets a
  uniformly smaller buffer (one effective scale for both axes) that the host scales up. The
  logical size is never limited, and buffer sides are kept within `i32` for the protocol.
- **Alternatives**: `wgpu` (large, hides the EGL/dmabuf path the compositor needs); Vulkan
  directly (much more code before the first pixel); CPU drawing into `wl_shm` (fine for a grid,
  but throwaway once client textures arrive).

## R5. Grid rendering and precision far from the origin

- **Decision**: one full-screen triangle; the fragment shader draws grid lines from uniforms.
  All uniforms are in physical pixels and small: the CPU computes, in `f64`, the screen
  position of a super-major grid line (`offset = (screen origin of canvas 0,0) mod
  (16 * minor_spacing_px)`, see R6) and the spacings in pixels. The shader never sees canvas
  coordinates.
  Lines are 1 physical pixel wide with 1-pixel anti-aliasing computed from the pixel distance
  to the nearest line.
- **Rationale**: FR-016 (no jitter at 1e6 units). `f32` in the shader has ~7 significant
  digits; canvas coordinates at 1e6 with sub-pixel zoom would visibly jitter. Reducing modulo
  the 16-line period on the CPU in `f64` keeps every shader value below a few thousand pixels.
- **Alternatives**: vertex buffers of line geometry rebuilt per frame (more CPU, same precision
  problem unless also reduced); `fwidth`-based procedural grid in canvas space (the jitter
  problem).

## R6. Adaptive grid levels (FR-004)

- **Decision**: minor spacing = the smallest `8 * 4^k` canvas units (integer `k`) whose
  on-screen size is at least 12 logical pixels; major lines every 4 minor lines and
  super-major lines every 16. Minor lines fade in between 12 and 24 logical pixels of spacing
  while major lines blend from minor to major opacity, so at a level change each tier takes
  over the look of the one above it and nothing pops; the screen offset is reduced modulo the
  16-line period. The two
  origin axes are drawn brighter. Colours: background `#1d1f24`, lines white at alpha 0.05
  (minor), 0.10 (major), 0.22 (axes).
- **Rationale**: base 4 keeps the major grid of one level equal to the minor grid of the next,
  so zooming is continuous. Across the 10%-1000% range `k` runs from -1 to 2.
- **Alternatives**: powers of 10 (jumps of 10x are visually abrupt); fixed spacing (fails
  FR-004).

## R7. Keyboard and shortcuts with non-Latin layouts

- **Decision**: `xkbcommon` 0.9. The keymap fd from `wl_keyboard.keymap` is first checked
  against its real length (`fstat`): a `size` larger than the file is an invalid keymap and is
  ignored, so nothing is allocated on the strength of the message alone. It is then read with a
  positional read (`FileExt::read_exact_at`) into a string and compiled with
  `Keymap::new_from_string`. Shortcuts match the keysym of the pressed key in **layout 0 at
  shift level 0** plus the exact set of active Ctrl/Alt/Shift/Super modifiers (locked
  modifiers such as Caps Lock and Num Lock ignored). Key repeat is not implemented: only press
  events trigger actions.
- **Rationale**: with a `us,ua` layout set and the Ukrainian layout active, the active keysym
  for Q is Cyrillic, so matching active keysyms would break Alt+Shift+Q. Layout 0 base level
  keeps shortcuts on the same physical keys. Positional read does not move the file offset;
  some compositors share one keymap fd between clients, so a plain `read` would corrupt other
  clients' reads. Reading a memory-backed keymap fd is a memory copy, not blocking I/O
  (Principle VII). `new_from_string` keeps `unsafe` (`new_from_fd`) out of the input module
  (Principle V).
- **Alternatives**: active keysyms (breaks with non-Latin layouts); raw keycodes (breaks with
  Dvorak/Colemak users' expectations); `mmap` of the fd (needs `unsafe` and gains nothing for a
  ~60 KiB keymap).

## R8. Camera model and input math

- **Decision**: the camera stores the canvas point at the viewer centre (`f64` x, y) and the
  zoom (`f64`, clamped to [0.1, 10]). Screen <-> canvas: `canvas = center + (p - size/2) /
  zoom`. Drag pans by anchoring: at drag start remember the canvas point under the pointer; on
  each motion set `center = anchor - (p - size/2) / zoom`. Zoom about a point: compute the
  canvas point under the pointer, change zoom, then solve for the centre that puts it back.
  Wheel: zoom by `1.1^(value120 / 120)` per event (one notch = one 10% step; hi-res wheels give
  fractions). Mod + finger or continuous scroll: one step per 50 logical pixels of scroll. Pinch: `zoom =
  zoom_at_begin * scale`. Finger and continuous (trackpoint) scroll without Mod pan by the
  scrolled distance, as delivered by the host, like a scrolled document; the host has already
  applied the user's natural-scrolling setting, so we do not need `wl_seat` v9's
  `axis_relative_direction`. Horizontal wheel notches pan 50 logical pixels each.
- **Rationale**: anchoring makes SC-002 exact (no accumulated drift); a centre-based camera
  makes FR-012 (resize keeps the centre) a no-op.
- **Alternatives**: top-left-based camera (resize needs compensation); incremental pan deltas
  (drift over long drags).

## R9. Headless testing (Principle III)

- **Decision**:
  - Camera, grid levels, bindings and the gesture state machine: unit tests; pan/zoom
    invariants (SC-002, SC-003, SC-007) also as `proptest` 1.12 property tests.
  - Viewer behaviour end to end: integration tests that feed backend-neutral `InputEvent`
    sequences into the viewer and assert the camera, the dirty flag and quit effects.
  - Keyboard: tests compile real keymaps from RMLVO names (`us`, `us,ua`) with xkbcommon - no
    host needed.
  - Rendering: pixel assertions, no stored images. An EGL display on Mesa's software
    rasteriser (llvmpipe), found via `EGL_EXT_device_enumeration` by picking the device that
    advertises `EGL_MESA_device_software` (the `renders_on_llvmpipe` test fails if another
    renderer is picked), renders scenes into an offscreen framebuffer; the pixels are read back
    and checked against facts computed from the inputs, each channel within 2: background
    colour between lines, the axes at the expected column/row and one pixel wide, canvas y
    pointing down, every pixel of a row at 100%, 1000% and scale 1.5 (line columns with their
    tier colour, the rest background), no solid fill and major lines at multiples of their
    spacing at 10%, and a view one million units away identical to the equivalent view near
    the origin. Failures are reported as text (position, got, expected).
  - The host backend (`backend::wayland_host`) is the thin layer verified manually through
    `quickstart.md`; it only translates host events into `InputEvent`s and window events.
- **Rationale**: picking the software device explicitly makes the rendered pixels the same
  on the author's hybrid Intel/NVIDIA laptop and on CI, independent of which GPU vendor glvnd
  would choose. Assertions on pixel facts survive small llvmpipe differences between Mesa
  versions and need no images in the repository (constitution III); they check only what is
  asserted, which suits a picture defined by formulas like the grid.
- **Alternatives**: reference images compared within a tolerance (Weston, Mutter) - rejected
  by the owner: no binary images in the repository; no automated render tests (niri,
  sway/wlroots) - weaker than constitution III.
- **Confirmed** with `eglinfo` on the author's machine (2026-11-01): the client extensions
  include `EGL_EXT_device_enumeration` and `EGL_EXT_platform_device`, and the device platform
  lists three devices - NVIDIA, Mesa iris (Intel) and a Mesa device with
  `EGL_MESA_device_software` (driver `swrast`, renderer llvmpipe). The surfaceless platform,
  by contrast, resolves to the NVIDIA driver here, so it is not a usable default; it stays only
  as a fallback for environments without device enumeration, with Mesa forced to software
  rendering for the test process.
- **CI**: the runner needs `libwayland-dev`, `libxkbcommon-dev`, `libegl-dev`, `libgles-dev`,
  `libegl-mesa0` and `xkb-data` installed before the full check.

## R10. Logging and errors

- **Decision**: `tracing` 0.1 + `tracing-subscriber` 0.3 (features `env-filter`, `fmt`),
  filtered by `RUST_LOG` (default `rubickway=info`), written to stderr. Module errors use
  `thiserror` 2; `main` prints the error chain and returns a failure exit code. No `anyhow`.
- **Rationale**: the user needs clear messages for FR-013/FR-014; debug logs are essential for
  protocol work. Logging to stderr at info level is a handful of lines per run; at debug level
  the write cost is accepted as a debugging mode (Principle VII note).
- **Alternatives**: `eprintln!` only (no levels or filtering); `anyhow` (not needed with one
  error type per module and `main` reporting).

## R11. Dependency summary (to approve with this plan)

Runtime: `wayland-client` 0.31, `wayland-backend` 0.3, `wayland-protocols` 0.32,
`wayland-egl` 0.32, `khronos-egl` 6, `glow` 0.18, `xkbcommon` 0.9, `calloop` 0.14, `tracing`
0.1, `tracing-subscriber` 0.3, `thiserror` 2.
Dev: `proptest` 1.
System libraries: libwayland-client, libwayland-egl, libEGL, libGLESv2, libxkbcommon.
