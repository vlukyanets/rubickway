# Contract: input for window management

Extends `specs/20261101-175316-client-windows/contracts/input.md`. `Mod` = Alt. Everything not
listed here behaves as there.

## Pointer

| Where | Input | Effect |
| --- | --- | --- |
| window | Mod + Shift + left drag | move the window (FR-002); focus + raise; the application gets nothing |
| background | Mod + Shift + left drag | pan, like Mod + left drag |
| window | Mod + right drag | resize from the nearest edge or corner of a 3 x 3 split (FR-006); focus + raise; the application gets nothing |
| background | Mod + right press | nothing |
| anywhere | Mod + Shift + right | same as Mod + right (Shift ignored) |
| window, button held | the application asks to move (its title bar) | move the window (FR-001) |
| window, button held | the application asks to resize from edges | resize from those edges (FR-005) |
| during a move | Mod + scroll, pinch | the canvas zooms around the pointer; the window stays under the pointer (FR-003) |

- While a move or resize runs, the window gets `wl_pointer.leave` and no pointer input until
  the operation ends; it ends on the last button release, a host pointer leave, or the window
  disappearing. Losing the keyboard in the host does not end it. Releasing Mod or Shift does not
  end it.
- A move or resize request whose serial is not the latest button press on that window, or
  arriving when no button is held on it, is ignored (FR-016).
- Mod + right picks the edges from a 3 x 3 split of the window; a point on a cell boundary
  belongs to the cell further right or down, and the centre cell resizes from the nearest corner
  (ties: bottom / right).

### During a move or resize

| Input | Effect |
| --- | --- |
| further button presses | ignored |
| scroll without Mod | nothing |
| Mod + scroll, pinch | zoom (the moved window, or the dragged edge, stays under the pointer) |
| Mod+Tab, Mod+Shift+Tab, Mod+0 | consumed, nothing |
| Mod+Q | closes the focused window as usual; if that is the dragged window, the operation ends when it disappears |
| Mod+Shift+Q | quits |
| other keys | to the focused window |

## Keyboard

| Keys | Behaviour |
| --- | --- |
| Mod+Tab | next window in most-recently-used order: focus, raise, bring into view (FR-011, FR-012) |
| Mod+Shift+Tab | previous window, likewise |
| Mod+Q | ask the focused window's application to close it (FR-010); nothing without focus |
| Mod+0, Mod+Shift+Q | as before |

- All five are consumed with their releases (FR-017).
- Repeated Mod+Tab while Mod stays held walks further; each window reached gets keyboard focus;
  the order is updated when Mod is released, or when RubickWay loses the host's keyboard focus.
- The view moves only if the window is not fully inside it: centred, zoom kept if it fits,
  else zoomed out to fit with a 5% margin (never below 10%); a maximized window gets its
  maximize view at 100%.

## Fullscreen

While a window is fullscreen, these leave fullscreen first and are consumed: a pan press (Mod +
left, Mod + Shift + left on the background, left on uncovered background), Mod + scroll or
scroll over uncovered background, pinch begin, Mod+0, Mod+Tab / Mod+Shift+Tab (which then also
cycle, the view going straight to the next window). Mod + Shift + left on the fullscreen window
leaves fullscreen and moves it at its earlier size under the pointer; Mod + right on it leaves
fullscreen and ends there. Mod+Q closes the fullscreen window as usual (the view returns to
where it was before fullscreen). Input to the fullscreen window itself works normally. A new
window that opens meanwhile does not get focus (Mod+Tab reaches it first), unless it is a
dialog of the fullscreen window, which ends fullscreen (spec FR-020).

## Cursor

| State | Cursor |
| --- | --- |
| canvas pan | grabbing (as before) |
| moving | move |
| resizing | the resize shape of the dragged edges (n, ne, e, se, s, sw, w, nw) |
| otherwise | default arrow |
