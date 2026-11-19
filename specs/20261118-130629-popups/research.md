# Research: Popups

Decisions for the plan, numbered P1-P16. References are for reading only (Principle I): the protocol
XML in `wayland-protocols` 0.32.13 (`protocols/stable/xdg-shell/xdg-shell.xml`, interfaces
`xdg_positioner`, `xdg_popup`), wlroots `types/xdg_shell/wlr_xdg_positioner.c` (the unconstrain
algorithm) and `wlr_xdg_popup.c`, sway `desktop/xdg_shell.c` (popup unconstrain against the output),
mutter `wayland/meta-wayland-popup.c` and `meta-wayland-pointer.c` (popup grab, `can_popup`), KWin
`popup_input_filter.cpp` (the click outside), and Smithay's `PopupManager` and `PopupPointerGrab`.

## P1. Protocol and version

- **Decision**: no new global. `xdg_wm_base` stays at version 6, as today. Popups get everything
  version 6 has for them: `xdg_positioner` with `set_reactive`, `set_parent_size` and
  `set_parent_configure` (version 3), `xdg_popup.grab`, `reposition` and `repositioned` (version 3).
  Version 7 (it adds nothing for popups, but would be a new version to test) is not offered.
- **Rationale**: version 6 is already advertised, so Principle VI asks that its popup parts be
  implemented and tested now; today they are accepted and ignored. GTK 4 and Qt 6 use `reposition`
  for menus that resize.
- **Alternatives considered**: lowering to version 2 to skip `reposition`. Rejected: it would take
  away `suspended` (version 6) and window-manager capabilities (version 5) that windows use today.

## P2. Who decides what

- **Decision**: the same split as windows (client-windows R7, window-management W1).
  - **Server** (`src/server/xdg_shell.rs`, new `src/server/popup.rs`): protocol objects, a copy of
    the positioner's rules as plain data, configure and ack, the protocol errors, the grab's serial
    check, and the popup's surfaces as part of its window for drawing, input, frame callbacks and
    outputs. It applies a placement when the application acks it and commits.
  - **Shell** (new `src/shell/popups.rs` and `src/shell/positioner.rs`, no Wayland types): placement
    (P3, P4), stacking and hit-testing (P5), the grab (P6, P7), and every decision to dismiss (P8).
  - The server tells the shell through new `WindowEvent`s; the shell answers with new
    `WindowCommand`s (data-model.md). The shell never calls the server.
- **Rationale**: placement needs the viewers' cameras and dismissing needs the bindings and gestures;
  both live in the shell, which is tested headless (Principle IV). The server keeps what only the
  protocol knows: objects, serials and errors.
- **Alternatives considered**: placing popups in the server. Rejected: the server would need viewers
  and cameras, which belong to the shell.

## P3. Placement

- **Decision**: a pure function `place(rules, parent, area) -> Rect` in `src/shell/positioner.rs`,
  in integer coordinates relative to the parent's window geometry:
  1. Start from the anchor point on the anchor rectangle (the anchor edge or corner, the centre for
     `none`), plus the offset, and grow the popup from there in the gravity's direction (centred on
     an axis the gravity does not name).
  2. Per axis, if the popup is not inside `area` on that axis:
     - **flip** (if allowed): invert anchor and gravity on that axis and recompute from the original
       anchor rectangle and offset; keep the flipped result only if it fits on that axis, otherwise
       go back to the result before the flip;
     - **slide** (if allowed): move it back inside; a popup larger than the area is aligned to the
       area's start edge (left or top), so its beginning is visible;
     - **resize** (if allowed): cut it to its intersection with the area; if nothing would be left
       (the popup lies wholly outside), keep the size.
  3. Without an allowed adjustment, the popup stays where step 1 put it.
- `area` is `None` when no monitor shows the window (P4): step 1 only.
- All arithmetic is in `i64` with saturation back to `i32` (Principle II); sizes stay at least 1.
- **Rationale**: the protocol fixes the order (flip, slide, resize) and leaves "constrained" to the
  compositor. wlroots `wlr_xdg_positioner_rules_unconstrain_box` implements exactly these steps;
  aligning an oversized popup to the start edge is what wlroots and Smithay do, and it keeps a long
  menu's first items visible.
- **Testing**: unit and property tests (proptest, already a dev-dependency): a popup that fits with
  slide or resize allowed ends inside the area; with no adjustment the result equals step 1; flipping
  twice gives back the original; every anchor and gravity value is covered at zoom 1 and with areas
  taken from zoomed cameras (SC-005).

## P4. The constraint area

- **Decision**: the canvas rectangle one viewer shows (`view_rect_of`), converted into the parent's
  coordinates and rounded inwards to integers. The viewer is chosen when the popup is placed:
  1. the viewer under the pointer, if it shows any part of the popup's window;
  2. otherwise the viewer that shows the largest part of the window;
  3. otherwise none: no adjustment (spec Edge Cases).
- **Rationale**: spec Edge Cases. Because the area is in canvas units, the same code works at any
  zoom (FR-019): at 50% zoom the viewer shows twice as many canvas units, and the popup is drawn at
  half size like its window.
- **Alternatives considered**: the union of all viewers. Rejected: a menu could then be placed across
  the gap between two monitors.

## P5. Drawing, input and frame callbacks

- **Decision**: a mapped popup is part of its toplevel window everywhere a window's surfaces are
  walked.
  - **Server**: a new `layers(window)` lists the window's surface trees in drawing order: the
    toplevel's tree, then each shown popup's tree in creation order (a child is always created after
    its parent, so it comes later), each with its origin in window coordinates.
    `window_parts`, `surface_at` (topmost first), `surface_origin`, `take_damage`, `presented`,
    `has_pending_callbacks`, `set_window_outputs` and `announce_scale` use it.
  - **Shell**: `Window` gets its popups (rect in window coordinates and input shape). `Window::extent`
    (the window rect together with its popups' rects) replaces `rect` for visibility, so a popup that
    sticks out of a window at the edge of the view is still drawn. Hit-testing a window tries its
    popups, topmost first, before the window itself, at the window's place in the stack (FR-004).
- **Rationale**: the renderer already draws `window_parts` in order, so drawing needs no change; input
  already goes to "window + position", and the server picks the surface. Frame callbacks and
  `wl_surface.enter` reach popup surfaces through the same paths, so GTK's popups keep animating and
  know their scale.
- **Alternatives considered**: popups as separate top-level entries in the shell's stack. Rejected:
  they would need their own stacking rules to stay with their window (FR-004), and everything that
  walks windows (fullscreen, visibility, outputs) would need a second case.

## P6. The grab

- **Decision**:
  - **Server**: `xdg_popup.grab` is accepted if the serial is that application's latest button press
    (kept after the release, unlike `last_press`) or latest key press or release (P9), and the popup
    has not been committed yet. Otherwise:
    - grab after the initial commit: `invalid_grab`;
    - the parent is a popup without a grab: `invalid_grab` (the protocol says "an error will be
      raised");
    - a wrong serial, or the parent is a grabbing popup already dismissed: the popup is dismissed at
      once (`popup_done`), no error.
  - **Shell**: one grab at a time, `PopupGrab { app, chain }`. A grabbing popup is accepted when no
    RubickWay gesture runs (move, resize, pan); otherwise it is dismissed. Its parent is the window
    or the chain's topmost popup (the server already checked this). A grab of another application,
    or of another window's chain, dismisses the old chain first.
  - **Pointer during the grab**: "owner events", as the protocol says: the grabbing application's
    windows and popups get the pointer as usual; other applications' windows get nothing (the pointer
    leaves them). The shell knows which windows belong to which application through a new `app` field
    of `WindowEvent::Mapped`.
  - **A press outside** every surface of the grabbing application dismisses all its popups. The
    press and its release are swallowed (FR-009, as mutter and KWin). A press on any surface of the
    application, including its window outside the menus, is delivered; the application closes its
    menus itself (spec Clarifications, analyze I1). A press on another window of the same application
    is delivered too, and focuses that window as any click does; the focus change then closes all
    popups (P8, analyze M2).
  - **Bindings first**: a press that matches a pointer binding (Mod + left, Mod + right,
    Mod+Shift + left) is not a press outside: all popups close (P8) and the gesture starts at once.
    A left press on the background has no binding while a grab is open: it is a press outside (spec
    Clarifications, analyze A1).
  - **Press, drag, release**: a menu opened on a press must take the release over one of its items.
    While a grab chain is open, the server re-picks the surface under the pointer among the grab
    window's surfaces and popups even while a button is held, and the shell's implicit grab (`router.grab`)
    becomes the popup grab. As in mutter's popup grab.
- **Rationale**: the protocol's rules, with the swallowed click confirmed by the owner (spec
  Assumptions).

## P7. The keyboard during a grab

- **Decision**: a new `ClientInput::PopupKeyboard { window, popup: Option<PopupId> }` moves the
  keyboard focus between a window's root surface and one of its popups, without changing which
  window is focused or `activated`. Keys (`ClientInput::Key { window, .. }`) go to the surface that
  has the focus, as today. The shell sends it when a grab starts (the topmost grabbing popup), when a
  nested grab opens or closes, and with `popup: None` when the grab ends (FR-013).
- **Rationale**: the protocol gives the keyboard to the topmost grabbing popup. GTK expects
  `wl_keyboard.enter` on the popup surface. A separate input leaves `KeyboardFocus` and the many
  tests that use it unchanged, and the window keeps `activated`, so its title bar does not flicker.

## P8. When the shell dismisses popups

- **Decision**: one function, `Shell::dismiss_popups(scope)`, that removes popups from the shell's
  state and pushes `WindowCommand::DismissPopup` for each, topmost first. It is called:
  - before any binding action (key bindings and pointer bindings);
  - when a move, resize or pan starts;
  - after an input event, if any viewer's camera changed during it (pan, zoom, view reset, entering
    or leaving fullscreen, focus cycling that moves the view): compared to a snapshot taken before
    the event;
  - when keyboard focus moves to another window (`focus_changed`);
  - when any window enters or leaves maximize or fullscreen, whoever asked (spec Clarifications,
    analyze I4);
  - when a viewer is removed;
  - for a press outside the grabbing application (P6) and for a new grab elsewhere (P6).
  - The scope is all popups in every case, except the press outside and the new grab, which close
    only the grabbing application's popups.
- **Rationale**: FR-010 lists RubickWay's actions; comparing cameras covers every path that moves a
  view (scroll, pinch, keys, fullscreen) without a call in each. A dismissed popup is never shown
  again (FR-012): the shell forgets it at once, and the server stops drawing it at once, even if the
  application keeps committing until it destroys it.
- **Order**: topmost first, for each chain (FR-012, the protocol's dismissing order).

## P9. The serial for a grab

- **Decision**: the server keeps, per application, the serial of its latest button press and of its
  latest key press or release. A grab counts if its serial matches either.
- **Rationale**: mutter's `meta_wayland_pointer_can_popup` and `meta_wayland_keyboard_can_popup` do
  the same. Some toolkits open a menu on the release, with the press's serial, so the press serial
  has to outlive the release. `last_press` is not reused: it is cleared on release on purpose
  (window-management FR-016).

## P10. Reactive popups and moving parents

- **Decision**:
  - A popup's position is relative to its parent, so it follows its window when the window moves for
    any reason other than a RubickWay action (those dismiss it, P8): FR-005.
  - When the parent's size or position changes (a window `Updated` with a new size, a parent
    popup placed again, or a parent popup's `PopupUpdated` with a new size - the last added by
    convergence T040), the shell places each **reactive** child again and sends a new placement
    without a token. Non-reactive children keep their relative position.
  - `set_parent_size` and `set_parent_configure` are accepted, checked and kept, but not used.
    Placement (P3) depends only on the anchor rectangle, which is already relative to the parent's
    window geometry, and on the constraint area; the parent's size is not an input. The two hints
    would matter only for constraining against a parent's future position, which RubickWay does not
    do: the protocol says the compositor "may" use them. (Found while implementing P3, 2026-11-18;
    the plan first said `set_parent_size` would be used.)
- **Rationale**: the protocol's `set_reactive`. Reactive repositioning matters when an application
  resizes its window while a popup is open (an autocomplete list).

## P11. Reposition

- **Decision**: `reposition(positioner, token)` copies the new rules and reports
  `PopupRepositionRequested`; the shell places it (P3, P4) and answers with the token. The server
  sends `repositioned(token)`, `xdg_popup.configure`, `xdg_surface.configure`. Only the newest
  request waiting for an answer is kept (the protocol allows skipping all but the last). The new
  position takes effect at the commit after the ack, as the protocol says.

## P12. Configure, ack and map

- **Decision**: as for toplevels:
  - the initial commit (no buffer) of a popup reports `PopupRequested` (rules, parent, grab); the
    shell answers `PlacePopup` or `DismissPopup`;
  - the server sends `xdg_popup.configure(x, y, w, h)` and `xdg_surface.configure(serial)`;
  - a buffer before the first ack is `unconfigured_buffer`, as for windows;
  - the popup is shown at the first commit with a buffer after the ack (`PopupMapped`), and later
    commits that change its position, size or input shape report `PopupUpdated`;
  - a commit without a buffer hides it and ends it: the popup's children are dismissed first, then the
    popup gets `popup_done` and the shell `PopupUnmapped`; a later buffer is never shown. Destroying
    its `wl_surface` first ends it the same way, with no error, as windows today (analyze U1, I3).
    Re-mapping a popup would need a new placement for an object the protocol treats as short-lived;
    ending it is simpler and the application opens a new one.
- **Parent rules**:
  - the parent must be an `xdg_surface` with a role (toplevel or popup), otherwise
    `invalid_popup_parent` (a client cannot name another client's object);
  - no parent at all: dismissed at the initial commit, no error (RubickWay has no other protocol to
    set one, spec Misbehaving Clients);
  - a parent that is not mapped at the initial commit: dismissed at once, no error;
  - a grabbing popup whose parent is not the topmost grabbing popup when it maps:
    `not_the_topmost_popup`;
  - destroying an `xdg_popup` that still has shown children: `not_the_topmost_popup`.
- **Rationale**: the protocol XML. Dismissing instead of an error where the protocol leaves room keeps
  applications running (Principle II applies to RubickWay, not to them, but there is no reason to
  disconnect where a dismissal suffices).

## P13. A window going away

- **Decision**: when a toplevel unmaps or is destroyed, the server dismisses all its popups (each
  gets `popup_done`, topmost first) and stops drawing them; the shell drops them with the window
  (`Unmapped`). When an application disconnects, its objects are destroyed and the same happens
  without events to send.
- **Rationale**: FR-011.

## P14. Drawing and damage

- **Decision**: no renderer change. A popup's content changes damage its window (P5); its map,
  unmap and placement set the server's `dirty` flag, as subsurfaces do, so every display showing it
  redraws once in the next frame (FR-016).
- **Rationale**: today's damage is per display: a display redraws when a window it shows changed. A
  popup is part of its window, so this already holds.

## P15. Misbehaving applications

- **Decision**: each case of the spec gets a protocol test (contracts/protocols.md). Values from the
  positioner are checked when set (size at least 1, anchor rectangle not negative, enum values in
  range: `invalid_input`), and placement saturates (P3). A flood of `reposition` requests keeps only
  the newest waiting one (P11). No popup can be shown outside its own window's group, so an
  application can only cover its own windows' place in the stack.

## P16. A test program

- **Decision**: `examples/popups.rs`, a small `wayland-client` program (already a dev-dependency) for
  the quickstart: a window whose right-click opens a grabbing menu at the pointer, with keys to open a
  nested submenu, a tooltip without a grab, a reactive popup, a reposition, and a popup that allows
  no adjustment. It makes the edge cases repeatable by hand; real applications cover the success
  criteria.
- **Rationale**: as `examples/cursor_shapes.rs` for cursor-shapes; GTK and Firefox menus cannot be
  told which adjustments to allow.
