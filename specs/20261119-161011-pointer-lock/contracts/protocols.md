# Contract: Wayland protocols

Globals added or changed by this feature (research L1). Everything else stays as in
`specs/20261119-142759-clipboard/contracts/protocols.md` and the earlier features' contracts.

| Global | Version | Offered when |
| --- | --- | --- |
| `zwp_relative_pointer_manager_v1` | 1 (new) | always |
| `zwp_pointer_constraints_v1` | 1 (new) | always |

"Applies" and "stops applying" are the shell's decisions (research L5-L9); this contract lists what
the protocol objects do with them.

## zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1

| Request / event | Behaviour |
| --- | --- |
| `get_relative_pointer(id, pointer)` | a relative pointer for the client; the first 32 live ones of a client, in creation order, get events, further ones are inert until an earlier one is destroyed (research L11) |
| `zwp_relative_pointer_v1.destroy` | forgotten |
| `zwp_relative_pointer_manager_v1.destroy` | the manager only; existing relative pointers keep working |
| event `relative_motion(utime_hi, utime_lo, dx, dy, dx_unaccel, dy_unaccel)` | to every live relative pointer of the client whose surface has the pointer, for each device motion while a window has the pointer and no pan, move or resize runs (FR-001, FR-002); `dx, dy` = device delta / zoom, `dx_unaccel, dy_unaccel` = unaccelerated device delta; time = libinput's microseconds; followed by `wl_pointer.motion` (unless locked) and `wl_pointer.frame` |

## zwp_pointer_constraints_v1

| Request / event | Behaviour |
| --- | --- |
| `lock_pointer(id, surface, pointer, region, lifetime)` | a lock for `surface`; `region` copied now (null: the whole surface); `oneshot` = single-use, `persistent` = lasting. Error `already_constrained` (1) if `surface` has a lock or confinement that is not defunct |
| `confine_pointer(id, surface, pointer, region, lifetime)` | the same, a confinement |
| unknown `lifetime` value | treated as `oneshot` (the protocol defines no error for it; single-use is the stricter choice) |
| `destroy` | the manager only; constraints keep working |

A constraint applies only on a window's root surface once the window is shown (FR-011); on any other
surface (a popup, a sub-surface, a cursor image, a surface with no role) it is accepted and never
applies.

## zwp_locked_pointer_v1

| Request / event | Behaviour |
| --- | --- |
| `set_cursor_position_hint(x, y)` | surface coordinates; applied on the next `wl_surface.commit` |
| `set_region(region)` | applied on the next commit; null: the whole surface |
| `destroy` | if it applied: the pointer moves to the release point when that is inside the window's effective area, else stays (FR-007), and the stop is logged with reason `application`; then no more effect |
| event `locked` | when the shell starts applying it (conditions in research L5) |
| event `unlocked` | when it stops applying (focus, pointer left, Mod+Escape, window gone, a resize leaving the pointer outside); a `oneshot` lock is then defunct: requests are ignored, no more events, and the surface may get a new constraint |
| while it applies | no `wl_pointer.motion` to any client (the shell sends none; the server does not filter); `relative_motion`, `button`, `axis`, `frame` continue; when it stops with a release point, `unlocked` comes before the `wl_pointer.motion` to that point |

## zwp_confined_pointer_v1

| Request / event | Behaviour |
| --- | --- |
| `set_region(region)` | applied on the next commit; if it applies and the pointer is outside the new area, the pointer moves to the nearest point inside with a `wl_pointer.motion` and no `relative_motion` (FR-014) |
| `destroy` | the pointer is free; no move |
| event `confined` / `unconfined` | as `locked` / `unlocked`, with the same `oneshot` rule |
| while it applies | the pointer cannot leave the effective area; `wl_pointer.motion` reports the clamped position, `relative_motion` the full movement |

`wl_region` is unchanged: no limit on rectangles, as in other compositors; limits may come later as
a separate change (research L10).

## wl_pointer (changed)

Replaces one row of `specs/20261109-110128-cursor-shapes/contracts/protocols.md` "wl_pointer"
(research L15); the serial and focus rules there are unchanged.

| Request | Behaviour |
| --- | --- |
| `set_cursor(serial, Some(surface), ..)` | accepted (role `cursor`; another role is error `role`, as before). The pointer shows what the surface shows: the arrow if it has a buffer (cursor images are not drawn), nothing if it has none. Each later commit of the surface that takes effect counts again, until the client's next cursor request or the next enter |

## Misbehaving clients

Each row gets a test with a second, well-behaved client that keeps working.

| # | Case | Expected |
| --- | --- | --- |
| M1 | second lock or confinement on a surface that has a live one | `already_constrained`, only that client disconnected |
| M2 | a new constraint after a `oneshot` one went defunct | accepted |
| M3 | constraint on a popup, sub-surface, cursor surface or role-less surface | accepted, never `locked`/`confined` |
| M4 | constraint on a window not shown yet | `locked`/`confined` only after it is shown and the conditions hold |
| M5 | the surface destroyed while a constraint exists | the constraint stops applying; requests on it are ignored afterwards |
| M6 | a confinement area or window input shape of 10,000 rectangles | no error; the confinement uses its bounding box (more than 256 rectangles); one motion stays within bounded time; other clients unaffected |
| M7 | regions and release points at the `i32` limits | no overflow, no panic; a release point outside the window is ignored |
| M8 | 10,000 lock/destroy pairs and 10,000 `set_region` + commit in one batch | handled in bounded time per request; other clients' input unaffected |
| M9 | 10,000 relative pointers on one client | 32 get events, the rest inert; one motion costs at most 32 sends |
| M10 | a client that never reads its socket while receiving relative motion | only it is disconnected when its buffer fills |
| M11 | a lock re-requested at once after every `unlocked` | Mod+Escape frees the pointer; a persistent one waits for a click (FR-008) |
