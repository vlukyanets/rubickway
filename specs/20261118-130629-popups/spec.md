# Feature Specification: Popups

**Feature Branch**: `worktree-popups`

**Created**: 2026-11-18

**Status**: Draft

**Input**: User description: "popups - show xdg_popup surfaces instead of dismissing them at once
(today src/server/xdg_shell.rs dismisses every popup with popup_done). Applications' context menus,
dropdown lists, combo boxes, nested submenus and tooltips (Firefox, GTK, Qt, foot) must appear next to
their parent window on the canvas and work with pointer and keyboard. Placement follows xdg_positioner
(anchor rect, anchor, gravity, offset, constraint adjustment: slide, flip, resize) in canvas
coordinates, kept inside the visible area of the viewer the user is looking through, while the canvas
camera can be panned and zoomed. Popups with an explicit grab take pointer and keyboard input and are
dismissed (popup_done, topmost first) on a click outside the popup chain, on Escape-like compositor
actions, when the parent window goes away, or when the user pans/zooms the canvas or moves the parent.
Nested popups stack; reposition requests are supported. Out of scope: clipboard and drag-and-drop (a
separate later feature), layer-shell popups."

## Clarifications

### Session 2026-11-18

- Q: At a zoom other than 1, is a popup drawn at the window's zoom or always at its natural size? ->
  A: At the window's zoom, like the rest of the window: a popup is part of the canvas, so it lines up
  with the window's items and its submenus open right beside them.
- Q: While a menu is open, does a click on another part of the same application's window go to the
  application, or does RubickWay close the menu? -> A: It goes to the application, which closes its
  menu itself, as the protocol's grab says and as on GNOME and KDE; only a press outside every surface
  of the application is swallowed (analyze finding I1).
- Q: While a menu is open, does a RubickWay pointer gesture with Mod (move, resize, pan) act at once,
  or is its press swallowed like a press outside? -> A: It acts at once, after all popups close; only
  presses without a binding count as a press outside (analyze finding A1).
- Q: Do popups close when the application itself enters or leaves maximize or fullscreen? -> A: Yes:
  any change of maximize or fullscreen closes all popups, whoever asked for it (analyze finding I4).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Menus Open and Can Be Used (Priority: P1)

The user right-clicks in an application window, or clicks a menu button or a dropdown list. The menu
appears next to the spot that opened it, on top of the window, and the user picks an item with the
pointer or the keyboard (arrow keys, Enter, Escape). Clicking anywhere outside the menu closes it:
RubickWay closes it for a click outside the application, and the application itself for a click on
its own window.
Today every menu is closed by RubickWay the moment the application asks for it, so the user sees
nothing.

**Why this priority**: This is the reason for the feature. Without menus, most applications cannot be
used fully: no context menu in a browser or file manager, no dropdown in a settings dialog, no menu bar
in GTK and Qt applications.

**Independent Test**: Open a GTK application and Firefox. Right-click in each, pick an item with the
pointer; open the menu again and pick an item with the arrow keys and Enter; open it again and close it
with Escape, and once more by clicking the canvas background.

**Acceptance Scenarios**:

1. **Given** an application window, **When** the user right-clicks inside it, **Then** the application's
   context menu appears at the pointer, above the window, within the next frame after the application
   draws it.
2. **Given** an open menu, **When** the user moves the pointer over its items, **Then** the item under
   the pointer is highlighted (the application gets the pointer), and a click on an item does what the
   item says and closes the menu.
3. **Given** an open menu, **When** the user presses the arrow keys, Enter or Escape, **Then** the menu
   gets the keys, not the window under it, and acts on them.
4. **Given** an open menu, **When** the user clicks outside the application (on the canvas background
   or another application's window), **Then** every open menu of that application closes, the most
   recently opened first, and the click goes nowhere else. **When** the user clicks a part of the same
   application's window outside every open menu, **Then** the application gets the click and closes
   its menus itself.
5. **Given** a dropdown list (combo box) in a dialog, **When** the user clicks it, **Then** the list
   appears below or over the box, as the application asks, and choosing an item sets it.
6. **Given** a menu bar in a GTK or Qt application, **When** the user opens one menu and moves the
   pointer along the bar, **Then** the next menu opens in its place, as on other desktops.

---

### User Story 2 - Submenus and Menus at the Edge of the View (Priority: P2)

The user opens a menu with submenus, opens a submenu, then a submenu of that one. Each opens beside its
parent item and stays visible. When a menu would not fit inside the monitor the user is looking at (the
window is near the edge of the view, or the menu is long), it opens on the other side, is shifted back
into view, or is made shorter, as the application allows.

**Why this priority**: Nested menus are common (Firefox, file managers, editors), and a menu that opens
off-screen cannot be used. It comes after US1 because it builds on single menus.

**Independent Test**: Place a window so its right edge is near the right edge of the monitor. Open a
context menu near that edge and then a submenu: the submenu opens to the left of the menu, fully on the
monitor. Do the same near the bottom edge with a long menu.

**Acceptance Scenarios**:

1. **Given** an open menu with a submenu item, **When** the user hovers or selects that item, **Then**
   the submenu appears beside it, above the menu, and both stay open.
2. **Given** three nested menus open, **When** the user clicks outside all of them, **Then** all three
   close, the innermost first.
3. **Given** a window near the right edge of the monitor, **When** a submenu would extend past that
   edge, **Then** it is placed as the application allows: flipped to the left side, slid back inside
   the monitor, or made narrower; it is never left partly outside the monitor when the application
   allows an adjustment that keeps it inside.
4. **Given** a menu taller than the monitor and an application that allows resizing, **When** it opens,
   **Then** it is made as tall as the monitor allows, and the application can scroll it.
5. **Given** an application that allows no adjustment, **When** its menu does not fit, **Then** the
   menu is placed exactly where the application asked, even if partly outside the monitor.
6. **Given** a window that is zoomed in or out on the canvas, **When** a menu opens, **Then** it is
   placed to fit the part of the canvas the monitor shows at that zoom, and it is drawn at the
   window's zoom, like the rest of the window (FR-019).

---

### User Story 3 - Tooltips and Other Popups Without a Grab (Priority: P3)

The user rests the pointer on a button and its tooltip appears next to it; the application can move it,
change it, or hide it. Popups that do not take the pointer and keyboard (tooltips, some search
suggestion lists, some panels) appear and go away as the application decides, while the window under
them keeps the keyboard.

**Why this priority**: Tooltips make applications easier to learn but are not needed to use them.

**Independent Test**: Hover over a toolbar button in a GTK application until its tooltip appears, then
move to the next button: the tooltip moves or changes; move off the toolbar: it goes away. Type in
Firefox's address bar: the suggestion list appears and typing continues into the address bar.

**Acceptance Scenarios**:

1. **Given** a toolbar button with a tooltip, **When** the pointer rests on it, **Then** the tooltip is
   shown next to it, above the window, and typing still goes to the window.
2. **Given** a shown tooltip, **When** the application moves it to another spot (repositions it),
   **Then** it is shown at the new spot, placed by the same rules as a new popup.
3. **Given** a popup without a grab, **When** the user clicks elsewhere in the same window, **Then**
   RubickWay does not close it; the application decides when it goes away. (A click on another window
   moves keyboard focus, which closes all popups by FR-010.)

---

### User Story 4 - Canvas Actions Close Popups Cleanly (Priority: P2)

While a menu is open, the user pans or zooms the canvas, uses a RubickWay shortcut (cycling focus,
closing a window, starting a program), starts moving or resizing a window with a RubickWay gesture, or
the window that owns the menu goes away. In every case the menus close cleanly, the application is told
so, and nothing is left floating on the canvas.

**Why this priority**: RubickWay's canvas actions must keep working while applications show menus, and
a menu left behind on the canvas, detached from its window, would be confusing.

**Independent Test**: Open a context menu, then press Mod+Tab: the menu closes and focus moves. Open it
again and Mod+scroll to zoom: the menu closes and the zoom happens. Open it again and close the
application from a terminal: the menu goes away with the window.

**Acceptance Scenarios**:

1. **Given** an open menu, **When** the user pans or zooms any viewer or resets a view, **Then** all
   open popups close first, and the pan or zoom happens as usual.
2. **Given** an open menu, **When** the user runs any RubickWay shortcut or starts a RubickWay pointer
   gesture (move, resize, pan), **Then** all open popups close, and the shortcut or gesture does what
   it does today, at once (a gesture's press is not swallowed).
3. **Given** an open menu, **When** its window is closed, unmapped, or its application quits, **Then**
   the menu disappears with it.
4. **Given** an open menu, **When** the window under it becomes maximized, fullscreen, or leaves those
   states, by a RubickWay action or at the application's request, **Then** the menu closes.
5. **Given** an open menu, **When** the user presses Escape, **Then** the application gets the key and
   closes the menu itself, as on other desktops (RubickWay does not treat Escape specially).

### Edge Cases

- A menu opens on a window that two monitors show: it is shown on both, as part of the window, and its
  placement fits the monitor the pointer is on; if the pointer is on neither, the monitor that shows
  the largest part of the window.
- A menu opens from the keyboard (for example the menu key) while the pointer is on a monitor that does
  not show the window: placement fits the monitor that shows the largest part of the window.
- The window that owns a menu is not visible on any monitor (it was opened by a keyboard shortcut while
  the window was off-screen): the menu is placed where the application asked, with no adjustment.
- The space between the window and the monitor edge is smaller than the menu on both sides: flipping
  does not help, so the allowed adjustments go on in FR-003's order (slide, then resize); what is left
  outside after them stays outside.
- A menu with a grab opens while another application's menu is open: the first application's menus
  close first.
- A tooltip is shown and the user opens a menu in the same application: both may be shown; the menu
  takes the keyboard.
- A submenu's parent menu is closed by the application while the submenu is open: the application must
  close the submenu first (Misbehaving Clients).
- An application opens a menu while a RubickWay gesture is running: the menu is closed at once, since
  the application does not have the pointer and cannot have asked in response to the user.
- The window owning a menu moves because the application resized it from the top or left edge: the
  menu stays at the same spot relative to the window; a popup the application marked as reactive is
  placed again by the rules.
- Many menus opened and closed quickly (holding the right button and moving, scrolling through a menu
  bar): no slow frames and no leftover menus.
- A popup without a grab on a window that is then covered by another window: it stays with its window,
  so it may be covered too.
- A menu is open when its monitor is unplugged: it closes.

### Misbehaving Clients (Principle II)

- A popup made with a positioner that has no size or no anchor rectangle: protocol error to that
  application (as today).
- A popup whose parent is missing (no parent given and no other way to set one): closed at once, the
  application is told, no error.
- A grab asked for after the popup's first commit: protocol error to that application. A grabbing
  popup shown while its parent is not the topmost grabbing popup of the chain: protocol error to that
  application.
- A grab asked for with a serial that is not from a recent click or key press the application got:
  the popup is closed at once (the application is told), no error.
- A child popup with a grab on a parent popup without one: protocol error to that application (the
  protocol requires it). A child popup with a grab on a parent grabbing popup that was already closed:
  closed at once, no error.
- A popup destroyed while it still has child popups open: protocol error to that application.
- A popup's surface destroyed before the popup: the popup disappears (its children are closed first),
  as windows do today; no error.
- A popup hidden by its application (a commit without content) that then draws again: it stays
  closed; hiding a popup ends it, and the application is told so.
- A popup that never draws, or draws before acknowledging its placement: handled as for windows today
  (not shown; protocol error where the protocol says so).
- Positioner values that are negative, zero or huge (size, anchor rectangle, offset, parent size):
  protocol error where the protocol says so; otherwise every computation saturates, and nothing outside
  that application is affected.
- A flood of popups, repositions or grab requests: handled without a stall; at most one redraw per
  frame.
- An application that disconnects with menus open: they disappear; the keyboard and pointer go back to
  what is under the pointer and the focused window.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: RubickWay MUST show applications' popups (menus, dropdown lists, submenus, tooltips)
  through the standard Wayland desktop shell protocol, at the version it advertises today, instead of
  closing them at once.
- **FR-002**: A popup MUST be placed relative to its parent (a window or another popup) by the rules
  the application gives: the anchor rectangle on the parent, the anchor edge or corner, the direction
  it grows, and the offset. Its position MUST be told to the application before it is shown.
- **FR-003**: When the placed popup would extend outside the area of the canvas the chosen monitor
  shows (Edge Cases), RubickWay MUST apply the adjustments the application allows, per axis, in the
  protocol's order: flip, then slide, then resize. Without any allowed adjustment the popup MUST be
  placed exactly as asked.
- **FR-004**: A popup MUST be drawn above its parent and all of its parent's other surfaces, with child
  popups above their parents and later popups above earlier ones; the whole group keeps its window's
  place among the other windows.
- **FR-005**: A popup MUST move with its window: when the window's position on the canvas changes
  without a RubickWay action (the application resized it from the top or left edge), the popup keeps
  its position relative to the window. A popup marked as reactive MUST be placed again by FR-002 and
  FR-003 when its parent's size or position changes, and the application told the new placement.
- **FR-006**: RubickWay MUST support moving a shown popup on the application's request (reposition):
  placed again by FR-002 and FR-003, the application told which request the new placement answers.
- **FR-007**: A popup with a grab MUST be accepted only when the application asks for it in response
  to a click or key it got (its serial is from that application's latest button press or latest key
  press or release),
  and only before the popup's first commit; otherwise it is closed at once (FR-012) or, where the
  protocol says, a protocol error.
- **FR-008**: While an application has a popup with a grab: the keyboard MUST go to its topmost
  grabbing popup; the pointer MUST go to whatever surface of that application is under it (windows and
  popups), and to nothing of other applications.
- **FR-009**: A button press without a RubickWay binding while a grab is active, outside every surface
  (windows and popups) of the grabbing application, MUST close all of that application's popups; that
  press and its release MUST NOT be delivered to anything else. A press on a surface of the grabbing
  application is delivered as usual (FR-008).
- **FR-010**: All open popups (of every application) MUST close before RubickWay acts on: a pan, zoom
  or view reset of any viewer; any RubickWay shortcut; the start of a RubickWay pointer gesture (which
  then starts at once); a change of keyboard focus to another window; any window entering or leaving
  maximize or fullscreen, whoever asked; the removal of a monitor.
- **FR-011**: When a window is closed, unmapped or its application quits, all its popups MUST
  disappear with it; when a popup is hidden or destroyed by its application, its child popups MUST be
  closed first, and a hidden popup MUST NOT be shown again.
- **FR-012**: Closing popups MUST tell each application which popups closed, the topmost first, and
  MUST NOT show a closed popup again, even if the application keeps drawing it.
- **FR-013**: After a grab ends (FR-009 to FR-012), the keyboard MUST go back to the window that had it,
  and the pointer to whatever is under it, with the usual enter and leave notifications.
- **FR-014**: Popups without a grab MUST NOT take the keyboard, and MUST NOT be closed by clicks
  elsewhere; they close when the application closes them, when their window goes away (FR-011), or by
  FR-010.
- **FR-015**: A popup on a window shown by two or more monitors MUST be shown on every monitor that
  shows its area, with the same placement.
- **FR-016**: Opening, moving and closing popups MUST NOT cause more than one redraw per display frame,
  and MUST redraw only the displays that show the change, as for windows today.
- **FR-017**: Pointer shapes named by an application (cursor-shapes) MUST work over its popups as over
  its windows.
- **FR-018**: Each misbehaving-client case listed above MUST be handled as stated, affecting only the
  offending application.
- **FR-019**: A popup MUST be part of the canvas like its window: drawn at the zoom of each monitor
  that shows it, with one canvas unit per application pixel, exactly as its window is; there is no
  separate size for popups.

### Key Entities

- **Popup**: a short-lived surface of an application attached to a parent (a window or another popup):
  a menu, list, submenu or tooltip. Has a placement, maybe a grab, and its child popups.
- **Placement rules**: what the application asks for: the size, the anchor rectangle on the parent, the
  anchor, the direction to grow, the offset, the allowed adjustments, and whether it is reactive.
- **Popup chain**: the open popups of one application from the window up to the topmost one; a grab
  belongs to the chain, and closing works from the top of it down.
- **Grab**: a chain's claim on the keyboard and pointer, from a user's click or key press, until it
  ends (FR-009 to FR-012).
- **Constraint area**: the part of the canvas the chosen monitor shows, which a popup is kept inside
  (FR-003).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a GTK 4 application, a Qt 6 application and Firefox, a right-click menu opens, an item
  is picked with the pointer, and another with the keyboard, in 10 out of 10 tries each.
- **SC-002**: A three-level submenu opened next to the right edge of the monitor is fully visible each
  time, in 10 out of 10 tries, for each of the applications in SC-001.
- **SC-003**: Opening and closing a context menu 100 times in a row, at debug log level, leaves no
  slow-frame line from RubickWay, no input lag line from libinput, and no menu left on the canvas.
- **SC-004**: After any of the FR-010 actions with a menu open, no menu remains visible and the action
  works as it does without a menu, in 10 out of 10 tries for each action.
- **SC-005**: Placement matches the protocol's rules for every combination of anchor, direction and
  allowed adjustment covered by the automated tests, at zoom 1 and at other zooms.
- **SC-006**: For each misbehaving-client case, only the offending application is affected; other
  applications and RubickWay keep running.

## Assumptions

- Applications draw and position their own menus through the standard desktop shell protocol; GTK 3
  and 4, Qt 5 and 6, Firefox and foot are expected to, which the quickstart checks by hand.
- Escape, arrow keys and Enter in a menu are the application's business; RubickWay only routes the keys
  (US4 scenario 5).
- The click that closes menus (FR-009) is not passed on, as on GNOME and KDE Plasma (compositors built
  on smithay, such as niri, pass it on); a second click is needed to act on what was under it.
  Confirmed by the owner 2026-11-18.
- Closing all popups before any RubickWay action (FR-010) matches other desktops, where a workspace
  switch, an overview or a compositor shortcut closes open menus; pan and zoom are RubickWay's
  counterpart of a workspace switch. Confirmed by the owner 2026-11-18.
- Popups of panels, bars and launchers (layer-shell), the clipboard and drag-and-drop, and touch input
  for menus are out of scope.
- Popups are placed only at their creation and on reposition or reactive changes; they do not follow
  the camera, since pan and zoom close them (FR-010).
- The automated tests cover placement, stacking, grabs and closing headless; the look and feel with
  real applications is checked by hand through the quickstart.
