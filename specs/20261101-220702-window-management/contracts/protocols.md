# Contract: xdg-shell window management

Changes to `specs/20261101-175316-client-windows/contracts/protocols.md`. No new globals; every
version stays as there (`xdg_wm_base` 6).

| Global | Version | Behaviour (changed) |
| --- | --- | --- |
| `xdg_wm_base` | 6 | toplevels with interactive move and resize, maximize and fullscreen; popups still get `popup_done` at once; minimize and window menu requests are ignored |

## xdg_toplevel

| Request / event | Behaviour |
| --- | --- |
| `wm_capabilities` (v5+) | `maximize`, `fullscreen` |
| `move(seat, serial)` | starts a move if `serial` is the latest `wl_pointer.button` press on this window and a button is still held; else ignored |
| `resize(seat, serial, edges)` | likewise, resizing from `edges`; `none` is ignored; a value outside the `resize_edge` enum is error `invalid_resize_edge` |
| `set_maximized` / `unset_maximized` | maximize (spec FR-013) / restore; only from the window with keyboard focus, otherwise ignored (FR-019) |
| `set_fullscreen(output)` / `unset_fullscreen` | fullscreen in the viewer (FR-014), `output` ignored / restore; only from the focused window (FR-019) |
| any of the four before the initial commit | remembered; the initial configure already carries the state (fullscreen wins, `maximized` kept with it), unless the window will open without focus because another window is fullscreen (FR-019, FR-020) |
| `set_parent(parent)` | stored; an unmapped parent counts as none; when a parent unmaps, its children get its own parent (protocol rule); the toplevel itself or one of its descendants is error `invalid_parent`; a window whose parent is fullscreen ends fullscreen when it maps (FR-020) |
| `set_minimized`, `show_window_menu` | ignored |
| `set_min_size` / `set_max_size` | double-buffered, applied on commit; used to clamp every size RubickWay asks for |
| `configure(w, h, states)` | `w x h` = the size RubickWay asks for, or 0 x 0 (the application chooses); states: `activated` (keyboard focus), `maximized`, `fullscreen` (both while fullscreen over maximized), `resizing` |
| `close` | sent on Mod+Q for the focused window |

- Sizes asked for are never below the minimum size, below 1 x 1, above the maximum size, or
  above the largest window RubickWay can display (GPU texture limit / ceil(scale)), so a
  well-behaved application is never disconnected for a size RubickWay asked for. On conflict
  the displayable limit wins over the minimum, and the minimum over a smaller maximum.
- While a configure with a new size is not acknowledged, newer sizes are not sent; the newest
  one is sent when the application acknowledges (coalescing).
- RubickWay never waits for a configure to be acknowledged or answered; the window shows the
  size the application last committed, anchored per spec FR-007.

## Errors that disconnect an application (added)

| Cause | Error |
| --- | --- |
| `resize` with an `edges` value outside the `resize_edge` enum | `xdg_toplevel.invalid_resize_edge` |
| `set_parent` with the toplevel itself or one of its descendants | `xdg_toplevel.invalid_parent` |
