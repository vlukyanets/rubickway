# Data Model: Nested Canvas Viewer

Plain data types and their rules. Units: **canvas units** (positions on the canvas),
**logical pixels** (host's logical coordinate space, origin top-left of the viewer, y down),
**physical pixels** (buffer pixels = logical x scale).

## canvas (no Wayland, GL or input types - Principle IV)

### Point / Vector / Size

- `Point { x: f64, y: f64 }` - a position (canvas units or logical pixels, by context).
- `Vector { x: f64, y: f64 }` - a displacement.
- `Size { w: f64, h: f64 }` - logical size; `w, h >= 0`.

### Camera

| Field | Type | Rule |
| --- | --- | --- |
| `center` | `Point` (canvas units) | any finite value |
| `zoom` | `f64` | clamped to `[ZOOM_MIN, ZOOM_MAX]` = `[0.1, 10.0]`; `1.0` = 100% |

Operations (all pure, take the viewer size in logical pixels):

- `to_canvas(p_logical, size) -> Point` = `center + (p - size/2) / zoom`
- `to_logical(p_canvas, size) -> Point` = `(p - center) * zoom + size/2`
- `pan_anchor(anchor_canvas, p_logical, size)` - sets `center` so `anchor` is under `p`.
- `pan_by(delta_logical)` - `center -= delta / zoom`.
- `zoom_at(factor, p_logical, size)` - new zoom = clamp(zoom x factor); the canvas point under
  `p` stays under `p`. At a limit the zoom and centre stay unchanged (FR-007).
- `set_zoom_at(zoom, p_logical, size)` - same, absolute (pinch).
- `reset()` - `center = (0, 0)`, `zoom = 1.0` (FR-009).
- `Camera::default()` == state after `reset()` (FR-003).

Invariants (property-tested): after `pan_anchor`, `to_logical(anchor) == p` within 1e-6;
after `zoom_at` (not at a limit), `to_canvas(p)` is unchanged within 1e-6 canvas units x zoom;
resize never changes `center` or `zoom` (FR-012).

## viewer

### Viewer

| Field | Type | Rule |
| --- | --- | --- |
| `size` | `Size` (logical px) | from host configure; `0` axes replaced by the default 1280x800 |
| `scale` | `f64` | host preferred scale, `> 0`; `1.0` when unknown |
| `max_buffer_side` | `u32` (physical px) | the GPU's largest drawable side, read by the renderer at startup; `i32::MAX` until then, never above it (protocol and EGL sizes are `int`) |
| `camera` | `Camera` | |
| `gesture` | `Gesture` | see state machine below |
| `modifiers` | `Modifiers` | latest effective modifiers |
| `pointer` | `Option<Point>` | last pointer position, `None` when pointer is outside |
| `dirty` | `bool` | something visible changed since the last frame |

`Viewer::handle(InputEvent) -> Effect` where `Effect` is `{ quit: bool }`. Redraws are
signalled only through `dirty`, read and cleared with `take_dirty()`.
`render_scale()` = `scale`, or `max_buffer_side / max(size.w, size.h)` when the longest side
times `scale` would exceed `max_buffer_side` (one uniform scale for both axes; the host scales
the smaller buffer up, and the grid is drawn at this scale). The logical size is never limited.
`buffer_size()` = `(round(size.w * render_scale()), round(size.h * render_scale()))`, each
within `[1, max_buffer_side]`.

### Gesture state machine

```text
Idle --(press left on background, or Mod+press left)--> Panning { anchor: canvas Point }
Panning --(motion)--> Panning                             camera.pan_anchor(anchor, pos)
Panning --(release left | focus lost | pointer leave)--> Idle

Idle --(pinch begin)--> Pinching { zoom_at_begin }
Pinching --(pinch update)--> Pinching                     camera.set_zoom_at(begin * scale)
Pinching --(pinch end | cancel | focus lost)--> Idle
```

- While `Panning`, wheel/scroll events are ignored (no zoom mid-drag).
- On Wayland the host keeps sending pointer events to the viewer while the button is held,
  even outside the window (implicit grab), so a drag outside the window continues. A
  `PointerLeave` during a drag only happens when the host breaks that grab, and then the drag
  ends.
- In this feature everything is background, so a plain left press always starts panning.

## input (backend-neutral events - Principle IV)

### InputEvent

| Variant | Fields |
| --- | --- |
| `PointerEnter` | `pos: Point` (logical) |
| `PointerMotion` | `pos: Point` |
| `PointerLeave` | - |
| `PointerButton` | `button: Button { Left, Right, Middle, Other(u32) }`, `pressed: bool` |
| `Scroll` | `source: ScrollSource { Wheel, Finger, Continuous }`, `delta: Vector` (logical px), `value120: Option<Vector>` (wheel notches x 120) |
| `PinchBegin` / `PinchUpdate` / `PinchEnd` | update: `scale: f64` (relative to begin, > 0); end: `cancelled: bool` |
| `Key` | `binding_sym: Keysym` (layout 0, level 0), `pressed: bool` |
| `Modifiers` | `mods: Modifiers { ctrl, alt, shift, logo }` (locked mods excluded) |
| `FocusLost` | - (keyboard or pointer focus left the viewer) |

Scroll values are used as the host delivers them: positive `y` = scroll down, already flipped
by the host when natural scrolling is on. Scroll panning moves the view like a scrolled
document: `center += delta / zoom` (i.e. `pan_by(-delta)`). `Continuous` behaves like `Finger`.
Wheel up (`value120.y < 0`) zooms in; a horizontal wheel notch (`value120.x = +-120`) pans
horizontally by 50 logical pixels.

### Bindings

`Binding { mods: Modifiers, sym: Keysym, action: Action }`; `Action { ResetView, Quit }`.
Fixed table (no config, Principle VI):

| Keys | Action |
| --- | --- |
| Mod+0 | `ResetView` |
| Mod+Shift+Q | `Quit` |

`Mod` = Alt in this feature. A key press matches when `sym` equals `binding_sym` and the
effective modifiers equal `mods` exactly. Releases never trigger. No key repeat.

## render

### GridParams (pure, computed per frame from Camera + Viewer)

| Field | Type | Rule |
| --- | --- | --- |
| `minor_units` | `f64` | minor spacing in canvas units |
| `minor_px` | `f32` | minor spacing in physical px, `>= 12 * scale` |
| `major_px` | `f32` | `4 * minor_px` |
| `offset_px` | `[f32; 2]` | screen position of a super-major line (every 16 minor lines), reduced to `[0, 16 * minor_px)` in f64 first |
| `minor_alpha` | `f32` | fade 0..1 as minor spacing goes 12..24 logical px |
| `axis_px` | `[Option<f32>; 2]` | physical-px position of the origin axes if on screen |

Minor spacing in canvas units: smallest `8 * 4^k` (integer k) with `8 * 4^k * zoom >= 12`.
The shader draws three tiers so level changes hand over without popping: minor lines at
`0.05 * minor_alpha`, major lines (every 4) at `mix(0.05, 0.10, minor_alpha)`, super-major lines
(every 16) at `0.10`; the origin axes at `0.22`.

### Frame scheduling (per viewer window)

```text
not configured --(first configure)--> ready
ready + dirty  --(render, request frame callback, swap)--> waiting
waiting        --(frame callback done)--> ready   (render again only if dirty)
```

Nothing is rendered while `waiting`; hosts stop sending frame callbacks for hidden windows, so
hidden windows cost no frames (FR-015).
