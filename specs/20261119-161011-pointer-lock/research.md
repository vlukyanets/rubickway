# Research: Pointer Lock

Decisions for [plan.md](plan.md). References read (reading only, Principle I): the protocol XML in
`wayland-protocols` 0.32.13 (`unstable/pointer-constraints`, `unstable/relative-pointer`), wlroots
`types/wlr_pointer_constraints_v1.c` and `util/region.c` (`wlr_region_confine`), sway
`input/cursor.c` (constraint activation, warp to the hint on unlock), mutter's
`meta-pointer-constraint` (sliding along the confinement edge), niri `input/mod.rs`.

## L1. Protocols and versions

- **Decision**: `zwp_relative_pointer_manager_v1` version 1 and `zwp_pointer_constraints_v1` version
  1, from `wayland-protocols` 0.32 (feature `unstable`, already enabled). No new dependency.
- **Rationale**: version 1 is the only version of both. Every game library (SDL2/3, GLFW), Firefox,
  Blender, gamescope and remote desktop clients use these two.
- **Alternatives**: none; `pointer-warp-v1` (a newer staging protocol letting applications move the
  cursor) is out of scope (spec Assumptions: no warp other than the lock's release point).

## L2. Who decides what

- **Decision**: the server owns the protocol objects and their double-buffered state (area, release
  point) and reports each window's constraint to the shell as a window event; the shell decides when a
  constraint applies, moves (or does not move) the pointer, and tells the server through client input
  when one starts or stops applying. The server then sends `locked`/`unlocked` or
  `confined`/`unconfined`.
- **Rationale**: the shell already owns the pointer position (the layout), focus, pans, operations and
  bindings; every rule of FR-004 to FR-009 needs them. This keeps the rules pure and headless-testable
  (Principle III, IV), the same split as popups (`PopupRequested` -> `PlacePopup`) and cursors
  (`CursorChanged`).
- **Alternatives**: deciding in the server (it would need the layout, cameras and bindings - rejected);
  a shared object both sides mutate (no such pattern in the code).

## L3. Identifying a constraint across the boundary

- **Decision**: every constraint gets a `ConstraintToken` (a `u64` counter in the server, never
  reused). Window events carry it, and the shell's decisions name it. A decision for a token whose
  object is gone or defunct is ignored.
- **Rationale**: the application can destroy a constraint and create a new one on the same window
  between two steps; without a token the shell's "start applying" could reach the new one, which may
  be single-use with a different area.
- A report with a new token for a window whose constraint applies counts as the old one removed
  (no decision for the old token, whose object is gone) followed by the new one, which is judged by
  itself.

## L4. Relative motion: what is measured

- **Decision**: `InputEvent::PointerMoved` gains `unaccel: Vector` and `time_us: u64`
  (`libinput_event_pointer_get_dx_unaccelerated`/`dy_unaccelerated`, `get_time_usec`). The shell sends
  `ClientInput::RelativeMotion { window, delta, unaccel, time_us }` for every `PointerMoved` while a
  window has the pointer (the window entered, or the window of a held-button grab), and not during a
  pan, a move or resize, or over the background (FR-001, FR-002). `delta` is the device's accelerated
  delta divided by the zoom of the viewer the pointer is on (window coordinates, FR-003; spec
  Clarifications); `unaccel` is unchanged. Both are taken from the device delta, before the layout
  clamps the pointer to the screen, so screen edges, a lock and a confinement edge do not shorten them.
- **Order**: `RelativeMotion`, then `PointerMotion` (unless locked), then `PointerFrame`, all for the
  same window. The protocol does not fix the order; this is wlroots' and sway's.
- **Absolute positions** (`PointerMotion` events, which only tests and the viewer produce today) get no
  relative motion and no constraint: there is no absolute pointing device in scope (tablets and touch
  are out).
- **Rationale**: the protocol defines the accelerated delta "in the same dimension as wl_pointer
  motion", i.e. surface coordinates, which at zoom z are screen pixels / z.

## L5. When a constraint applies (FR-006, FR-011)

A window's constraint applies when all hold:

1. the window is mapped, has the keyboard focus, and the viewer has the keyboard;
2. no popup grab of that window holds the keyboard (spec Edge Cases);
3. the pointer is over the window or one of its popups (`entered`, or the grab window) and its
   position in window coordinates is inside the *effective area* (a popup of the same window drawn on
   top does not matter, spec FR-006):
   the constraint's area (or the whole window) intersected with the window's input shape and with the
   part of the window shown on the viewer the pointer is on;
4. the constraint is not `Released` (L7).

- **Decision**: one function, `Shell::update_constraint`, runs at the end of every `handle` and
  `apply` and compares this with the current state: `Waiting` -> `Active` sends "start applying";
  `Active` -> not applicable sends "stop applying" with a reason (FR-015). Only the focused window
  needs checking, plus the previously active one (bounded work, FR-012).
- **Single-use**: on stopping, the shell forgets it and the server marks the object defunct; a new
  constraint on that window is then allowed (the protocol forbids only a second one "requested or
  active"; wlroots does the same).
- **Persistent**: on stopping it goes back to `Waiting`.

## L6. Pans, zooms, moves and resizes (FR-009)

- **Decision**: while a pan, a window move or a resize is in progress, an `Active` constraint is
  *suspended without telling the application*: the pointer moves freely, the application gets no
  motion of any kind (FR-002 already says so). When the operation ends, `update_constraint` checks
  the conditions again; only if they fail is the application told the constraint stopped (and a
  single-use one ends).
- **Rationale**: a pan drags the canvas with the pointer, a zoom keeps the point under the pointer,
  and a move keeps the pointer's offset in the window, so after each the pointer is on the same point
  of the window (SC-004) and the application never sees a difference. Telling it would end every
  single-use lock (Firefox's Pointer Lock API and SDL use them) on every pan, which would make US3
  impossible for them. A resize may leave the pointer outside the area: then the normal rule ends or
  pauses the constraint.
- **Zoom** (Mod + scroll, pinch) is instantaneous and keeps the pointer's canvas point; it needs no
  suspension: the next motion uses the new camera.
- **View jumps** (Mod+0, a viewer's camera set by maximize, fullscreen or a display change) are not
  suspensions either: the pointer stays on screen, `update_constraint` checks the conditions with the
  new view, and the constraint stops ("pointer left") if the pointer is no longer over its area (spec
  Edge Cases). When a window changes size or place under a still lock (maximize, fullscreen) and the
  pointer is still over the area, `locked_at` becomes the window point now under the pointer.

## L7. The release shortcut (FR-008)

- **Decision**: a new built-in `Action::ReleasePointer` on Mod+Escape, in `builtin_table`, so a
  configured binding cannot take it (existing rule FR-025 of window-management). It is consumed
  whether or not anything applies. If a constraint is `Active`: stop it with reason `Shortcut`; a
  persistent one becomes `Released`, a single-use one ends. A `Released` constraint returns to
  `Waiting` when a button press is delivered to its window (not a press taken by a binding).
- **Note**: a user config that bound Mod+Escape to a spawn command gets the existing warning "the
  combination is a built-in shortcut" (with its line) and loses that binding, while the other
  bindings keep working; the README says so.

## L8. Confinement geometry

- **Decision**: a pure function `confine(area: &[Rect], from: Point, to: Point) -> Point` in
  `src/shell/constraint.rs`, in window coordinates:
  1. if `to` is inside the area, `to`;
  2. otherwise `to` clamped into the rectangle that contains `from` (the cursor slides along that
     rectangle's edge, like mutter, instead of stopping dead like `wlr_region_confine`);
  3. if no rectangle contains `from` (the area changed), the nearest point of the area to `to`.

  Clamping keeps the point a small margin inside the half-open rectangle, as the layout does at
  display edges (`INSIDE` in `src/canvas/layout.rs`). The area is the effective area of L5 (intersected with the view), so the
  cursor cannot cross to another monitor (FR-013). Pointer and area are converted between layout
  and window coordinates through the viewer's camera.
- **Cost**: O(n) in the area's rectangles, n bounded by L10.
- **Area change while active** (FR-014): if the pointer is outside the new area, it moves to the
  nearest point inside with a `PointerMotion` and no relative motion, as the protocol says.

## L9. Lock and release point (FR-004, FR-007)

- **Decision**: while a lock is `Active` and not suspended, `PointerMoved` does not move the layout
  pointer; it produces only `RelativeMotion` + `PointerFrame`. The cursor is drawn where it is (the
  application usually hides it). When a lock stops applying for any reason - including the application
  destroying it - and the latest committed release point lies inside the window's effective area on
  the current viewer, the pointer moves there and the window gets a `PointerMotion` (no relative
  motion, as the protocol says); otherwise it stays. The shell's output puts the `Stop` before that
  `PointerMotion`, so the application gets `unlocked` first. The shell is the only guard against
  `PointerMotion` while a lock applies; the server delivers what the shell sends, as for every other
  input, and does not filter it. Only a stop for the focus or the release shortcut, or the
  application removing the lock, moves the pointer; a stop because the pointer left the area, the
  window went away or a resize left the pointer outside does not (2026-11-20, while implementing:
  moving the pointer back into the area would start a lasting lock again at once).
- **Rationale**: sway also warps to the hint on destroy; Blender's and SDL's "warp" emulation sets
  the hint and then destroys the lock.

## L10. Bounding regions (FR-012)

- **Finding**: `wl_region.add` today splits every existing rectangle against the new one and keeps
  all the pieces, with no limit. A client can make a region of millions of rectangles; every
  hit-test of its input shape then walks all of them. This is a pre-existing gap against Principle
  II. Other compositors set no limit either: wlroots and Weston keep regions in pixman (canonical
  bands, so the count follows the shape's complexity and a point lookup is a binary search); mutter
  and KWin use pixman-like regions; Smithay (niri, COSMIC) keeps a list of add/subtract operations
  and walks it, like RubickWay.
- **Decision** (owner, 2026-11-19): `wl_region` stays as it is. Only confinement is bounded: when a
  constraint's area or the window's input shape holds more than `MAX_CONFINE_RECTS = 256`
  rectangles, the shell uses its bounding box for the effective area (L5, L8). A real application
  uses a handful, so nothing changes for it; a hostile one only loosens its own confinement. The
  effective area is computed when the area, the input shape or the view changes (at most 256 x 256
  intersections) and is itself replaced by its bounding box if it has more than 256 rectangles, so
  each motion walks at most 256.
- **Possible later change** (owner, 2026-11-19): limits for `wl_region` itself (a cap with a protocol
  error, or a banded representation like pixman's), as a separate change.
- **Alternatives considered**: a cap of 1024 rectangles on every `wl_region` with a protocol error
  (stricter than every other compositor, and it changes behaviour for all clients); an own banded
  region (about 300 more lines of geometry, and a client could still make 100,000 disjoint
  rectangles).

## L11. Bounding relative pointers

- **Decision**: at most 32 relative-pointer objects per client get events (the first 32 live ones,
  in creation order); further ones are created
  (the protocol has no error) but stay inert, with one debug line per client. Every motion therefore
  costs at most 32 events for the focused client.
- **Rationale**: one per `wl_pointer` is normal; a client creating thousands would make every motion
  cost thousands of sends on the loop (Principle VII).

## L12. Server state and commits

- **Decision**: new module `src/server/pointer_constraints.rs`: the constraint objects (lock and
  confine share one `ConstraintData`), a map from surface to its live constraint (for
  `already_constrained`), pending/current area and release point applied in `compositor::commit`.
  The area given with the creating request applies at once; `set_region` and
  `set_cursor_position_hint` apply on the next commit (double-buffered). Areas are converted from
  surface coordinates to window coordinates (minus the window geometry origin, as in
  `popup::layers`) when reported.
- **Reporting**: `WindowEvent::ConstraintChanged { id, constraint: Option<ConstraintInfo> }` when a
  constraint is created, its committed state or the window geometry changes, it is destroyed or
  becomes defunct, and after `Mapped` for a window that already has one. Constraints on surfaces that
  are not window roots are never reported (FR-011).
- New module `src/server/relative_pointer.rs` for the manager and relative-pointer objects.

## L13. Logging (FR-015)

- **Decision**: the server writes the debug lines, as it knows the application
  (`describe_process`, read once when the application connects and kept with its connection, so a
  lock never reads `/proc`; 2026-11-20) and the lifetime: "pointer locked for application 3 (pid 4321, game),
  lasting", "pointer unlocked for ...: release shortcut" with reasons `application`, `release
  shortcut`, `focus`, `pointer left`, `window gone`, `resize`. The lifetime in the line also tells
  the manual checks which applications use single-use and which lasting constraints (quickstart). The reason comes from the shell's decision (`ConstraintEnd`), except when the
  object goes away while it applies: then no decision follows (the shell's would name a gone
  token), so the server logs the stop itself, with `application` when the client destroyed it and
  `window gone` when its surface or client went away. No coordinates.

## L14. Testing

- Shell rules (L4-L9) in a new `tests/pointer_constraints.rs` through `Shell::apply` and
  `Shell::handle`, asserting `ClientInput`s and `shell.pointer()`; `confine` unit tests plus a
  property test (the result is always inside the area when `from` is, for random areas and moves).
- Protocol in `tests/protocol/pointer_constraints.rs`: globals, creation, `already_constrained`,
  defunct and re-creation, double-buffering, `ConstraintChanged` reports, `locked`/`unlocked` from
  decisions, stale tokens, relative motion events (time split, values), caps.
- `tests/protocol/misbehaving.rs`: oversized constraint area, relative-pointer flood, constraint flood, constraint on
  a popup and a cursor surface, each with a well-behaved second client.
- Backend: `translate` unit test for the new fields.
- Real applications: [quickstart.md](quickstart.md).

## L15. A cursor surface without an image hides the pointer (found in quickstart Q1)

- **Finding** (2026-11-20): Firefox's pointer lock captured the pointer but the arrow stayed on
  screen. GTK 3 (Firefox) hides the pointer with `wl_pointer.set_cursor(serial, surface)` and a
  commit of that surface with no buffer, not with `set_cursor(serial, None)` as SDL does. Since
  cursor-shapes, any cursor surface meant "the arrow", so the spec's edge case "the application
  hides the cursor while locked: RubickWay shows nothing" did not hold for GTK applications.
- **Decision**: what the cursor surface shows counts, as in other compositors: with a committed
  buffer it is a cursor image (still drawn as the arrow, as before), without one the pointer is
  hidden. The seat remembers the surface of the latest counted `set_cursor`; each commit of it that
  takes effect is checked again (the serial was checked at `set_cursor`), until another cursor
  request or a new enter. This changes the cursor-shapes contract row for
  `set_cursor(serial, Some(surface))` (contracts/protocols.md "wl_pointer").
- **Rejected**: hiding the pointer whenever a lock applies - the protocol leaves the cursor to the
  application, and a lock without a hidden cursor (a drag in a 3D tool that keeps its own cursor)
  is legitimate.

## L16. The cursor after Mod+Escape (found in quickstart Q2)

- **Finding** (2026-11-20): after Mod+Escape Firefox got `unlocked` (logged "release shortcut") but
  ignores it for its persistent lock: the page still holds the pointer, the cursor stays hidden, and
  the demo turns the now ordinary motion into erratic movement. Firefox leaves its lock only on its
  own Escape or on losing focus. The user could not see where the pointer was.
- **Decision** (owner, 2026-11-20): while a window is released (Mod+Escape, until a button press it
  receives - FR-008), a hidden cursor the application asks for is shown as the arrow over it; other
  shapes it asks for show as usual. Decided in the shell's `cursor()`, from the `released` set; the
  application's choice is kept and counts again after the click. Covers any application that
  ignores `unlocked`.
- **Rejected**: also withholding motion from the released window until the click (the page would
  calm down, but it goes beyond the protocol and hides hover and scrolling from a window the user
  may want to use); changing nothing (Mod+Escape is the user's guarantee, and an invisible pointer
  breaks it in practice).
