# Research: Clipboard

Decisions for [spec.md](spec.md). References, read only (Principle I): wlroots
(`types/data_device/wlr_data_device.c`, `wlr_data_source.c`, `wlr_data_offer.c`,
`types/wlr_primary_selection.c`, `types/wlr_data_control_v1.c`, `types/wlr_ext_data_control_v1.c`),
sway (`sway/input/seat.c`, selection serial checks), mutter (`meta-wayland-data-device.c`), Smithay
(`wayland::selection::{data_device, primary_selection, ext_data_control, wlr_data_control}`), niri
(which uses Smithay's modules).

## C1. Protocols and versions

- **Decision**:
  - `wl_data_device_manager` stays at version 3, as today; its selection parts are implemented.
    Drag-and-drop stays cancelled (spec FR-010, C9).
  - `zwp_primary_selection_device_manager_v1` version 1 (`wayland-protocols`, `wp::primary_selection`),
    offered only while the primary selection is on (C10).
  - `ext_data_control_manager_v1` version 1 (`wayland-protocols`, staging, `ext::data_control`).
  - `zwlr_data_control_manager_v1` version 2 (the older protocol, spec Clarifications). Its bindings
    are not in `wayland-protocols`; they come from **`wayland-protocols-wlr` 0.3** (feature `server`;
    `client` as a dev-dependency for the tests). It is a bindings crate generated from the protocol
    XML by the same project as `wayland-protocols` (wayland-rs), not a compositor framework
    (Principle I; plan Complexity Tracking).
- **Rationale**: version 2 of the wlr protocol adds the primary selection, which wl-clipboard and
  wl-clip-persist use; version 1 of ext is the only one. wl-clipboard 2.3 and newer speak ext; older
  builds and some managers speak only wlr (spec Clarifications: both, same behaviour).
- **Alternatives considered**: generating the wlr bindings in a build script with `wayland-scanner`.
  Rejected: more code to own than one maintained bindings crate, for the same result.

## C2. Where the clipboard lives

- **Decision**: entirely in the server, in a new module `src/server/selection.rs`: the two slots
  (clipboard and primary), their owners, the devices to tell, the offers. The protocol modules
  (`data_device.rs`, new `primary_selection.rs`, new `data_control.rs`) translate requests into calls
  on it. The shell does not change: the keyboard focus it decides already reaches the server
  (`ClientInput::KeyboardFocus`), and nothing about the clipboard depends on the canvas, viewers or
  zoom (FR-009).
- **Rationale**: the clipboard is protocol state between clients plus "who has the keyboard", which
  the server knows (`seat.keyboard_surface`). Putting it in the shell would need new events and
  commands for no decision the shell makes (Principle VI).

## C3. Who is offered the selections

- **Decision**:
  - The **focused application** is the client of `seat.keyboard_surface` (the window's root surface
    or, during a popup grab, the popup's surface: the same client, so popup keyboard moves change
    nothing). Each of its `wl_data_device` gets the clipboard; each of its primary selection devices
    gets the primary selection. Other applications get nothing (FR-002).
  - **Data-control devices** (both protocols) all get every change of both slots (FR-007).
  - **When**: the server remembers what it last announced, and to which client. After dispatching
    client requests and after applying `ClientInput`, a single `selection::announce` sends what
    changed: to the focused client if it changed or the slot changed, to data-control devices if a
    slot changed. A flood of changes in one batch is announced once, with the newest content (C11).
  - A device created by the focused client gets the current selection at once (applications create
    their device after the keyboard enter).
- **Rationale**: the protocol's rule (`wl_data_device.selection` is sent "before the keyboard
  enter"); wlroots sends it on focus change and to new devices. Announcing once per batch keeps one
  client's flood from filling other clients' buffers (Principle II).
- **Order with the keyboard enter**: the protocol asks for the `selection` event before
  `wl_keyboard.enter`. `keyboard_focus` in `seat.rs` calls `selection::focus_changed` before it sends
  the enter, which announces to the new client at once (not deferred to the batch end).

## C4. Accepting a selection: the serial

- **Decision**: per application, the serials of its **16 latest input events** of these kinds: key
  press and release, button press and release, keyboard enter (window or popup). `set_selection` on
  a `wl_data_device` or a primary selection device is accepted if its serial is among them, whether
  or not the application still has the focus (spec Clarifications, FR-004). Otherwise it is ignored
  without error, and the source is told it is cancelled so the application does not wait.
  Data-control devices take no serial (FR-007).
- **Rationale**: sway validates the serial against recent events the client got; GNOME requires the
  focus and loses a quick Ctrl+C then Mod+Tab (spec Clarifications). 16 covers a key press, its
  release and modifiers around it, and keeps a background application from reusing a stale serial
  long after.
- **Implementation note**: `seat.rs` already keeps per-client serials for popup grabs
  (`popup_serials`, latest button press and latest key); the new `input_serials` ring is fed at the
  same points plus button release and keyboard enter, and pruned the same way when a client's input
  objects are gone.

## C5. Pasting: passing the descriptor

- **Decision**: `receive(mime_type, fd)` on an offer: if the offer is for the current content of its
  slot and the source offered that type, the server sends `send(mime_type, fd)` to the source and
  closes its own copy of `fd`; otherwise it only closes it, so the pasting application reads end of
  file at once. RubickWay never reads or writes the pipe (FR-003); the work per paste is one event.
- **Rationale**: the protocol's design; every compositor does this.

## C6. Offers

- **Decision**: each announcement creates a new offer object per device (`data_offer` /
  `data_offer`-like event, then one `offer(mime_type)` per type, then `selection(offer)`); an empty
  slot is announced as `selection(null)`. An offer remembers its slot and the generation of the
  content it was made for; once the slot changes, the old offer is stale: `receive` on it closes the
  descriptor and does nothing else (Misbehaving Clients). Destroying an offer is the client's
  business.
- **Formats**: a source keeps at most **256 distinct** MIME types; repeats are ignored; types after
  the 256th are ignored (FR-011). Types are kept as the client sent them.

## C7. Replacing and losing the content

- **Decision**:
  - New content in a slot (from an application or a tool): the previous source gets `cancelled`; the
    slot then has the new owner (FR-005).
  - `set_selection(null)`: the slot becomes empty; the previous source gets `cancelled`.
  - The owning source destroyed (by its client, or because the client disconnected): the slot
    becomes empty; nothing to cancel.
  - Each change bumps the slot's generation and marks it for announcement (C3).
- **Rationale**: wl_data_source and the data-control sources all document `cancelled` for
  "replaced by another selection".

## C8. Protocol errors

| Case | Answer | Source |
| --- | --- | --- |
| a source used again in a data-control `set_selection` / `set_primary_selection` | `used_source` on the device (the protocols say it "triggers" the error) | ext / wlr XML |
| a source used again in `wl_data_device.set_selection` (also after `start_drag`) or in the primary selection's `set_selection` | no error: the current owner set again changes nothing, another source becomes the owner. `wl_data_device` only says the error "may" be sent, and Firefox sets the same source again - the first version sent `used_source` and Firefox was disconnected on every Copy (found on the laptop, 2026-11-19) | `wl_data_device` XML; quickstart run |
| `offer` on a data-control source after it was used | `invalid_offer` on that source | ext / wlr XML |
| `offer` on a `wl_data_source` or primary source after it was used | ignored (no error defined); the type list is frozen | - |
| `finish` on a selection offer | `invalid_finish` on the offer | `wl_data_offer` XML |
| `set_actions` on a selection offer | `invalid_offer` on the offer | `wl_data_offer` XML |
| `accept` on a selection offer | ignored | `wl_data_offer` XML |
| a second data-control device for the same seat from one client | allowed; each is told | - |

## C9. Drag-and-drop

- **Decision**: unchanged: `start_drag` with a source gets `cancelled` at once; the source counts as
  used (C8). No `wl_data_device.enter` is ever sent (FR-010).

## C10. Switching the primary selection off

- **Decision**: a top-level `primary-selection` node in `config.kdl` (`primary-selection off`; `on`/`off` are plain strings in KDL 2.0, and `#true`/`#false` are accepted
  too, while a bare `false` is a KDL 2.0 syntax error - chosen with the owner while implementing); on by
  default (spec Clarifications, FR-006). Read once at startup, like every setting. Off:
  - the `zwp_primary_selection_device_manager_v1` global is not created, so applications see no
    primary selection and do not paste on a middle click;
  - data-control devices get no `primary_selection` events, and `set_primary_selection` is ignored,
    as the ext protocol says for a compositor without primary selection; its source is cancelled so
    the tool does not wait.
- The server gets the switch through `Server::set_primary_selection(enabled)`, called before
  applications connect, like `set_gpu_offer` and `set_timelines`: `Server::new()` creates the
  primary selection global as the default (on), and `set_primary_selection(false)` removes it and
  turns the slot off. Tests use it the same way (`Harness::new()`, then the call), so the test
  harness does not change. (Changed from a `ServerOptions` argument during analysis: the setter is
  the project's existing pattern.)

## C11. Floods and misbehaving clients

- **Decision**: per change, the work is constant (cancel one source, bump a generation); the number
  of events another client receives is bounded by announcements per batch (C3), not by the number of
  changes. A source's types are capped (C6). Nothing waits on a client.
- **Rationale**: Principle II (one client cannot fill another's buffer through RubickWay) and VII.

## C12. Logging

- **Decision**: one debug line per announced change: which slot, the owner application (the same
  `application N (pid, name)` form as the connection log line) and the number of formats, never the
  types' values or any content. Ignored `set_selection` requests (bad serial) log one debug line per
  application per minute at most.
- **Rationale**: the content can be secret (spec Assumptions); RubickWay never sees it anyway, but
  type names can hint at it (`x-kde-passwordManagerHint`), so only the count is logged.

## C13. Tests

- **Decision**:
  - `tests/protocol/clipboard.rs` (new): two or three test clients; transfers go through a real pipe
    (`std::io::pipe`, stable since Rust 1.87): the test client that owns the source writes on `send`,
    the pasting one reads to the end. Covers FR-001-FR-005 and FR-009-FR-011, ordering of `selection`
    before `keyboard.enter`, serial acceptance and refusal, stale offers.
  - Primary selection and both data-control protocols in the same file, with
    `wayland-protocols` client bindings (`wp::primary_selection`, `ext::data_control`) and
    `wayland-protocols-wlr` client bindings (dev-dependency).
  - `tests/protocol/misbehaving.rs`: the contract's misbehaving rows, each with a well-behaved client
    that keeps working.
  - `tests/config.rs`: `primary-selection`.
  - Real applications in [quickstart.md](quickstart.md).
- No shell or rendering test: neither changes.

## C14. Non-goals confirmed

- RubickWay keeps no copy of content (FR-008): no reading pipes, no history.
- No X11 selection bridging (no XWayland yet).
- No restriction of data-control to chosen programs (spec Clarifications; a possible later change).
