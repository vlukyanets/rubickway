# Data Model: Standalone Session on the Hardware

Plain data unless marked *(backend)*. Types from earlier features (`Camera`, `Viewer`,
`Window`, `WindowId`, `Edges`, ...) keep their meaning; changes are listed. Decisions: research
S1-S22.

## Configuration (`config`, new module)

### `Config`

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `mod_key` | `ModKey` (`Super`, `Alt`, `Ctrl`) | `Super` | FR-005 |
| `spawns` | `Vec<SpawnConfig { combo: ComboSpec, command: Vec<String> }>` | one: `Mod+Return`, `["kitty"]` | each `command` not empty; each `combo` not a built-in and not bound twice (first wins); a valid list replaces the default (S16) |
| `watchdog_timeout` | `Duration` | 10 s | whole seconds, 1..=600 (FR-024, FR-030) |

`Config::parse(text) -> (Config, Vec<ConfigWarning>)` never fails: every problem becomes a
warning and that setting keeps its default (FR-026). `ConfigWarning { line: Option<usize>,
message: String }`; `line` is 1-based, from the KDL span. `Config::load(path)` reads the file
(missing file: defaults, no warning; unreadable: defaults, one warning).

### `KeyCombo`

`{ mods: ComboMods { mod_: bool, super_: bool, alt: bool, ctrl: bool, shift: bool }, sym:
Keysym }` from `Mod+Shift+t`-style text: `+`-separated, modifiers first, exactly one key last.
Resolved against the running `ModKey` into `Modifiers` (`input::Modifiers`) before matching;
letters are lowered (S16). Rejected: unknown modifier or key name, no key, two keys, a
combination equal to a built-in shortcut after resolution, Ctrl+Alt+F1..F12.

## Shortcuts (`input::bindings`, changed)

### `Bindings`

Built from `Config`: `mod_mods: Modifiers` (the configured Mod) and the table of built-in
shortcuts with Mod substituted, plus the spawn bindings (`SpawnBind { combo: KeyCombo, command }`;
`lookup` gives `Action::Spawn(index)` into them). `lookup(mods, sym, pressed) ->
Option<Action>` and `mod_held(mods) -> bool` become methods. Owned by the shell, passed to the
viewers.

Also the pointer table (FR-032, S24): `PointerBind { combo: PointerCombo { mods: Modifiers,
button: Button }, action: PointerAction }` with `PointerAction` = `MoveWindow | ResizeWindow |
PanCanvas`; `pointer_lookup(mods, button) -> Option<PointerAction>` matches exactly. The default
table follows Mod; `with_pointer(binds)` replaces it with the configured one.

### `Action` (changed)

Adds `SpawnTerminal`. `Quit` now means "a quit press" (see `QuitConfirm`).

### `QuitConfirm` (in the shell's router)

| State | On Mod+Shift+Q press at `t` | On any other key press, `FocusLost` |
| --- | --- | --- |
| `Idle` | -> `Pending { since: t }` | stays `Idle` |
| `Pending { since }` | `t - since <= 2000 ms`: quit (`Effect.quit`); else -> `Pending { since: t }` | -> `Idle` |

Key releases and modifier key presses do not change it. Times are the input events' `time_ms`
(wrapping `u32`, compared with wrapping subtraction).

## Input (`input`, changed)

- `InputEvent::PointerMoved { delta: Vector }` replaces the backend's absolute `PointerEnter` /
  `PointerMotion { pos }`: the backend reports relative, accelerated motion in logical pixels
  (S8, S9). `PointerEnter`, `PointerMotion` and `PointerLeave` remain as the shell's internal,
  per-viewer events produced by the layout.
- `InputEvent::Key` gains `time_ms: u32`.
- `PointerLeave` and `FocusLost` are also what the backend sends on `Paused`; `FocusGained` on
  `Resumed` (S12).

## Layout (`canvas::layout`, new)

### `Layout`

| Field | Meaning |
| --- | --- |
| `entries: Vec<LayoutEntry>` | in row order |
| `pointer: Point` | layout position (logical pixels, origin at the top-left of the first entry) |

`LayoutEntry { id: ViewerId, size: Size, internal: bool, connected_seq: u64 }`. Order: internal
entries first, then by `connected_seq` (FR-018). Each entry's `x` is the sum of the widths
before it; `y` is 0 (top edges aligned).

Operations (all pure, unit- and property-tested):

- `add(entry)`, `remove(id)`: re-flow the row; if the pointer was on the removed entry it moves
  to the centre of the first entry (FR-021). With no entries the pointer is `(0, 0)` and maps to
  nothing.
- `move_by(delta)`: adds `delta`, clamps x to the row, takes the entry whose horizontal range
  contains x and clamps y to its height, so crossing to a shorter entry clamps y to its bottom
  edge (edge case "different resolutions").
- `locate(point) -> Option<(ViewerId, Point)>`: the entry containing the point and the position
  relative to its top-left.
- `relative_to(id, point) -> Point`: the point relative to entry `id`'s top-left, also outside
  it (for a pan that started there, FR-019, and for moves and resizes, FR-023).
- Home views are stored in the layout (`home(id) -> Option<Camera>`), since only it knows the
  row: `add(entry)` gives the new entry a home continuing the rightmost entry's home view to
  the right (the first entry's home is the canvas origin); `recentre_homes()`, called once the
  displays present at startup are known, gives every entry the home of the whole row: zoom 1,
  the row's bounding box centred on the canvas origin, each camera centred on its entry's part
  (FR-020). `origin(id)`, `entries()` and `pointer()` are read accessors.

### `ViewerId`

`u32`, the backend's output id; never reused while RubickWay runs.

## Shell (changed)

| Field (new or changed) | Meaning |
| --- | --- |
| `viewers: Vec<ViewerSlot>` | replaces `viewer: Viewer` |
| `layout: Layout` | S9 |
| `active: Option<ViewerId>` | the viewer under the pointer (FR-019) |
| `pan_viewer: Option<ViewerId>` | the viewer a pan started on, until the button is released |
| `bindings: Bindings` | from the configuration |
| `router.quit: QuitConfirm` | FR-031 |

`ViewerSlot { id: ViewerId, viewer: Viewer, fullscreen: Option<Fullscreen> }` (its home view
is the layout's) -
`Fullscreen` moves here from the shell (per viewer, S14).

New operations: `add_viewer(id, size, internal)`, `remove_viewer(id)`, `viewers()`,
`viewer(id)`, `active_viewer()`, `visible_in(id) -> Vec<WindowId>`,
`visible_any() -> Vec<WindowId>` (frame callbacks), `window_viewers(window) -> Vec<ViewerId>`
(`wl_surface.enter`/`leave`), `pointer() -> Option<(ViewerId, Point)>` (where to draw it),
`cursor()` (gains `CursorShape::Hidden` when the client under the pointer hid it and no gesture
runs), `take_dirty_viewers() -> Vec<ViewerId>`.

The window's viewer (FR-022): the active viewer if it shows any part of the window, else the
viewer showing the largest area of it, else the active viewer.

## Viewer (changed)

`DEFAULT_SIZE` and `configure`'s "zero means default" go (nested only); `configure(size)` takes
the display mode's size. Scale stays 1 (Assumptions). Everything else unchanged.

## Backend events (changed)

```text
BackendEvent::Input(InputEvent)
BackendEvent::OutputAdded { id: ViewerId, info: OutputInfo }
BackendEvent::OutputRemoved { id: ViewerId }
BackendEvent::Presented { id: ViewerId, time_ms: u32 }
BackendEvent::Paused
BackendEvent::Resumed
BackendEvent::Keymap(String)
BackendEvent::RepeatInfo { rate: i32, delay: i32 }
```

### `OutputInfo`

`{ name: String, make: String, model: String, physical_mm: (u32, u32), mode: Mode { width,
height: u32, refresh_mhz: u32 }, internal: bool }`. Built from the connector and its EDID
(`edid::parse(bytes) -> EdidInfo { make, model }`, pure; S13).

## Output *(backend)*

| Field | Meaning |
| --- | --- |
| `id: ViewerId` | |
| `connector`, `crtc`, `plane` | DRM handles (S5) |
| `mode` | the preferred mode |
| `surface` | GBM surface + EGL surface (S6) |
| `framebuffers` | DRM framebuffers per GBM buffer object |
| `state: FrameState` | below |
| `needs_modeset: bool` | first commit, after `Resumed`, after a failed commit |
| `shown: Vec<WindowId>` | windows in the frame waiting for its flip (kept by `App`) |

### `FrameState`

```text
Idle --dirty--> Queued --draw + commit (NONBLOCK, modeset or flip)--> WaitingForFlip --flip event--> Idle
Queued --commit refused (EBUSY)--> Queued (retry at the next flip event of any output, or after 100 ms)
Idle --apps wait, nothing dirty--> WaitingForEstimatedVblank --timer--> Idle
any --Paused--> Idle (no commits until Resumed)
```

A modeset (first commit, after `Resumed`, after a failed commit) is an ordinary non-blocking
commit; the output simply waits longer in `WaitingForFlip` (research S5).

Drawing happens in `App::before_sleep` for every `Queued` output, so several changes in one
loop iteration give one frame.

## Session *(backend)*

`Seat` (libseat handle), `active: bool`, open devices by id. Transitions: `enable` -> active,
`disable` -> acknowledge -> inactive (S3, S12).

## Watchdog (`watchdog`, new)

`Heartbeat(Arc<AtomicU64>)` (milliseconds on the monotonic clock), `timeout: Duration`.
`expired(last_ms, now_ms, timeout) -> bool` is the whole decision (pure). The thread checks
every 250 ms (S19).

## Spawn requests

`SpawnRequest { command: Vec<String> }` sent to the spawner (S18); `SpawnResult::Failed {
command, error }` comes back over a channel and is logged.

## Cursor images

`CursorImage { width, height, hotspot: (u32, u32), pixels: Vec<u8> (premultiplied ARGB) }`
per `CursorShape` mapping (default, grabbing, move, eight resize directions onto four images),
from the theme or the built-in ASCII art (S10). Loaded once at startup.
