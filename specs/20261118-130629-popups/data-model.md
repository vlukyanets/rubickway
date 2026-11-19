# Data Model: Popups

Types this feature adds or changes. Shell types have no Wayland types (Principle IV).

## Shell (`src/shell/`)

### PopupId (new, `popups.rs`)

`PopupId(pub u64)`, assigned by the server when an `xdg_popup` makes its initial commit, like
`WindowId`. Never reused within a run.

### AppId (new, `windows.rs`)

`AppId(pub u64)`: the application a window belongs to (the server's client number). Used only to
decide which surfaces get the pointer during a grab (research P6).

### PopupRules (new, `positioner.rs`)

A copy of an `xdg_positioner` at the time it was used (the protocol says later changes do not apply).

| Field | Type | Notes |
| --- | --- | --- |
| `size` | `(i32, i32)` | at least 1 x 1 (checked by the server) |
| `anchor_rect` | `IRect` | relative to the parent's window geometry; width and height at least 0 |
| `anchor` | `Anchor8` | `None`, `Top`, `Bottom`, `Left`, `Right`, `TopLeft`, `BottomLeft`, `TopRight`, `BottomRight` |
| `gravity` | `Anchor8` | same values; the direction the popup grows |
| `adjust` | `Adjust` | bit set: `slide_x`, `slide_y`, `flip_x`, `flip_y`, `resize_x`, `resize_y` |
| `offset` | `(i32, i32)` | |
| `reactive` | `bool` | |
| `parent_size` | `Option<(i32, i32)>` | `set_parent_size`; kept, not used by placement (research P10) |

`IRect { x, y, w, h: i32 }` is a small integer rectangle for protocol coordinates; it converts to the
canvas `Rect`.

### place (new, `positioner.rs`)

`pub fn place(rules: &PopupRules, area: Option<IRect>) -> IRect`: research P3. `area` is the
constraint area in the parent's coordinates. Pure; no other inputs.

### Popup (new, `popups.rs`)

A popup the shell knows, kept in its window (`Window::popups`, creation order).

| Field | Type | Notes |
| --- | --- | --- |
| `id` | `PopupId` | |
| `parent` | `Option<PopupId>` | `None`: the window itself |
| `rules` | `PopupRules` | the latest (initial or from a reposition) |
| `grab` | `bool` | the application asked for a grab and the shell accepted it |
| `placed` | `IRect` | the latest placement sent, relative to the parent |
| `shown` | `Option<Shown>` | `None` until `PopupMapped` |

`Shown { at: (i32, i32), size: Size, input: Vec<Rect> }`: where the application actually shows it
(relative to the parent, from the server, after its ack) and its input shape (popup coordinates).
The rectangle in window coordinates is the parent's shown position plus `at`, computed when needed.

States: requested (placed, not shown) -> shown -> gone. "Gone" is removal from the list: dismissed by
the shell, unmapped or destroyed by the application, or with its window.

### PopupGrab (new, router state in `route.rs`)

| Field | Type | Notes |
| --- | --- | --- |
| `app` | `AppId` | whose surfaces get the pointer (research P6) |
| `window` | `WindowId` | the chain's window |
| `chain` | `Vec<PopupId>` | grabbing popups, bottom to top; the last has the keyboard |
| `swallow` | `Vec<u32>` | buttons whose press dismissed the chain; their releases are swallowed |

At most one. Ends when its chain is empty.

### Window (changed, `windows.rs`)

- `app: AppId` (new).
- `popups: Vec<Popup>` (new).
- `extent()` (new): the window rect together with the rects of its shown popups; `Windows::visible`
  uses it instead of `rect()` (research P5).
- `Windows::hit` and `hit_window` try the window's shown popups, topmost first, before its input
  shape, and return the point in window coordinates either way. With a grab, `Shell::hit` keeps only
  windows of the grab's application.

### WindowEvent (changed, `mod.rs`)

| Variant | Fields | When |
| --- | --- | --- |
| `Mapped` | + `app: AppId` | as before |
| `PopupRequested` (new) | `id: PopupId`, `window: WindowId`, `parent: Option<PopupId>`, `rules: PopupRules`, `grab: bool` | a popup's initial commit; the shell answers `PlacePopup` or `DismissPopup` |
| `PopupRepositionRequested` (new) | `id`, `rules`, `token: u32` | `xdg_popup.reposition`; answered with `PlacePopup` carrying the token |
| `PopupMapped` (new) | `id`, `at: (i32, i32)`, `size: Size`, `input: Vec<Rect>` | first commit with a buffer after the ack |
| `PopupUpdated` (new) | `id`, `at`, `size`, `input` | a later commit changed any of them (a new placement applied, a new size) |
| `PopupUnmapped` (new) | `id` | the application hid it (commit without a buffer, which ends it), destroyed it or its surface; its children were dismissed first |

`PopupRequested` with `grab: true` means the server's serial check passed (research P9).

### WindowCommand (changed, `mod.rs`)

| Variant | Fields | Effect in the server |
| --- | --- | --- |
| `PlacePopup` (new) | `id: PopupId`, `at: IRect`, `token: Option<u32>` | `repositioned(token)` if any, `xdg_popup.configure`, `xdg_surface.configure`; applied at the commit after the ack |
| `DismissPopup` (new) | `id: PopupId` | `popup_done`; stops drawing it and ignores its later commits for display |

The shell pushes `DismissPopup` topmost first; the server sends them in that order.

### ClientInput (changed, `route.rs`)

| Variant | Fields | Effect in the server |
| --- | --- | --- |
| `PopupKeyboard` (new) | `window: WindowId`, `popup: Option<PopupId>` | keyboard `leave` on the current surface and `enter` on the popup's surface (or the window's root for `None`), with the pressed keys and modifiers; the window's `activated` state does not change (research P7) |

## Server (`src/server/`)

### PositionerState (changed, `xdg_shell.rs`)

Now keeps every value set, not only whether size and anchor rectangle were seen, and builds
`PopupRules`. Values are checked when set: size below 1 or a negative anchor rectangle is
`invalid_input` (as today); an anchor, gravity or adjustment value outside the protocol's values is
`invalid_input` too.

### PopupData (new, `popup.rs`)

Per `xdg_popup`, keyed by the `xdg_surface`'s id.

| Field | Type | Notes |
| --- | --- | --- |
| `resource` | `XdgPopup` | |
| `id` | `Option<PopupId>` | given at the initial commit |
| `parent` | `Option<ObjectId>` | the parent `xdg_surface` |
| `window` | `Option<WindowId>` | the toplevel at the root of the parent chain, once known |
| `rules` | `PopupRules` | |
| `grab` | `GrabState` | `None`, `Asked`, `Refused` (serial wrong: dismissed) |
| `pending` | `Vec<(u32, IRect)>` | configure serials sent with their placement |
| `placement` | `Option<IRect>` | the acked placement, applied at the next commit |
| `at` | `Option<IRect>` | the placement in effect (shown) |
| `reposition` | `Option<(u32, PopupRules)>` | a reposition waiting for the shell; a newer one replaces it |
| `dismissed` | `bool` | `popup_done` sent, or hidden by a commit without a buffer; never shown again (FR-011, FR-012) |
| `shown` | `bool` | mapped |
| `reported` | `Option<((i32, i32), Size, Vec<Rect>)>` | what the shell was told last |

### SeatState (changed, `seat.rs`)

- `popup_serials: HashMap<ClientId, (Option<u32>, Option<u32>)>` (new): per application, the serial of
  its latest button press and of its latest key press or release (research P9). Removed when the
  client disconnects.
- `keyboard_surface` may now be a popup's surface (`PopupKeyboard`); `keyboard_window` stays the
  window.
- While a client has a shown grabbing popup, `PointerMotion` re-picks the surface under the pointer
  among that window's surfaces even with buttons held (research P6).

### layers (new, `popup.rs`)

`fn layers(state, window) -> Vec<(ObjectId, Point)>`: the root surfaces to draw for a window, back to
front, with their origins in window coordinates: the toplevel, then each shown, not dismissed popup
(research P5). Used by `window_parts`, `surface_at`, `relative_to`, `take_damage`, `presented`,
`has_pending_callbacks`, `set_window_outputs` and `announce_scale`.

## App (`src/standalone.rs`)

No change beyond what the shell and server types bring: windows are drawn from `window_parts`, which
now include popups, and `visible_in` uses `extent`.
