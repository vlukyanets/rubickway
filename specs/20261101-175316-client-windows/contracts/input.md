# Contract: input with windows on the canvas

Extends `specs/20261101-040740-nested-canvas/contracts/input.md`. `Mod` = Alt.

## Pointer

| Where | Input | Goes to |
| --- | --- | --- |
| anywhere | Mod + left drag | canvas: pan (FR-012) |
| anywhere | Mod + scroll (any device) | canvas: zoom around the pointer (FR-012) |
| anywhere | touchpad pinch | canvas: zoom (no gestures are offered to applications) |
| background | left drag, scroll, touchpad scroll | canvas, as in nested-canvas (FR-013) |
| window | motion, any button, scroll (no Mod) | the application, in window coordinates (FR-010) |

- A press on a window without keyboard focus focuses and raises it, and is delivered (FR-009).
  A Mod + left press on a window pans and neither focuses nor raises it (FR-012).
- A press on a window grabs the pointer for that window until all buttons are released:
  motion, buttons and scrolling are delivered to it also outside the window or over another
  window; leaving the window does not end the grab. If its application disconnects, the grab
  ends.
- A press on the background never changes keyboard focus (FR-013, clarification 4); right and
  middle presses there do nothing.
- Where a window declares no input (its input region), pointer input goes to what is below.
- While a canvas pan is in progress, no pointer input reaches applications.
- Scroll values reach the application exactly as the host delivered them (source, notches,
  distance), not scaled by zoom.

## Keyboard

| Keys | Behaviour |
| --- | --- |
| Mod+0, Mod+Shift+Q | compositor shortcuts as before; press and release are not delivered |
| everything else | to the window with keyboard focus, raw keycodes and modifier state, host keymap |

- A new window gets keyboard focus (FR-008); when it disappears, the topmost remaining window
  gets it (FR-014).
- Key repeat is done by the application from the forwarded repeat rate and delay.
- When the viewer loses keyboard focus in the host, the focused application gets
  `wl_keyboard.leave`, and `enter` again when the viewer regains it.

## Cursor

The host's default arrow over windows and the background; "grabbing" during a canvas pan.
Cursor images requested by applications are not shown in this feature.
