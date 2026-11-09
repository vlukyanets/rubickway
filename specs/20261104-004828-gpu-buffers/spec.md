# Feature Specification: GPU Buffers

**Feature Branch**: `worktree-gpu-buffers`

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "GPU buffers for applications (linux-dmabuf) together with control of
which GPU renders application windows. Without linux-dmabuf, Mesa EGL in clients such as kitty
fails native DRI ("failed to get driver name for fd -1"), tries zink, whose Vulkan
libVkLayer_MESA_device_select opens a second short-lived Wayland connection to learn the
compositor's GPU via dmabuf feedback, then falls back to llvmpipe. Result: applications render on
the CPU and kitty takes about 500 ms to show its first frame (faster on niri). Target machine:
laptop with Intel UHD 620 driving the displays and an NVIDIA MX130 with no outputs. dmabuf feedback
(main device / tranches) is also the mechanism for telling clients which GPU to render on, so the
scope includes choosing which GPU renders application windows. The drm-backend spec listed
"rendering on the second GPU" and direct scanout as not in that feature."

## Clarifications

### Session 2026-11-04

- Q: Can a start binding name the GPU its program renders on (`gpu=`)? -> A: No, removed. RubickWay
  does not place individual programs on a GPU; how a program ends up on another GPU by its own means
  is not RubickWay's concern.
- Q: Is explicit synchronisation in this feature? -> A: Yes.
- Q: Is choosing or switching the GPU RubickWay composes on in this feature? -> A: No (owner, after
  planning): composing on a GPU without displays needs a copy of every frame through main memory, and
  it and switching are about half of the work and most of the risk. RubickWay composes on the GPU
  driving the displays. Choosing it at start and switching while running are a later feature;
  what planning measured for it is kept in research.md.
- Q: Do automated tests use real GPU images? -> A: No test depends on `/dev/udmabuf` or any other
  device. Real imports are checked by hand, through the quickstart.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Applications Render on the GPU (Priority: P1)

The user opens kitty, or any other application that draws with the GPU. The application renders on
the GPU driving the displays and hands RubickWay its finished GPU images directly, without copying
them through main memory. The window appears quickly, redraws smoothly, and the CPU is not doing the
application's drawing.

**Why this priority**: This is the reason for the feature. Today every GPU-capable application
silently falls back to software rendering, which wastes the CPU and battery and delays the first
frame by about 500 ms.

**Independent Test**: Start RubickWay on the laptop, open kitty and a program that reports its
renderer. The renderer is the Intel GPU, not a software renderer, and kitty's window shows up in
under half of today's time.

**Acceptance Scenarios**:

1. **Given** RubickWay running on the laptop, **When** the user starts a program that reports its
   renderer, **Then** it reports the display GPU, not a software renderer.
2. **Given** RubickWay running, **When** the user presses the binding that starts kitty, **Then**
   kitty's first frame is on screen within the time set in SC-001.
3. **Given** a GPU-rendered window on a monitor, **When** the application redraws, **Then** the
   new content appears with correct position, size, colours and transparency, the same as a
   shared-memory window showing the same image.
4. **Given** a GPU-rendered window visible on two monitors at once, **When** the application
   redraws, **Then** both monitors show the new content, each in its own view of the canvas.
5. **Given** a GPU-rendered window, **When** the user moves, resizes, focuses, fullscreens or
   closes it, or pans and zooms the canvas, **Then** it behaves exactly like a shared-memory
   window.
6. **Given** a GPU-rendered window, **When** the application keeps redrawing for a long time,
   **Then** it gets each of its images back once RubickWay no longer shows it, and never runs
   out of images to draw into.
7. **Given** a program playing video in a format the GPU can sample (for example a two-plane video
   format), **When** it hands those images to RubickWay, **Then** the video shows with the colours
   the GPU driver's own conversion gives (not green or pink).

---

### User Story 2 - Correct Frames, and a Slow Application Cannot Stall the Desktop (Priority: P2)

The user runs a heavy GPU application next to other windows. The application may hand over a frame
before the GPU has finished drawing it. RubickWay shows a frame only when it is finished, so the
window never flickers or shows half-drawn content, and RubickWay does not wait for it: the pointer,
the other windows and the canvas stay smooth, and the slow window keeps showing its last finished
image until the next one is ready. Applications can use explicit synchronisation, which is how the
NVIDIA driver avoids flicker.

**Why this priority**: GPU images can be handed over before the GPU has finished drawing them, so
this is a new way for one application to hurt all the others. Principles II and VII forbid it. It
comes after US1 because it only matters once GPU images are accepted at all.

**Independent Test**: Run a program that takes much longer than a frame to draw next to a window
that animates. The animation and the pointer stay smooth, and the slow window shows its last finished
image. Run a program that uses explicit synchronisation and confirm it draws without flicker.

**Acceptance Scenarios**:

1. **Given** an application whose frames take much longer than a display refresh, **When** the
   user pans the canvas and moves the pointer, **Then** both stay as smooth as with no such
   application running.
2. **Given** an application that hands over an image whose drawing never finishes, **When** time
   passes, **Then** its window keeps its last finished image, nothing else is affected, and
   RubickWay's watchdog (which ends a frozen RubickWay) does not fire.
3. **Given** a slow application with a window visible, **When** its next image becomes ready,
   **Then** the window shows it in the next frame of each monitor showing that window, and frames
   are never shown out of order.
4. **Given** an application using explicit synchronisation, **When** it hands over an image with a
   "ready" point and a "finished reading" point, **Then** RubickWay shows the image only after the
   ready point is reached, and reaches the finished-reading point once no monitor can still be
   reading the image.
5. **Given** an application not using explicit synchronisation, **When** it runs next to one that
   does, **Then** both draw correctly.

---

### User Story 3 - Everything Keeps Working Without GPU Buffers (Priority: P3)

Programs that only use shared memory keep working exactly as before, and if the renderer cannot
accept GPU images at all, RubickWay still runs: it simply does not offer them, applications fall
back to shared memory, and the log says why.

**Why this priority**: Today's behaviour must not regress.

**Independent Test**: Run a shared-memory client next to a GPU-rendered one and confirm both draw
correctly. Automated tests check that a renderer reporting no GPU image import leads to no offer.

**Acceptance Scenarios**:

1. **Given** a shared-memory application, **When** it runs next to GPU-rendered applications,
   **Then** it behaves as before this feature.
2. **Given** one application, **When** it switches between shared-memory and GPU images on the same
   window, **Then** each image shows correctly.
3. **Given** a renderer that cannot import GPU images, **When** RubickWay starts, **Then** it does
   not offer GPU images, logs one plain-words line saying that applications will draw in software,
   and applications work through shared memory.

---

### Edge Cases

- **Misbehaving applications** (Principle II; earlier cases stay covered; each new case affects
  only the offending application, RubickWay and all other applications keep running and
  responding, and the log says which application was affected and why):
  1. a GPU image description with invalid values: zero or negative size, a plane number out of
     range, the same plane given twice, a missing plane, or sizes, offsets and strides whose
     arithmetic overflows: answered with a protocol error;
  2. a description used twice, or a change to a description already used: protocol error;
  3. a file descriptor that is not a GPU image (for example an ordinary file or shared memory), or
     one too small for the declared layout: only that image fails, reported to the application as
     a failed import or a protocol error per the protocol's rules; no crash and no out-of-bounds
     read;
  4. a format and modifier pair RubickWay did not offer, different modifiers across planes, more
     planes than the driver accepts for that format and layout, or flags RubickWay does not support:
     only that image fails, as in case 3, or a protocol error where the protocol names one;
  5. an image the GPU refuses to import even though it was offered: only that image fails, as in
     case 3;
  6. an image larger than the GPU can sample: not an import failure but the existing oversized-window
     case of client-windows (case 7), found on commit: the application is disconnected with an error,
     nothing is shown scaled or clipped;
  7. an image whose drawing never finishes, or a synchronisation point that is never reached (US2):
     the window keeps its last finished image, and any later commits on it wait behind that one;
  8. an application floods RubickWay with GPU images, descriptions, synchronisation objects or waiting
     commits: their number is not limited, as in other compositors (see Assumptions). RubickWay does not
     crash, but while the file descriptors are used up other applications may stall too, the one
     exception to "affects only the offending application" above;
  9. an application destroys an image or a synchronisation object while it is shown, while its
     drawing is unfinished, or while a commit using it is waiting: nothing crashes; the window keeps
     a valid picture until the protocol's rules say it is gone;
  10. an application crashes or disconnects while its images are waiting or on screen: its window
      disappears and all its images, synchronisation objects and file descriptors are released;
  11. an application asks for GPU information about a window that has already been destroyed:
      protocol error or an inert answer per the protocol's rules, no crash;
  12. explicit synchronisation misused: a file descriptor that is not a synchronisation object, a
      ready point without a finished-reading point or the reverse, points without an image, an image
      without points on a window that promised them, the same point used for ready and finished
      reading, or a second synchronisation object for one window: protocol error.
- **Switching consoles and suspend**: GPU images stay valid; after coming back, windows show their
  images again without the applications redrawing. Applications that keep drawing while RubickWay
  is away do not block its return.
- **Monitors plugged in or removed**: the GPU does not change, so applications are told nothing new
  and keep their images.
- **The window's image changes size**: handled as for shared-memory images, including the existing
  rules about the window size limit and buffer scale and transform.
- **Image upside down**: an image marked as vertically flipped shows the right way up.
- **The second GPU stays asleep**: RubickWay never opens or wakes a GPU other than the one driving
  the displays (FR-012).
- **A program rendering on another GPU** by its own means (for example `prime-run`): its window shows
  correctly when the display GPU can read its images (FR-011). When it cannot (for example a layout
  it does not support), only that image fails, as in misbehaving case 5, and the log names the
  application.

## Requirements *(mandatory)*

### Functional Requirements

**Accepting GPU images (US1)**

- **FR-001**: RubickWay MUST offer applications a way to hand over images held in GPU memory,
  using the standard Wayland protocol for this, without copying the pixels through main memory.
- **FR-002**: RubickWay MUST tell applications which GPU it composes the picture with, and which
  image formats and memory layouts that GPU can read, in the form current Mesa and NVIDIA client
  libraries use to pick their GPU, so that they render on a GPU and stop falling back to software.
- **FR-003**: The formats and layouts offered MUST be the ones the GPU reports it can sample, read
  from the GPU rather than a fixed list, and MUST include those mainstream GPU applications use (with
  and without transparency) and, where the GPU reports them, multi-plane video formats.
- **FR-004**: GPU-image windows MUST behave the same as shared-memory windows in position, size,
  crop, scale, buffer transform, transparency, subsurfaces, damage, frame callbacks, focus and
  input, on every monitor that shows them, and at every canvas zoom (no aliasing when zoomed out).
- **FR-005**: RubickWay MUST keep each GPU image usable for as long as it may still draw it, and
  MUST tell the application it can reuse an image as soon as the image is no longer needed, so a
  continuously redrawing application does not run out of images. This includes an image that failed
  to import.
- **FR-006**: GPU images MUST survive console switches and suspend; windows MUST show correct content
  after coming back, without requiring the application to redraw.

**Correct frames, not stalling the desktop (US2)**

- **FR-007**: RubickWay MUST NOT wait on the event loop or in frame drawing for an application's
  drawing to finish. A commit whose image is not finished MUST take effect when the image is
  finished; until then the window keeps showing its previous content.
- **FR-008**: Commits on one window MUST take effect in the order they were made, including when a
  later image finishes first. Commits that depend on each other through subsurfaces MUST take
  effect together.
- **FR-009**: RubickWay MUST offer applications explicit synchronisation, using the standard Wayland
  protocol for this, whenever the GPU supports it. An image with a ready point MUST NOT be shown before
  that point is reached, and the finished-reading point MUST be reached only when no monitor can still
  be reading the image.
- **FR-010**: Applications that do not use explicit synchronisation MUST keep working, with finished
  frames detected per FR-007.

**Other GPUs (US1)**

- **FR-011**: Windows of programs rendering on a GPU other than the one driving the displays MUST show
  correct content, without flicker, tearing or out-of-order frames, whenever the display GPU can read
  their images.
- **FR-012**: RubickWay MUST NOT open, initialise or wake a GPU other than the one driving the
  displays.

**Fallback and logging (US3)**

- **FR-013**: Shared-memory applications MUST keep working as before, and one window MAY mix
  shared-memory and GPU images over time, except a window using explicit synchronisation, for which
  the protocol does not allow shared-memory images.
- **FR-014**: If the GPU cannot import GPU images, RubickWay MUST NOT offer them, MUST keep running
  through shared memory, and MUST log one plain-words line saying that applications will draw in
  software.
- **FR-015**: At startup RubickWay MUST log which GPU composes the picture and how many formats it
  offers applications. Failed imports and fallbacks in this feature MUST leave a line in the log in
  plain words naming the application or GPU, not just an error code, at most once per application for
  the same cause, so a redrawing application cannot flood the log. Running out of file descriptors (misbehaving
  case 8) MUST leave one plain-words line when it starts and one when it ends, naming the application holding
  the most GPU images.

**Misbehaving applications (Principle II)**

- **FR-016**: All application-supplied image values (sizes, offsets, strides, plane counts) MUST be
  checked with overflow-safe arithmetic before use. Every misbehaving case in Edge Cases MUST leave
  RubickWay running and, except as case 8 says, other applications too. A failed import MUST affect only
  that image, and MUST have changed nothing on screen or about the window when it is found.
- **FR-017**: The owner MAY configure less work for distant windows, as games do with level of detail:
  windows on a display whose canvas zoom is at or below a set percentage. Two separate parameters: how
  often such a window is asked for a new frame, and the resolution of the copy that smooths it, relative
  to the size it is drawn at. With nothing configured, nothing changes (owner, 2026-11-04). A window also
  shown at a nearer zoom on another display is asked for frames as usual there.

### Key Entities

- **GPU image**: an application's picture held in GPU memory: size, format, memory layout and up to
  four planes, handed over as file descriptors. Belongs to one application; shown by any number of
  monitors at once.
- **Display GPU**: the GPU the monitors are connected to; it draws the final picture and reads the
  applications' images.
- **GPU offer**: what RubickWay tells applications: the display GPU, and the formats and layouts it
  can read.
- **Pending commit**: a window update whose image is not finished yet; it waits without blocking
  anything and takes effect in order.
- **Synchronisation points**: for an image using explicit synchronisation, a "ready" point set by the
  application and a "finished reading" point set by RubickWay.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the target laptop, kitty's first frame is on screen under 200 ms after kitty connects
  to RubickWay, measured from the log's `application <id> connected` line to the line saying that
  window's first frame was presented.
- **SC-002**: A program reporting its renderer reports the Intel GPU, in 10 of 10 starts.
- **SC-003**: While a program whose frames take about 10 times a display refresh or more runs visibly
  on the GPU composing the desktop, the event loop stays free and the desktop keeps moving. Frame work
  on the loop stays under 10 ms, apart from creating a copy's texture once for each size; libinput
  reports no input read late; and the pointer and canvas panning average at least 40 frames a second on
  a 60 Hz display at 100% zoom and zoomed out with `distant-windows`, and at least 35 zoomed out without
  it, counted from the frame-time lines of the debug log.
- **SC-004**: With a window whose image never becomes finished, an automated test shows that another
  application keeps getting frames and input, and that the event loop never waits for that image, so the
  watchdog has nothing to fire on.
- **SC-005**: A continuously redrawing application runs for 10 minutes without running out of
  images, without file descriptors or GPU memory held by RubickWay growing, and with no flicker seen.
- **SC-006**: Every misbehaving-application case in Edge Cases is covered by an automated test,
  except those that need a real GPU, which are listed in the quickstart as manual steps; none of
  them disturbs RubickWay or another application.
- **SC-007**: After a console switch and back, and after a suspend and resume, 100% of visible
  GPU-image windows show correct content without the applications redrawing.
- **SC-008**: A full session (start, use GPU applications, quit) leaves the second GPU unopened by
  RubickWay: none of its device files appear among RubickWay's open files.
- **SC-009**: All shared-memory checks of the earlier features still pass; a renderer reporting no GPU
  image import leads to no offer, and applications still work through shared memory.

## Assumptions

- **Target machine**: Intel UHD 620 (Mesa) driving the displays, NVIDIA GeForce MX130 with the
  proprietary 580xx driver and no outputs, verified on 2026-11-04. Other GPU combinations are not
  verified.
- **The second GPU stays out of it, checked on the laptop on 2026-11-04** (research G5): composing on
  Intel, RubickWay opens no NVIDIA device; only the driver's library is mapped, by the system's EGL
  dispatcher.
- **How programs find the GPU**: through the GPU information of FR-002, which the client libraries read
  when a program connects. Programs started with a setting of their own for another GPU (`prime-run`)
  are the user's choice; RubickWay only shows their windows (FR-011).
- **Not in this feature**: choosing the GPU RubickWay composes on, switching it while running, and
  everything that needs them (research G13 keeps what planning found); placing individual programs on a
  GPU (a per-program or per-binding setting, or any support for launchers that do it); direct scanout
  (showing a window's image on a monitor without composing); a hardware cursor plane; monitors on the
  second GPU; power management of the second GPU; HDR and colour management.
- **No limits per application on GPU images, descriptions, synchronisation objects or waiting commits**
  (owner, 2026-11-04, as in other compositors; for images, wlroots, weston and Smithay were checked): a
  limit would only ever hit normal programs, and ending one closes all its windows. RubickWay raises its
  file descriptor limit instead. The accepted risk is that a program creating them without end can use up
  descriptors and memory, and while the descriptors are used up other programs may stall and new ones
  cannot connect (research G10); this is to be looked at again later.
- **GPU resets**: recovery from a hung or reset GPU is not added here; behaviour stays as it is
  today.
- **Automated tests** run on the software renderer, with no device (Principle III). Protocol behaviour,
  validation, commit ordering and external-texture drawing are tested without a real GPU image. Real GPU
  import and SC-001 to SC-003, SC-005, SC-007 and SC-008 are checked through the feature's
  `quickstart.md` on the laptop.
- **SC-001's number**: the first target, 250 ms from the spawn binding, was not met: about 225 ms pass
  before kitty connects (quickstart run of 2026-11-04: 356-366 ms in all, 131-183 ms of them after
  connecting; about 500 ms before this feature). The owner chose to measure only the part after
  connecting; that part still includes kitty's own GL start-up.
- **SC-003's numbers** (owner, 2026-11-05): the first target, no more dropped frames than with no such
  program, was not met. On the UHD 620 with `glmark2` terrain at 7680x4320, the desktop gave 60 frames a
  second alone, about 44 next to it at 100% zoom, about 40 at 24% and about 37 at 69% (quickstart runs of
  2026-11-05). The loop is free by then (research G4), so what remains is most likely the GPU shared with
  that program, which was not measured apart. A higher GPU priority for RubickWay could be tried later: it
  needs `CAP_SYS_NICE`, so it is a separate decision.
- **Dependencies**: `drm-backend` (renderer, session, configuration file, spawn bindings) is merged.
  New crates, if any, are named in the plan and follow Principle I. The configuration file does not
  change.
