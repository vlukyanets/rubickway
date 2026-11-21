# Data Model: Pointer Lock

The rules live in the shell (research L2); the server keeps the protocol objects.

## Shared types (crate API, `src/shell/mod.rs` and `src/shell/route.rs`)

### ConstraintToken

`u64`, from a counter in the server; identifies one constraint object for its whole life and is never
reused (research L3).

### ConstraintInfo (in `WindowEvent::ConstraintChanged`)

| Field | Type | Notes |
| --- | --- | --- |
| `token` | `ConstraintToken` | |
| `kind` | `ConstraintKind` (`Lock` / `Confine`) | |
| `persistent` | `bool` | `false`: single-use (`oneshot`) |
| `area` | `Option<Vec<Rect>>` | window coordinates; `None`: the whole window; any number of rectangles - the shell uses the bounding box beyond 256 (research L10) |
| `hint` | `Option<Point>` | locks only: the latest committed release point, window coordinates |

### WindowEvent (changed)

- `ConstraintChanged { id: WindowId, constraint: Option<ConstraintInfo> }`: the window's current
  constraint after a creation, a commit that changed its area, hint or the window geometry, or its
  end (`None`: destroyed by the application, defunct, or the surface gone). Sent only for window
  roots, and after `Mapped` for one created earlier (research L12).

### ClientInput (changed)

- `RelativeMotion { window: WindowId, delta: Vector, unaccel: Vector, time_us: u64 }`: `delta` in the
  window's coordinates (device delta / zoom), `unaccel` the device's unaccelerated delta, unscaled
  (research L4).
- `Constraint { token: ConstraintToken, change: ConstraintChange }` with
  `ConstraintChange::Start` or `ConstraintChange::Stop(ConstraintEnd)`;
  `ConstraintEnd` = `Focus` | `PointerLeft` | `Shortcut` | `WindowGone` | `Resize` (a resize left
  the pointer outside) - for the log (research L13). `Start` sends `locked`/`confined`; `Stop` sends
  `unlocked`/`unconfined` and, for a single-use constraint, makes it defunct.

### InputEvent (changed, `src/input/mod.rs`)

- `PointerMoved { delta: Vector, unaccel: Vector, time_us: u64 }` (research L4).

### Action (changed, `src/input/bindings.rs`)

- `ReleasePointer`: Mod+Escape, built in (research L7).

## Shell state (new, `src/shell/constraint.rs`)

### Constraint

| Field | Type | Notes |
| --- | --- | --- |
| `info` | `ConstraintInfo` | as last reported |
| `state` | `ConstraintState` | below |
| the effective area | cached | with what it was computed from (an input-shape generation, the window size, the view); see "Effective area" |

There is no stored lock point: while a lock applies the layout pointer does not move, so the window
point under it is the lock point, also after the window changes size or place under it (research
L6).

`Shell.constraints: HashMap<WindowId, Constraint>`; `Shell.applying: Option<WindowId>` - the window
whose constraint is `Active` (at most one: only the focused window's can be); `Shell.released:
HashSet<WindowId>` - windows released with Mod+Escape that have not received a button press since.

### ConstraintState

```text
Waiting --conditions hold (research L5)--> Active            [ClientInput Start]
Active  --conditions fail--> Waiting (persistent)             [Stop(reason)]
                         \-> removed (single-use)              [Stop(reason)]
Active  --pan / move / resize starts--> Active, suspended      [nothing]
Active, suspended --pan or move ends--> Active (pointer on the same window point)
Active, suspended --resize ends--> conditions checked again   [Stop(Resize) if the pointer is outside]
Active  --view jumps (Mod+0, display change)--> conditions checked again   [Stop(PointerLeft) if outside]
Waiting (hidden window) --shown again--> conditions checked (lasting only; single-use ended on hide)
Active  --Mod+Escape--> Waiting (persistent) / removed (single-use)   [Stop(Shortcut)]
                         and the window is released: none of its constraints starts, also one
                         reported later, until a button press is delivered to it (FR-008, M11)
any     --window unmapped--> removed  [Stop(WindowGone) if Active]
any     --ConstraintChanged None--> removed  [no decision: the object is gone and the server logs
                                              the stop itself; a lock's release point still applies]
Active  --viewer removed (monitor unplugged)--> conditions checked again   [Stop(PointerLeft)]
```

"Suspended" is not a stored state: it is `Active` while `router.operation` or a pan is running.

### Effective area (computed, not stored)

`info.area` (or the window rect) intersected with `window.input` and with the view rect of the
pointer's viewer in window coordinates; empty: the constraint cannot apply. An area or input shape of
more than `MAX_CONFINE_RECTS = 256` rectangles counts as its bounding box, and so does the result if
it has more than 256 (research L10). Kept with
the constraint and computed again only when the area, the input shape or the view changes.

### confine (pure function)

`confine(area: &[Rect], from: Point, to: Point) -> Point` (research L8).

## Server state (new)

### `src/server/pointer_constraints.rs`

| Item | Notes |
| --- | --- |
| `ConstraintData`, in `ServerState.constraints.by_token: HashMap<ConstraintToken, ConstraintData>` (the user data of `ZwpLockedPointerV1` and `ZwpConfinedPointerV1` is their token) | `resource`, `surface: ObjectId`, `client`, `kind`, `persistent`, `pending_area: Option<Option<Region>>`, `area: Option<Region>`, `pending_hint: Option<Point>`, `hint: Option<Point>` (surface coordinates), `applying: bool` (between a `Start` and a `Stop`, so a destroy while applying is logged), the last report (window and info); a defunct one is removed from both maps, so its requests find nothing |
| `ServerState.constraints.by_surface: HashMap<ObjectId, ConstraintToken>` | the live, not defunct constraint per surface (for `already_constrained`) |
| `ServerState.constraints.next_token` | counter, never reused |

Commit (`compositor::commit` of a surface with a constraint): pending area and hint become current;
if the surface is a window root, report `ConstraintChanged` when anything reported changed.

### `src/server/relative_pointer.rs`

| Item | Notes |
| --- | --- |
| `SeatState.relative_pointers: HashMap<ClientId, Vec<ZwpRelativePointerV1>>` | in creation order; the first 32 live ones of a client get events (research L11), so a later one starts getting them once an earlier one is destroyed; pruned on destroy and with the client |

### `src/server/compositor.rs` (changed)

- `commit` applies a surface's pending constraint area and release point. `wl_region` is unchanged
  (research L10).

## Backend (changed, `src/backend/drm/input.rs`)

- `RawEvent::Motion { dx, dy, dx_unaccel, dy_unaccel, time_us }`; `translate` fills
  `InputEvent::PointerMoved`.
