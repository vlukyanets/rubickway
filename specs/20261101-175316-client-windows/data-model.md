# Data Model: Client Windows on the Canvas

Builds on `specs/20261101-040740-nested-canvas/data-model.md` (canvas, camera, viewer, grid).
Units as there; **window coordinates** = logical pixels of the application relative to the
window geometry's top-left (equal to canvas units, at any zoom).

## shell (pure core - no Wayland, GL or device types, Principle IV)

### WindowId

Opaque `u64`, unique for the life of the process, assigned by the server when a toplevel maps.

### Window

| Field | Type | Rule |
| --- | --- | --- |
| `id` | `WindowId` | |
| `origin` | `Point` (canvas units) | top-left of the window geometry; set once on map, never changed by resizes (clarification 2) |
| `size` | `Size` (canvas units) | window geometry size; `w, h > 0` |
| `input` | `Vec<Rect>` (window coordinates) | union of the surface tree's input regions, clipped to each surface; empty -> the window cannot be hit |

Placement on map (FR-004): `origin = camera.center - size / 2`.
Resize: `size` replaced, `origin` unchanged.

### Windows (stacking + focus)

| Field | Type | Rule |
| --- | --- | --- |
| `stack` | `Vec<Window>` | back to front; at most one entry per `WindowId` |
| `focus` | `Option<WindowId>` | refers to a window in `stack`, or `None` |

Operations:

- `map(id, size, input, camera)` - push on top, place per above, focus it (FR-008).
- `update(id, size, input)` - new size/input, origin kept.
- `unmap(id)` - remove; if it had focus, focus the new topmost window or `None` (FR-014).
- `raise_and_focus(id)` - move to top, focus (FR-009).
- `hit(canvas_point) -> Option<(WindowId, local Point)>` - topmost window whose input rects
  contain `canvas_point - origin`.

Invariants (tested): focus always names a mapped window or is `None`; `hit` never returns a
window under another window that also contains the point; `origin` changes only on `map`.

### WindowEvent (server -> shell, backend-neutral)

| Variant | Fields | Meaning |
| --- | --- | --- |
| `Mapped` | `id`, `size`, `input` | a toplevel showed its first content |
| `Updated` | `id`, `size`, `input` | its geometry size or input shape changed |
| `Unmapped` | `id` | it was hidden, destroyed, or its client disconnected |

The server only emits these (queued like `BackendEvent`s); the application applies them to the
shell. The server never reads or changes `Windows`. Ownership: `App` owns the `Shell` (viewer +
windows) and the `Server`; for drawing, `App` walks the shell's stack (positions, order,
visibility in the view) and asks the server for each visible window's surface parts in window
coordinates; after presenting it tells the server which window ids were visible.

### Shell

Owns the `Viewer`, `Windows` and the routing state. `apply(WindowEvent) -> Vec<ClientInput>`
(map uses the viewer's camera for placement; returns the keyboard focus change, if any);
`handle(&InputEvent) -> (Effect, Vec<ClientInput>)`; `visible() -> ids back to front`;
`take_dirty()`.

### Pointer routing state (in the shell, next to the viewer's `Gesture`)

```text
Free --(press on window, no Mod)--> Grabbed { window, buttons }      focus + raise if unfocused
Grabbed --(more buttons)--> Grabbed            motion, buttons, scroll -> window, also outside it
Grabbed --(last button released | window unmapped | focus lost | pointer leave)--> Free
                              (a host pointer leave while a button is held = the host broke the grab)
Free --(left press on background, or Mod + left anywhere)--> viewer gesture (nested-canvas),
                                                             no focus change
Free --(right/middle press on background)--> Free             ignored
```

- `Pointer target` = the window under the pointer while `Free` (enter/leave on change), the
  grabbed window while `Grabbed`, or the background.

### ClientInput (shell output, consumed by the server)

| Variant | Fields | Meaning |
| --- | --- | --- |
| `PointerEnter` | `window`, `pos` (window coords) | pointer now over/grabbed by this window |
| `PointerLeave` | `window` | |
| `PointerMotion` | `window`, `pos` | |
| `PointerButton` | `window`, `button` (evdev code), `pressed` | |
| `Scroll` | `window`, `source`, `delta`, `value120` | unchanged host values (logical px / notches) |
| `PointerFrame` | `window` | end of one event group |
| `KeyboardFocus` | `Option<WindowId>` | focus changed (leave old, enter new with pressed keys) |
| `Key` | `window`, `keycode` (evdev), `pressed` | not a compositor shortcut |
| `Modifiers` | `window`, `RawModifiers` | |

`Shell::handle(&InputEvent) -> (Effect, Vec<ClientInput>)`; `Effect` as in nested-canvas
(`quit`), the viewer's dirty flag as before; any change to `Windows` also marks the view dirty.

## input (changes to the nested-canvas types)

| Change | Why |
| --- | --- |
| `Key { keycode: u32, binding_sym, pressed }` - adds the raw evdev keycode | forwarded to applications |
| `Modifiers { mods, raw: RawModifiers }` with `RawModifiers { depressed, latched, locked, group: u32 }` | forwarded unchanged (layout group included) |
| `PointerButton { button, code: u32, pressed }` - adds the evdev button code | forwarded to applications |
| `FocusGained` - new; the host gave the viewer the keyboard again (`wl_keyboard.enter`) | the focused application gets `enter` again |

## backend (changes)

| `BackendEvent` variant | Fields | Why |
| --- | --- | --- |
| `Keymap(String)` | host keymap text | sent to applications (R11) |
| `RepeatInfo { rate: i32, delay: i32 }` | | forwarded |

## server (Wayland protocol state - not part of the core)

### ClientState

Per connection: id for logs, nothing else (objects belong to their resources).

### Surface (per `wl_surface`)

| Field | Rule |
| --- | --- |
| `pending`, `current`: `SurfaceState` | double-buffered; `commit` applies (subsurface sync: into `cached` until the parent commits) |
| `SurfaceState.buffer` | `Option<(ShmBuffer, offset)>`; `None` after attaching null -> unmaps a toplevel |
| `SurfaceState.scale` | `>= 1` (error `invalid_scale` otherwise) |
| `SurfaceState.transform` | one of the 8 `wl_output.transform` values (error otherwise) |
| `SurfaceState.input` | region; default infinite |
| `SurfaceState.viewport` | source rect (inside the buffer, error `out_of_buffer`), destination size (> 0) |
| `SurfaceState.frame_callbacks` | completed per R12 |
| `role` | none / toplevel / popup / subsurface / cursor (cursor surfaces are accepted and not shown) |
| `texture` | renderer handle, replaced on upload; `needs_upload` set by a commit with a new buffer |
| `logical_size` | derived (R6); error `invalid_size` if the buffer is not divisible by `scale` without a viewport |

### ShmPool / ShmBuffer

| Field | Rule |
| --- | --- |
| pool `fd`, `size` | `size > 0`; `resize` only grows (error `invalid_fd` / protocol error otherwise); sealed-against-shrink flag read once |
| buffer `offset, width, height, stride, format` | `width, height > 0`, `stride >= width * 4`, `offset >= 0`, `offset + stride * height <= pool size` (all in checked arithmetic), format `argb8888` or `xrgb8888` - else error `invalid_stride` / `invalid_format` |

### Toplevel

| Field | Rule |
| --- | --- |
| `configured` | the first configure was acked; a buffer committed before -> error `unconfigured_buffer` |
| `geometry` | `set_window_geometry`, width/height > 0, else invalid; `None` -> tree bounding box |
| `window` | `Option<WindowId>`; `Some` while mapped; mapping, size/input changes and unmapping emit `WindowEvent`s |
| `title`, `app_id`, `min/max size` | stored |
| `activated` | mirrors keyboard focus; change -> new configure |

Lifecycle: created -> initial commit (no buffer) -> configure(0x0, activated) -> ack -> commit with
buffer -> **mapped** (`Windows::map`) -> commits update size/input -> null buffer or destroy ->
**unmapped** (`Windows::unmap`). Client disconnect destroys every object, which unmaps.

### Output (one)

`wl_output` resources per client get geometry, mode (viewer buffer size), scale (ceil), name,
description, done; re-sent when the viewer's size or scale changes.

### Keyboard keymap

One sealed memfd holding the current host keymap; replaced (and re-sent to all keyboards) when
the host sends a new keymap.
