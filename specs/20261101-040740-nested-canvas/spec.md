# Feature Specification: Nested Canvas Viewer

**Feature Branch**: `worktree-nested-canvas`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "Running `rubickway` inside an existing Wayland session opens one
window (a viewer) that shows the endless 2D canvas, drawn as a background with a grid so
movement is visible. The user can pan the view by dragging, zoom in and out around the pointer
with the scroll wheel, and reset the view to the origin at 100% zoom. Resizing the window keeps
the canvas point at the window centre fixed. Closing the window or pressing a quit shortcut
exits cleanly. No Wayland clients connect yet - this feature establishes the nested viewer, the
canvas/camera model and rendering."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See the canvas in a nested viewer (Priority: P1)

The user starts RubickWay from a terminal inside their normal desktop session. A window opens
that shows the canvas: a background with a grid, centred on the canvas origin at 100% zoom.
Resizing the window shows more or less of the canvas while the canvas point at the window
centre stays put. Closing the window, or pressing the quit shortcut, ends the program cleanly.

**Why this priority**: Everything else (moving around, and later windows of real
applications) needs a viewer that opens, draws the canvas and shuts down cleanly. On its own it
proves the program can live inside a desktop session.

**Independent Test**: Start the program, see the grid, resize the window, close it; the
program exits with success status and leaves nothing running.

**Acceptance Scenarios**:

1. **Given** a running desktop session, **When** the user starts RubickWay, **Then** one
   window opens showing the grid centred on the canvas origin at 100% zoom.
2. **Given** the viewer is open, **When** the user resizes the window, **Then** the canvas
   point at the window centre stays at the window centre and the grid keeps its on-screen
   spacing (only the visible area changes).
3. **Given** the viewer is open, **When** the user closes the window through the desktop
   (title bar, keyboard shortcut of the host desktop), **Then** the program exits with success
   status.
4. **Given** the viewer is open and focused, **When** the user presses the quit shortcut,
   **Then** the program exits with success status.
5. **Given** no desktop session is available, **When** the user starts RubickWay, **Then** it
   prints a message saying it needs a running Wayland session and exits with failure status.

---

### User Story 2 - Pan and zoom around the canvas (Priority: P2)

The user moves around the canvas: dragging pans the view so the canvas follows the pointer, and
the mouse wheel zooms in and out around the point under the pointer. On a touchpad,
two-finger scrolling pans and pinching zooms.

**Why this priority**: The canvas is only "endless" if the user can travel across it and change
scale; it is the core interaction of the whole compositor.

**Independent Test**: With the viewer open, drag and scroll; the grid point grabbed stays under
the pointer during a drag, and the grid point under the pointer stays there while zooming.

**Acceptance Scenarios**:

1. **Given** the viewer is open, **When** the user drags with the pan gesture, **Then** the
   canvas point under the pointer at the start of the drag stays under the pointer for the
   whole drag.
2. **Given** the viewer is open, **When** the user scrolls up (away from themselves) one
   notch, **Then** the view zooms in by one step and the canvas point under the pointer stays
   under the pointer.
3. **Given** the viewer is open, **When** the user scrolls down one notch, **Then** the view
   zooms out by one step around the pointer.
4. **Given** the zoom is at its maximum (or minimum), **When** the user keeps zooming in (or
   out), **Then** the zoom stays at the limit and the view does not move.
5. **Given** a drag is in progress, **When** the pointer leaves the window, **Then** the pan
   continues until the button is released.
6. **Given** the viewer is open, **When** the user scrolls with two fingers on a touchpad,
   **Then** the view pans like a scrolled document (following the host's natural or traditional
   scrolling setting); **When** the user pinches, **Then** the view zooms
   around the pointer.
7. **Given** the viewer is open, **When** the user holds Mod and scrolls (wheel or touchpad),
   **Then** the view zooms around the pointer.
8. **Given** the viewer is open, **When** the user holds Mod and left-drags, **Then** the view
   pans exactly as with a plain drag on the background.

---

### User Story 3 - Return home (Priority: P3)

After wandering around, the user presses the reset shortcut and the view jumps back to the
canvas origin at 100% zoom.

**Why this priority**: A safety net against getting lost on an endless canvas; cheap, but the
feature is usable without it.

**Independent Test**: Pan and zoom away, press the reset shortcut, and the view is identical to
the view at startup.

**Acceptance Scenarios**:

1. **Given** the view has been panned and zoomed, **When** the user presses the reset shortcut,
   **Then** the window centre shows the canvas origin at 100% zoom, the same as at startup.

---

### Edge Cases

- **Far from the origin**: after panning one million canvas units away from the origin, the
  grid still draws evenly spaced, without jitter or gaps, and panning/zooming still behave
  exactly as near the origin.
- **Zoom extremes**: at minimum zoom the grid never becomes so dense that it turns into a solid
  fill; at maximum zoom grid lines stay visible within the window.
- **Tiny window**: the window can be resized down to very small sizes (even 1x1 pixel) without
  errors; the centre point rule still holds.
- **Focus lost mid-drag**: if the viewer loses pointer or keyboard focus while dragging (for
  example the host desktop switches workspace), the drag ends and the view stays where it is.
- **Window hidden or minimised**: while the window is not visible, the program does not keep
  drawing frames.
- **Host session ends**: if the host desktop session goes away or disconnects the program, the
  program exits with a message instead of hanging or crashing.
- **Host scale factor**: on a high-density or fractionally scaled display, the grid is drawn at
  the display's native resolution (sharp lines), and "100% zoom" means one canvas unit per
  logical pixel of the host.
- **Shortcut held down**: holding the reset shortcut (key repeat) has the same effect as one
  press; holding the quit shortcut quits once.
- **Misbehaving clients**: none in this feature - no client can connect yet, so there is no
  client-facing surface to misbehave against. The host session is the only external party and
  its failures are covered above.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Starting the program inside a running Wayland desktop session MUST open exactly
  one viewer window.
- **FR-002**: The viewer MUST show the canvas as a solid background with a grid aligned to the
  canvas coordinate system, so that any pan or zoom is visible.
- **FR-003**: At startup the viewer MUST show the canvas origin at the window centre at 100%
  zoom (one canvas unit per logical pixel).
- **FR-004**: The grid MUST adapt to the zoom level so that adjacent grid lines are never closer
  than a minimum on-screen spacing and never so far apart that no line is visible in a window
  of normal size.
- **FR-005**: Dragging with the pan gesture MUST move the view so that the canvas point under
  the pointer at drag start stays under the pointer until the drag ends. Pan gestures:
  left-button drag on the empty background, and Mod + left-button drag anywhere (in later
  features this pans even over application windows). Mod is defined in FR-010.
- **FR-006**: Each mouse-wheel notch, and each Mod + scroll step from any device, MUST zoom by a fixed multiplicative step of 10%
  around the canvas point under the pointer, which MUST stay under the pointer.
- **FR-007**: Zoom MUST be limited to a range from 10% to 1000%; requests beyond a limit leave
  the zoom at the limit and the view unchanged.
- **FR-008**: Two-finger touchpad scrolling and other continuous scrolling (without Mod) MUST
  pan the view by the scrolled distance, moving the canvas the way a scrolled document moves
  under the host's scroll-direction setting (natural or traditional). Horizontal mouse-wheel
  steps (wheel tilt) MUST pan horizontally by 50 logical pixels per notch. A touchpad pinch MUST zoom
  continuously around the point under the pointer, within the FR-007 limits, when the host
  desktop reports pinch gestures.
- **FR-009**: The reset shortcut Mod+0 MUST return the view to the canvas origin at 100% zoom.
- **FR-010**: The quit shortcut Mod+Shift+Q MUST exit the program with success status. Mod,
  the compositor modifier used by all compositor shortcuts and gestures, is Alt while running
  nested (the host desktop usually reserves Super).
- **FR-011**: Closing the viewer window through the host desktop MUST exit the program with
  success status.
- **FR-012**: When the window is resized, the canvas point at the window centre MUST remain at
  the window centre and the zoom MUST stay the same.
- **FR-013**: When no Wayland session is available, or the host session lacks something the
  viewer needs, the program MUST print a message naming the problem and exit with failure
  status.
- **FR-014**: When the host session disconnects the program, it MUST exit with a message and
  failure status, without hanging.
- **FR-015**: The program MUST NOT draw new frames while nothing on screen changes or while the
  window is not visible.
- **FR-016**: Pan and zoom MUST behave identically at any position within one million canvas
  units of the origin, with no visible jitter of the grid.
- **FR-017**: The viewer MUST render at the host display's native pixel density, including
  fractional scale factors.

### Key Entities

- **Canvas**: the single, unbounded 2D plane that all content lives on (empty in this feature
  except for the grid). Positions are in canvas units.
- **Viewer**: one window onto the canvas - here, the nested window. Has a size in logical
  pixels and a scale factor from the host display.
- **Camera**: what a viewer looks at - the canvas point at the viewer's centre and the zoom
  level. Converts between canvas positions and viewer positions.
- **Grid**: the background pattern drawn from the camera; its line spacing is chosen from the
  zoom level (FR-004).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From launch to the grid being visible takes under 1 second on the author's
  machine.
- **SC-002**: During a drag, the grabbed canvas point stays within 1 logical pixel of the
  pointer.
- **SC-003**: While zooming, the canvas point under the pointer moves by no more than 1 logical
  pixel per zoom step.
- **SC-004**: Continuous panning and zooming keep up with the display's refresh rate with no
  visible stutter on the author's machine.
- **SC-005**: When idle (no input, no resize), the program uses effectively no CPU (under 1% of
  one core).
- **SC-006**: The quit shortcut or window close ends the program within 1 second, with success
  status.
- **SC-007**: After pressing reset, the view matches the startup view exactly.

## Assumptions

- The user runs RubickWay from inside a Wayland desktop session (the "host"); running it
  standalone on the hardware is a later feature.
- One viewer only; several viewers of the same canvas come later.
- No application windows and no client connections in this feature; misbehaving-client cases
  therefore do not apply yet.
- No configuration file: colours, grid look, zoom step and shortcuts are fixed defaults for
  now.
- The view is not remembered between runs; every start shows the origin at 100%.
- Grid look: a dark neutral background with subtle lines, plus a stronger marker at the canvas
  origin axes so the user can tell where home is.
- Keyboard shortcuts work only while the viewer window has keyboard focus.
- Mod is Alt while nested; switching it to Super is expected with the standalone (hardware)
  backend, which is a later feature.
- The pointer shows the host's default arrow over the canvas and a "grabbing" hand while
  panning, when the host lets clients choose cursor shapes.
- The host desktop draws the window decorations (title bar, borders) if it provides them; the
  window is titled "RubickWay".
