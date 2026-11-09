# Contract: Wayland protocols

Adds two globals; everything else stays as in `specs/20261102-143401-drm-backend/contracts/protocols.md`.

| Global | Version | Offered when |
| --- | --- | --- |
| `zwp_linux_dmabuf_v1` | 5 | the GPU can import dma-bufs (`EGL_EXT_image_dma_buf_import_modifiers`) |
| `wp_linux_drm_syncobj_manager_v1` | 1 | also, the GPU reports `DRM_CAP_SYNCOBJ_TIMELINE` |

If not offered, one log line says why (`contracts/cli.md`). The offer does not change while RubickWay
runs.

## zwp_linux_dmabuf_v1

- Bound at version 4 or 5: no `format` or `modifier` events; `get_default_feedback` and
  `get_surface_feedback` work.
- Bound below 4: `format` and `modifier` events at bind, from the table.

### zwp_linux_dmabuf_feedback_v1

Sent once at creation, in this order: `format_table`, `main_device`, `tranche_target_device`,
`tranche_flags` (0), `tranche_formats`, `tranche_done`, `done`.

- `main_device` and `tranche_target_device`: the display GPU's render node.
- `tranche_formats`: all table entries.
- Surface feedback equals default feedback. Feedback for a destroyed surface stays inert.

### zwp_linux_buffer_params_v1

| Situation | Answer |
| --- | --- |
| `add` with plane index 4 or more | error `plane_idx` |
| `add` for a plane already set | error `plane_set` |
| `add` with a modifier other than the earlier planes' | error `invalid_format` |
| `add` or `create` after `create` | error `already_used` |
| planes not contiguous from 0, or fewer than the format's minimum (for example NV12: 2) | error `incomplete` |
| width or height zero or negative | error `invalid_dimensions` |
| `offset + stride * height` beyond plane 0's buffer, `offset` beyond any plane's buffer, or overflow | error `out_of_bounds` |
| format and modifier not in the table | error `invalid_format` |
| flags other than `y_invert` | `failed` |
| a file that is not a dma-buf | `failed` |
| more planes than the driver accepts for that format and modifier | `failed` |
| the GPU refuses the import | `failed` |

- `create`: `created` or `failed`, after the import, right after the requests are read.
- `create_immed`: the `wl_buffer` exists at once; on failure `failed` is sent, and it is marked failed.
  An import failure never ends an application.
- A commit of a buffer not imported yet waits for the import like any other wait (see Commit order); this
  takes no longer than the same pass of the event loop.
- An image larger than the GPU's limit: on commit, the existing `wl_surface.invalid_size` rule.

### wl_buffer from a GPU image

- `wl_surface.attach` and `commit` as for any buffer, with viewport, scale and transform unchanged.
- A failed buffer committed to a surface: nothing changes on screen or about the window, its frame
  callbacks complete as usual, it goes back as usual (FR-005: `release`, or the release point on a surface
  using explicit synchronisation), and it is logged once per application for that cause. No error.
- `release` is sent when no frame in flight can read it any more (research G9), not at commit. For a
  surface with `wp_linux_drm_syncobj_surface_v1`, `release` is not sent: the release point is signalled.
- A buffer destroyed while shown: the surface keeps its picture until the next commit.

## wp_linux_drm_syncobj_v1

| Situation | Answer |
| --- | --- |
| `get_surface` for a surface that already has one | error `surface_exists` |
| `import_timeline` with an fd that cannot be imported | error `invalid_timeline` |
| `set_acquire_point` or `set_release_point` after the surface was destroyed | error `no_surface` |
| commit with a point and no buffer | error `no_buffer` |
| commit with a buffer and no acquire point | error `no_acquire_point` |
| commit with a buffer and no release point | error `no_release_point` |
| commit with a shared-memory buffer and points | error `unsupported_buffer` |
| the same timeline with acquire not below release | error `conflicting_points` |

- The commit takes effect after the acquire point is signalled. Implicit fences are then ignored.
- The release point is signalled once no frame in flight can read the buffer (G9).
- A point set before a `destroy` of the timeline stays valid.
- An acquire point never signalled: the surface keeps its last picture. On disconnect everything is
  dropped, and the pending release points are not signalled.

## Commit order

- A surface's update takes effect when its image, and those of its synchronised subsurfaces, are
  finished, and not before earlier updates of that surface.
- A desynchronised subsurface has its own queue.
- Frame callbacks of an update complete only after it takes effect and is shown.
- No wait ever holds up another surface's queue, input or a frame.

## Limits

None per application on GPU-image buffers, parameter objects, timelines or waiting updates (spec
Assumptions).
