# Contract: Wayland protocols

No new global. `xdg_wm_base` stays at version 6; its popup parts, accepted and ignored until now, are
implemented (research P1). Everything else stays as in
`specs/20261102-143401-drm-backend/contracts/protocols.md`,
`specs/20261104-004828-gpu-buffers/contracts/protocols.md` and
`specs/20261109-110128-cursor-shapes/contracts/protocols.md`.

| Global | Version | Offered when |
| --- | --- | --- |
| `xdg_wm_base` | 6 (unchanged) | always |

## xdg_positioner

| Request | Behaviour |
| --- | --- |
| `set_size(w, h)` | kept; `w` or `h` below 1: `invalid_input` (as today) |
| `set_anchor_rect(x, y, w, h)` | kept; negative `w` or `h`: `invalid_input` (as today) |
| `set_anchor(a)`, `set_gravity(g)` | kept; a value outside 0-8: `invalid_input` |
| `set_constraint_adjustment(bits)` | kept; bits outside `0x3f`: `invalid_input` |
| `set_offset(x, y)` | kept |
| `set_reactive` | kept (research P10) |
| `set_parent_size(w, h)` | kept, not used (research P10); negative values: `invalid_input` |
| `set_parent_configure(serial)` | accepted, not used (research P10) |

The rules are copied when the positioner is used (`get_popup`, `reposition`).

## xdg_surface

| Request | Behaviour (changed) |
| --- | --- |
| `get_popup(id, parent, positioner)` | popup role; an incomplete positioner: `invalid_positioner` (as today). `parent` with no role: `invalid_popup_parent`. No parent: dismissed at the initial commit. Before: always dismissed at once |
| `ack_configure(serial)` | as today; for a popup, the placement of that configure applies at the next commit |
| `destroy` | before its `xdg_popup`: `defunct_role_object` (as for toplevels) |

## xdg_popup

| Request | Behaviour |
| --- | --- |
| `grab(seat, serial)` | before the initial commit, `serial` the client's latest button press or latest key press or release: a grab (research P6, P9). Wrong serial: dismissed at the initial commit, no error. After the initial commit: `invalid_grab`. Parent is a popup without a grab: `invalid_grab`. Parent is a dismissed grabbing popup: dismissed |
| `reposition(positioner, token)` | the shell places it again; `repositioned(token)`, `configure`, `xdg_surface.configure`. Only the newest waiting request is answered |
| `destroy` | with shown child popups: `not_the_topmost_popup`. Otherwise the popup goes; a grab returns to its parent if that has one |

| Event | Sent |
| --- | --- |
| `configure(x, y, w, h)` | after the initial commit, after each reposition, and when a reactive popup is placed again; followed by `xdg_surface.configure`. `x, y` relative to the parent's window geometry |
| `repositioned(token)` | before the `configure` that answers a reposition |
| `popup_done` | when RubickWay dismisses it (contracts/input.md), its parent goes away, its window unmaps, or it is refused; topmost first within a chain |

### Life of a popup

1. `get_popup`, maybe `grab`, then an initial commit without a buffer.
2. RubickWay answers with `configure` + `xdg_surface.configure`, or `popup_done`.
3. The application acks, attaches a buffer, commits: the popup is shown.
4. Later commits change its content, size or input shape; a commit after acking a new placement
   moves it.
5. A commit without a buffer hides it and ends it: its children are dismissed first, then it gets
   `popup_done` (research P12). `popup_done` or `destroy` ends it too; an ended popup is never shown
   again, whatever it commits. Destroying its `wl_surface` first ends it the same way, without an
   error, as for windows today.
6. A grabbing popup that maps while its parent is not the topmost grabbing popup:
   `not_the_topmost_popup`.

## wl_surface on popups

`wl_surface.enter`/`leave` for the displays that show the popup's window, `wp_fractional_scale_v1`
preferred scale and frame callbacks: as for the window's own surfaces. Subsurfaces of a popup are part
of it.

## wl_pointer and wl_keyboard during a grab

- Pointer: the grabbing application's windows and popups get `enter`, `motion`, `button`, `axis` as
  usual; other applications' windows get `leave` and nothing more until the grab ends. With a button
  held, the pointer still moves between the grab window's surfaces and its popups (press, drag,
  release on an item).
- Keyboard: `leave` on the window's root surface and `enter` on the topmost grabbing popup; back to the
  root surface when the grab ends. The toplevel keeps `activated`.
- Cursor requests (`set_cursor`, `set_shape`) over a popup count as over its window
  (cursor-shapes contracts).

## Misbehaving applications (added cases, Principle II)

| Case | Result | Test |
| --- | --- | --- |
| positioner without size or anchor rectangle | `invalid_positioner` (as today), in `get_popup` and in `reposition` | protocol |
| `set_anchor`, `set_gravity` 9 or `u32::MAX`; adjustment bit `0x40` | `invalid_input` | protocol |
| `set_parent_size(-1, 5)` | `invalid_input` | protocol |
| huge size, anchor rectangle and offset (`i32::MAX`) | placed with saturation; no panic; the client keeps running | protocol + unit |
| `get_popup` with a parent `xdg_surface` that has no role | `invalid_popup_parent` | protocol |
| `get_popup` with no parent | `popup_done` after the initial commit | protocol |
| parent window unmapped before the popup's initial commit | `popup_done` | protocol |
| `grab` after the initial commit | `invalid_grab` | protocol |
| `grab` with serial 0 or a stale serial | `popup_done`, no error | protocol |
| grabbing child of a popup without a grab | `invalid_grab` | protocol |
| grabbing popup mapped while not on top of the grab chain | `not_the_topmost_popup` | protocol |
| destroying a popup with a shown child | `not_the_topmost_popup` | protocol |
| `xdg_surface.destroy` before its `xdg_popup` | `defunct_role_object` | protocol |
| buffer committed before the first ack | `unconfigured_buffer` | protocol |
| `ack_configure` of a serial never sent | `invalid_serial` (as today) | protocol |
| commits after `popup_done` | ignored for display; nothing shown | protocol |
| a buffer committed again after a commit without a buffer | `popup_done` was sent at the hiding; nothing shown | protocol |
| `wl_surface.destroy` before its `xdg_popup` | the popup ends (children first); no error | protocol |
| 10 000 `reposition` requests in one batch | one answer, for the newest token; no stall | protocol |
| client disconnects with a grab open | its popups vanish; pointer and keyboard go to what is under the pointer and the focused window | protocol + shell |
