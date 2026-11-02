# Feature Specification: Window Management on the Canvas

**Feature Branch**: `worktree-window-management`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "Window management on the canvas. Today an application's window
opens centred in the view and then never moves; RubickWay cannot move, resize or close it, and
requests from applications to move or resize interactively (dragging their own title bar or
edges) are ignored. This feature lets the user arrange windows on the endless canvas. The user
can move a window by dragging it with a compositor gesture anywhere inside it, and applications
that draw their own title bars (kitty, GTK apps) can start a move by a drag on their title bar;
the window follows the pointer in canvas units at any zoom. The user can resize a window with a
compositor gesture and by dragging an application's own edges when it asks to; the application
is asked for the new size and the window keeps the edge opposite the dragged one fixed. The user
can close the focused window with a shortcut: the application is asked to close (it may ask to
save first). Applications asking to maximize or go fullscreen get a sensible canvas-native
behaviour instead of being ignored. The user can bring a window into view: cycle keyboard focus
between windows with a shortcut, and the view moves to show the newly focused window. Window
positions are canvas positions; panning and zooming keep working as before, and Mod + left-drag
on a window still pans. Out of scope: GPU buffers, popups, clipboard, server-side decorations,
tiling, saving the layout between runs, multiple viewers."

## Clarifications

### Session 2026-11-01

- Q: Which gestures move and resize a window, given that Mod + left-drag pans everywhere? -> A:
  Mod + Shift + left-drag moves, Mod + right-drag resizes.
- Q: What does maximize mean on an endless canvas? -> A: The view moves to the window at 100%
  zoom and the window is resized to fill the viewer.
- Q: Which shortcut closes the focused window? -> A: Mod+Q; quitting stays Mod+Shift+Q.
- Q: When the user pans or zooms away from a maximized window, does it stay maximized? -> A: Yes;
  pan and zoom only move the view (the owner may revisit this later, e.g. for games in
  gamescope).
- Q: When an application goes fullscreen while nested, does RubickWay also make its own window
  fullscreen on the host? -> A: No; it only fills RubickWay's viewer, the host window stays as
  it is.

### Session 2026-11-02 (pre-tasks review, `checklists/review.md`)

Decisions informed by the source of DriftWM, niri, sway, Hyprland, KWin and Mutter.

- Q: Are maximize and fullscreen requests from a window without keyboard focus honoured? -> A: No,
  they are ignored (stricter than the six compositors, which apply them silently or, in DriftWM,
  with focus; on the canvas both requests move the view, so a background application could take
  the view at any time).
- Q: Are maximize and fullscreen requested before a window is first shown honoured? -> A: Yes, the
  window opens in that state (as niri, KWin, Mutter); if both are requested, fullscreen wins and
  maximize is kept underneath.
- Q: What happens when a maximized window goes fullscreen and leaves it again? -> A: It returns to
  maximized (as all six); each state keeps its own saved position and size.
- Q: What happens when a new window opens while another window is fullscreen? -> A: Fullscreen
  stays; the new window opens without focus, ready behind it (as DriftWM, niri, sway); a window
  whose parent is the fullscreen window (a dialog) ends fullscreen and opens as usual.
- Q: Does focus cycling move the view to a window that is already fully visible, even when it is
  the only window? -> A: No (as DriftWM and niri).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Move windows around the canvas (Priority: P1)

The user arranges their windows on the endless canvas: they drag a window by its own title bar,
or anywhere inside it with a compositor gesture, and drop it at another canvas position. The
window stays exactly under the pointer while it moves, at any zoom. To carry a window far away,
the user zooms out while still dragging it, moves it, and zooms back in.

**Why this priority**: without moving, every window piles up at the place it opened, and the
canvas cannot be organised at all; it is the core of window management on a canvas.

**Independent Test**: open two kittys (they open on top of each other), drag one by its title
bar to the right, drag the other with the compositor gesture to the left, zoom out during a drag
and drop the window far away; both windows stay where they were dropped while panning and
zooming.

**Acceptance Scenarios**:

1. **Given** a window that draws its own title bar, **When** the user presses the left button on
   the title bar and drags, **Then** the window moves with the pointer until the button is
   released, and stays at the new canvas position afterwards.
2. **Given** any window, **When** the user performs the compositor move gesture on it and drags,
   **Then** the window moves with the pointer, and the application receives no input from the
   gesture.
3. **Given** a move in progress at any zoom from 10% to 1000%, **When** the pointer moves,
   **Then** the point of the window that was under the pointer when the move started stays under
   the pointer.
4. **Given** a move in progress, **When** the user scrolls with Mod or pinches, **Then** the
   canvas zooms around the pointer and the window stays under the pointer.
5. **Given** a window is moved, **When** the move starts, **Then** the window gets keyboard focus
   and is raised above all other windows.

---

### User Story 2 - Resize windows (Priority: P2)

The user changes the size of a window: by dragging an edge or corner the application draws
itself (when the application asks RubickWay to resize), or with a compositor gesture from
anywhere inside the window. The application is asked for the new size and redraws; the edge or
corner opposite to the dragged one stays fixed on the canvas.

**Why this priority**: windows open at whatever size the application picks; resizing is the
second basic arrangement tool. It builds on the same drag handling as moving.

**Independent Test**: resize a kitty by its bottom-right corner, then by its top-left corner,
then with the compositor gesture near its left edge; each time the opposite edge or corner stays
put and kitty redraws at the new size.

**Acceptance Scenarios**:

1. **Given** an application asks to be resized from an edge or corner (the user pressed on its
   own border), **When** the user drags, **Then** the application is asked for sizes that follow
   the pointer, and the window keeps the opposite edge or corner at the same canvas position.
2. **Given** any window, **When** the user performs the compositor resize gesture in it,
   **Then** the window is resized from the edge or corner nearest to where the gesture started,
   as in scenario 1.
3. **Given** a resize in progress, **When** the application chooses a different size than the
   one asked for (for example it snaps to whole character cells, or has a minimum size), **Then**
   the window shows the application's size, still anchored at the opposite edge or corner.
4. **Given** a resize ends, **When** the application later changes its size by itself, **Then**
   the top-left corner stays fixed, as before this feature.

---

### User Story 3 - Find windows and switch between them (Priority: P3)

On an endless canvas windows can be far outside the view. The user cycles keyboard focus between
windows with a shortcut; each newly focused window is raised and, if it is not fully visible,
the view moves so that it is.

**Why this priority**: once windows are spread over the canvas, the user needs a fast way back to
each of them without hunting by panning; it uses the arrangement from Stories 1 and 2.

**Independent Test**: open three kittys, move them far apart, pan away from all of them, press
the focus-cycling shortcut repeatedly; each press shows and focuses the next window, and typing
goes to it.

**Acceptance Scenarios**:

1. **Given** two or more windows, **When** the user presses the next-window shortcut, **Then**
   keyboard focus moves to the next window in most-recently-used order and that window is raised.
2. **Given** the newly focused window is not fully inside the view, **When** focus moves to it,
   **Then** the view pans so the window is centred, keeping the zoom if the window fits, and
   zooming out just enough to show it whole if it does not.
3. **Given** the user holds Mod and presses the shortcut several times, **When** they release
   Mod, **Then** the window reached is the one focused, and the next cycle starts from it (two
   short presses with Mod released in between switch back and forth between two windows).
4. **Given** the previous-window shortcut, **When** pressed, **Then** focus moves in the
   opposite order.

---

### User Story 4 - Close windows (Priority: P3)

The user closes the focused window with a shortcut. RubickWay asks the application to close its
window; the application may ask the user first (for example to save), or close at once.

**Why this priority**: today a window can only be closed from inside the application; a
compositor close shortcut is basic, but small.

**Independent Test**: focus a kitty, press the close shortcut; kitty closes (or asks for
confirmation when a program is running in it, as kitty does).

**Acceptance Scenarios**:

1. **Given** a window has keyboard focus, **When** the user presses the close shortcut, **Then**
   its application is asked to close that window, and the application does not receive the key.
2. **Given** no window has keyboard focus, **When** the user presses the close shortcut,
   **Then** nothing happens.
3. **Given** the application ignores the request, **When** nothing else happens, **Then** the
   window stays; RubickWay does not force it closed.

---

### User Story 5 - Maximize and fullscreen requests (Priority: P3)

Applications that ask to be maximized (double click on their title bar, a maximize button) or to
go fullscreen (a video player, kitty's fullscreen toggle) get a canvas-native behaviour, and get
their previous size and position back when they ask to leave that state.

**Why this priority**: applications' own maximize and fullscreen controls currently do nothing,
which looks broken; it is a smaller part of the feature.

**Independent Test**: double click kitty's title bar, see the maximize behaviour, double click
again; toggle kitty fullscreen and back; each time the window returns to its earlier position
and size.

**Acceptance Scenarios**:

1. **Given** a window, **When** its application asks to be maximized, **Then** the view moves
   to the window at 100% zoom and the window is resized to fill the viewer exactly; the grid and
   other windows stay as they are, and other windows can still be drawn over it when raised.
2. **Given** a window, **When** its application asks to go fullscreen, **Then** it fills the
   whole viewer at 100% zoom, nothing else (grid, other windows) is drawn over or around it, and
   it has keyboard focus.
3. **Given** a maximized or fullscreen window, **When** its application asks to leave that state
   or the user resizes it, **Then** the window gets back the canvas position and size it had
   before, and the view returns to where it was before fullscreen; when the user moves it, it
   gets back its earlier size and follows the pointer (Edge Cases, "Dragging a maximized
   window").

---

### Edge Cases

- **Requests without a press**: an application asks to start a move or resize although the user
  is not pressing a button on that window (or the request refers to an older press): the request
  is ignored.
- **Window disappears mid-drag**: the window being moved or resized closes, or its application
  disconnects or crashes: the move or resize ends, the pointer returns to normal, nothing else
  changes.
- **Slow or unresponsive application during a resize**: RubickWay never waits; the window is
  drawn at the last size the application drew, anchored at the opposite edge or corner. Sizes
  asked for while the application has not answered yet are coalesced.
- **Size limits**: a resize never asks for less than the application's minimum or more than its
  maximum size, never for less than 1 x 1, and never for more than RubickWay can display (the
  graphics limit from client-windows, so a well-behaved application is never disconnected because
  of a size RubickWay asked for). When the limits contradict each other, the displayable limit
  wins over the minimum, and the minimum wins over a smaller maximum.
- **Invalid resize edges**: an application asking to resize from an edge value that does not
  exist is disconnected (a protocol error, unlike the ignored requests above).
- **Other input during a move or resize**: further button presses are ignored; Mod+Tab,
  Mod+Shift+Tab and Mod+0 are ignored (consumed); Mod+Q works as usual (if the dragged window
  closes, the operation ends); Mod+Shift+Q quits; scrolling without Mod does nothing; keys keep
  going to the focused window; Mod + scroll and pinch zoom the canvas around the pointer, and
  the moved window or the dragged edge keeps following the pointer.
- **Moving out of view**: a window can be dropped anywhere on the canvas, also completely outside
  the view; focus cycling brings it back.
- **Pointer leaves the viewer mid-drag**: the host ends the drag (as for client-windows grabs):
  the window stays where it was at that moment.
- **Viewer loses keyboard focus mid-drag**: the move or resize continues until the button is
  released, as panning does.
- **Mod released mid-gesture**: a compositor move or resize continues until the button is
  released (as panning does).
- **Focus cycling with zero or one window**: nothing happens with zero windows; with one window
  it is focused and raised, and the view moves only if the window is not fully inside it
  (FR-012).
- **Focus cycling to a window too large for the view even at 10% zoom**: the view is centred on
  it at 10%.
- **Mod released while RubickWay does not have the host's keyboard focus**: the focus order is
  updated when RubickWay loses the keyboard focus, as if Mod had been released.
- **Windows the user moved and new windows**: a new window still opens centred in the current
  view, on top, with focus (except while another window is fullscreen, FR-020).
- **Repeated maximize or fullscreen requests**, or a request to leave a state the window is not
  in: no change.
- **Requests from windows without focus**: see FR-019.
- **Requests before the window is shown**: a window asking to start maximized or fullscreen opens
  in that state (FR-019); it opens with focus, so its requests count, unless another window is
  fullscreen (FR-020).
- **Maximized and fullscreen together**: a maximized window that goes fullscreen returns to
  maximized when fullscreen ends, and to its normal position and size when maximize ends; each
  state keeps its own saved position and size.
- **Application ignores a maximize or fullscreen request** (never draws the new size): the
  window is shown at the size it draws, centred where the maximized or fullscreen window would
  be (FR-018).
- **Fullscreen window disappears** (closes, application disconnects): the view returns to where
  it was before fullscreen.
- **Moving or resizing a fullscreen window** (compositor gesture): moving leaves fullscreen, the
  view returns, and the window gets back its earlier size under the pointer as in "Dragging a
  maximized window"; the resize gesture leaves fullscreen and ends there.
- **Dragging a maximized window**: a move started on a maximized window first restores its
  previous size; the grabbed point keeps the same relative position across the window's width
  and stays under the pointer.
- **Compositor gestures on the background**: Mod + Shift + left-drag on the background pans like
  Mod + left-drag; Mod + right-drag on the background does nothing.
- **Viewer resized while a window is maximized or fullscreen**: the window is resized to the new
  viewer size.
- **Minimize requests**: still ignored (there is nothing to minimize to on the canvas yet).
- **Fullscreen window and other input**: while a window is fullscreen, panning and zooming
  gestures leave fullscreen first (FR-014); focus cycling leaves fullscreen
  and focuses the next window, the view going straight to that window (the restored view is
  never shown); the close shortcut closes the fullscreen window.
- **Two windows ask for fullscreen**: the later one wins; the earlier one leaves fullscreen
  first, so the view restored at the end is always the one from before any window was
  fullscreen.
- **New window while another is fullscreen**: see FR-020.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: When an application asks to start an interactive move while the user holds the
  left button pressed on its window, RubickWay MUST move that window with the pointer until the
  last button is released.
- **FR-002**: RubickWay MUST offer a compositor move gesture that works anywhere inside any
  window: Mod + Shift + left-drag (Mod + left-drag without Shift keeps panning). The application
  MUST NOT receive the input of the gesture, and the press does not count as a click for it.
- **FR-003**: During a move, the window's position MUST follow the pointer so the window point
  grabbed at the start stays under the pointer at any zoom (10% to 1000%); Mod + scroll and pinch
  MUST keep zooming the canvas around the pointer during a move. Other input during a move or
  resize is handled as listed in Edge Cases ("Other input during a move or resize").
- **FR-004**: Starting a move or resize MUST give the window keyboard focus and raise it, as a
  click does.
- **FR-005**: When an application asks to start an interactive resize from an edge or corner
  while the user holds a button pressed on its window, RubickWay MUST ask the application for
  sizes that follow the pointer until the last button is released.
- **FR-006**: RubickWay MUST offer a compositor resize gesture, Mod + right-drag (with or without
  Shift), that resizes
  any window (the application receiving none of its input) from the edge or corner nearest to where the gesture started (the window divided into a
  3 x 3 grid: corners resize both directions, edges one direction, the centre resizes from the
  nearest corner; a point on a boundary belongs to the cell further right or down, and a tie in
  the centre picks the bottom or right side).
- **FR-007**: During and after a resize, the edge or corner opposite to the dragged one MUST stay
  at the same canvas position, whatever size the application actually chooses; after the resize
  ends, size changes the application makes by itself keep the top-left corner fixed.
- **FR-008**: Requested sizes MUST respect the application's minimum and maximum size, be at
  least 1 x 1 and never exceed the largest window RubickWay can display. If these conflict, the
  displayable limit wins over the minimum, and the minimum wins over a smaller maximum.
- **FR-009**: While moving or resizing, the pointer MUST show a cursor matching the operation
  (move, or resize in the dragged direction).
- **FR-010**: RubickWay MUST provide a close shortcut that asks the application of the focused
  window to close it; the key press and its release are not forwarded. RubickWay MUST NOT force a
  window closed in this feature.
- **FR-011**: RubickWay MUST provide next-window and previous-window shortcuts that move keyboard
  focus through the windows in most-recently-used order and raise the newly focused window; the
  order is updated when Mod is released (or RubickWay loses the host's keyboard focus), so
  repeated presses while Mod is held walk further. Each window reached gets keyboard focus.
  "Most recently used" counts every focus change (click, new window, move or resize start,
  these shortcuts); a new window goes to the front.
- **FR-012**: When focus moves by these shortcuts to a window that is not fully inside the view,
  the view MUST pan to centre the window, keeping the zoom if the window fits in the view, else
  zooming out just enough to show it whole with a margin of 5% of the view on each side (the
  margin applies only when zooming out; never below 10% zoom). A window already fully inside the
  view is only focused and raised. A maximized window gets the maximize view (FR-013) instead.
- **FR-013**: A maximize request MUST move the view to the window at 100% zoom, centred on the
  window's centre, resize the window to the viewer's size in logical units (within its size
  limits), keep it centred there whatever size it draws (also when its maximum is smaller or
  its minimum larger than the viewer), and focus and raise it; leaving maximize or resizing a
  maximized window MUST restore the previous position and size (a resize then continues from
  that size), and moving it MUST restore the previous size with the window under the pointer
  (Edge Cases); the view stays where it is. Panning and zooming do not end maximize: a
  maximized window stays where it is, at its size, while the view moves.
- **FR-014**: A fullscreen request MUST show the window filling the whole viewer (in logical
  units) at 100% zoom with nothing else drawn, and give it keyboard focus; the viewer itself
  does not change (while nested, RubickWay's host window keeps its size and state); leaving
  fullscreen (by the application, by the user panning, zooming, moving, resizing or cycling
  focus, or by the window disappearing) MUST restore the window's previous position and size
  (for a window maximized before, its maximized state) and the previous view.
- **FR-015**: Applications MUST be told their current state (maximized, fullscreen, being
  resized, focused) so they can draw accordingly.
- **FR-016**: Move and resize requests that do not match a button press currently held on that
  window MUST be ignored without disconnecting the application. A resize request with an edge
  value that does not exist is a protocol error and disconnects the application.
- **FR-017**: The new shortcuts and gestures MUST NOT reach applications; all other keys and
  clicks keep going to applications as in client-windows. Existing behaviour (Mod + left-drag pans
  everywhere, Mod + scroll and pinch zoom, Mod+0 resets the view, Mod+Shift+Q quits) MUST stay.
- **FR-018**: RubickWay MUST NOT wait for any application during a move or resize (as
  client-windows FR-016); a window that has not drawn its new size yet is shown at its last
  drawn size.
- **FR-019**: Maximize and fullscreen requests (and requests to leave them) MUST be honoured only
  from the window with keyboard focus; others are ignored without disconnecting. Requests made
  before a window is first shown MUST be honoured when it opens with focus: the window's first
  configuration already carries the state; if both are requested, fullscreen wins and maximize
  is kept underneath.
- **FR-020**: When a new window opens while another window is fullscreen, fullscreen MUST stay:
  the new window opens without focus, on top, centred in the view that fullscreen will return
  to, and next in the focus order (so focus cycling reaches it first); its maximize and
  fullscreen requests count as from a window without focus. Exception: a window whose parent is
  the fullscreen window ends fullscreen first and then opens as usual, with focus.

### Key Entities

- **Window** (from client-windows): gains a state - normal, maximized, fullscreen, or fullscreen
  over maximized - with, per state, the position and size to return to, and its parent window
  (set by the application, for dialogs).
- **Interactive operation**: a move or a resize in progress: the window, which edges are dragged
  (for a resize), where the pointer and window were at the start; ends on the last button release.
- **Focus history**: the windows in most-recently-used order, used for focus cycling.
- **Saved view**: for fullscreen, the view to return to when it ends.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: During a move at any zoom from 10% to 1000%, the grabbed point of the window stays
  within 1 screen pixel of the pointer (checked automatically).
- **SC-002**: During and after a resize, the opposite edge or corner stays within 1 canvas unit
  of its starting position, whatever size the application answers with (checked automatically).
- **SC-003**: With five windows spread outside the view, each press of the next-window shortcut
  shows the next window fully in the view and focused, in one step.
- **SC-004**: Moving and resizing a kitty keep up with the display's refresh rate on the author's
  machine (manual check): during a 10-second drag in circles the window never visibly trails
  the pointer by more than one frame, and while resizing kitty redraws continuously at its own
  pace without the window jumping.
- **SC-005**: kitty's own title bar drag moves the window, its own border drag resizes it, its
  fullscreen toggle and title bar double click work, and the close shortcut closes it.
- **SC-006**: Every misbehaving request in Edge Cases is covered by an automated test that ends
  with RubickWay and other applications working: requests without a press or with an older
  press, invalid resize edges (only that application disconnected), contradictory size limits,
  windows disappearing mid-operation or while maximized or fullscreen, applications that never
  answer a size request, and maximize or fullscreen requests from windows without focus.

## Assumptions

- Mod is still Alt while nested, as in nested-canvas and client-windows.
- Default shortcuts (all with Mod; consumed, never forwarded): next window Mod+Tab, previous
  window Mod+Shift+Tab, close window Mod+Q (as niri). Quitting RubickWay stays Mod+Shift+Q.
- No visible window switcher list: focus cycling switches windows directly.
- Changes of the view caused by focus cycling, maximize and fullscreen are immediate jumps, not animations;
  animated view changes come later.
- No auto-panning when a window is dragged to the edge of the viewer; zooming out during the drag
  covers long distances.
- No snapping to other windows or to the grid, no keyboard-driven move or resize, no "always on
  top", no minimize.
- No forced close (kill) of unresponsive applications in this feature.
- RubickWay draws no decorations; the compositor gestures work on windows with or without their
  own title bars.
- The window layout is not saved between runs.
