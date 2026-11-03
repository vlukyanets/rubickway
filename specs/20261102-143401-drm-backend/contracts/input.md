# Contract: input standalone

Extends `specs/20261101-220702-window-management/contracts/input.md` (and through it
client-windows and nested-canvas). `Mod` = the configured `mod-key`, default Super
(`contracts/config.md`). Everything not listed here behaves as there, with Super in place of
Alt.

## Devices

| Device | Gives |
| --- | --- |
| keyboards (built-in and external) | keys with the US English keymap (FR-007) |
| touchpad | pointer motion, buttons, tap-to-click (on), two-finger scroll, pinch; natural scrolling off; libinput's acceleration |
| mice | motion, buttons, wheel (high-resolution wheels in 1/120 steps) |

Devices plugged in or removed while running work, or stop, without anything else changing
(edge case); keys and buttons held on a removed device are released for applications.

## Keyboard

| Keys | Effect |
| --- | --- |
| Ctrl+Alt+F1 .. Ctrl+Alt+F12 | switch to that text console; checked before everything else, never forwarded, not configurable (FR-011); nothing when it is RubickWay's console |
| spawn bindings (configurable; default Mod+Return -> `kitty`) | start the bound command (FR-014); consumed with its release |
| Mod+Shift+Q | first press: nothing visible, consumed; a second press within 2 s quits (FR-031). Any other non-modifier key press, a console switch, or 2 s passing cancels the first press; pressing or releasing Shift or Mod in between does not. The same during a move or resize (this replaces window-management's "Mod+Shift+Q quits" row) |
| Mod+0, Mod+Q, Mod+Tab, Mod+Shift+Tab | as before; Mod+0 resets only the active viewer to its home view (FR-019) |

Applications get the US English keymap (FR-007) and repeat info 25 keys/s after 600 ms;
RubickWay itself does not repeat keys. Shortcuts match the key's symbol without modifiers.

## Pointer bindings (FR-032)

The Mod pointer gestures of window-management are pointer bindings from `binds`
(contracts/config.md "Pointer combinations"). The defaults swap window-management's two
left-button gestures (owner, 2026-11-03), replacing its rows for them:

| Pointer | Action (default binding) |
| --- | --- |
| Mod + left drag | `move-window`: on a window focus, raise, move; over the background pan |
| Mod+Shift + left drag | `pan-canvas`: pans anywhere, no focus change |
| Mod + right drag, Mod+Shift + right drag | `resize-window`: on a window focus, raise, resize from the nearest edge or corner; over the background nothing |

Modifiers match exactly (window-management's "Mod + left" also matched with Ctrl or Alt held as
well; now such a press is an ordinary click). A press matching no pointer binding is an ordinary
click: left on the background pans, on a window it goes to the window. The gesture ends when
its button is released; further presses during a move or resize are swallowed as before.
Dragging the background without Mod, title bars and borders, Mod + scroll and pinch are
unchanged.

## Pointer and several displays

- The pointer is drawn by RubickWay, above everything (FR-008), and starts at the centre of the
  leftmost display. It stops at the outer edges of the row of displays and crosses between
  neighbours at their shared edge (FR-009, FR-018).
- The active viewer is the one under the pointer. Pan, zoom and Mod+0 act on it; new windows
  open centred in it; focus cycling brings a window into it unless the window is already fully
  inside some viewer (FR-019, FR-022).
- A pan drag keeps acting on the viewer it started on until the button is released, also when
  the pointer crosses to another display.
- During a move or resize the pointer's canvas position is taken in the viewer it is on, so a
  window can be carried to another display, appearing at that viewer's zoom (FR-023); at the
  crossing it jumps to the region that display shows.
- Crossing towards a shorter display at a height it does not have: the pointer crosses with its
  height clamped to that display's bottom edge.
- Without any pointing device the pointer stays at the centre of the leftmost display, which is
  then the active viewer.
- A pan, move or resize ends when its viewer (or the pointer's) is unplugged, the window staying
  where it was.
- With a fullscreen window on one viewer, pan, zoom and Mod+0 on another viewer leave it alone;
  focus cycling leaves fullscreen only on the active viewer.

## Cursor shapes

Default arrow, grabbing hand (pan), move (window move), resize (eight directions shown with four
double arrows) - from the installed `default` cursor theme if there is one, else built-in
images (FR-008). Over a window whose application hid the pointer (`set_cursor` without a
surface) no pointer is drawn, unless a pan, move or resize is running; an application's own
cursor image is replaced by the default arrow.

## Console switching and pause

When the console is switched away (or the session becomes inactive): applications get
`wl_pointer.leave` and `wl_keyboard.leave` (held keys and buttons count as released), any pan,
move or resize ends where it is, a pending quit press is cancelled. Nothing is drawn and no
frame callbacks complete until the console is shown again; then keyboard focus returns to the
focused window and the pointer re-enters where it was.
