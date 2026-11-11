# Data Model: Cursor Shapes

Types the feature adds or changes. Research references: C2-C11.

## Shell (`src/shell/`, no Wayland types)

### CursorName (new, `src/shell/cursor.rs`)

The 36 shapes of the cursor shape protocol, as a plain enum:

- `Default`, `ContextMenu`, `Help`, `Pointer`, `Progress`, `Wait`, `Cell`, `Crosshair`, `Text`,
  `VerticalText`;
- `Alias`, `Copy`, `Move`, `NoDrop`, `NotAllowed`, `Grab`, `Grabbing`;
- `EResize`, `NResize`, `NeResize`, `NwResize`, `SResize`, `SeResize`, `SwResize`, `WResize`,
  `EwResize`, `NsResize`, `NeswResize`, `NwseResize`, `ColResize`, `RowResize`;
- `AllScroll`, `ZoomIn`, `ZoomOut`, `DndAsk`, `AllResize`.

- `ALL: [CursorName; 36]`, in protocol order.
- `css_name()`: the name used in log lines (`e-resize`, `dnd-ask`, ...).

### AppCursor (new)

What an application asked for since its latest pointer enter.

| Variant | From |
| --- | --- |
| `Shape(CursorName)` | `set_shape`; also `set_cursor` with a surface, as `Shape(Default)` (FR-004: the arrow), and the reset at each enter |
| `Hidden` | `set_cursor` without a surface |

### WindowEvent::CursorChanged (replaces `CursorHidden`)

`CursorChanged { id: WindowId, cursor: AppCursor }`. The cursor the application of window `id` asks
for while the pointer is over that window. The server sends it only when the cursor changes (C3).

### Shell state

- Removed: `cursor_hidden: Vec<WindowId>`.
- Added: `app_cursor: Option<(WindowId, AppCursor)>`.
  - Set by `CursorChanged`.
  - Cleared when `leave()` takes the entered window, and when that window is unmapped.

### CursorShape (changed)

Gains `Named(CursorName)`. `Shell::cursor()` decides in this order:

1. a move gesture gives `Move`;
2. a resize gesture gives `Resize(edges)`;
3. panning gives `Grabbing`;
4. if the entered window is the one in `app_cursor`, its cursor: `Hidden` gives `Hidden`,
   `Shape(n)` gives `Named(n)`;
5. otherwise `Default`.

## Server (`src/server/`)

### SeatState (changed, `seat.rs`)

- Removed: `hidden_cursor: Vec<ClientId>` and the per-client fan-out `cursor_hidden()`.
- Added:
  - `pointer_window: Option<WindowId>`: the window of `pointer_surface`. Set at `pointer_enter`
    from the `ClientInput` that caused it.
  - `enter_serial: Option<u32>`: the serial of the latest `wl_pointer.enter`. Since a request
    counts only from the focused client, this is that client's latest enter serial (C2).
  - `focus_cursor: AppCursor`: what the focused client asked for since that enter.

State transitions:

| Event | Effect |
| --- | --- |
| `pointer_enter(window, surface)` | new serial; `enter_serial`, `pointer_window` set; if `focus_cursor` was not `Shape(Default)`, it becomes that and `CursorChanged { window, Shape(Default) }` is pushed |
| `pointer_leave` | `pointer_surface` cleared; `focus_cursor` untouched (the shell forgets on its own leave; the next enter resets) |
| cursor request from client `c` with serial `s` | ignored unless `pointer_surface` belongs to `c` and `s == enter_serial`; otherwise, if the cursor differs from `focus_cursor`, it is stored, `CursorChanged` is pushed, and a debug line is written (C8) |
| surface destroyed | as today: `pointer_surface` cleared if it was that surface |

Cursor requests are `wl_pointer.set_cursor` and `wp_cursor_shape_device_v1.set_shape`, through one
function: `request_cursor(state, client, serial, AppCursor)`.

### Cursor shape objects (new, `cursor_shape.rs`)

| Object | User data | Notes |
| --- | --- | --- |
| `wp_cursor_shape_manager_v1` (global, version 2) | none | `get_pointer` makes a device; `get_tablet_tool_v2` cannot be called (C1) |
| `wp_cursor_shape_device_v1` | the `WlPointer` it was made for | `set_shape`: version and value checks (C4), then `request_cursor` if the pointer is alive |

`shape_name(Shape) -> CursorName`: total over the 36 entries, pure, tested.

## Render (`src/render/cursor.rs`)

### CursorKind (changed)

| Before | After |
| --- | --- |
| `Arrow`, `Grab`, `Move`, `ResizeEw`, `ResizeNs`, `ResizeNwse`, `ResizeNesw` | `Shape(CursorName)` (36), `WindowMove` |

- `ALL`: 37 entries.
- `theme_names()`: the list in research C5.
- `texture_key()`: `u64::MAX - index`, indexes 0-36.

### Art (new, private)

The 19 built-in drawings (C5, C10), each with its hotspot:

- `Arrow`, `Pointer`, `Help`, `Progress`, `Wait`;
- `Cell`, `Crosshair`, `Text`, `VerticalText`;
- `NotAllowed`, `Grab`, `Grabbing`, `AllDirections`;
- `ResizeEw`, `ResizeNs`, `ResizeNwse`, `ResizeNesw`;
- `ZoomIn`, `ZoomOut`.

`art_of(CursorKind) -> Art` implements FR-006/FR-007. `has_own_art(CursorKind)` is false for the
FR-007 arrow shapes: `context_menu`, `alias`, `copy`, `move`, `dnd_ask`.

### PointerImages (new; replaces `Vec<(CursorKind, CursorImage)>`)

- One entry per `CursorKind`: the image and where it came from (`Theme` or `Builtin`).
- `get(kind) -> &CursorImage`.
- `arrow_stand_in(kind) -> bool`: the entry is built in and `!has_own_art`. This means a first-use
  log line (FR-007).

### ThemeReport (new)

What `load` found, turned into log lines by `lines()` (pure, tested; contracts/cli.md):

| Field | Meaning |
| --- | --- |
| `theme` | `ThemeUse::Used(name)`, `NotInstalled`, or `NoImages { name, missing }` with the inherited themes that are missing |
| `configured_missing` | the configured name, if it was not installed |
| `missing` | `css_name`s of entries the theme has no file for (only when a theme is used) |
| `damaged` | `Damaged { shape, path, reason }` per damaged file, one per path (C7) |

`load(icon_dirs, configured: Option<&str>) -> (PointerImages, ThemeReport)`. `lines()` gives
`ReportLine { warn, text }`: a warning for each fallback, information for the theme used.

### XcursorError (new)

What `parse_xcursor` returns instead of `None`: `NotACursorFile`, `CutShort`, `NoImage`,
`TooLarge`. Each has a plain-words `Display` used in the damaged-file line.

## Configuration (`src/config.rs`)

`Config.cursor_theme: Option<String>`: `None` by default. Validation is in C11; the theme name check
is the same function as in C6.

## App (`src/standalone.rs`)

- `cursors` becomes `pointer_images: PointerImages`. `upload_cursors` uploads all 37.
- `pointer_kind()`:

  | `CursorShape` | Image |
  | --- | --- |
  | `Default` | `Shape(Default)` |
  | `Grabbing` | `Shape(Grabbing)` |
  | `Move` | `WindowMove` |
  | `Resize(edges)` | `Shape(resize_name(edges))`, one of the four axis shapes |
  | `Named(n)` | `Shape(n)` |
  | `Hidden` | `None` |

- `arrow_noted: HashSet<CursorName>`. In `mark_dirty`, when the drawn kind changes to one where
  `arrow_stand_in` is true and the name is not in the set yet, it is added and the FR-007 line is
  logged.
