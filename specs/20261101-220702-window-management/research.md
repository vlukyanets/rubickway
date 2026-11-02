# Research: Window Management on the Canvas

Builds on `specs/20261101-175316-client-windows/research.md` (R1-R14). Decisions here are
numbered W1-W14. References: wlroots `types/xdg_shell/wlr_xdg_toplevel.c` and sway
`input/seatop_move_*.c`, `seatop_resize_*.c` (interactive move/resize, serial checks, anchored
resize); Smithay `wayland/shell/xdg/mod.rs` (configure state, `ToplevelState`); niri
`layout/tile.rs` and `input/resize_grab.rs` (resize anchor kept until the final configure is
committed). Reference only - no code is taken from them (Principle I).

## W1. Who decides: shell commands, server executes

- **Decision**: all window-management decisions stay in the pure `shell` (Principle IV). The
  server reports requests from applications as new `WindowEvent` variants (`MoveRequested`,
  `ResizeRequested { edges }`, `MaximizeRequested(bool)`, `FullscreenRequested(bool)`) and the
  application's size limits in `Mapped` / `Updated`. The shell answers through a new queue of
  `WindowCommand`s (`Configure { window, size, states }`, `Close { window }`), taken by `App`
  with `Shell::take_commands()` and passed to `Server::command(&[WindowCommand])`.
- **Rationale**: keeps the client-windows split (R7: the server emits, `App` applies, the shell
  never calls the server); every rule (gestures, anchors, maximize geometry) is unit-testable
  without Wayland. A queue like `take_window_events` instead of changing `handle`'s return type
  keeps the existing input API and tests unchanged.
- **Alternatives considered**: extending `ClientInput` with configure/close variants (mixes
  input and window state under an input name); the server deciding move/resize itself (puts
  canvas geometry into protocol code, untestable without clients).

## W2. Validating interactive move and resize requests

- **Decision**: the server records, per seat, the serial and window of the latest
  `wl_pointer.button` press it delivered. `xdg_toplevel.move` / `resize` are forwarded to the
  shell only if their serial equals that serial and the window matches; the shell accepts them
  only while its pointer grab (client-windows `Grab`) is on that window with a button held.
  Otherwise the request is ignored, without an error (FR-016; xdg-shell says the compositor
  "may ignore" such requests). `edges` = `none` is a valid enum value and is ignored; values
  outside the enum are a protocol error (`xdg_toplevel.invalid_resize_edge`), as the protocol
  requires.
- **Rationale**: two independent checks - the server knows serials, the shell knows which
  buttons are still held - so a stale request or one after the release does nothing. Same rule
  as wlroots (`wlr_seat_validate_pointer_grab_serial`).
- **Alternatives considered**: accepting any serial (lets an application drag its window
  without a press - misbehaving case); keyboard serials (not needed: no keyboard-started moves).

## W3. Interactive operations in the shell

- **Decision**: a new routing state next to `Grab`: `Op::Move { window, offset }` and
  `Op::Resize { window, edges, start_pointer, start_rect }`, started by an accepted request (the
  grab turns into the operation) or by a compositor gesture (Mod + Shift + left on a window ->
  move; Mod + right on a window -> resize, edges from a 3 x 3 split of the window rect, the
  centre cell mapping to the nearest corner). While an operation runs the window gets a
  `PointerLeave` and no pointer input; buttons are tracked so the last release ends it; a host
  `PointerLeave` or the window unmapping also end it; losing the keyboard (`FocusLost`) does not
  (spec Edge Cases), unlike a client-windows grab. During an operation further button presses
  are swallowed, scroll without Mod is dropped, the `NextWindow` / `PreviousWindow` /
  `ResetView` actions are consumed and do nothing, `CloseWindow` and `Quit` work, and keys go to
  the focused window as usual (spec Edge Cases "Other input during a move or resize"). The 3 x 3
  split uses half-open cells (`[0, w/3)`, `[w/3, 2w/3)`, `[2w/3, w]`), so a boundary point goes to
  the cell further right or down; in the centre cell a tie between sides picks bottom / right.
  Move: `origin = pointer_canvas - offset` on every motion, and again after Mod + scroll or
  pinch zooms the canvas (the viewer keeps handling those during a move), so the grabbed point
  stays under the pointer (FR-003, SC-001). Resize: wanted size = `start_rect` grown on the
  dragged edges by `pointer_canvas_now - start_pointer` (both canvas points, so after a zoom
  during the resize the dragged edge is still under the pointer), clamped (W6), sent as
  `Configure` with the `resizing` state.
- **Rationale**: one state machine for both sources keeps behaviour identical; canvas units
  (screen delta / zoom) make the window follow the pointer at any zoom.
- **Alternatives considered**: moving windows in screen pixels (drifts when zooming during a
  move); letting the client see motion during a move (xdg-shell expects the compositor to take
  the pointer).

## W4. Anchors: which point stays fixed when the size changes

- **Decision**: each window carries an `Anchor` that decides how `origin` changes when the
  application commits a new size: `TopLeft` (default, client-windows clarification 2),
  `Edges { fixed }` during and after a resize (the edges opposite the dragged ones keep their
  canvas coordinates), `Centre { point }` for fullscreen and for a maximized window whose
  maximum size is smaller than the viewer. The resize anchor stays until the application has
  committed the configure sent at the end of the resize (the first one without `resizing`);
  then it returns to `TopLeft`. For that the server reports in `Updated` whether the commit
  applied the newest configure that carried a size or state from the shell (`configured:
  bool`; activation-only configures do not count) and emits `Updated` also when only that flag
  turned true, so windows that never get such a configure (all existing tests) see no extra
  events.
- **Rationale**: applications answer resize configures asynchronously and may pick other sizes
  (kitty snaps to cells), so the anchor must apply to whatever size arrives, including the
  final one after the button release (niri does the same). Three uses justify the type
  (Principle VI).
- **Alternatives considered**: computing the origin once when the configure is sent (wrong when
  the client picks another size); dropping the anchor on button release (the final commit
  jumps).

## W5. Configure state in the server

- **Decision**: the server keeps per toplevel the state the shell last asked for: `size`
  (`None` = 0 x 0, the application chooses) and the `maximized`, `fullscreen`, `resizing`
  states, plus `activated` from keyboard focus as before. Every configure carries all of them.
  At most one configure with a new size is outstanding: while one is unacked, a newer wanted
  size replaces the queued one and is sent when the application acks (coalescing, Edge Cases).
  When a resize settles, the shell sends `Configure { size: None }`, so later activation
  changes do not fight sizes the application picks itself. `wm_capabilities` (v5+) advertises `maximize` and
  `fullscreen` only; `minimize` and `window_menu` are not advertised, and their requests stay
  ignored.
- **Rationale**: xdg-shell configures are full state, not deltas; one outstanding size bounds
  the work a slow client causes (FR-018) - the window keeps showing its last drawn size.
- **Alternatives considered**: a configure per pointer motion (floods a slow client, R8 limit);
  advertising `window_menu` without a menu (Principle VI).

## W6. Size limits

- **Decision**: `set_min_size` / `set_max_size` are double-buffered and applied on commit (as
  the protocol says); `Mapped` / `Updated` carry `SizeLimits { min, max }` (0 = unlimited). The
  shell clamps every wanted size to `[max(min, 1), min(max, largest)]`, where `largest` is the
  largest window RubickWay can display in logical pixels: the GPU texture limit divided by the
  ceiling of the output scale, given by `App` through `Shell::set_largest_window` (unlimited
  until the renderer exists, like the server's own check; the viewer size used for maximize and
  fullscreen is the configured one, re-sent by `viewer_configured`). A max below
  min (allowed but contradictory) uses min. The bounds are applied in order - lower bound
  `max(min, 1)`, then the application's max (if not below min), then `largest` last - so the
  displayable limit always wins (spec FR-008).
- **Rationale**: never ask a well-behaved client for a size that client-windows case 7 would
  disconnect it for (FR-008); the ceiling covers fractional scales, whose buffers are rounded up.
- **Alternatives considered**: clamping in the server (it would have to know the shell's
  wanted size rules).

## W7. Maximize

- **Decision**: on `MaximizeRequested(true)` (from the focused window, W13) the shell saves the
  window's rect, sets the camera to zoom 1 centred on the window's centre, anchors the window
  there (`Anchor::Centre { point }`) and sends `Configure` with the viewer's logical size
  (clamped, W6) and `maximized`. Whatever size the application draws - the viewer size, a
  smaller maximum, a larger minimum, or its old size if it ignores the request - stays centred
  on that point, which is the view centre. Pan and zoom do not end it (clarification); leaving
  (`MaximizeRequested(false)`, a compositor resize, or a resize request) restores the saved rect:
  origin at once, size via `Configure { size: saved }` with `TopLeft`; a resize then continues
  with `start_rect` = the saved rect. A move first restores the saved size with the grabbed
  point at the same relative x position (`offset.x * saved_w / current_w`), y offset kept. A
  viewer resize re-sends the new viewer size, point kept. A window maximized before it was
  first shown has no saved rect: leaving sends `size: None` (the application chooses) and keeps
  the centre.
- **Rationale**: the owner's choice (spec clarification). Growing around the centre (as
  DriftWM's `fit_window`) makes the maximize view, the "smaller maximum" case and fullscreen
  one rule, `Anchor::Centre`.
- **Alternatives considered**: keeping the window's top-left as the view's top-left (an earlier
  draft; needs a second rule when the size differs from the viewer).

## W8. Fullscreen

- **Decision**: at most one fullscreen window (`Shell::fullscreen: Option<Fullscreen { window,
  saved_rect, saved_camera }>`). Entering saves rect and camera, focuses and raises the window,
  sets the camera to zoom 1 centred on the window, sends the viewer size with `fullscreen`, and
  anchors at the view centre (`Anchor::Centre`). For a maximized window, `saved_rect` is its
  maximized rect and its `Mode::Maximized { saved }` stays, so leaving fullscreen returns it to
  maximized (two restore rects, as KWin's `fullscreenGeometryRestore` and Mutter's
  `saved_rect_fullscreen`); the configure keeps `maximized` alongside `fullscreen`. While
  fullscreen: `visible()` returns only that window, and `App` draws only it over a black
  background (no grid) - `GlesRenderer` gains a plain clear; frame callbacks follow `visible()`
  as before. It ends when the application asks, when another window goes fullscreen (so the
  saved camera is always the pre-fullscreen one), when the window unmaps (the camera is
  restored), when a child of it maps (W14), on a move or resize gesture on it (move: the saved
  size comes back under the pointer as in W7; resize: ends there, consumed), and before any
  canvas navigation (pan press, Mod/background scroll, pinch begin, Mod+0) or focus-cycling
  shortcut, which are consumed (the user's next gesture then works normally); the saved rect and
  camera are restored - except for focus cycling, which sets the camera for the next window
  directly. The output passed in the request is ignored (one viewer). The host window is not
  changed (clarification).
- **Rationale**: "fills the viewer, nothing else drawn" (FR-014) with a simple, testable rule;
  consuming the navigation event avoids applying a pan computed against the fullscreen view.
- **Alternatives considered**: drawing other windows behind (they would show around a client
  that picks a smaller size); asking the host for fullscreen too (rejected in clarification).

## W9. Focus history and cycling

- **Decision**: `Windows` keeps a most-recently-used list next to the stacking order. Every
  focus change (map, click, operation start, shortcut) moves the window to the front, except
  while cycling: Mod+Tab / Mod+Shift+Tab step an index through the list (wrapping), focusing
  and raising each window (each gets real keyboard focus), and the list is reordered once when
  Mod is released (a `Modifiers` event without Mod) or on `FocusLost` (the release may never be
  seen). A new window goes to the front of the list (second while another window is
  fullscreen, W14). After each step the camera is set per FR-012: if the window rect is fully
  inside the view (no margin), the camera stays - also with a single window (as DriftWM's
  `reveal_element` and niri's `compute_new_view_offset`); otherwise centre on it with zoom =
  `min(current, fit)`, where `fit` makes the rect fill at most 90% of the view in each direction,
  clamped to the zoom limits (so at least 10%); a maximized window gets its maximize camera (W7)
  instead; no animation.
- **Rationale**: classic Alt-Tab semantics without a switcher UI (spec Assumptions); one press
  toggles between the two latest windows.

## W10. Shortcuts

- **Decision**: three new `bindings::Action`s: `NextWindow` (Mod+Tab), `PreviousWindow`
  (Mod+Shift+Tab - the binding symbol is the level-0 symbol, so Shift+Tab still matches `Tab`),
  `CloseWindow` (Mod+Q). The shell handles them (they need windows); `ResetView` and `Quit`
  stay in the viewer. Consumed with their releases as in client-windows.
- **Rationale**: one table for all shortcuts; the shell already intercepts keys before the
  viewer.

## W11. Cursor shapes

- **Decision**: the shell reports the wanted cursor as a pure enum (`CursorShape::{Default,
  Grabbing, Move, Resize(Edges)}`); `App` maps it to the host backend's `Cursor`, which gains
  `Move` and the eight resize shapes of `wp_cursor_shape_device_v1` (already bound, version 1).
  Panning keeps `Grabbing`.
- **Rationale**: FR-009 with no new host protocol; the core stays backend-neutral.

## W12. Blocking and robustness

- **Decision**: no new blocking work (Principle VII): operations are memory updates, configures
  are queued socket writes bounded by R8. Misbehaving inputs and their handling: stale or
  foreign serials (ignored, W2), invalid resize edges (protocol error), negative min/max (already
  an error), a client that never acks during a resize (one outstanding configure, W5), a window
  unmapping mid-operation or while fullscreen/maximized (state dropped with the window), sizes
  beyond limits (W6), maximize or fullscreen requests from unfocused windows (ignored, W13). Each
  has a protocol or shell test.

## W13. Who may maximize or go fullscreen, and initial state

- **Decision**: the shell honours `MaximizeRequested` / `FullscreenRequested` (on or off) only
  for the window with keyboard focus; others are ignored, and the server's last configure stays
  valid. Requests made before the first commit are kept by the server. If there are any, the
  server assigns the window id at the initial commit (instead of at map), emits a new
  `WindowEvent::Created { id, parent, limits, maximized, fullscreen }` and holds the initial
  configure; the shell answers with a `Configure` for that window in the same loop iteration
  (`App` takes commands right after handling events), and the server sends it as the initial
  configure: `maximized` with the viewer size; `fullscreen` (with `maximized` kept if both were
  asked) with the viewer size; no state (size `None`) if the window will open without focus
  (W14). Windows that request no initial state get the initial configure at once, as before,
  and no `Created` (existing behaviour and tests unchanged). When the window maps, it is placed
  centred in the view as before and enters the state there (W7 / W8).
- **Rationale**: all six reference compositors apply unfocused requests (niri, KWin, Hyprland
  silently; Mutter raising fullscreen; DriftWM taking focus and camera), but on the canvas both
  states move the view, so a background application could take it at any time; the owner chose
  to ignore them (pre-tasks review). Requests come from user input inside the focused window in
  practice, so little is lost. Initial state in the first configure follows niri
  (`send_initial_configure`), KWin (`initialMaximizeMode`) and Mutter; sway and Hyprland apply
  fullscreen only after map (an extra resize) and drop initial maximize.
- **Alternatives considered**: applying the state silently (needs a "fullscreen but not shown"
  state and a view-free maximize); applying initial state after the first map (extra resize,
  visible flash); the server deciding the initial configure (it does not know the viewer size).

## W14. New windows while a window is fullscreen; parents

- **Decision**: the server stores `xdg_toplevel.set_parent` (a mapped toplevel or none; when a
  parent unmaps, its children get its own parent; the toplevel itself or a descendant is
  `invalid_parent` - all as the protocol says) and reports it in `Created` (W13) and
  `Mapped { parent }`; the shell decides at `Created` whether the window will open without
  focus, and re-checks at `Mapped`.
  When a window maps while another is fullscreen: if its parent is the fullscreen window,
  fullscreen ends first (camera and rect restored) and the window maps as usual; otherwise
  fullscreen stays and the window is placed centred in the saved camera's view, on top of the
  stack but not focused, second in the MRU list (so Mod+Tab, which leaves fullscreen, goes to
  it), and its pending maximize / fullscreen requests count as unfocused (W13).
- **Rationale**: DriftWM, niri and sway keep fullscreen and do not focus the new window ("Don't
  steal focus from an active fullscreen window", niri `workspace.rs`); niri, sway (`smart`) and
  KWin treat dialogs of the fullscreen window specially. RubickWay draws nothing over a
  fullscreen window, so a dialog shown "above" it would be hidden; ending fullscreen for it
  avoids an invisible dialog holding focus (a DriftWM pitfall). Placing the window in the saved
  view makes it visible as soon as fullscreen ends.
- **Alternatives considered**: ending fullscreen for every new window (Hyprland's default
  `exit_fullscreen`; a background application could break fullscreen video); keeping fullscreen
  for dialogs too (hidden dialog); drawing dialogs over the fullscreen window (a stacking rule
  for one case, Principle VI).
