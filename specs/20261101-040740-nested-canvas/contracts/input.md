# Contract: input bindings and gestures

`Mod` = Alt in this feature. Shortcuts act only while the viewer has keyboard focus, match the
key's symbol in the first keyboard layout (so they work with any active layout), require the
exact modifier set shown, and fire once per press (no key repeat).

## Keyboard

| Keys | Action | Spec |
| --- | --- | --- |
| Mod+0 | reset view: canvas origin at the window centre, zoom 100% | FR-009 |
| Mod+Shift+Q | quit, exit status 0 | FR-010 |

## Pointer

| Input | Action | Spec |
| --- | --- | --- |
| left drag on the background | pan; the grabbed canvas point stays under the pointer | FR-005 |
| Mod + left drag (anywhere) | pan, same as above | FR-005 |
| mouse wheel, one notch | zoom 10% in (wheel up) / out (wheel down) around the pointer | FR-006 |
| high-resolution wheel | fractional steps, same total per notch | FR-006 |
| Mod + scroll (any device) | zoom around the pointer; touchpad: one step per 50 logical px | FR-006 |
| two-finger touchpad scroll, trackpoint scroll | pan by the scrolled distance like a scrolled document, following the host's natural/traditional setting | FR-008 |
| horizontal wheel (tilt), one notch | pan horizontally by 50 logical px | FR-008 |
| touchpad pinch | zoom continuously around the pointer (if the host reports pinch) | FR-008 |

Zoom is clamped to 10%..1000% (FR-007). Scrolling is ignored while a drag is in progress. A
drag continues outside the window while the button is held, and ends on button release,
focus loss, or pointer leave (the host broke the implicit grab).

## Cursor

Default arrow over the canvas; "grabbing" while panning (only when the host supports
`wp_cursor_shape_v1`).
