# Research: GPU Buffers

Builds on client-windows (R1-R14), window-management (W1-W14) and drm-backend (S1-S23). Decisions
here are numbered G1-G12; G13 keeps what planning found for the later feature. References, reading
only (Principle I): weston `libweston/linux-dmabuf.c` (buffer validation), wlroots
`types/wlr_linux_dmabuf_v1.c` and `types/wlr_linux_drm_syncobj_v1.c`, Smithay `wayland/dmabuf`,
`wayland/drm_syncobj` and its transaction blockers (`wayland/compositor`).

Facts marked **measured** come from throwaway C probes run on the target laptop on 2026-11-04
(Intel UHD 620 with Mesa 26.2.6, NVIDIA MX130 with driver 580.196.07, kernel 7.2). The probes opened
the render nodes directly and were not kept: the numbers are recorded here, and the quickstart
checks them again through RubickWay itself.

## G1. How programs find the GPU

- **Finding**: dmabuf feedback tells a client which GPU the compositor reads with. Mesa and NVIDIA's
  client libraries use it when a program connects (egl-wayland since 1.1.14). With it, Mesa stops
  failing native DRI, skips the zink detour and renders on the Intel GPU.
- **Decision**: RubickWay announces the display GPU and never places programs. No environment
  variables are set for spawned programs.

## G2. Protocol version and format table

- **Decision**: `zwp_linux_dmabuf_v1` version 5. Version 4 adds feedback, which Mesa and egl-wayland
  need; version 5 makes the `invalid_format` error mandatory. Version 6 replaces `main_device` with a
  sampling flag: its value is for a compositor that changes GPU (G13), so it waits (Principle VI).
  Clients bound below 4 get `format` and `modifier` events at bind and no feedback.
- **Decision**: the table comes from `eglQueryDmaBufFormatsEXT` and `eglQueryDmaBufModifiersEXT`.
  External-only pairs are included, because all imports are sampled as external (G4). A format
  reporting no modifiers gets one `DRM_FORMAT_MOD_INVALID` entry; otherwise implicit layouts are not
  offered, because they do not work across GPUs (FR-011). The table is one sealed memfd, sent to every
  feedback object as a read-only duplicate. **Measured**: 65 formats on Intel, under 500 pairs: a few
  KB.
- **Decision**: `main_device` and `tranche_target_device` are the `st_rdev` of the display GPU's render
  node, found through sysfs from the card `gpu.rs` already picked. One tranche, flags 0 (no direct
  scanout, spec Assumptions). Surface feedback has the same content as default feedback, and is sent
  once: the offer does not change while RubickWay runs.

## G3. Validation (Principle II)

- **Decision**, following weston: on `add`: plane below 4 (`plane_idx`), not set (`plane_set`), the
  same modifier as earlier planes (`invalid_format`). On `create` and `create_immed`: once only
  (`already_used`), planes contiguous from 0 and at least the format's minimum (`incomplete`), width
  and height positive (`invalid_dimensions`), offset and stride arithmetic in `u64` (`out_of_bounds`):
  `offset + stride * height` must fit for plane 0 only, because other planes may be subsampled;
  `offset` must lie inside the buffer for all. The rest of a subsampled plane's bounds is the driver's
  to check when it imports: EGL refuses, and only that image fails (spec case 3). Flags other than
  `y_invert` fail the image. The pair must be in the offered table (`invalid_format`).
- **Decision**: a dma-buf is recognised by `readlink("/proc/self/fd/N")` starting with `/dmabuf:`.
  **Measured**: a real dma-buf gives `/dmabuf:`, a memfd `/memfd:...`. This runs before any operation on
  the file, because `lseek(SEEK_END)` on a client-supplied FUSE file could block on its daemon. Anything
  else fails that image. Size comes from `lseek` afterwards.
- **Decision**: plane counts that depend on the modifier (Intel's auxiliary compression planes) are
  left to the driver: EGL refuses, and only that image fails.

## G4. Sampling: always external, with a mipmap copy when zoomed out

- **Decision**: imported images are always bound to `GL_TEXTURE_EXTERNAL_OES`, in a second program
  (`window_external.frag`). This handles multi-plane video and external-only layouts with one path, and
  needs no swizzle: the driver returns correct channels and alpha 1 for formats without alpha.
  Shared-memory windows keep today's path.
- **Finding**: the canvas zooms, and shared-memory windows use mipmaps so zooming out does not alias
  (`tests/render_windows.rs`). External textures cannot have mipmaps, and FR-004 requires equal
  behaviour.
- **Decision**: when a quad draws an image at less than its pixel size, the renderer draws the image
  into an owned RGBA8 texture and generates mipmaps, once per new image, before the frame; that quad
  samples the copy. At 100% or more, the external texture is sampled directly with no copy. The copy
  goes with the surface's next image, not when zooming back in: a window shown on two displays at
  different zooms would otherwise make a copy every frame.
- **Alternatives rejected**: generating mipmaps on the imported texture (undefined for EGLImages);
  aliasing at low zoom (violates FR-004).
- **Finding, quickstart of 2026-11-04**: zooming out with a 7680x4320 window on the UHD 620, already
  busy with that program, took another window from 43 to 3-5 frames a second, at the same moment the
  copy starts being made for each new image. Cause not measured apart from the load itself.
- **Decision (owner)**: distant windows are configurable, off by default (FR-017,
  `contracts/config.md`). Frame callbacks are throttled, which spares the application's own GPU work
  and so also the copies; and the copy can be smaller, its size being a pure function of the image, the
  drawn size and the parameter. The copy is one bilinear pass, so far below the image's size it may
  shimmer: a choice the owner makes with the parameter, and default behaviour stays unchanged.
- **Measured, 2026-11-05 (UHD 620, 7680x4320 image)**: the cause was found in RubickWay, not in the
  load. Each new image cost the event loop a median of 220 ms, up to 440 ms, 98% of it on the CPU:
  103 ms creating the copy's texture and 90 ms generating mipmaps, against 0-1 ms for drawing and
  showing the frame. libinput reported input read up to 247 ms late at those moments (Principle VII).
- **Decision**: a copy's texture outlives its image. When the next image comes, it stays as a spare
  for that image's copy, and one nobody takes is freed within 8 calls of `prepare_frame`, so memory
  is not held for a window drawn at full size again. A texture is created with all its mipmap levels
  (`glTexStorage2D`), as generating them into one without makes the driver allocate it again. The
  first copy of a size still costs the loop once.
- **Measured after that decision, same load**: over 61 s with no `distant-windows`, one copy cost the
  loop 138 ms, when zooming out started; no other frame work reached 10 ms, and mipmaps no longer
  appear in the timing. libinput reported nothing late (it had reported up to 247 ms). The desktop went
  from 3-4 frames a second to about 40 at 24% zoom and about 37 at 69%. The owner: input lag is still
  there but very slight, much better.

## G5. Keeping the second GPU out

- **Measured**: composing on Intel, the process opens no `/dev/nvidia*` and not the NVIDIA render node;
  only the library `libEGL_nvidia` gets mapped, by glvnd. So FR-012 and SC-008 hold if RubickWay avoids
  device enumeration.
- **Measured**: `eglQueryDevicesEXT` opens `/dev/nvidia*`. It stays reserved for
  `Egl::software_device`, which only tests use. The render node is found through sysfs (G2).
- **Note**: card numbers vary between boots: today `card0` is NVIDIA, which differs from the comments
  in `gpu.rs`; those comments get corrected.

## G6. Knowing an image is finished without blocking

- **Implicit**: `poll` with `POLLIN` on each plane's dma-buf reports that writers finished. At commit
  a zero-timeout `poll` tells most cases at once; otherwise a duplicate of the fd goes to the loop as
  a one-shot source.
- **Explicit**: the timeline is imported on the display GPU's render node. **Measured**: a timeline made
  on NVIDIA imports, waits and signals on the Intel node, so a `prime-run` program can use it (FR-011).
  `DRM_IOCTL_SYNCOBJ_EVENTFD` (flags 0: signalled, not merely available) gives an eventfd for the
  acquire point; it also fires for a point already reached. **Measured**: `DRM_CAP_SYNCOBJ_TIMELINE` is 1
  on Intel.
- **Decision**: both kinds become the same thing for the server: a `Wait { id, fd }`. `Server` hands
  waits to `App`, which inserts one-shot calloop sources and calls `Server::wait_done(id)`. The server
  stays free of the event loop and tests drive waits by hand.
- **Decision**: the global is offered only if the GPU reports timeline support.

## G7. Commit queue

- **Finding**: today `commit` applies at once. Waiting must keep order per surface (FR-008) and keep
  synchronised subsurfaces with their parent (Smithay's blockers solve the same problem).
- **Decision**: a pure `CommitQueue<T>` keyed by the commit root: the nearest desynchronised
  subsurface or the toplevel root. A transaction holds the committed state of that surface and the
  cached state of its synchronised descendants. It is applied when its waits are done and all
  transactions before it in the queue have been applied. A synchronised subsurface's commit is cached
  as today and never waits by itself. With nothing to wait for and an empty queue, the transaction
  applies immediately, so shared-memory behaviour and timing do not change.
- **Decision**: `xdg_surface` geometry and size limits are captured in the transaction, not read at
  apply time: today `tree_committed` reads `pending_geometry`, which a later request could already
  have changed.
- **Decision**: the queue has no length limit (G10). A real application waits for frame callbacks and
  stays at 1-3.
- **Finding**: Mesa sends `create_immed`, `attach` and `commit` together in one `eglSwapBuffers`
  (`platform_wayland.c`), so a commit is handled before its image can be imported: importing needs the GL
  context, which `App` has, after the requests are read. wlroots and Smithay import inside the request,
  because their renderer is at hand.
- **Decision**: an import not done yet is one more thing a transaction waits for, with no fd. Right after
  the requests are read, `process_imports` imports and settles those waits, then transactions apply and
  the frame is drawn, all in one pass of the loop, so no frame is delayed. So a failed import has changed
  nothing when it is found (G8). The server stays free of GL.
- **Protocol errors found by applying** (size limit, viewport) are posted when the transaction
  applies, not at commit. The client is ended either way.
- **While paused**: waits keep finishing and transactions keep applying, as commits do today; nothing
  is drawn until the resume, when every display redraws. Application frames are not held back by a
  pause beyond the frame callbacks drm-backend already holds.

## G8. Import failures never end an application

- **Protocol**: "the compositor must not raise protocol errors to the client when dmabuf import later
  fails" for a buffer already created; `create_immed` may send `failed` instead of ending the client.
- **Measured**: importing is cheap enough for the loop: `eglCreateImage` plus binding took 0.02-0.04 ms
  for an idle LINEAR buffer at 1080p and 4K on Intel, at worst 0.07 ms over 100 (other layouts and a busy
  buffer not measured).
- **Decision**: `create` and `create_immed` both answer `failed`. Committing a failed `wl_buffer` keeps
  the last picture and logs once per application (FR-015). The buffer goes back as usual so the
  application does not run out (FR-005): `wl_buffer.release`, or its release point on a window using
  explicit synchronisation. Its frame callbacks complete.
- **Too large is different**: an image above the GPU's size limit found on commit is the existing
  `wl_surface.invalid_size` rule (spec case 6), not an import failure.

## G9. Releasing buffers

- **Finding**: shared memory is copied and released at once. GPU images are read during frames, and a
  frame may be in flight on any output.
- **Decision**: when a surface's image is replaced it is retired with the current frame sequence
  number; each output remembers the sequence of its frame awaiting a flip. It is released when no
  in-flight frame has a sequence at or below that number: the GPU finished reading before the flip
  (KMS waits for implicit fences). While paused nothing is in flight, so retired images go back at once.
  Buffers with a release point get `drmSyncobjTimelineSignal` on the CPU, with no `wl_buffer.release`,
  whose delivery the protocol leaves undefined for those surfaces. Others get `wl_buffer.release`. Pure
  logic, unit-tested.
- **Alternatives rejected**: a native fence per frame transferred to the release point: unblocks the
  client slightly earlier and adds a fence path for no visible gain (Principle VI).

## G10. File descriptors and limits

- **Finding**: plane fds stay open for the life of a `wl_buffer` (readiness polling). The default soft
  limit could run out.
- **Decision**: raise the soft `RLIMIT_NOFILE` to the hard limit at startup (524288 on the laptop), and
  restore the original in `ffi/spawn.rs`'s `pre_exec`, so programs do not inherit it, as sway does. An
  import that fails with `EMFILE` affects only that image.
- **Decision (owner, 2026-11-04)**: no limits per application on GPU-image buffers, parameter objects,
  timelines or waiting commits. wlroots, weston and Smithay have none on buffers and parameter objects
  (their dmabuf sources; timelines and queues there, and Mutter and KWin, not checked). Rejected: 256
  buffers with disconnection, and 1024 answering `failed`. To be looked at again later (spec Assumptions).
- **Finding, what running out of descriptors does** (read in `wayland-backend` 0.3.17 and libwayland, not
  run): RubickWay does not crash. `recvmsg` drops descriptors the table has no room for, and the backend does
  not check for that, so any client sending one at that moment stalls on a message missing its descriptor; a
  later descriptor from that client may land on the wrong request. Accepting a new client fails with a log
  line and retries on every pass of the level-triggered loop, as libwayland does (`failed to accept`), which
  spins until a descriptor is free. Accepted as part of the same risk, with 524288 descriptors to use up
  first.
- **Decision (owner, 2026-11-04)**: it is logged, in plain words, once when it starts and once when it ends,
  naming the application holding the most GPU images (FR-015): a pure tracker of "exhausted or not", fed by
  `EMFILE` or `ENFILE` from `accept` and from getting a file to wait for an image on. An EGL import cannot
  feed it: it takes no descriptor of ours and reports only an EGL error. It ends when a pass of the loop can
  open a descriptor, so the end is logged even if nothing else happens to need one. A client stalled on a
  lost descriptor cannot be detected.

## G11. Testing headless

- **Decision (owner, 2026-11-04)**: no test depends on `/dev/udmabuf` or any other device: no kernel
  module to load, no CI change. Real dma-buf imports are checked through the quickstart.
- **Measured**: llvmpipe samples `GL_TEXTURE_EXTERNAL_OES` from an EGLImage made of an ordinary GL
  texture (`EGL_KHR_gl_texture_2D_image`), with correct colours and alpha, with no dma-buf.
- **Decision**: rendering tests build their external textures that way, so `window_external.frag`,
  alpha, y-invert, transforms and the mipmap copy run on llvmpipe by read-back (Principle III). What
  this does not reach, the import call and multi-plane formats, stays thin and is in the quickstart.
- **Decision**: protocol tests need no real dma-buf either: how a file is recognised and sized is
  behind `trait ImageFiles`, with `ffi/dmabuf.rs` as the real one and a double in the harness that
  accepts a memfd (Principle VI's exception: the real one needs a device for its positive case).
  Rejecting a memfd and an ordinary file is a unit test of the real one.
- **Decision**: timelines need a DRM device, so `trait Timelines` has the real implementation in
  `backend/drm/timelines.rs` and a test double in the harness (Principle VI's exception: the real one
  cannot run headless). The double makes acquire points controllable, which also tests ordering.
- **Decision**: implicit waits need a real unfinished image, so they are covered by `CommitQueue`'s
  unit tests and proptests, and by the quickstart.
- **Decision**: whether anything is offered is a pure function of what the renderer reports, tested
  with "no import" as input (FR-014, SC-009): llvmpipe itself reports import, so it cannot stand in for
  a renderer without it.

## G12. Dependencies

- **Decision**: no new crates. `wayland-protocols` already has `linux_dmabuf::zv1` and
  `linux_drm_syncobj::v1` (feature `staging`, on); `drm` 0.14 has syncobj calls including
  `syncobj_eventfd`; `khronos-egl` covers the rest, with extension entry points through
  `eglGetProcAddress`.

## G13. For the later feature: choosing and switching the composing GPU

Out of this feature by the owner's decision (spec Clarifications). Recorded so nothing is measured
twice. All from 2026-11-04 on the target laptop unless marked as read from source.

- **Why anyone would compose on NVIDIA**: only so that programs follow it there. RubickWay itself draws
  plain quads.
- **Measured, why it needs a copy**: LINEAR is `external-only` for every format on the NVIDIA driver: an
  imported linear buffer can be sampled but not rendered to (`GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT`).
  GBM on NVIDIA allocates LINEAR only without `GBM_BO_USE_RENDERING`. Intel cannot import NVIDIA's tiled
  layout (`EGL_BAD_MATCH`), and NVIDIA cannot import Intel's. So with EGL and GL, NVIDIA cannot draw into
  anything Intel can show, and each frame goes through main memory: renderbuffer and pixel-pack buffer on
  NVIDIA, a native fence polled by the loop, rows copied into one of three dumb buffers on the Intel card
  (`XB24` works), atomic commit. A `TEST_ONLY` commit tells whether it can be shown. Vulkan on NVIDIA can
  render to LINEAR dma-bufs, at the price of a second graphics API.
- **Measured, what the copy costs** (GPU readback over PCIe x4, about 1.7 GB/s, plus about 1.1 ms of CPU
  copy per 1080p frame on the loop):

  | Frame | ms per frame | Limit |
  | --- | --- | --- |
  | 1920x1080 | 5.3 | 190 fps |
  | two 1920x1080 | 10.0 | 101 fps |
  | 2560x1440 | 8.8 | 113 fps |
  | 3840x2160 | 19.8 | 50 fps |
  | 10% of 1920x1080 | 0.7 | - |
  | 10% of 3840x2160 | 2.3 | - |

  Two 1080p monitors fit 60 Hz; 4K at 60 Hz does not. The idea then: below the refresh the rate falls, with
  no tearing and no queue, newest frame wins, one log line. Reading back only damaged rows scales down well
  but does not help while panning.
- **Measured, starting and leaving NVIDIA**: about 110 ms the first time (35 ms context, 74 ms shaders),
  about 35 ms later. After `eglTerminate`, `eglReleaseThread` and closing the render node, the driver keeps
  14 fds (`/dev/nvidia0`, `nvidiactl`, `nvidia-modeset`, the render node) and 64 MB until exit; after 21
  cycles still 14 fds and 65 MB. So no leak, but it cannot be released in-process.
- **Measured, power**: the MX130 has no runtime D3 (`Runtime D3 status: Disabled by default`, `Video Memory
  Off: Not Supported`, sysfs `runtime_status` active). Holding the driver's fds after teardown it sits in
  P8 at 135 MHz, as before. Watts unknown: the laptop was charging. Per NVIDIA's README, runtime D3 needs
  Turing or newer; by default it is on (fine-grained) only on Ampere or newer notebooks, off on desktops.
  Whether open device files keep an RTX notebook awake is not stated and not checked.
- **Read from source, running programs after a switch**: egl-wayland 1.1.24 (installed) re-evaluates PRIME
  offload from per-surface feedback, but only when bound at `zwp_linux_dmabuf_v1` version 6, through
  sampling tranches (`wlEglUpdateSurfacePrimeState`): a program on NVIDIA then blits to LINEAR for an Intel
  compositor. So that feature should offer version 6: `main_device` for clients below 6, the sampling flag
  for others, and `set_sampling_device`. Mesa 26.2.6 binds at version 4, reallocates on new surface feedback
  and picks modifiers from the new table, which leaves LINEAR on NVIDIA. Both are unverified at run time.
- **The design worked out**: a pure switch state machine; preparing the new GPU on a worker thread with its
  context current nowhere; a snapshot of every window into main memory, because shared-memory content exists
  only as a texture on the old GPU; a cutover in one loop step; new feedback for all clients; `invalid_format`
  checked against every table offered since startup, so a client that has not read the new feedback is not
  ended. Choosing by vendor name (`render-gpu "nvidia"`), which survives card renumbering. Open points that
  review raised: a timeout for a stuck worker, quitting during a switch, a console switch during a switch, a
  GPU that cannot import.

Sources:

- [wayland-protocols: linux-dmabuf feedback](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/8a5cd28a0e84911a5339855cb32ed63fb57ea40a/unstable/linux-dmabuf/feedback.rst)
- [Mesa: egl/wayland: add initial dma-buf feedback support](https://lists.freedesktop.org/archives/mesa-commit/2021-November/126660.html)
- [NVIDIA's EGL-Wayland library v1.1.14 adds explicit sync](https://www.phoronix.com/news/NVIDIA-EGL-Wayland-1.1.14)
- [libnvidia-egl-wayland changelog](https://build.opensuse.org/projects/home:bmwiedemann:reproducible:distribution:ring1/packages/libnvidia-egl-wayland/files/libnvidia-egl-wayland.changes?expand=0)
- [egl-wayland: history of `wayland-eglsurface.c`](https://github.com/NVIDIA/egl-wayland/commits/master/src/wayland-eglsurface.c)
- [NVIDIA README: Dynamic Power Management](https://download.nvidia.com/XFree86/Linux-x86_64/580.95.05/README/dynamicpowermanagement.html)
