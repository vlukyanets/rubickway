# Contract: Wayland protocols

Adds one global and changes `wl_pointer.set_cursor`. Everything else stays as in
`specs/20261102-143401-drm-backend/contracts/protocols.md` and
`specs/20261104-004828-gpu-buffers/contracts/protocols.md`.

| Global | Version | Offered when |
| --- | --- | --- |
| `wp_cursor_shape_manager_v1` | 2 | always |

## Which cursor requests count (research C2)

A cursor request is `wp_cursor_shape_device_v1.set_shape` or `wl_pointer.set_cursor`. It takes effect
only if both hold:

- the pointer is over a surface of the requesting client;
- its serial is the one of the latest `wl_pointer.enter` sent to that client.

Otherwise it is ignored, with no error. At every `wl_pointer.enter` the cursor is the arrow until the
client asks for something. The latest counted request decides what is shown (spec FR-004). During
RubickWay gestures the client has no pointer (it got `leave`). After the gesture it gets a new
`enter` and sets its cursor again.

## wp_cursor_shape_manager_v1

| Request | Behaviour |
| --- | --- |
| `get_pointer(id, pointer)` | a device for that `wl_pointer`; any number per pointer |
| `get_tablet_tool_v2` | cannot be made: no client can have a `zwp_tablet_tool_v2` |
| `destroy` | the manager goes; its devices keep working |

## wp_cursor_shape_device_v1

| Request | Behaviour |
| --- | --- |
| `set_shape(serial, shape)` with shape 1-34, or 35-36 on version 2 | a cursor request for `shape` (research C5 for the image) |
| `set_shape` with a value outside 1-36 | error `invalid_shape` |
| `set_shape` with 35 (`dnd_ask`) or 36 (`all_resize`) on a device whose manager was bound at version 1 | error `invalid_shape` |
| `set_shape` after its `wl_pointer` was destroyed | ignored |
| `destroy` | the device goes; the cursor stays as it is |

## wl_pointer

| Request | Behaviour (changed) |
| --- | --- |
| `set_cursor(serial, None, ..)` | a cursor request to hide the pointer while it is over the client's window, unless a RubickWay gesture runs. Before: always counted and kept across enters |
| `set_cursor(serial, Some(surface), ..)` | accepted (role `cursor`; a surface with another role is error `role`, as before), not shown: a cursor request for the arrow |

## Misbehaving applications (added cases, Principle II)

| Case | Result | Test |
| --- | --- | --- |
| shape value 0, 37 or `u32::MAX` | `invalid_shape` to that client only; other clients keep their cursor and connection | protocol |
| version 2 shape on a version 1 device | `invalid_shape` | protocol |
| `set_shape` or `set_cursor` with a stale or made-up serial | ignored | protocol |
| `set_shape` or `set_cursor` while the pointer is over another client's window or the background | ignored; the other client's cursor stays | protocol |
| `set_shape` after its `wl_pointer` was destroyed | ignored | protocol |
| 10 000 `set_shape` requests in one batch, alternating shapes or all the same | handled in one dispatch; one `CursorChanged` per actual change, none for repeats | protocol |
| client disconnects with its shape shown | the pointer leaves its window; the shape of what is under the pointer is shown | protocol + shell |
