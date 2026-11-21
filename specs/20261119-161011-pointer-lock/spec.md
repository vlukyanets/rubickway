# Feature Specification: Pointer Lock

**Feature Branch**: `worktree-pointer-lock`

**Created**: 2026-11-19

**Status**: Draft

**Input**: User description: "pointer-lock - relative pointer motion and pointer constraints for
applications, so games (also through gamescope), Blender, 3D viewers and remote desktop clients work.
Today the backend turns libinput motion into InputEvent::PointerMoved { delta } and the shell moves the
pointer across the canvas; applications only ever get absolute positions. Scope: (1) relative pointer
motion (zwp_relative_pointer_manager_v1): the application under the pointer (or holding it) gets each
motion's accelerated and unaccelerated deltas, also when the pointer cannot move further (screen edge,
lock); (2) pointer constraints (zwp_pointer_constraints_v1): lock - the pointer stays where it is and
the application gets only relative motion, with an optional position hint for where the pointer
reappears when the lock ends; confine - the pointer cannot leave a region of the window. Constraints
become active only while the window has keyboard focus and the pointer is over the region, as the
protocol says, and end (oneshot) or pause (persistent) when that stops. The canvas stays usable: Mod +
drag pans and Mod + scroll zooms even while locked or confined, and RubickWay's shortcuts keep
working; a dedicated shortcut always breaks a lock or confinement at once. Works on any viewer and at
any zoom: a confinement region is in window coordinates and follows the window on screen while the
user pans or zooms. Misbehaving clients (regions of huge size or count, constraints on surfaces they do
not own or that are not mapped, floods of requests, a second constraint on the same surface) must be
handled without stalling the event loop or affecting other applications. Out of scope: touchpad
gestures for applications (pointer-gestures protocol), tablets and touch, XWayland (no XWayland yet),
and a nested mode locking a host pointer (RubickWay runs only on the hardware now)."

## Clarifications

### Session 2026-11-19

- Q: Which combination always releases a captured pointer, and can the user change it? -> A:
  Mod+Escape, fixed like the other built-in shortcuts (FR-008). Making it reassignable in the
  configuration may be added later; see Assumptions.
- Q: After Mod+Escape, when does an application that asked for a lasting capture get it back? -> A:
  On the next click inside its window, any button (FR-008); a focus change and return alone does not
  bring it back after the shortcut.
- Q: At a canvas zoom other than 100%, are movement reports scaled by the zoom? -> A: The
  accelerated movement is, in the window's coordinates like the cursor position over it (as the
  protocol defines); the raw movement is not (FR-003).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Look Around in a Game or a 3D View (Priority: P1)

The user plays a first-person game, or orbits a model in Blender or another 3D viewer, by moving the
mouse. The application captures the pointer: the cursor disappears and stays put, and every movement
of the mouse turns the view, without end - the cursor never reaches a screen edge that would stop the
turn. When the user is done, the application releases the pointer, or the user takes it back with a
RubickWay shortcut that always works, whatever the application does. Today applications only ever
learn where the cursor is, so a game's view stops turning when the cursor hits a screen edge.

**Why this priority**: mouse-look is what games, 3D tools and remote desktops need first; without it
they cannot be used on RubickWay. The shortcut that takes the pointer back belongs to the same slice:
a capture the user cannot always escape would make the desktop unusable.

**Independent Test**: start a first-person game (natively or through gamescope) or a browser page that
uses pointer lock, click into it, and turn in one direction for many full turns: the view keeps
turning. Press the release shortcut: the cursor reappears and moves freely. Click into the game again:
mouse-look resumes. In Blender, drag a value or orbit the view: the drag continues past the screen
edge.

**Acceptance Scenarios**:

1. **Given** an application with the keyboard focus asks to lock the pointer while the pointer is over
   its window, **When** the user moves the mouse, **Then** the cursor stays where it was and the
   application is told each movement, also movements that would have taken the cursor past a screen
   edge.
2. **Given** the pointer is locked, **When** the application releases it, **Then** the cursor
   reappears at the point the application asked for, or where it was locked if the application did
   not ask for one or asked for a point outside the lock's area as shown on the cursor's monitor
   (FR-007).
3. **Given** the pointer is locked, **When** the user presses the release shortcut, **Then** the lock
   ends at once, the shortcut does not reach the application, and the cursor moves freely.
4. **Given** the user released a lock with the shortcut, **When** the user clicks inside the window
   (a click the window receives, not a Mod + click that pans), **Then** an application that asked for
   a lasting lock gets it again; one that asked for a single lock does not, until it asks again.
5. **Given** the pointer is locked, **When** the keyboard focus moves to another window (Mod+Tab, a
   new window opening, the window closing), **Then** the lock ends or pauses, and the cursor moves
   freely.
6. **Given** a lasting lock paused because the focus moved away, **When** the focus comes back while
   the pointer is over the window, **Then** the lock is active again without a click.
7. **Given** the pointer is not locked, **When** the user moves the mouse over a window whose
   application asked for movement reports, **Then** it gets each movement in addition to the cursor
   position, including movements the cursor could not follow at a screen edge.
8. **Given** an application without the keyboard focus asks to lock the pointer, **Then** nothing
   happens until it has the focus and the pointer is over it.

---

### User Story 2 - Keep the Pointer Inside a Window (Priority: P2)

The user plays a strategy game that scrolls its map when the cursor touches the window edge, uses a
remote desktop client, or drags in an application that must not lose the cursor. The application
confines the pointer to its window, or to a part of it: the cursor moves normally but cannot leave
that area until the confinement ends.

**Why this priority**: edge scrolling in strategy games and remote desktops need it, but fewer
applications than mouse-look (US1).

**Independent Test**: start a strategy game in a window or a test client that confines the pointer;
flick the mouse hard toward every edge: the cursor stops at the edge of the area each time. Press the
release shortcut: the cursor leaves the window freely.

**Acceptance Scenarios**:

1. **Given** an application with the keyboard focus confines the pointer to its window while the
   pointer is over it, **When** the user moves the mouse toward any edge, however fast, **Then** the
   cursor stops at the edge of the area and the application is still told the full movement.
2. **Given** the confinement area is only part of the window (a rectangle or several), **When** the
   user moves the cursor, **Then** it stays inside that area.
3. **Given** a confinement is active, **When** the application changes the area, **Then** the new
   area applies from the window's next update, and a cursor left outside the new area is moved to
   the nearest point inside it.
4. **Given** an application asks for a confinement while the cursor is outside the area, **Then** it
   becomes active only once the user moves the cursor into the area.
5. **Given** a confinement is active, **When** the user presses the release shortcut or the focus
   moves away, **Then** the cursor can leave the window, as for a lock (US1).

---

### User Story 3 - The Canvas Stays in the User's Hands (Priority: P3)

While a game or a 3D tool holds the pointer, the user still navigates the canvas: Mod + drag pans,
Mod + scroll zooms, Mod+Shift + drag moves the window, and all RubickWay shortcuts work. The captured
pointer stays with its window while the view changes, on any monitor and at any zoom.

**Why this priority**: RubickWay's canvas is the point of the desktop; a captured pointer must not
take it away. The basic capture (US1, US2) is useful even before this is polished.

**Independent Test**: lock the pointer in a game window, Mod + drag: the canvas pans and the game gets
no movement; release Mod: the cursor is on the same point of the game window and mouse-look resumes.
Mod + scroll zooms around the locked point. With a confined window at 50% zoom on the second monitor,
the cursor stays inside the window's area as it is shown there.

**Acceptance Scenarios**:

1. **Given** the pointer is locked or confined, **When** the user pans with Mod + drag, **Then** the
   canvas pans as usual, the application is told no movement during the pan, and once the pan ends
   the cursor is on the same point of the window as before and the lock or confinement applies again.
2. **Given** the pointer is locked or confined, **When** the user zooms with Mod + scroll or a pinch,
   **Then** the canvas zooms around the cursor and the lock or confinement keeps applying to the
   window as it is now shown.
3. **Given** the pointer is locked or confined in a window, **When** the user moves that window with
   Mod+Shift + drag, **Then** the window moves with the cursor and the lock or confinement applies
   again once the move ends.
4. **Given** the pointer is confined, **When** the user resizes the window with Mod + right-drag,
   **Then** the resize works; afterwards the confinement applies again once the cursor is inside the
   area.
5. **Given** a confined window at any zoom on any monitor, **When** the user moves the cursor, **Then**
   it stays inside the area as shown on the monitor the cursor is on, and does not cross to another
   monitor.
6. **Given** the pointer is locked or confined, **When** the user presses any RubickWay shortcut
   (Mod+Tab, Mod+Q, Mod+0, a spawn binding, quit, console switch), **Then** it works as usual.

---

### Edge Cases

- The application asks for a second lock or confinement on the same window while one exists: that is
  a protocol error for that application only (FR-010).
- The window is closed or destroyed while it holds the pointer: the lock or confinement ends and the
  cursor moves freely.
- The window is hidden while it holds the pointer, then shown again: hiding stops it (a single-use
  one ends); a lasting one applies again once the window is shown, has the focus and the cursor is
  over its area.
- The view jumps under a still cursor while a lock or confinement applies (Mod+0, a monitor's view
  changing): if the cursor is no longer over the area, it stops ("cursor left"); a lasting one
  applies again when the cursor is over the area again. Unlike a pan, nothing keeps the cursor on the
  same point of the window.
- The window is visible on two monitors at once: a lock holds the cursor where it is, on its monitor;
  a confinement keeps it inside the area as shown on the monitor the cursor is on (FR-013).
- A popup of the same window (a tooltip, a menu without a grab) lies under the cursor: the cursor
  still counts as over the area when its position in the window's coordinates is inside the area;
  the popup does not stop the lock or confinement.
- The window is panned or zoomed fully off the monitor the cursor is on: a confinement cannot hold the
  cursor and pauses; it applies again once the cursor is inside the area.
- The monitor with a locked cursor is unplugged: the cursor moves to another monitor and the lock
  ends or pauses.
- A switch to a text console and back: any lock or confinement pauses; a lasting one applies again on
  return when its conditions hold.
- The application's popup takes the keyboard (a menu with a grab): the user must be able to use the
  menu, so the lock or confinement stops as for a focus change: a single-use one ends, a lasting one
  pauses until the popup closes.
- The application asks for a release point outside the lock's area as shown on the cursor's monitor,
  or never asks: the cursor appears where it was locked (FR-007).
- An empty confinement area: the pointer can never be inside it, so the confinement never applies.
- The application hides the cursor while locked (usual for games): RubickWay shows nothing; once the
  lock ends, the application chooses the cursor again - except after Mod+Escape, when the cursor
  shows over the window until the user clicks it (FR-008). During a pan, a move or a resize RubickWay shows
  its own cursor for that gesture (grabbing, move, resize), as it does today, and the hidden cursor
  comes back when the gesture ends.
- The window goes maximized or fullscreen, or leaves it, while holding the pointer: the cursor stays
  where it is on screen; if it is still over the area of the window at its new size and place, the
  lock or confinement keeps applying (a lock then holds it at the window point now under it);
  otherwise it stops ("cursor left").
- The release shortcut is pressed while nothing holds the pointer: nothing happens and the key does
  not reach the application.

### Misbehaving Clients *(Principle II)*

Each case matches a row of contracts/protocols.md (M1-M11).

- A second lock or confinement for the same window: protocol error to that application only (FR-010,
  M1). A new one after a single-use one ended is allowed (M2).
- A lock or confinement on a surface that is not a window (a popup, a cursor image, a sub-surface):
  accepted, but never applies (FR-011, M3). On a window not shown yet: applies only once it is shown
  (M4).
- The surface destroyed while a constraint exists: it stops applying, and later requests on it are
  ignored (M5).
- A confinement area or window input shape of a huge number of rectangles: no stall; beyond 256
  rectangles the confinement uses their bounding rectangle, so a hostile application only loosens its
  own confinement (FR-012, M6).
- Areas and release points at extreme coordinates: no arithmetic overflow; a release point outside
  the area is ignored (FR-007, FR-012, M7).
- A flood of requests (creating and destroying constraints, changing areas and release points): each
  costs bounded work, and other applications' input is not delayed (FR-012, SC-005, M8).
- Thousands of movement subscriptions from one application: only its first 32 are told movements, so
  one movement costs at most 32 reports (FR-012, M9).
- An application that locks the pointer and never releases it, or asks again at once after every
  release: the release shortcut and a focus change always free the cursor; after the shortcut, the
  lock applies again only after the user clicks the window (FR-008, M11).
- An application that does not read its events while it is told every movement: its own buffer fills
  and only it is disconnected, as for any other event (FR-012, M10).
- An application referring to another application's window: impossible in the protocol (each
  application can only name its own objects); nothing to handle.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001 Movement reports**: an application that asks for movement reports MUST be told every
  pointer movement while one of its windows has the pointer (the cursor is over it, or a button
  pressed on it is held), with the movement as the cursor would make it (accelerated) and the raw
  movement of the device (unaccelerated), also when the cursor cannot follow it (a screen edge, a
  lock, a confinement edge).
- **FR-002 No reports elsewhere**: no application is told movements made during a canvas pan, a
  window move or resize, or while the cursor is over the background or a RubickWay-drawn part (title
  bar, border).
- **FR-003 Zoom**: the accelerated movement MUST be in the window's own coordinates, scaled by the
  zoom as the cursor's position over the window is; the raw movement MUST NOT depend on the zoom.
- **FR-004 Lock**: while a lock is active, the cursor MUST stay on the same point of the window, the
  window MUST get no position changes, only movement reports (FR-001), and buttons and scrolling MUST
  still reach it.
- **FR-005 Confinement**: while a confinement is active, the cursor MUST NOT leave its area (the
  application's area, or the whole window when it gives none, within the window's input area); a
  movement that would take it out MUST stop it at the area's edge.
- **FR-006 When a constraint applies**: a lock or confinement MUST apply only while its window has the
  keyboard focus (and no grabbing popup of it holds the keyboard) and the cursor is over its area, and
  MUST stop applying as soon as either is no longer true. "Over its area" means the cursor's position
  in the window's coordinates is inside the area, whether or not a popup of the same window is drawn
  on top. A single-use one MUST then end for good; a lasting one MUST pause and apply again when both
  are true again.
- **FR-007 Release point**: when a lock stops applying, including when the application removes it,
  the cursor MUST appear at the release point the application last gave, if it is inside the lock's
  area as shown on the monitor the cursor is on; otherwise where it was locked. A lock that stops
  because the cursor is no longer over its area, its window went away, or a resize left the cursor
  outside does not move the cursor (found while implementing, 2026-11-20: moving it back into the
  area would start a lasting lock again at once).
- **FR-008 Release shortcut**: Mod+Escape MUST end any active lock or confinement at once, MUST NOT
  reach the application, and MUST do nothing else. A lasting lock or confinement released this way
  MUST NOT apply again until its window receives a button press (a press taken by a RubickWay pointer
  binding, such as Mod + click, does not count); nor may a new one the application asks for on that
  window in the meantime (found while implementing, 2026-11-20: otherwise an application asking
  again at once would capture the pointer right back, SC-002, M11). Until that button press the
  cursor MUST stay visible over that window, even if the application hides it (found in the manual
  checks, 2026-11-20: Firefox ignores the end of its lasting lock and keeps the cursor hidden).
- **FR-009 Canvas navigation**: every RubickWay shortcut and pointer binding MUST keep working while a
  lock or confinement applies. A pan, a window move or a resize MUST suspend it without telling the
  application - from its side the lock or confinement still applies - and the application MUST NOT
  be told movements during it. When the pan or move ends, the cursor MUST be on the same point of the
  window as before and the lock or confinement applies on; when a resize ends, it applies on only if
  the cursor is still over the area, otherwise it stops under FR-006. A zoom MUST NOT suspend it: the
  zoom keeps the point under the cursor, and the lock or confinement keeps applying to the window as
  now shown.
- **FR-010 One per window**: a second lock or confinement for a window that already has one MUST be
  answered with the protocol's error, to that application only.
- **FR-011 Windows only**: a lock or confinement MUST apply only to an application's window, and only
  once it is shown; on any other surface it MUST be accepted and never apply.
- **FR-012 Robustness**: handling one pointer movement, one request or one window update MUST take
  bounded time whatever areas, release points and subscriptions applications made: one movement
  walks at most 256 rectangles of a confinement area (beyond that its bounding rectangle is used) and
  sends at most 32 movement reports. Area coordinates MUST be handled without overflow; a misbehaving
  application MUST affect only itself.
- **FR-013 Monitors and zoom**: the area MUST follow the window on screen as the user pans and zooms,
  on any monitor; a locked cursor stays where it is and a confined cursor MUST stay on the monitor it
  is on.
- **FR-014 Changes**: changes to an area or release point MUST take effect with the window's next
  update, as the protocol requires; a cursor outside a changed confinement area MUST move to the
  nearest point inside it.
- **FR-015 Debug log**: when a lock or confinement starts or stops applying, RubickWay MUST write a
  debug log line naming the application, the kind (lock or confinement), its lifetime (single-use or
  lasting), and why it stopped (released by the application, the release shortcut, focus, cursor
  left, window gone, a resize that left the cursor outside).
- **FR-016 Documentation**: the README MUST describe pointer capture and the release shortcut in its
  controls table.

### Key Entities

- **Pointer constraint**: a request by an application to lock or confine the pointer for one of its
  windows. It has a kind (lock or confinement), a lifetime (single-use or lasting), an area (a set of
  rectangles in window coordinates, or the whole window), for a lock an optional release point, and a
  state: waiting (its conditions do not hold), applying, released by the user (waits for a click),
  or ended (single-use ones only).
- **Movement subscription**: an application's request to be told every pointer movement while its
  window has the pointer.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a first-person game with the pointer locked, the user can turn in one direction for at
  least 10 full turns without the view stopping.
- **SC-002**: The release shortcut frees the cursor on its first press in every case, whatever the
  locking application does, including one that asks for the lock again at once.
- **SC-003**: With a confined window, 100 fast flicks toward the edges and corners leave the cursor
  inside the area 100 times, at zoom 50%, 100% and 200%.
- **SC-004**: After a pan, a zoom or a window move while the pointer is locked, the cursor is on the
  same point of the window within 1 logical pixel of the screen, at zoom 50%, 100% and 200%.
- **SC-005**: When one application, in a single batch of requests, creates and destroys 10,000
  constraints, sets an area of 10,000 rectangles and creates 10,000 movement subscriptions, another
  application's input sent in the same step still reaches it in that step (a headless test); on the
  laptop, such an application causes no "lagging behind" line in the log.
- **SC-006**: Every pointer movement while a subscribing window has the pointer produces exactly one
  movement report, including at screen edges.

## Assumptions

- The release shortcut is Mod+Escape, fixed like the other built-in shortcuts (Mod+Tab, Mod+0); a
  configured binding cannot take it. Possible later change (owner, 2026-11-19): let the configuration
  reassign it, once there is a need; the built-in shortcuts are not configurable today. No on-screen
  notice is shown when an application captures the pointer (RubickWay has no notices yet); the README
  documents the shortcut. Possible later change: a short notice naming Mod+Escape when a capture
  starts, once RubickWay can show notices.
- After the release shortcut, a lasting capture applies again on the next click inside the window, the
  usual "click to capture" of games and browsers, and a focus change and return alone does not bring
  it back (FR-008). A capture paused only by a focus change (no shortcut) applies again without a
  click when the focus returns (FR-006).
- An application can capture the pointer only while it has the keyboard focus, so a background
  application cannot take it.
- Movement reports follow the window's zoom for the accelerated movement (so they match the cursor's
  movement over the window) and not for the raw one; games at 100% zoom (maximized or fullscreen) see
  ordinary values.
- Constraints apply to windows (the application's top-level surfaces) only; popups, sub-surfaces and
  cursor images do not hold the pointer. Games and 3D tools capture on their main window.
- One seat and one pointer; touchpad movement counts as pointer movement.
- Applications cannot move the cursor except through the release point of a lock (FR-007) and by
  shrinking an active confinement area (FR-014); there is no other warp.
- gamescope needs no XWayland in RubickWay: it runs its own XWayland inside and connects to RubickWay
  as an ordinary Wayland client using these same protocols (checked by quickstart Q6). Found in the
  manual checks (2026-11-22): gamescope 3.17 does not start in RubickWay at all, with its Wayland or
  its SDL backend - it reports the missing `wp_presentation` (presentation-time) and gives up with
  "Failed to create backend" before any pointer capture. Supporting gamescope is a separate change;
  Q6 is skipped for this feature.
- Out of scope: touchpad gestures for applications, tablets and touch, applications blocking
  RubickWay's shortcuts (keyboard shortcut inhibit), XWayland, a nested mode.
- Depends on keyboard focus, pointer input and the multi-monitor canvas already on master; no new
  configuration options.
