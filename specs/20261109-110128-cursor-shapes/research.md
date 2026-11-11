# Research: Cursor Shapes

Decisions for the plan, numbered C1-C12. References are for reading only (Principle I): the protocol
XML in `wayland-protocols` 0.32.13 (`protocols/staging/cursor-shape/cursor-shape-v1.xml`), wlroots
`types/wlr_cursor_shape_v1.c` and `types/seat/wlr_seat_pointer.c`, Smithay's `cursor_shape` module,
the freedesktop icon theme specification and the CSS `cursor` property names.

## C1. Protocol and version

- **Decision**: offer `wp_cursor_shape_manager_v1` at version 2, always, with no setting. Version 2 is
  the newest in `wayland-protocols` 0.32.13; it adds the shapes `dnd_ask` (35) and `all_resize` (36).
  The crate's `staging` feature is already on, so no dependency changes.
- **Rationale**: FR-001 asks for the newest version RubickWay implements and tests. Version 2 costs
  two extra entries in the shape table and one version check (C4), both tested, so Principle VI
  holds.
- **Alternatives considered**: version 1 only. Rejected: applications written for version 2 could
  not name `all_resize` and `dnd_ask`, and version 2 is no harder to test.
- `get_tablet_tool_v2` takes a `zwp_tablet_tool_v2`, an object no client can have, since RubickWay
  offers no tablet protocol. There is nothing to handle (spec, Misbehaving Clients).

## C2. Which requests count

- **Decision**: a cursor request counts only when both of these hold, otherwise it is ignored with
  no error:
  - the pointer is over a surface of the requesting client;
  - the request's serial is the one of the latest `wl_pointer.enter` sent to that client.

  This applies to all three ways an application sets the cursor: `set_shape`, and `set_cursor`
  with or without a surface. At every `wl_pointer.enter` the cursor goes back to the arrow, until
  the application asks for something (FR-003).
- **Rationale**:
  - The protocol says so for `set_shape`, and the same holds for `wl_pointer.set_cursor`. wlroots
    checks both conditions for both requests (`wlr_seat_pointer_request_set_cursor`).
  - The protocol expects a client to set its cursor after each enter, so resetting on enter is what
    clients are written for.
  - The same rule for all three requests is what makes "the latest of them decides" (FR-004)
    well-defined.
- **What changes from today**: today hiding (`set_cursor` without a surface) checks neither serial
  nor focus. It is kept per client and lasts across enters. With the new rule, a hide request with a
  stale serial is ignored, and hiding ends at the next enter until the application hides again.
  Applications that hide the pointer (kitty while typing, video players) do so with the enter serial
  while they have the pointer, so they keep working. The existing protocol test that hides with
  serial 0 before any enter changes with this rule (tasks note it).
- **Gestures**:
  - A RubickWay gesture (move, resize, pan) starts with `wl_pointer.leave` and ends with a new
    `wl_pointer.enter` (`Shell::leave` and `retarget`). The application names its shape again on
    that enter, and that is how the shape "comes back" after a gesture (FR-005).
  - A request made during the gesture is ignored, because the application has no pointer then. The
    spec's edge case is worded this way.
- **Alternatives considered**:
  - Keep today's unchecked hiding and check only shapes. Rejected: two rules for one cursor make
    FR-004 ambiguous, and a stale hide could outlive the enter it was meant for.

## C3. Where the state lives

- **Decision**: three layers, each with one job.
  1. **Server** (`src/server/seat.rs`, `src/server/cursor_shape.rs`):
     - knows the pointer's surface, its window and the latest enter serial;
     - keeps the cursor the focused client asked for since that enter (`focus_cursor`);
     - when it changes, pushes one `WindowEvent::CursorChanged { id, cursor }` for the window under
       the pointer, and only then. This holds for a request and for the reset at an enter (for
       example, moving between subsurfaces of one window).
  2. **Shell** (`src/shell/`):
     - keeps the latest `CursorChanged` for the window it has entered;
     - forgets it when the pointer leaves that window or the window is unmapped;
     - `cursor()` puts gestures and panning first, then that window's cursor, then the arrow.

     The shell's types (`CursorName`, `AppCursor`) are plain enums with no Wayland types, so it
     stays headless-testable (Principle IV).
  3. **App** (`src/standalone.rs`): maps the shell's `CursorShape` to a pointer image (C5) and
     redraws when the image changes (C9).
- **Rationale**: it follows today's flow for hidden pointers (server -> `WindowEvent` -> shell ->
  `CursorShape` -> image), with the per-client list replaced by state tied to the current enter
  (C2). Every decision has a home that tests can reach: protocol tests for the server, `tests/shell.rs`
  for the shell, unit tests for the mapping.
- **Alternatives considered**:
  - App asks the server directly. Rejected: gestures would then be decided in two places.
  - One event per window of the client, as today's hiding. Rejected: the state is per enter, not per
    client (C2).

## C4. Errors and lifecycles

| Situation | Answer |
| --- | --- |
| `set_shape` with a value outside 1-36 | protocol error `invalid_shape` |
| `set_shape` with 35 or 36 on a device of a manager bound at version 1 | protocol error `invalid_shape` (the entries are `since="2"`) |
| `set_shape` on a device whose `wl_pointer` was destroyed | ignored |
| `wp_cursor_shape_device_v1.destroy` | the shape stays as it is (the protocol says so) |
| `wp_cursor_shape_manager_v1.destroy` | devices made from it keep working |
| two devices for one pointer | both work; the latest request of either counts |
| client disconnects | its surfaces go, so the pointer leaves its window; the shell forgets that window's cursor at unmap |
| very many requests | each request is a few comparisons; an event only on change; at most one redraw per frame (C9) |

wayland-server hands the shape over as `WEnum<Shape>`. `WEnum::Unknown` and the two version 2
values on an older device are the error cases. The device keeps its `WlPointer` as user data and
checks `is_alive()`.

## C5. Pointer images: one per shape

- **Decision**: the image table has one entry per protocol shape (36) plus RubickWay's window move
  gesture, 37 in all. `CursorKind` becomes `Shape(CursorName) | WindowMove`.
  - The pan gesture uses `Shape(Grabbing)`.
  - The resize gesture uses the shape of its axis: `EwResize`, `NsResize`, `NwseResize`,
    `NeswResize`.
  - The theme names those entries look up are the same as today's, so RubickWay's own images do
    not change (US3).

  Each entry looks up its own list of Xcursor names in the theme (FR-009). Built-in images come
  from 19 drawings, the ones FR-006 lists, assigned as FR-007 says.
- **Rationale**:
  - A theme often has images for shapes the built-in set shares, for example `e-resize` next to
    `ew-resize`, or `copy`. Keying by shape shows the theme's own image where it has one.
  - The window move gesture keeps its own entry, because its theme names (`move`, `fleur`) differ
    from the drag-and-drop `move` shape's (`move`, `dnd-move`).
- **Alternatives considered**: one entry per built-in drawing (19). Rejected: a theme's `copy` or
  `e-resize` image could never be shown.

| Shape | Theme names, in order | Built-in drawing | Hotspot |
| --- | --- | --- | --- |
| `default` | default, left_ptr | arrow | tip |
| `context_menu` | context-menu | arrow (FR-007) | tip |
| `help` | help, question_arrow, whats_this, left_ptr_help | help: small arrow with `?` | tip |
| `pointer` | pointer, hand2, pointing_hand, hand1 | hand, index finger up | fingertip |
| `progress` | progress, left_ptr_watch, half-busy | progress: small arrow with hourglass | tip |
| `wait` | wait, watch | hourglass | centre |
| `cell` | cell, plus | thick plus | centre |
| `crosshair` | crosshair, cross, tcross | thin cross | centre |
| `text` | text, xterm, ibeam | I-beam | centre |
| `vertical_text` | vertical-text | I-beam on its side | centre |
| `alias` | alias, dnd-link, link | arrow (FR-007) | tip |
| `copy` | copy, dnd-copy | arrow (FR-007) | tip |
| `move` | move, dnd-move | arrow (FR-007) | tip |
| `dnd_ask` | dnd-ask | arrow (FR-007) | tip |
| `no_drop` | no-drop, dnd-no-drop, not-allowed, crossed_circle | not allowed | centre |
| `not_allowed` | not-allowed, crossed_circle, forbidden | not allowed: circle with a slash | centre |
| `grab` | grab, openhand | open hand | centre |
| `grabbing` | grabbing, closedhand, dnd-move (today's) | closed hand (today's grab drawing) | centre |
| `e_resize`, `w_resize` | e-resize / w-resize, right_side / left_side, then ew-resize, sb_h_double_arrow | left-right | centre |
| `n_resize`, `s_resize` | n-resize / s-resize, top_side / bottom_side, then ns-resize, sb_v_double_arrow | up-down | centre |
| `ne_resize`, `sw_resize` | ne-resize / sw-resize, top_right_corner / bottom_left_corner, then nesw-resize, fd_double_arrow | top-right to bottom-left | centre |
| `nw_resize`, `se_resize` | nw-resize / se-resize, top_left_corner / bottom_right_corner, then nwse-resize, bd_double_arrow | top-left to bottom-right | centre |
| `ew_resize` | ew-resize, sb_h_double_arrow, h_double_arrow (today's) | left-right | centre |
| `ns_resize` | ns-resize, sb_v_double_arrow, v_double_arrow (today's) | up-down | centre |
| `nesw_resize` | nesw-resize, fd_double_arrow, size_bdiag (today's) | top-right to bottom-left | centre |
| `nwse_resize` | nwse-resize, bd_double_arrow, size_fdiag (today's) | top-left to bottom-right | centre |
| `col_resize` | col-resize, split_h, then ew-resize, sb_h_double_arrow | left-right | centre |
| `row_resize` | row-resize, split_v, then ns-resize, sb_v_double_arrow | up-down | centre |
| `all_scroll` | all-scroll, fleur | four arrows (today's move drawing) | centre |
| `all_resize` | all-resize, all-scroll, fleur | four arrows | centre |
| `zoom_in` | zoom-in | magnifier with `+` | centre of the lens |
| `zoom_out` | zoom-out | magnifier with `-` | centre of the lens |
| window move (RubickWay) | move, fleur, all-scroll (today's) | four arrows | centre |

Notes on the table:

- The later names in each list are the same meaning under older names, or the image of the same axis
  (FR-007 applied to themes). Themes that have only the double arrows then show them for single
  borders, as the built-in set does.
- The drawings: twelve new ones (pointer, help, progress, wait, cell, crosshair, text, vertical
  text, not allowed, open hand, zoom in, zoom out). Today's arrow, closed hand, four arrows and
  resize arrows stay. That gives 19 drawings in all; with the 37 table entries that leaves 18 entries
  sharing a drawing.
- All drawings are 12x12 cells, two colours plus transparency, shown doubled at 24 px (FR-008).
- "Tip" is the image's top-left point (0, 0), as today's arrow. "Centre" is (12, 12), as today's
  other images. The hand's hotspot is its fingertip. The exact cells are chosen when drawing, and
  tests check that each hotspot lands on a drawn pixel or the centre.

## C6. Choosing and reading the theme

- **Decision**:
  1. **The theme.** It is the `cursor { theme }` setting, or `default` without it (FR-010). A theme
     is installed when one of the icon directories (today's `icon_dirs`) has a directory with its
     name.
  2. **The theme chain.** Resolved once: the theme, then its `Inherits=` themes, breadth first, at
     most 8 themes, as today's `theme_image`. Each theme contributes its `cursors/` directory from
     every icon directory that has it, in icon-directory order.
  3. **Reading.** Each `cursors/` directory is listed once (`read_dir`). For each table entry, the
     first of its names that some directory in the chain has is read and parsed. That is at most 37
     files.
  4. **Theme names are checked**, from the configuration and from `Inherits=`. A name that is
     empty, `.` or `..`, or contains `/` or a NUL is not a theme. In the configuration that is a
     warning (C11). In `Inherits=` it is skipped.
  5. **Fallbacks.**
     - A configured theme that is not installed gives one warning, then `default` is tried.
     - A chain with no cursor images at all counts as no theme (spec edge case), so every entry
       gets its built-in image. This is the owner's laptop: `/usr/share/icons/default/index.theme`
       inherits `Adwaita`, and `Adwaita` is not installed.
- **Rationale**:
  - One listing per directory instead of up to 37 x names x directories failed opens.
  - Name checks keep `Path::join` from leaving the icon directories: a name starting with `/`
    would replace the whole path.
- **Alternatives considered**:
  - Read `XCURSOR_THEME` and `XCURSOR_PATH`. Rejected by the owner's answer (spec Clarifications).
    Setting `XCURSOR_*` for started programs belongs to a later feature (spec Assumptions).

## C7. Damaged files

- **Decision**: `parse_xcursor` returns a reason instead of `None`:
  - not a cursor file;
  - cut short;
  - no image;
  - image larger than 256 px.

  If the first file found for an entry is damaged, that entry gets its built-in image and a warning.
  The warning names the shape, the file and the reason. A file used by several entries (for example
  `fleur`) is reported once.
- **Rationale**: US2 scenario 4 asks for the built-in image and one line saying which shape and why.
  Trying further names after a damaged file would hide the damage.
- The existing checks stay: every length and offset is checked, and the size nearest 24 is picked.
  For an animated cursor the first image of that size is used, since animation is out of scope.

## C8. Log lines

- **Decision**: the lines in [contracts/cli.md](contracts/cli.md).
  - **Once per run, at startup.** `load` returns a report instead of logging (FR-012, SC-003). The
    report is pure and tested, and the app logs it. It covers:
    - the theme chosen, or why none;
    - the shapes the theme lacks, in one line;
    - each damaged file.
  - **Once per run per shape, at first use.** When one of the FR-007 shapes is shown with the arrow
    because there is no image for it, one line says so. `App` keeps the set of shapes already
    reported.
  - **Debug.** A debug line on every accepted cursor change: `application <id> asks for the <name>
    pointer` / `hides the pointer` / `shows its own pointer image (the arrow is drawn)`. It is
    written only on change, so a flood of identical requests writes nothing. The quickstart uses
    this line to see which shapes GTK and Qt name.
- **Rationale**: the owner's rule that every fallback leaves one plain line. The debug line answers
  "does this application name shapes at all?" without a protocol dump.

## C9. Redraws

- **Decision**: no new mechanism. `App::mark_dirty` already compares the pointer's (display,
  position, image) with what was drawn and marks the displays it was and is on.
  - A new image is a new `CursorKind`, so it redraws.
  - A request that changes nothing changes no `CursorKind`, so nothing redraws.
  - Dirty flags are drawn at most once per display frame (FR-013).
- The server pushes `CursorChanged` only on change (C3), so a flood of requests costs one comparison
  each and no events (SC-004, Misbehaving Clients).

## C10. Built-in drawings

- **Decision**: drawn by hand as 12x12 ASCII art in `src/render/cursor.rs`, like today's five
  (`#` outline, `.` fill, space transparent). The tests:
  - each table entry has a 24x24 image, its hotspot inside, and something drawn;
  - the art only uses the three characters;
  - the 19 drawings differ from each other, pixel by pixel (FR-006, "each recognisable on its own"),
    including after today's mirroring and transposing;
  - the entries that share a drawing (FR-007) have identical images;
  - the hotspot of each drawing is the one in C5.
- `vertical text` is the I-beam transposed and `zoom out` is `zoom in` without the vertical stroke,
  as today's resize arrows are made by mirroring and transposing.

## C11. Configuration

- **Decision**: a new top-level block `cursor` with one child, `theme`, a string
  ([contracts/config.md](contracts/config.md)). It goes into `Config.cursor_theme: Option<String>`.
  Problems are warnings with the line, and the default applies:
  - an empty string;
  - a name that fails C6's check;
  - a value that is not a string;
  - extra values;
  - an unknown child (`cursor > size`, for example).
- **Rationale**: the owner's answer (spec Clarifications). Size stays a fixed 24 px; no `size` child
  (spec Assumptions, Principle VI).

## C12. Work that can block (Principle VII)

- Reading the theme: listing at most 8 x 4 directories and reading at most 37 small files, with the
  index files of the chain. It runs once at startup, before the event loop starts, as today's theme
  read and the configuration do. It takes milliseconds and never runs on the loop afterwards
  (FR-011).
- Uploading 37 textures of 24x24, also at startup, as today's 7.
- Protocol handlers do comparisons only. No I/O, no waiting.
