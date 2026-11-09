---
description: "Task list for GPU Buffers"
---

# Tasks: GPU Buffers

**Input**: Design documents from `specs/20261104-004828-gpu-buffers/`

**Prerequisites**: plan.md, spec.md, research.md (G1-G12), data-model.md, contracts/, quickstart.md

**Tests**: Required by Principle III: each test task is written and seen failing before its
implementation task. No test may depend on `/dev/udmabuf` or any other device (research G11).

**Organization**: Tasks are grouped by user story. As the plan's order of work says, the shared-memory
path is refactored first (Phase 2), so each step leaves a working desktop.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup

- [X] T001 Run `cargo fetch` in the worktree and the full check from `AGENTS.md` to confirm a green baseline; no new crates are needed (research G12)
- [X] T002 [P] Correct the comment about card numbers in `src/backend/drm/gpu.rs`: they vary between boots (research G5)

---

## Phase 2: Foundational (blocks all stories, no behaviour change)

**Purpose**: `wl_buffer` of either kind, renderer image sources, and the commit path as transactions with no
waits. Every existing test stays green throughout.

- [X] T003 [P] Write failing unit tests and proptests in `src/server/commit_queue.rs` for `CommitQueue<T>` (data-model "CommitQueue"): `push(T, waits)` with no waits on an empty queue returns it at once; `done(wait)` returns every transaction now at the front with no waits, in order; a later transaction with finished waits never overtakes an earlier one; `remove_surface` drops a destroyed surface's parts; `clear` when its client goes; an unknown wait is ignored
- [X] T004 Implement `CommitQueue<T>` as pure code in `src/server/commit_queue.rs` so T003 passes, with no length limit (research G10)
- [X] T005 Replace the value type of `ServerState::buffers` by `BufferKind` with `Shm(ShmBuffer)` only for now, giving width and height, in `src/server/mod.rs`, `src/server/shm.rs` and `src/server/compositor.rs` (`apply` no longer depends on which kind it holds)
- [X] T006 Turn `compositor::commit` and `apply_cached_if_desync` into transactions in `src/server/compositor.rs` and `src/server/mod.rs` (research G7): the commit root is the nearest desynchronised subsurface or the toplevel root; a `Transaction` holds `parts` (the committed state and the cached state of synchronised descendants) and `waits` (always empty for now); a synchronised subsurface's commit is cached as today; with no waits and an empty queue it applies immediately, so timing does not change
- [X] T007 Capture `xdg_surface` geometry and limits in the transaction (`xdg` field) instead of reading `pending_geometry` at apply time, in `src/server/xdg_shell.rs` and `src/server/compositor.rs` (research G7); post protocol errors found by applying when the transaction applies
- [X] T008 Introduce per-surface `Source` with `Copied { texture, opaque }` only in `src/render/gles.rs`, with `draw_windows` drawing through it; `tests/render_windows.rs` stays green unchanged
- [X] T009 [P] Write a failing test in `src/ffi/spawn.rs` that a spawned program sees the original soft `RLIMIT_NOFILE` after RubickWay raised its own (research G10)
- [X] T010 Raise the soft `RLIMIT_NOFILE` to the hard limit first thing in `run` in `src/standalone.rs`, through `src/ffi/spawn.rs`, and restore the original in `pre_exec`, so T009 passes
- [X] T011 Run the full check: nothing has changed for applications

**Checkpoint**: shared-memory behaviour is unchanged; stories can start.

---

## Phase 3: User Story 1 - Applications Render on the GPU (Priority: P1) MVP

**Goal**: kitty and other GPU programs render on the Intel GPU and hand over their images without a copy.

**Independent Test**: quickstart Q2 reports the Intel GPU, not llvmpipe; Q3 under 250 ms.

### Tests for User Story 1 (write first, see them fail)

- [X] T012 [P] [US1] Write failing unit tests in `src/render/formats.rs`: the table from EGL's formats and modifiers, including external-only pairs; a format reporting no modifiers gets one `DRM_FORMAT_MOD_INVALID` entry, otherwise implicit layouts are not offered (research G2); table entries are `(fourcc: u32, pad: u32, modifier: u64)`; minimum planes per format (NV12: 2); the offer decision, a pure function of what the renderer reports, gives no offer with "no import" as input (FR-014)
- [X] T013 [P] [US1] Write failing unit tests in `src/ffi/dmabuf.rs`: `is_dma_buf` rejects a memfd and an ordinary file, before any operation on the file (research G3) (the sealed memfd is the keymap's, already tested; the readable poll comes with T035)
- [X] T014 [P] [US1] Write failing unit tests in `src/server/release.rs` for `ReleaseTracker` (data-model "ReleaseTracker"): `retire(buffer, seq)`; `release_ready(oldest_in_flight)` returns entries with `retired_at < oldest_in_flight`, or all when nothing is in flight
- [X] T015 [P] [US1] Add `ImageFiles` and its double accepting a memfd to `tests/protocol/harness.rs` and write failing protocol tests in `tests/protocol/dmabuf.rs` per `contracts/protocols.md`: the global at version 5; bound below 4, `format` and `modifier` events at bind; bound at 4 or 5, none; feedback events in the contract's order with `tranche_flags` 0, sent once; surface feedback equals default feedback; feedback for a destroyed surface stays inert; every row of the `zwp_linux_buffer_params_v1` table; `create` answers after the import step, `create_immed` marks a failed buffer; `create_immed`, `attach` and `commit` sent together, as Mesa does: the commit applies only after the import step, the window maps with the image's size on success, and on failure nothing about the window has changed (research G7, FR-016); a failed buffer committed: nothing changes on screen, frame callbacks complete, it is released, no error (research G8, FR-005)
- [X] T016 [P] [US1] Write failing rendering tests in `tests/render_images.rs` with external textures made from ordinary textures through `EGL_KHR_gl_texture_2D_image` on `Egl::software_device`, through `GlesRenderer::add_image_from_pixels`, since tests may not hold `unsafe` (research G11), asserting pixel facts: colours and alpha equal to a shared-memory window with the same image; alpha 1 for a format without alpha where it can be expressed; y-invert; every buffer transform; at less than the pixel size the mipmap copy gives the same non-aliased result as a shared-memory window; at 100% no copy is made; the copy goes with the next image
- [X] T017 [US1] Write failing tests (after T015, which adds the double) in `tests/protocol/misbehaving.rs` for spec Edge Cases 1-6 and 9-11: invalid descriptions, a description used twice, a file that is not a GPU image or too small, an unoffered pair, different modifiers across planes, unsupported flags, a refused import through the double, an image above the size limit (the existing `invalid_size` rule, on commit), destroying an image while shown, disconnecting with images on screen, feedback for a destroyed window; each leaves RubickWay and another client running

### Implementation for User Story 1

- [X] T018 [US1] Implement `src/render/formats.rs` and the EGL queries (`eglQueryDmaBufFormatsEXT`, `eglQueryDmaBufModifiersEXT`, through `eglGetProcAddress`) in `src/render/egl.rs` so T012 passes; never call `eglQueryDevicesEXT` on this path (FR-012, research G5)
- [X] T019 [US1] Implement `src/ffi/dmabuf.rs` (`readlink` on `/proc/self/fd/N` first, then `lseek` for the size; poll comes with T035; the sealed memfd is in `src/ffi/shm_map.rs`) with `// SAFETY:` comments so T013 passes
- [X] T020 [US1] Implement `ReleaseTracker` in `src/server/release.rs` so T014 passes
- [X] T021 [US1] Add the render node lookup through sysfs from the picked card in `src/backend/drm/gpu.rs`, its decision a pure function with unit tests, returning the `st_rdev` for `main_device` (research G2)
- [X] T022 [US1] Implement `GpuOffer`, the `zwp_linux_dmabuf_v1` global and feedback objects in `src/server/dmabuf.rs`, registered from `src/server/mod.rs` only when an offer exists: `main_device` and `tranche_target_device` the display GPU's render node, one tranche with all table entries, flags 0
- [X] T023 [US1] Implement `zwp_linux_buffer_params_v1` and `trait ImageFiles` in `src/server/dmabuf.rs` with all checks in `u64` arithmetic per research G3 and the contract's table: planes 1 to 4 of `{ fd, offset, stride }`, one modifier for all planes, width and height positive, `offset + stride * height` for plane 0, `offset` inside the buffer for all, the pair in the table, flags other than `y_invert` fail; add `BufferKind::Gpu(GpuBuffer)` with `key`, `state` `Pending` -> `Ready` or `Failed`, shared `WlBuffer` dispatch in `src/server/shm.rs`, so the validation part of T015 and T017 passes
- [X] T024 [US1] Implement `ImageDesc` in `src/render/image.rs`, the import in `src/render/egl.rs` and `ImportedImage` in `src/render/gles.rs`: import with `EGL_LINUX_DMA_BUF_EXT` and bind to `GL_TEXTURE_EXTERNAL_OES`, destroyed with the buffer or the renderer; the part binding an EGLImage to a texture takes the image, so T016 can give it one made from an ordinary texture; `render` stays free of DRM and Wayland types, `unsafe` with `// SAFETY:` comments
- [X] T025 [US1] Add `src/render/shaders/window_external.frag`, the external program and `Source::Image { image, y_invert, mip }` to `src/render/gles.rs`, with the same geometry, crop, transform and alpha as shared-memory quads
- [X] T026 [US1] Add `MipCopy` to `src/render/gles.rs` (research G4): at less than the pixel size draw the image into an owned RGBA8 texture and generate mipmaps, once per new image, before the frame; sample the copy for that quad; free it with the surface's next image; T016 passes
- [X] T027 [US1] Implement `Server::process_imports` in `src/server/mod.rs` and call it from `step` in `src/standalone.rs`, before window events, so a window mapped by a waiting commit is seen in the same pass: the import outcome sets `Ready` or `Failed`, answers `created` or `failed`, and settles the transactions waiting for it, before the frame is drawn in the same pass of the loop; `create_immed` sends `failed` and marks the buffer; an import failure never ends an application (research G7, G8)
- [X] T028 [US1] Accept GPU buffers in `commit` and `apply` in `src/server/compositor.rs`: a commit of a buffer in `Pending` becomes a transaction waiting for its import (research G7); held image, surface part naming its `BufferKey`, existing size limit rule, a failed buffer per the contract; log once per application for the same cause (FR-015, `contracts/cli.md`); a finished image is otherwise treated as immediate for the moment (real waits come in US2); T015 and T017 pass
- [X] T029 [US1] Wire release tracking in `src/standalone.rs`: each frame gets a sequence number, each output keeps `in_flight: Option<u64>` (cleared on pause), replaced images are retired inside the server and go back with `wl_buffer.release` through `Server::release_images` when `release_ready` allows (research G9, FR-005)
- [X] T030 [US1] Build the offer at startup in `src/standalone.rs` and log `composing on <GPU> (<render node>), offering applications <n> image formats`; add the debug line `window <id>: first frame presented` for SC-001 (`contracts/cli.md`)
- [X] T031 [US1] Run the full check, then quickstart Q1-Q6, Q8-Q10 on the laptop (done 2026-11-04 on one display; Q4 on two displays is left to T045) (manual; the owner runs them on a text console). If Q2 does not report Intel, stop and read `EGL_LOG_LEVEL=debug WAYLAND_DEBUG=1` before going on (review CHK010); fix what fails

**Checkpoint**: applications render on Intel; this is a usable MVP.

---

## Phase 4: User Story 2 - Correct Frames, and a Slow Application Cannot Stall the Desktop (Priority: P2)

**Goal**: a window's update waits for its finished image without blocking anything, and explicit
synchronisation is offered.

**Independent Test**: quickstart Q11: the pointer and other windows stay smooth next to a slow program.

### Tests for User Story 2 (write first, see them fail)

- [X] T032 [P] [US2] Write failing protocol tests in `tests/protocol/commits.rs`, driving waits by hand through `Server::take_waits` and `wait_done`: the window keeps its last picture while a wait is open; order per surface when a later image finishes first (FR-008); synchronised subsurfaces go with their parent; a desynchronised subsurface has its own queue; frame callbacks complete only after the update took effect and was shown; geometry and limits come from the transaction (research G7); no wait holds up another surface; releases in order
- [X] T033 [P] [US2] Add the `Timelines` double with controllable acquire points and write failing protocol tests, both in `tests/protocol/syncobj.rs`: every row of the `wp_linux_drm_syncobj_v1` table in `contracts/protocols.md`; the commit takes effect after the acquire point and implicit readiness is then ignored; the release point is signalled once no frame in flight can read the buffer, and `wl_buffer.release` is not sent; a point set before the timeline's `destroy` stays valid; an acquire point never signalled keeps the last picture; on disconnect everything is dropped and pending release points are not signalled
- [X] T034 [P] [US2] Write failing tests in `tests/protocol/misbehaving.rs` for spec Edge Cases 7, 8 and 12: a never-finished image with later commits behind it, while another client keeps getting frames and input (SC-004); a client creating many images, parameter objects, timelines and waiting commits is not ended (no limits, spec Assumptions); every misuse of explicit synchronisation is a protocol error for that client only

### Implementation for User Story 2

- [X] T035 [US2] Create `Wait { id, fd }` from each plane's dma-buf at commit in `src/server/compositor.rs` and `src/server/mod.rs`: a zero-timeout poll settles most cases at once, otherwise a duplicate of the fd is a wait in the transaction; add `Server::take_waits` and `wait_done`; a wait whose transaction is gone is ignored (research G6, FR-007); T032 passes
- [X] T036 [US2] Insert one-shot calloop sources for waits in `src/standalone.rs`, calling `Server::wait_done` and never blocking the loop (Principle VII); remove those `Server::take_cancelled_waits` names, as their surface is gone
- [X] T037 [US2] Implement the manager, timeline and surface objects with `SyncPoints` and `trait Timelines` in `src/server/syncobj.rs`, registered from `src/server/mod.rs` when `App` hands the server a `Timelines`, as it does with `GpuOffer`, which it does when the GPU reports `DRM_CAP_SYNCOBJ_TIMELINE`, otherwise logging `explicit synchronisation is off: <GPU> does not support it`; timeline objects are reference-counted; T033 and T034 pass
- [X] T038 [US2] Implement the real `Timelines` on the display GPU's render node in `src/backend/drm/timelines.rs` (`DRM_IOCTL_SYNCOBJ_EVENTFD` with flags 0 for the acquire point, CPU signal for the release point), opened only for the display GPU (FR-012), with `// SAFETY:` comments where needed
- [X] T039 [US2] Extend what the release tracker holds to a release point (`ToRelease` in `src/server/syncobj.rs`) and signal it in `Server::release_images` instead of `wl_buffer.release`, unless the application has gone (research G9)
- [X] T040 [US2] Run the full check, then quickstart Q7, Q10 (with `WAYLAND_DEBUG=1`, recording whether explicit synchronisation is in use) and Q11 on the laptop (manual); fix what fails. Q11 found the event loop stopped by creating a copy's texture for each image: fixed (research G4), and SC-003 reworded by the owner. With `distant-windows` the final numbers come from T045

**Checkpoint**: a slow or stuck application hurts nobody else.

---

## Phase 5: User Story 3 - Everything Keeps Working Without GPU Buffers (Priority: P3)

**Goal**: shared memory stays as it was, and a renderer without import still gives a working desktop.

**Independent Test**: automated: no offer means no global, and shared-memory clients work.

- [X] T041 [P] [US3] Write protocol tests in `tests/protocol/dmabuf.rs`: with no offer there is no `zwp_linux_dmabuf_v1` or syncobj global and a shared-memory client works; one window alternating shared-memory and GPU images shows each correctly; on a window with explicit synchronisation a shared-memory buffer with points is `unsupported_buffer` (FR-013, in `tests/protocol/syncobj.rs`); a shared-memory client next to a GPU one is unaffected. They pass from the start, as the code they check came with US1 and US2
- [X] T042 [US3] Log `<GPU> cannot import GPU images (<reason>): applications will draw in software` when there is no offer, in `src/standalone.rs` (FR-014)

**Checkpoint**: SC-009 holds.

---

## Phase 5b: Distant Windows (FR-017, added by the owner after quickstart Q11)

**Goal**: less work for distant windows, each part a parameter, nothing changed by default.

- [X] T047 [P] Write failing tests in `tests/config.rs` for the `distant-windows` block per `contracts/config.md`: no block means nothing set; each value and its range; a wrong value is a warning with its line; a value without `below-zoom-percent` is a warning
- [X] T048 [P] Write failing unit tests in `src/distant.rs`: at or below the zoom counts as distant, above does not, nothing configured never does; frame pace lets a window through at most `max-fps` times a second, others are independent, the clock may wrap, a forgotten window starts anew; the copy's size from the image, drawn size and parameter, never above the image and never zero
- [X] T049 [P] Write failing rendering tests in `tests/render_images.rs`: with a detail set the copy has the expected smaller size and the picture stays the same colours; zooming in past what the copy holds makes a larger one; without a detail the copy has the image's size
- [X] T050 Implement the block in `src/config.rs`, `src/distant.rs`, and the size in `GlesRenderer::prepare_frame` so T047-T049 pass
- [X] T051 Wire it in `src/standalone.rs`: both places completing frame callbacks go through the pace, the detail comes from the display's zoom, a closed window is forgotten, and the startup line of `contracts/cli.md` is logged; checked by quickstart Q13-Q15, as `App` runs on real hardware only

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T043 [P] Update `README.md` (plan "Supporting changes"): GPU rendering; remove the "shared memory only" status and the note that Mesa draws GPU applications with its software renderer
- [X] T044 [P] Add a *Gotchas* line to `AGENTS.md` if implementation found something every agent would otherwise rediscover (for example how rendering tests make external textures)
- [X] T046 Write failing unit tests, then implement a pure tracker of "out of file descriptors" in `src/server/descriptors.rs`: `EMFILE` or `ENFILE` from `accept_clients` in `src/standalone.rs` or from getting a file to wait for an image on starts it, a pass of the loop able to open a descriptor ends it, each logs one line per `contracts/cli.md` naming the application holding the most GPU images, and the existing warnings are not repeated meanwhile (FR-015, research G10); do this before T045
- [ ] T045 Run the full check from `AGENTS.md` and the whole quickstart once more; update spec, plan or contracts first wherever implementation had to deviate. The full check passed on 2026-11-09. The owner committed without the rest and will fix whatever comes up as bugs. Not run yet: Q11 with `distant-windows`, Q12, Q13-Q15, Q4 on two displays, and the repeat of the others (all of which passed once, in T031 and T040)

---

## Dependencies & Execution Order

- **Setup (Phase 1)**: first
- **Foundational (Phase 2)**: after Setup; blocks all stories
- **US1 (Phase 3)**: after Foundational; the MVP
- **US2 (Phase 4)**: after US1 (it needs GPU images to wait on)
- **US3 (Phase 5)**: after US1 (T041 needs GPU images next to shared-memory ones)
- **Polish (Phase 6)**: after all stories

### Within each story

- Tests first, seen failing, then implementation (Principle III)
- Pure code (formats, file checks, trackers) before the protocol objects using it, and those before the hardware wiring

### Parallel opportunities

- Foundational: T003 and T009
- US1 tests: T012, T013, T014, T015, T016 (different files); T017 after T015
- US2 tests: T032, T033, T034
- Polish: T043, T044

## Implementation Strategy

### MVP first (User Story 1)

1. Setup + Foundational: nothing visible changes
2. US1: kitty on Intel - validate with Q1-Q6, Q8-Q10
3. Stop and let the owner try it

### Incremental delivery

1. US2: correct frames, slow applications, explicit synchronisation
2. US3: the fallback checks
3. Polish: README

Each story is committed only when the owner asks and the full check passes (`AGENTS.md`).
