# Implementation Plan: GPU Buffers

**Branch**: `worktree-gpu-buffers` | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261104-004828-gpu-buffers/spec.md`

## Summary

Applications hand RubickWay GPU images through `zwp_linux_dmabuf_v1` version 5 with feedback, so they
stop falling back to software. Images are imported with EGL and sampled as external textures, with a
mipmap copy only when zoomed out. A pure commit queue holds a window's update until its image is
finished, known through a non-blocking poll or, with `wp_linux_drm_syncobj_v1`, a timeline eventfd;
the slow window keeps its last picture and nothing else waits. Buffers go back when no frame in
flight can read them.

RubickWay composes on the GPU driving the displays. The second GPU is never opened. Choosing and
switching the composing GPU are a later feature (research G13). Details:
[research.md](research.md) G1-G12.

## Technical Context

**Language/Version**: Rust 1.99, edition 2024

**Primary Dependencies**: no new crates (G12). `wayland-protocols` (`linux_dmabuf::zv1`,
`linux_drm_syncobj::v1`), `drm` 0.14 (syncobj), `khronos-egl`, `glow`, `libc`, `calloop`. System: Mesa
through glvnd.

**Storage**: none new; `config.kdl` gains the optional `distant-windows` block (FR-017)

**Testing**: `cargo test` (debug and release). Unit and `proptest`: commit queue, release tracker,
buffer validation, format table, the offer decision. Protocol tests in `tests/protocol/` with test
doubles for image files and timelines. Rendering tests on llvmpipe with external textures made from
ordinary textures: colours, alpha, mipmap copy, y-invert. No test needs a device (G11). Real imports,
two-plane video and other hardware behaviour manually through [quickstart.md](quickstart.md) Q1-Q12

**Target Platform**: Linux with DRM/KMS and logind; the author's laptop: Intel UHD 620 driving eDP and
HDMI, NVIDIA MX130 with no outputs, never opened; Arch, Mesa 26, kernel 7.2

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: kitty's first frame under 200 ms after it connects (SC-001); no extra dropped frames next to a
slow application (SC-003)

**Constraints**: never block the loop (G6); no device enumeration through EGL (G5); import failure
never ends an application (G8); `unsafe` only in `backend`, `render`, `ffi`

**Scale/Scope**: one GPU, a few displays, a handful of windows; roughly 2-3k lines including tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | No compositor framework. Hardware through drm and EGL, as the principle names. References named in research: weston, wlroots, Smithay, for reading only. |
| II. Client can't take it down | PASS | All image values in checked `u64` arithmetic before use (G3). A file is recognised as a dma-buf before anything touches it (G3). Import failure affects only that image (G8). No per-application limits on images, parameters, timelines or queued commits, as in other compositors: an accepted risk recorded in the spec's Assumptions (G10). A fuller file table fails one request, not RubickWay. 12 misbehaving cases each get a test, or a quickstart step where a real GPU is needed. |
| III. Test-first, headless | PASS | External drawing runs on llvmpipe by read-back, with no dma-buf and no device (G11). All decisions in pure code with tests first. Hardware layer thin: the import call and timelines are verified in the quickstart. |
| IV. Backend-independent core | PASS | `canvas` and `shell` do not change. `CommitQueue` and the release tracker are plain data. Render and server exchange plain descriptions, not DRM or GL types. |
| V. Contained unsafe | PASS | `unsafe` in `src/render/` (EGL and GL extension calls), `src/backend/drm/` (timelines), `src/ffi/dmabuf.rs` (`readlink`, `lseek`, `poll`, `memfd`), `src/ffi/spawn.rs` (rlimit in `pre_exec`), each with `// SAFETY:`. |
| VI. No speculative generality | PASS | dmabuf version 5, not 6; syncobj version 1. Two traits, `Timelines` and `ImageFiles`, under the principle's exception (G11). One block of settings, asked for by the owner after measuring (FR-017), off by default. Nothing prepared for a second composing GPU. |
| VII. Never block the loop | PASS | Work that can block, named: EGL start-up before the loop, as today; image readiness through polled fds (G6). `eglCreateImage` runs on the loop, in driver code: measured at under 0.1 ms (G8). Creating a copy's texture runs on the loop and is slow for a large window, so it happens once per size, not per image (G4). `readlink` on `/proc/self/fd` is an in-memory lookup, as in `describe_process`. |
| VIII. 2D core with plugins | PASS | Core only. |

Post-design re-check (after data-model, contracts and quickstart): all PASS.

## Project Structure

### Documentation (this feature)

```text
specs/20261104-004828-gpu-buffers/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (G1-G12; G13 for the later feature)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q12 manual checks)
|-- contracts/
|   |-- protocols.md     # zwp_linux_dmabuf_v1, wp_linux_drm_syncobj_v1, errors
|   |-- config.md        # no changes
|   `-- cli.md           # log lines
|-- checklists/
|   |-- requirements.md
|   `-- review.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- standalone.rs            # waits as event sources, release tracking, first-frame log line
|-- config.rs                # distant-windows block (FR-017)
|-- distant.rs               # NEW: what counts as distant, frame pace of distant windows, pure
|-- server/
|   |-- mod.rs               # GPU offer, take_waits / wait_done, process_imports, frame_drawn, release_images
|   |-- compositor.rs        # commit -> transactions; WlBuffer of either kind; held images
|   |-- commit_queue.rs      # NEW: CommitQueue, pure (G7)
|   |-- release.rs           # NEW: ReleaseTracker, pure (G9)
|   |-- dmabuf.rs            # NEW: global, params, feedback, validation; trait ImageFiles (G2, G3, G8)
|   |-- syncobj.rs           # NEW: manager, timeline, surface; trait Timelines (G6, G11)
|   |-- shm.rs               # WlBuffer dispatch shared with GPU images
|   `-- xdg_shell.rs         # geometry and limits taken from the transaction
|-- render/
|   |-- egl.rs               # formats, image import
|   |-- gles.rs              # external program, imported images, image sources, mipmap copy
|   |-- image.rs             # NEW: description of an image and its EGL attributes (G4)
|   |-- formats.rs           # NEW: format table from EGL, minimum planes, pure parts (G2)
|   `-- shaders/window_external.frag   # NEW
|-- backend/drm/
|   |-- gpu.rs               # render node of the picked card through sysfs; comment about card numbers
|   `-- timelines.rs         # NEW: Timelines on the display GPU's render node (G6)
`-- ffi/
    |-- dmabuf.rs            # NEW: is_dma_buf, size, poll
    |-- shm_map.rs           # sealed_memfd takes bytes too, for the format table
    `-- spawn.rs             # restores RLIMIT_NOFILE

tests/
|-- render_images.rs         # NEW: external textures on llvmpipe, pixel facts
`-- protocol/
    |-- harness.rs           # + ImageFiles and Timelines doubles
    |-- dmabuf.rs            # NEW: lifecycle, feedback, every validation error
    |-- syncobj.rs           # NEW: every error, ordering through held acquire points
    |-- commits.rs           # NEW: queueing with subsurfaces, last picture, releases
    `-- misbehaving.rs       # + cases 1-12
```

**Structure Decision**: same single crate and module direction. `render` stays free of DRM and Wayland
types.

### Order of work (by user story)

Each step leaves a working desktop.

1. **US3 first, the foundation**: `wl_buffer` of either kind, `render` image sources, shared-memory
   behaviour unchanged, the commit path as transactions with no waits. Refactors with today's tests
   staying green.
2. **US1** (P1): dmabuf global, validation, feedback, import (a commit waits for its import, G7), external
   drawing, mipmap copy, release tracker. Implicit readiness is treated as ready for the moment. Hardware
   check: kitty on Intel.
3. **US2** (P2): commit queue with real waits, implicit polling, syncobj protocol, timelines.

### Supporting changes

- `README.md`: GPU rendering; that applications no longer fall back to software; remove the "shared
  memory only" status and the note that Mesa draws GPU applications with its software renderer.
- Comment in `src/backend/drm/gpu.rs` about card numbers: they vary between boots.

## Complexity Tracking

No constitution violations and nothing to justify.
