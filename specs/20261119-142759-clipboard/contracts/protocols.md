# Contract: Wayland protocols

Globals added or changed by this feature (research C1). Everything else stays as in
`specs/20261118-130629-popups/contracts/protocols.md` and the earlier features' contracts.

| Global | Version | Offered when |
| --- | --- | --- |
| `wl_data_device_manager` | 3 (unchanged) | always |
| `zwp_primary_selection_device_manager_v1` | 1 (new) | the primary selection is on (default; `config.md`) |
| `ext_data_control_manager_v1` | 1 (new) | always |
| `zwlr_data_control_manager_v1` | 2 (new) | always |

"The focused application" below is the client of the surface that has the keyboard (a window's root
surface or, during a popup grab, a popup's surface).

## wl_data_device_manager, wl_data_device, wl_data_source, wl_data_offer (clipboard)

| Request / event | Behaviour |
| --- | --- |
| `create_data_source` | a source with no types |
| `wl_data_source.offer(mime)` | kept, up to 256 distinct types (repeats and further types ignored); after the source is used: ignored |
| `wl_data_source.set_actions` | accepted (drag-and-drop only) |
| `get_data_device(seat)` | a device; if its client is focused, it gets the current clipboard at once |
| `set_selection(source, serial)` | accepted if `serial` is one of the client's 16 latest key, button or keyboard-enter serials (focus not required); otherwise ignored and `source.cancelled` sent. A source used before is **not** an error (the protocol only says it "may" be; Firefox sets the same source again and was disconnected for it on the laptop, 2026-11-19): the current owner set again changes nothing, any other source simply becomes the owner. `null`: the clipboard becomes empty |
| `start_drag(source, ...)` | `source.cancelled` at once; its type list is frozen (as today, drag-and-drop out of scope) |
| `release` | the device is forgotten |
| event `data_offer` + `wl_data_offer.offer`* + `selection(offer)` | sent to each device of the focused application when it gains the focus (before `wl_keyboard.enter`) and when the clipboard changes while it has it; `selection(null)` for an empty clipboard |
| `wl_data_offer.receive(mime, fd)` | current offer and a type the source offered: `wl_data_source.send(mime, fd)` to the owner; RubickWay's copy of `fd` is closed in every case; a stale offer or unknown type: only closed |
| `wl_data_offer.accept` | ignored |
| `wl_data_offer.finish` | `invalid_finish` (not a drag-and-drop offer) |
| `wl_data_offer.set_actions` | `invalid_offer` (not a drag-and-drop offer) |
| `wl_data_source.cancelled` | when its content is replaced, set to `null` by a device, refused (bad serial), or used for a drag |

## zwp_primary_selection_device_manager_v1 (primary selection)

Same rules as the clipboard, for the second slot:

| Request / event | Behaviour |
| --- | --- |
| `create_source`, `zwp_primary_selection_source_v1.offer(mime)` | as `wl_data_source` (256 distinct types; frozen once used) |
| `get_device(seat)` | a device; told the current primary selection at once if its client is focused |
| `set_selection(source, serial)` | as `wl_data_device.set_selection` (serial rule, `null`, cancel on refusal, a source used before is no error) |
| events `data_offer`, `selection` | as for the clipboard, to the focused application |
| `zwp_primary_selection_offer_v1.receive(mime, fd)` | as `wl_data_offer.receive` |

## ext_data_control_manager_v1 and zwlr_data_control_manager_v1 (clipboard tools)

The two protocols behave the same; version 2 of the wlr one is needed for the primary selection.

| Request / event | Behaviour |
| --- | --- |
| `create_data_source`, source `offer(mime)` | 256 distinct types; `offer` after the source was used: `invalid_offer` |
| `get_data_device(seat)` | a device; it gets the current clipboard and (if on) primary selection at once |
| `set_selection(source)` | no serial, no focus needed; replaces the clipboard (the previous source cancelled); `null` empties it; a used source: `used_source` |
| `set_primary_selection(source)` | the same for the primary selection; while it is off: ignored and the source cancelled |
| events `data_offer`, `selection`, `primary_selection` | to every data-control device on every change of either slot (at most once per dispatch batch), and once on creation; `primary_selection` only while it is on |
| offer `receive(mime, fd)` | as `wl_data_offer.receive` |
| source `send`, `cancelled` | as for `wl_data_source` |
| `finished` (device event) | never sent: the seat never goes away |

## Misbehaving clients (spec, Principle II)

| Case | Answer | Test |
| --- | --- | --- |
| `set_selection` with a serial never sent, too old, or from another client's input | ignored, source cancelled, clipboard unchanged | protocol |
| a source used twice | clipboard tools: `used_source` (their protocols require it); applications (`wl_data_device`, primary selection): no error, the owner set again changes nothing | protocol |
| 10 000 `offer` calls on one source, many duplicates | 256 distinct kept; client keeps running | protocol |
| `receive` on an offer for replaced content | descriptor closed, nothing sent to anyone | protocol |
| `receive` with a type the source did not offer | descriptor closed | protocol |
| `finish` / `set_actions` on a selection offer | `invalid_finish` / `invalid_offer` | protocol |
| owner never writes / reader never reads | only those two clients affected; RubickWay never touches the pipe | protocol (owner never writes: the reader's end stays open, the server keeps dispatching) |
| 10 000 `set_selection` in a row from one client | each previous source cancelled; other clients are told at most once per dispatch (a few times, as the requests arrive over a few socket reads), the last time with the newest content | protocol |
| owner disconnects with the clipboard and primary selection | both empty; focused client and data-control devices told | protocol |
| a tool sets a selection, then disconnects | empty; told | protocol |
| `offer` on a data-control source after use | `invalid_offer` | protocol |
