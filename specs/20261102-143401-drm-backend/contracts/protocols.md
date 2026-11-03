# Contract: Wayland protocols standalone

Changes to `specs/20261101-175316-client-windows/contracts/protocols.md` and
`specs/20261101-220702-window-management/contracts/protocols.md`. No new interfaces; every
version stays as there (Principle VI).

| Global | Version | Behaviour (changed) |
| --- | --- | --- |
| `wl_output` | 4 | one global per connected display, created when it connects and removed when it disconnects (S13) |
| `wl_compositor` | 6 | `preferred_buffer_scale` 1 (scale is 1 on every display) |
| `wp_fractional_scale_manager_v1` | 1 | `preferred_scale` 120 |
| `wl_seat` | 8 | unchanged; `wl_keyboard.keymap` = the US English keymap (FR-007), `repeat_info` 25 / 600 |

All other globals unchanged.

## wl_output

| Event | Value |
| --- | --- |
| `geometry` | x, y = the display's position in the row (logical pixels); physical size in mm from the display (0 when unknown); subpixel `unknown`; make and model from the EDID (`Unknown` when absent); transform `normal` |
| `mode` | `current | preferred`, the display's mode width x height, refresh in mHz |
| `scale` | 1 |
| `name` | the connector name, e.g. `eDP-1`, `HDMI-A-1` |
| `description` | `<make> <model> (<name>)` |
| `done` | after the above |

- When a display disconnects, its global is removed; `wl_output` objects clients still hold
  stay valid and inert until they destroy them (no error for a late bind or request).
- Output ids and names are not reused for another display while RubickWay runs; a display
  reconnected later gets a new global.

- When the row re-flows (a display added or removed to the left), the moved outputs get a new
  `geometry` with their position, then `done`.

## xdg_toplevel

| Request | Behaviour (changed) |
| --- | --- |
| `set_fullscreen(output)` | fullscreen on that output's viewer when `output` is a current display; otherwise (none, unknown, removed) on the window's viewer (spec FR-022). Window-management's "`output` ignored" no longer holds |

## wl_pointer

| Request | Behaviour (changed) |
| --- | --- |
| `set_cursor(serial, None, ..)` | the pointer is hidden while over that client's window, unless a RubickWay gesture runs (spec FR-008) |
| `set_cursor(serial, Some(surface), ..)` | accepted (role `cursor`), not shown: RubickWay's default arrow is drawn instead (unchanged) |

## wl_surface

- `enter(output)` / `leave(output)`: a mapped window's surfaces enter every output whose viewer
  shows any part of the window and leave it when it no longer does (FR-015); while a viewer has
  a fullscreen window, only that window counts as shown there. While RubickWay is paused no
  `leave` is sent: windows keep their outputs.

## Frame callbacks

Completed for surfaces of windows shown on some output, when that output presents the frame
that contains them (page flip), or at the output's estimated vblank when nothing changed
(research S7). A window on several outputs has a callback completed by the first output to
present a frame showing it after the callback was requested. None while RubickWay is paused
(console switched away); they complete after it is shown again.

## Misbehaving applications (added cases, Principle II)

| Case | Result |
| --- | --- |
| commits and frame requests while paused | content kept (latest per surface), callbacks held; no growth beyond the latest content per window |
| windows created, mapped or destroyed while paused | shown correctly after resume |
| move or resize with a serial from before a pause | ignored (no button is held after the pause) |
| binding or using a `wl_output` whose display was removed | inert, no error; in `set_fullscreen` it counts as no output |
| binding the same output global many times | each bind gets the full description; no limit beyond the client's buffer limit |
| cursor images (`wl_pointer.set_cursor`) | accepted, not shown (unchanged) |
