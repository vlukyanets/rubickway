# Feature Specification: Client Windows on the Canvas

**Feature Branch**: `worktree-client-windows`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "RubickWay becomes a real Wayland compositor for applications.
While the nested viewer runs, it offers its own Wayland socket; an application started with
that socket (for example a terminal such as foot or weston-terminal) connects, and its window
appears on the canvas, opened at the centre of the current view. Windows live on the canvas:
they pan and zoom together with the grid, drawn at their size in canvas units. Clicking a window
gives it keyboard and pointer focus; keys and pointer input go to the focused application, so
the user can type in a terminal on the canvas. Mod + left-drag keeps panning the canvas even
over windows. Closing the application removes its window. A misbehaving or crashing application
never takes the compositor down. Only applications that draw into shared memory are supported
in this feature; GPU buffers, popups/menus, clipboard, moving and resizing windows come in later
features."

## Clarifications

### Session 2026-11-01

- Q: Should RubickWay start applications itself, or does the user start them with the socket
  name RubickWay prints? -> A: The user starts them with the printed socket name; RubickWay
  launches nothing in this feature.
- Q: When an application changes its own window size, which point of the window stays fixed on
  the canvas? -> A: The top-left corner; the window grows or shrinks to the right and down.
- Q: What happens when an application offers a window larger than RubickWay can display? ->
  A: That application is disconnected with an error; nothing else is affected.
- Q: When the user clicks the empty canvas background, does the focused window keep the
  keyboard? -> A: Yes; background clicks and drags only pan and never change keyboard focus.
- Q: When zooming, are windows magnified as pictures, or asked to redraw at the zoomed density?
  -> A: Magnified and shrunk as pictures, smoothly filtered; sharp at 100% zoom only. Redrawing
  at the zoomed density is a later feature.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An application's window appears on the canvas (Priority: P1)

With RubickWay running in their desktop session, the user starts an application (for example a
terminal) so that it connects to RubickWay instead of the host desktop. The application's window
appears on the canvas at the centre of the current view, at its own size at 100% zoom, and
shows the application's live content. Panning and zooming moves and scales the window together
with the grid. When the application closes its window or exits, the window disappears.

**Why this priority**: showing other programs' windows is what makes RubickWay a compositor
rather than a grid viewer; every later feature (window management, GPU applications, more
viewers) builds on it.

**Independent Test**: start RubickWay, start a terminal against RubickWay's socket, see its
window on the canvas with the prompt drawn, pan and zoom, then exit the terminal; the window is
gone and RubickWay keeps running.

**Acceptance Scenarios**:

1. **Given** RubickWay is running, **When** it starts, **Then** it tells the user the name of
   the socket applications connect to.
2. **Given** RubickWay is running, **When** an application connects and opens a window,
   **Then** the window appears centred on the canvas point at the centre of the current view,
   at the application's chosen size in canvas units, on top of all other windows.
3. **Given** a window is on the canvas, **When** the application draws new content, **Then**
   the window shows the new content.
4. **Given** a window is on the canvas, **When** the user pans or zooms, **Then** the window
   moves and scales exactly like the canvas under it (it stays attached to the same canvas
   position).
5. **Given** a window is on the canvas, **When** the application closes the window, exits or
   crashes, **Then** the window disappears and RubickWay keeps running.
6. **Given** applications are connected, **When** the user quits RubickWay, **Then** RubickWay
   exits as before and the applications lose their connection.

---

### User Story 2 - Use an application on the canvas (Priority: P2)

The user works with the applications on the canvas: a newly opened window receives the
keyboard, so the user can type into a terminal right away; clicking another window moves the
keyboard to it and brings it to the front. The pointer interacts with whatever window is under
it (clicks, scrolling, selecting text), at the right place inside the window at any zoom.

**Why this priority**: a visible but unusable window proves little; input turns the canvas into
a place to work. It depends on Story 1.

**Independent Test**: open two terminals, type into the newest one, click the other one and
type there; scroll inside a terminal; select text with the pointer at 50% and at 200% zoom.

**Acceptance Scenarios**:

1. **Given** a new window opens, **When** it appears, **Then** it has keyboard focus and typed
   keys go to its application.
2. **Given** two windows are open, **When** the user clicks the one without keyboard focus,
   **Then** it gets keyboard focus and is raised above all other windows, and the click itself
   is also delivered to it.
3. **Given** the pointer is over a window, **When** the user moves, clicks or scrolls, **Then**
   the application under the pointer receives the input at the matching position inside its
   window, regardless of the zoom level.
4. **Given** a window has keyboard focus, **When** the user presses a compositor shortcut
   (Mod+0, Mod+Shift+Q), **Then** RubickWay performs it and the application does not receive
   those keys; all other keys and key combinations go to the application.
5. **Given** the focused window disappears, **When** other windows remain, **Then** the topmost
   remaining window gets keyboard focus.

---

### User Story 3 - Navigate the canvas over windows (Priority: P3)

With windows on the canvas, the user still moves around freely: Mod + left-drag pans and
Mod + scroll zooms anywhere, including over windows, while plain dragging and scrolling on the
empty background keep working as before.

**Why this priority**: windows must not trap the user; without it, a window covering the view
blocks navigation. It refines Story 2's input routing.

**Independent Test**: zoom in until a window fills the viewer, then pan away with Mod +
left-drag and zoom out with Mod + scroll while the pointer is over the window; the application
receives none of these inputs.

**Acceptance Scenarios**:

1. **Given** the pointer is over a window, **When** the user holds Mod and left-drags, **Then**
   the canvas pans exactly as on the background and the application receives no button or
   motion input during the drag.
2. **Given** the pointer is over a window, **When** the user holds Mod and scrolls, **Then** the
   canvas zooms around the pointer and the application receives no scroll input.
3. **Given** the pointer is over the empty background, **When** the user drags or scrolls
   without Mod, **Then** the canvas pans or zooms as in the nested-canvas feature.
4. **Given** a window has keyboard focus, **When** the user clicks or drags on the empty
   background, **Then** the window keeps keyboard focus and typing still goes to it.

---

### Edge Cases

- **Misbehaving applications** - exactly these seven cases (each affects only the offending
  application; RubickWay and all other applications keep running and responding; RubickWay
  logs which application it disconnected and why, and shows nothing on screen):
  1. an application sends a request the protocol forbids or with invalid values (for example
     a negative or overflowing buffer size, an unknown object, drawing into a window before
     RubickWay has told it the window is ready): that application is disconnected with a
     protocol error;
  2. an application misuses the shared memory behind its buffers - shrinks or replaces the
     file while RubickWay reads it, or asks to make a memory pool smaller: RubickWay does not
     crash; the application is disconnected with an error;
  3. an application crashes or disconnects at any moment, including while focused, while its
     window is being drawn, while the user holds a key, or in the middle of a pointer drag
     inside its window: its window disappears, a drag inside it ends (the pointer then targets
     whatever is under it), keys held for it are forgotten, and keyboard focus moves per
     FR-014;
  4. an application stops reading its messages: RubickWay never waits for it; once its
     unread messages exceed a limit (set in the plan), it is disconnected;
  5. an application floods requests: other applications and the user's input stay responsive;
  6. an application never draws anything: no window appears for it, and it does not block
     others;
  7. an application offers a window larger than RubickWay can display - wider or taller than
     the largest image the graphics hardware supports, a limit RubickWay reads from the
     hardware at startup: that application is disconnected with an error, nothing else is
     affected; its window is never shown scaled down or clipped.
- **Hidden viewer**: while the viewer window is hidden, applications are not asked to draw new
  frames, and RubickWay itself draws nothing.
- **Idle applications**: when no application changes its content and the user does nothing,
  RubickWay draws no frames (as in nested-canvas).
- **Many windows at the same place**: each new window opens at the view centre on top of the
  others; overlapping is allowed.
- **Window outside the view**: a window panned out of view keeps running but is not drawn and
  is not asked to draw new frames.
- **Pointer leaves a window mid-drag inside it**: a drag that started inside a window keeps
  sending motion, buttons and scrolling to that window until the last button is released,
  even outside it or over another window.
- **Other buttons on the background**: right and middle clicks on the empty background do
  nothing and do not change keyboard focus.
- **New window while typing elsewhere**: a newly shown window always takes keyboard focus,
  even while the user is typing into another window (no focus-stealing prevention yet).
- **Viewer loses keyboard focus in the host**: the focused application loses the keyboard
  too, and gets it back when the viewer regains it.
- **Second RubickWay instance**: it chooses a different socket name and reports it.
- **Keyboard layout**: applications receive the same keyboard layout the user has in the host
  desktop, including layout switching.
- **Display scale**: applications are told the viewer's display scale, so at 100% zoom an
  application that honours it is drawn sharp at the display's native density. When the scale
  changes while running (for example the viewer window moves to another monitor),
  applications are told the new scale.
- **Applications that need GPU buffers**: RubickWay shows nothing for them; the application
  itself reports that it cannot start (or falls back to software drawing, if it can).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: While running, RubickWay MUST accept Wayland application connections on its own
  socket in the user's session runtime directory, reachable only by the same user, and MUST
  report the socket name on startup.
- **FR-002**: The socket name MUST be chosen so it does not collide with an existing socket
  (the host's or another RubickWay instance's).
- **FR-003**: RubickWay MUST support applications that draw their windows into shared memory;
  applications that need other buffer types are out of scope.
- **FR-004**: When an application opens a top-level window and draws its first content, the
  window MUST appear on the canvas centred on the canvas point at the centre of the current
  view, with a size in canvas units equal to the window's size in the application's logical
  pixels, above all other windows. When the application later changes the window's size, the
  window's top-left corner MUST stay at the same canvas position and the window grows or
  shrinks to the right and down.
- **FR-005**: Windows MUST be drawn as part of the canvas: panning and zooming move and scale
  them together with the grid, and the grid is drawn behind them. At zoom levels other than
  100% a window's latest content is magnified or shrunk smoothly: a window showing one-pixel
  white lines on every fourth column of black looks evenly dark grey (a quarter brightness) at
  25% zoom (no moire, no blocky pixels); applications are not asked to redraw because of a
  zoom change. Translucent parts of
  a window are blended over what is behind them (the grid or other windows), and the parts of
  a window that an application layers (sub-windows) are drawn in the order it requests.
- **FR-006**: A window MUST show each new frame the application draws in the next frame
  RubickWay draws, and RubickWay MUST tell each application when it is a good time to draw the
  next frame only while its window at least partly intersects the view (a window covered by
  other windows still counts).
- **FR-007**: When an application destroys its window, disconnects or crashes, the window MUST
  disappear and RubickWay MUST release everything it held for that application.
- **FR-008**: A newly shown window MUST receive keyboard focus.
- **FR-009**: A left, right or middle click on a window (without Mod) MUST give it keyboard
  focus, raise it above all other windows, and be delivered to the application.
- **FR-010**: Pointer motion, buttons and scrolling over a window (without Mod) MUST go to that
  window's application, with positions in the window's own coordinates, so the position the
  application sees matches the point under the pointer at any zoom. Where an application
  declares part of its window as not accepting input, pointer input there goes to whatever is
  below (another window or the background).
- **FR-011**: Key presses MUST go to the application with keyboard focus, except presses that
  match a compositor shortcut, which RubickWay handles; neither the press nor the matching
  release of a shortcut key is forwarded. The focused application MUST receive the user's
  keyboard layout and modifier state.
- **FR-012**: Mod + left-drag and Mod + scroll MUST pan and zoom the canvas anywhere, including
  over windows, and the application under the pointer MUST NOT receive those inputs; a
  Mod + left press on a window does not focus or raise it. A touchpad pinch zooms the canvas
  anywhere, including over windows (applications receive no pinch gestures).
- **FR-013**: Plain left-drag and scrolling on the empty background MUST keep the
  nested-canvas behaviour (pan, zoom, touchpad pan and pinch). Clicks and drags on the
  background MUST NOT change keyboard focus.
- **FR-014**: When the window with keyboard focus disappears, keyboard focus MUST move to the
  topmost remaining window, or to none.
- **FR-015**: An application that violates the protocol, misuses shared memory, offers a
  window larger than RubickWay can display, or does not read its messages in time MUST be
  disconnected with an error, without affecting RubickWay or other applications.
- **FR-016**: RubickWay MUST NOT wait for any application at any time; a slow or stuck
  application MUST NOT delay drawing, input or other applications.
- **FR-017**: Applications MUST be told the viewer's display scale, again whenever it changes,
  and a window drawn at that scale MUST be shown at the display's native density at 100% zoom.
- **FR-018**: RubickWay MUST NOT draw frames while no window content changes and the user does
  nothing, or while the viewer is hidden (as nested-canvas FR-015).
- **FR-019**: Quitting RubickWay (shortcut, window close, signal) MUST still exit with success
  status while applications are connected.

### Key Entities

- **Application (client)**: a program connected to RubickWay's socket; owns zero or more
  windows; can disconnect at any time.
- **Window**: an application's top-level window placed on the canvas; has a canvas position
  (its top-left corner, set so the window was centred in the view when it first appeared), a
  size in canvas units, its latest content, and a place in the stacking order.
- **Stacking order**: front-to-back order of windows; new and clicked windows go to the front.
- **Keyboard focus**: the one window (or none) that receives key presses.
- **Pointer target**: the window under the pointer that receives pointer input, or the canvas
  background.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: kitty (with software rendering, see Assumptions) started against RubickWay's
  socket shows its window on the canvas within 1 second of starting, on the author's machine,
  and is fully usable: typing, scrolling, selecting text with the pointer.
- **SC-002**: Typed characters appear in a focused terminal without perceptible delay: new
  content an application draws is shown in the very next frame RubickWay draws (checked
  automatically).
- **SC-003**: At any zoom from 10% to 1000%, a click lands within 1 pixel of the window's own
  coordinates (1 application logical pixel, whatever its size on screen) of the point under
  the pointer.
- **SC-004**: Each of the seven misbehaving-application cases listed in Edge Cases, exercised
  by an automated test, ends with only the offending application disconnected and RubickWay
  plus other applications still working: 7 of 7 cases.
- **SC-005**: With five terminals open and idle (no output, cursor not blinking), RubickWay uses
  effectively no CPU (under 1% of one core).
- **SC-006**: Panning and zooming with five windows open keeps up with the display's refresh
  rate with no visible stutter on the author's machine.

## Assumptions

- The user starts applications themselves, setting the socket name RubickWay reports (for
  example `WAYLAND_DISPLAY=<name> kitty` in a terminal); RubickWay does not launch programs in
  this feature (no launcher shortcut, no command to run at startup).
- The acceptance application is kitty, the author's terminal. kitty draws with OpenGL, whose
  GPU buffers come in a later feature, so in this feature it runs with Mesa's software
  renderer (`LIBGL_ALWAYS_SOFTWARE=1`), which makes it draw into shared memory. Native
  shared-memory clients (foot, weston-terminal, weston-simple-shm) are expected to work too but
  are not part of the acceptance checks. If kitty's software path turns out not to work with
  RubickWay, foot becomes the acceptance application and the change is recorded here.
- Only top-level windows; popups, menus and tooltips are not shown yet: a menu an application
  opens is closed again at once, so applications that rely on them work only partially.
- Only shared-memory applications; applications that require GPU buffers fail to start or fall
  back, depending on the application. GPU buffers come in a later feature.
- Windows cannot be moved, resized or closed by RubickWay in this feature; the application
  chooses its size, and the window's top-left corner stays where it first was. Requests from
  applications to maximize, fullscreen, minimize, or move or resize interactively (for example
  by dragging their own title bar) are accepted and ignored. Window management comes later.
- No window decorations (title bar, borders) are drawn by RubickWay; applications that draw
  their own decorations show them.
- No clipboard, drag-and-drop or keyboard shortcuts inhibition yet. Some applications refuse to
  start without a clipboard service, so RubickWay offers one that never transfers anything.
- The pointer shows the host's default arrow over windows; cursor images that applications
  request are not shown yet.
- The number of windows and other resources an application may create is not limited in this
  feature (a known limit, as in most compositors).
- One viewer, as in nested-canvas; applications see one display with the viewer's scale.
- Windows look sharp at 100% zoom; zoomed in they look soft, zoomed out they look smaller.
  Asking applications to redraw at the zoomed density comes in a later feature.
- Mod is still Alt while nested, so Alt + left-drag and Alt + scroll over a window go to the
  canvas, not to the application; other Alt key combinations reach applications.
