# Data Model: GPU Buffers

Plain data unless noted. Limits and protocol errors are in [contracts/protocols.md](contracts/protocols.md).

## Server

### BufferKind (per `wl_buffer`)

- `Shm(ShmBuffer)`: as today.
- `Gpu(GpuBuffer)`.

Both give width and height, so `compositor::apply` no longer depends on which kind it holds.

### GpuBuffer

| Field | Meaning |
| --- | --- |
| `key` | `BufferKey(u64)`, never reused; the renderer's name for its image |
| `width`, `height` | positive, validated |
| `fourcc`, `modifier` | one modifier for all planes |
| `planes` | 1 to 4 of `{ fd: OwnedFd, offset: u32, stride: u32 }`, kept for the buffer's life (polling) |
| `y_invert` | from the `create` flags |
| `state` | `Pending` -> `Ready` or `Failed` (import outcome) |

Dropped with the `wl_buffer`. If a commit still holds it, it stays until released (`ReleaseTracker`).

### BufferParams (`zwp_linux_buffer_params_v1`)

`planes: [Option<Plane>; 4]`, `modifier: Option<u64>`, `used: bool`. See validation in the contract.

### GpuOffer

The display GPU as the server sees it, set once at start. `None` when the GPU cannot import images: no
global (FR-014).

| Field | Meaning |
| --- | --- |
| `main_device` | `dev_t` of the display GPU's render node |
| `table` | sealed memfd of `(fourcc: u32, pad: u32, modifier: u64)` entries |
| `pairs` | the same pairs for look-ups |

### Transaction

One entry of a commit root's queue.

| Field | Meaning |
| --- | --- |
| `parts` | `(surface, SurfaceState)` of the commit root and synchronised descendants |
| `xdg` | captured `pending_geometry` and `pending_limits` for a toplevel root |
| `waits` | ids of waits still outstanding: `Wait`s, and imports not done yet (no fd; `process_imports` settles them) |

`SurfaceState` gains `sync_points: Option<SyncPoints>`.

### CommitQueue

Generic and pure: `CommitQueue<T>` per commit root.

Rules, each with a unit test or proptest:

- `push(T, waits)`: added last. If it has no waits and the queue is empty, returned at once for
  applying (today's timing).
- `done(wait)`: marks it finished; returns every transaction now at the front with no waits, in order.
- A later transaction with finished waits never overtakes an earlier one.
- `remove_surface` drops a destroyed surface's parts; `clear` when its client goes.

### Wait

`{ id: WaitId, fd: OwnedFd }`: readable means finished. Comes from a duplicate dma-buf fd (implicit) or
a timeline eventfd (explicit). `App` owns the source; `Server::wait_done(id)` reports it. A wait whose
transaction is gone is ignored, and `Server::take_cancelled_waits` names those of a destroyed surface so
that `App` drops their files, since an acquire point may never be reached.

### SyncPoints

`{ acquire: (TimelineId, u64), release: (TimelineId, u64) }`. Timeline objects are reference-counted:
a point stays valid after `wp_linux_drm_syncobj_timeline_v1.destroy`, per the protocol.

### Timelines (trait)

`import(OwnedFd) -> Result<TimelineId>`, `wait_fd(id, point) -> Result<OwnedFd>`, `signal(id, point)`,
`forget(id)`. Real implementation on the display GPU's render node, double in the test harness (G11).

### ImageFiles (trait)

`describe(BorrowedFd) -> Option<u64>`: the file's size if it is a dma-buf, `None` otherwise. Real
implementation in `ffi/dmabuf.rs` (recognition first, then size, G3); the double in the test harness
accepts a memfd (G11).

### ReleaseTracker

Pure. Entries are `(ToRelease, retired_at: u64)`, where `ToRelease` is a `wl_buffer.release` or a
release point, the latter with its application: it is not signalled once that one has gone.

- `retire(buffer, seq)`: `seq` is the sequence of the newest frame drawn so far.
- `release_ready(oldest_in_flight: Option<u64>)`: returns entries with `retired_at <
  oldest_in_flight`, or all when nothing is in flight.

## Renderer

### ImageDesc

Plain description, built from `GpuBuffer`: width, height, fourcc, modifier, planes as borrowed fds with
offset and stride. The renderer knows nothing of Wayland.

### ImportedImage (keyed by `BufferKey`)

EGLImage and its `GL_TEXTURE_EXTERNAL_OES` texture. Destroyed with the buffer or the renderer.

### Source (per surface, keyed by `SurfaceKey`)

- `Copied { texture, opaque }`: today's path.
- `Image { key, y_invert, mip: Option<MipCopy> }`.

### MipCopy

Owned RGBA8 texture with mipmaps, for an image drawn below its pixel size. `of: BufferKey` tells whether
it is still current.

## App

Each output keeps `in_flight: Option<u64>`, the sequence of its frame awaiting a flip (cleared on pause);
the oldest of them goes to `ReleaseTracker::release_ready`.
