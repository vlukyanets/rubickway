# Data Model: Clipboard

All of it is server state (research C2); the shell does not change.

## SelectionState (new, `src/server/selection.rs`, in `ServerState`)

| Field | Type | Notes |
| --- | --- | --- |
| `clipboard` | `Slot` | the clipboard |
| `primary` | `Slot` | the primary selection; stays empty while it is off |
| `primary_enabled` | `bool` | `true` until `Server::set_primary_selection(false)` (research C10) |
| `devices` | `Vec<Device>` | every live device of every client |
| `announced` | `Option<(ClientId, u64, u64)>` | the focused client last told, with the generations of both slots it was told (research C3) |
| `control_announced` | `(u64, u64)` | the generations data-control devices were last told |

### Slot

| Field | Type | Notes |
| --- | --- | --- |
| `owner` | `Option<Source>` | `None`: empty |
| `generation` | `u64` | bumped on every change; offers made for an older one are stale (C6) |

### Source (enum)

One of the four source kinds; each carries its `SourceData` as user data.

- `Data(WlDataSource)` - `wl_data_device_manager.create_data_source`
- `Primary(ZwpPrimarySelectionSourceV1)`
- `ExtControl(ExtDataControlSourceV1)`
- `WlrControl(ZwlrDataControlSourceV1)`

Operations: `send(mime, fd)`, `cancelled()`, `client()`, `mime_types()`.

### SourceData (user data of every source, `Mutex<SourceData>`)

| Field | Type | Notes |
| --- | --- | --- |
| `mime_types` | `Vec<String>` | distinct, in the order offered, at most 256 (C6) |
| `used` | `bool` | set by `set_selection`, `set_primary_selection` or `start_drag`: the type list is frozen; a second use is `used_source` for clipboard tools only (C8) |

### Device (enum)

- `Data(WlDataDevice)` - told the clipboard while its client has the focus
- `Primary(ZwpPrimarySelectionDeviceV1)` - told the primary selection while its client has the focus
- `ExtControl(ExtDataControlDeviceV1)` - told both slots, always
- `WlrControl(ZwlrDataControlDeviceV1)` - told both slots, always (primary only from version 2)

Dead devices are dropped when their resource is destroyed.

### OfferData (user data of every offer)

| Field | Type | Notes |
| --- | --- | --- |
| `slot` | `Which` (`Clipboard` / `Primary`) | |
| `generation` | `u64` | the slot's generation it was made for; older than the slot's: stale |

## SeatState (changed, `src/server/seat.rs`)

| Field | Type | Notes |
| --- | --- | --- |
| `input_serials` (new) | `HashMap<ClientId, VecDeque<u32>>` | per application, the serials of its 16 latest key press/release, button press/release and keyboard enter events (research C4); pruned with `popup_serials` |

Function `selection_serial_ok(state, client, serial) -> bool`.

## Server API (changed, `src/server/mod.rs`)

- `Server::set_primary_selection(enabled: bool)`: called before applications connect, like
  `set_gpu_offer`; `false` removes the primary selection global and turns the slot off. `Server::new()`
  is unchanged and starts with it on.

## Config (changed, `src/config.rs`)

| Field | Type | Notes |
| --- | --- | --- |
| `primary_selection` (new) | `bool` | `primary-selection off` in `config.kdl` (contracts/config.md); default `true` |

## Life of a slot

```text
empty --set_selection(source, valid serial)--> owned(source, gen+1)
owned(a) --set_selection(b)--> owned(b, gen+1), a.cancelled()
owned(a) --set_selection(null)--> empty(gen+1), a.cancelled()
owned(a) --a destroyed / a's client gone--> empty(gen+1)
any --set_selection with a bad serial--> unchanged; the new source gets cancelled()
```

Every `gen+1` marks the slot for announcement at the end of the batch (research C3).
