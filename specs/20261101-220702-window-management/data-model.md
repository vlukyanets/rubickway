# Data Model: Window Management on the Canvas

Builds on `specs/20261101-175316-client-windows/data-model.md`. Units as there: canvas units =
the application's logical pixels; screen positions in logical pixels of the viewer.

## shell (pure core)

### Edges

Set of `top`, `bottom`, `left`, `right` (as `xdg_toplevel.resize_edge`, but backend-neutral);
valid combinations: one edge or two adjacent ones.

### SizeLimits

| Field | Type | Rule |
| --- | --- | --- |
| `min` | `Size` | from the application; 0 = none |
| `max` | `Size` | from the application; 0 = unlimited; a component below `min` counts as `min` |

`clamp(size, largest) = size` limited per component to `[max(min, 1), min(max or inf, largest)]`
(research W6).

### Window (extended)

| Field | Type | Rule |
| --- | --- | --- |
| `id`, `origin`, `size`, `input` | as client-windows | `origin` now also changes by moves and anchors |
| `limits` | `SizeLimits` | from `Mapped` / `Updated` |
| `anchor` | `Anchor` | how `origin` follows size changes (below) |
| `mode` | `Mode` | `Normal`, `Maximized { saved: Option<Rect> }` (`None` when maximized before first shown, W7); fullscreen is tracked in `Shell` and may sit over `Maximized` |
| `parent` | `Option<WindowId>` | from `Created` / `Mapped`; used only when a window maps during fullscreen (W14) |

### Anchor (research W4)

| Variant | On a new size `s` |
| --- | --- |
| `TopLeft` | origin kept (default) |
| `Edges { fixed: Edges, right: f64, bottom: f64 }` | if `left` is not fixed (the left edge was dragged): `origin.x = right - s.w`; likewise `origin.y = bottom - s.h` for the top edge; other axes keep the origin |
| `Centre { point }` | `origin = point - s / 2` (maximized and fullscreen windows; `point` = the view centre at entry) |

`Edges` returns to `TopLeft` when an `Updated { configured: true }` arrives after the resize
ended.

### Windows (extended)

| Field | Rule |
| --- | --- |
| `stack`, `focus` | as client-windows |
| `mru: Vec<WindowId>` | every mapped window exactly once; front = most recently focused; a window mapped during fullscreen goes second (W14) |
| `cycle: Option<usize>` | index into `mru` while Mod is held after Mod+Tab; cleared (and `mru` reordered) on Mod release or `FocusLost` |

Operations added: `move_to(id, origin)`, `resize_settled(id)`, `cycle(step) -> WindowId`,
`end_cycle()` (reorders `mru`), `set_limits`, `insert_unfocused(id)` (top of the stack, second
in `mru`, focus unchanged). Invariants (tested): `mru` is a permutation of the
stack's ids; `focus` names a mapped window or `None`.

### Op (routing state, research W3)

```text
Free / Grabbed (client-windows)
Grabbed{w} --(MoveRequested w, serial ok)--------> Op::Move{w, offset}           PointerLeave to w
Grabbed{w} --(ResizeRequested w, edges)----------> Op::Resize{w, edges, start}   PointerLeave to w
Free --(Mod+Shift+left on w)---------------------> Op::Move{w, offset}           focus + raise w
Free --(Mod+right on w)--------------------------> Op::Resize{w, edges(3x3), start}
Op::Move|Op::Resize --(last button released | host pointer leave | w unmapped)--> Free
```

During `Op::Move` / `Op::Resize`: extra presses swallowed, scroll without Mod dropped,
`NextWindow` / `PreviousWindow` / `ResetView` consumed as no-ops, `CloseWindow` / `Quit` work,
keys go to the focused window (W3). The 3 x 3 split uses half-open cells; centre ties pick
bottom / right.

- `offset` = pointer canvas point - window origin at the start (for a maximized window: x scaled
  to the saved width, W7).
- `start` = pointer canvas point and window rect at the start.

### Fullscreen (in Shell)

| Field | Rule |
| --- | --- |
| `window` | the fullscreen window |
| `saved_rect` | restored on leave (for a maximized window: its maximized rect; its `Mode::Maximized` stays) |
| `saved_camera` | restored on leave, also when the window unmaps; always the camera from before any fullscreen (a second fullscreen window ends the first one first) |

### WindowEvent (server -> shell, extended)

| Variant | Fields |
| --- | --- |
| `Created` | `id`, `parent`, `limits`, `maximized: bool`, `fullscreen: bool` (initial commit of a window that requested an initial state, before any configure; the shell must answer with a `Configure` for `id`, W13; `Mapped` later uses the same `id`) |
| `Mapped` | `id`, `size`, `input`, `limits`, `parent` |
| `Updated` | `id`, `size`, `input`, `limits`, `configured: bool` (this commit applied the newest configure sent for a `WindowCommand::Configure`; activation-only configures do not count) |
| `Unmapped` | `id` |
| `MoveRequested` | `id` (serial already checked by the server) |
| `ResizeRequested` | `id`, `edges` |
| `MaximizeRequested` | `id`, `on: bool` (after the initial commit; honoured only for the focused window, W13) |
| `FullscreenRequested` | `id`, `on: bool` (likewise) |

### WindowCommand (shell -> server, new)

| Variant | Fields | Meaning |
| --- | --- | --- |
| `Configure` | `window`, `size: Option<Size>`, `states: States` | ask for this size (`None` = application chooses) and these states |
| `Close` | `window` | `xdg_toplevel.close` |

`States { maximized, fullscreen, resizing }` (`activated` stays with keyboard focus).

### CursorShape (new)

`Default`, `Grabbing` (pan), `Move`, `Resize(Edges)`; `Shell::cursor()`.

### Shell API additions

`take_commands() -> Vec<WindowCommand>`, `cursor() -> CursorShape`, `set_largest_window(Size)`,
`viewer_configured(Size)` (replaces direct `viewer_mut().configure` so maximized and fullscreen
windows follow viewer size changes), `fullscreen() -> Option<WindowId>`.

## input (changes)

`bindings::Action` gains `NextWindow` (Mod+Tab), `PreviousWindow` (Mod+Shift+Tab),
`CloseWindow` (Mod+Q).

## server (changes, not core)

| Where | Field | Rule |
| --- | --- | --- |
| seat | `last_press: Option<(WindowId, serial)>` | latest `wl_pointer.button` press delivered |
| toplevel | `wanted: { size: Option<(i32, i32)>, maximized, fullscreen, resizing }` | from `WindowCommand::Configure`; every configure carries it plus `activated` |
| toplevel | `queued_size` | newest wanted size while a size configure is unacked (W5) |
| toplevel | `pending_limits`, `limits` | `set_min_size` / `set_max_size`, applied on commit |
| toplevel | `newest_serial`, `applied_serial` | for `Updated.configured` |
| toplevel | `parent: Option<ObjectId>` (the parent's xdg_toplevel) | `set_parent`; an unmapped parent counts as none; when a parent unmaps, its children get its own parent; self or a descendant is `invalid_parent` (W14); reported as the parent's `WindowId` |
| toplevel | `initial: { maximized, fullscreen }` | requests before the initial commit; if any, reported in `Created` and the initial configure waits for the shell's answer in the same loop iteration (W13); otherwise the initial configure is sent at once as before |
| toplevel | `early_id: Option<WindowId>` | id assigned at the initial commit for `Created`, reused by `map` |

## backend (changes)

`Cursor` gains `Move` and eight resize variants (`ResizeN`, `ResizeNE`, ..., matching the
`wp_cursor_shape_device_v1` shapes); `App` maps `CursorShape` to them, so the backend does not
depend on `shell` types.
