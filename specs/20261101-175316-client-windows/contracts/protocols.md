# Contract: Wayland protocols offered to applications

What an application connecting to RubickWay's socket sees. Versions are maxima: a global is
advertised at exactly this version (constitution VI). Details and rationale: research R2,
R6, R9, R10, R12.

| Global | Version | Behaviour |
| --- | --- | --- |
| `wl_compositor` | 6 | surfaces and regions with full commit semantics; `preferred_buffer_scale` = ceil(viewer scale) |
| `wl_subcompositor` | 1 | subsurfaces, sync and desync modes, place above/below, position |
| `wl_shm` | 1 | formats `argb8888`, `xrgb8888`; pools may grow only |
| `xdg_wm_base` | 6 | toplevels; popups get `popup_done` at once; window-management requests are accepted and ignored |
| `wl_seat` | 8 | capabilities pointer + keyboard, name `seat0` |
| `wl_output` | 4 | one output `RUBICKWAY-1`: mode = viewer buffer size, scale = ceil(viewer scale) |
| `wl_data_device_manager` | 3 | stub: devices and sources exist, nothing is ever transferred; drags are cancelled |
| `wp_viewporter` | 1 | source crop and destination size |
| `wp_fractional_scale_manager_v1` | 1 | `preferred_scale` = round(viewer scale x 120) |

## Errors that disconnect an application

| Cause | Error |
| --- | --- |
| buffer parameters outside the pool, bad stride or format | `wl_shm.invalid_stride` / `invalid_format` |
| pool fd shrank while RubickWay read it (SIGBUS), or pool resized smaller | `wl_shm.invalid_fd` |
| buffer wider or taller than the GPU's largest texture | `wl_surface.invalid_size` ("buffer WxH larger than the <n> px the GPU can show"; wayland-server cannot send `wl_display` errors) |
| any request the protocol defines as an error (roles, scale, transform, viewport, subsurface parent, xdg configure order, ...) | the error code from that protocol |
| more than 1 MiB of unsent messages (the application does not read) | connection closed |

All other applications and RubickWay keep running (FR-015).

## Frame callbacks

Completed only for surfaces of windows that intersect the view, when the host shows the frame
that contains them (FR-006). None while the viewer is hidden or for windows outside the view.
