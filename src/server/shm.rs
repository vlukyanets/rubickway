//! `wl_shm`, `wl_shm_pool`, `wl_buffer`: shared-memory buffers, validated with checked
//! arithmetic and read only through the SIGBUS-safe mapping (research R4, data-model.md).

use std::cell::RefCell;
use std::rc::Rc;

use wayland_server::backend::ClientId;
use wayland_server::protocol::wl_buffer::{self, WlBuffer};
use wayland_server::protocol::wl_shm::{self, WlShm};
use wayland_server::protocol::wl_shm_pool::{self, WlShmPool};
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use super::{ServerState, ShmImage, SurfaceKey};
use crate::ffi::shm_map::ShmMap;

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlShm, ()>(1, ());
}

/// A client's pool: its mapping, shared with the buffers created from it.
pub(crate) struct Pool {
    map: Rc<RefCell<ShmMap>>,
}

/// A buffer inside a pool.
pub(crate) struct ShmBuffer {
    map: Rc<RefCell<ShmMap>>,
    offset: usize,
    pub width: i32,
    pub height: i32,
    stride: i32,
    opaque: bool,
}

impl GlobalDispatch<WlShm, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlShm>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
        shm.format(wl_shm::Format::Xrgb8888);
    }
}

impl Dispatch<WlShm, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        shm: &WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let wl_shm::Request::CreatePool { id, fd, size } = request else {
            return;
        };
        let pool = data_init.init(id, ());
        let Ok(len) = usize::try_from(size) else {
            shm.post_error(wl_shm::Error::InvalidStride, "negative pool size");
            return;
        };
        match ShmMap::new(fd, len) {
            Ok(map) => {
                state.pools.insert(
                    pool.id(),
                    Pool {
                        map: Rc::new(RefCell::new(map)),
                    },
                );
            }
            Err(err) => shm.post_error(
                wl_shm::Error::InvalidFd,
                format!("cannot map the pool: {err}"),
            ),
        }
    }
}

impl Dispatch<WlShmPool, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WlShmPool,
        request: wl_shm_pool::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_shm_pool::Request::CreateBuffer {
                id,
                offset,
                width,
                height,
                stride,
                format,
            } => {
                let buffer = data_init.init(id, ());
                let Some(pool) = state.pools.get(&resource.id()) else {
                    return;
                };
                let opaque = match format {
                    WEnum::Value(wl_shm::Format::Argb8888) => false,
                    WEnum::Value(wl_shm::Format::Xrgb8888) => true,
                    _ => {
                        resource.post_error(wl_shm::Error::InvalidFormat, "unsupported format");
                        return;
                    }
                };
                let pool_len = pool.map.borrow().len() as i64;
                let fits = width > 0
                    && height > 0
                    && offset >= 0
                    && i64::from(stride) >= i64::from(width) * 4
                    && i64::from(offset) + i64::from(stride) * i64::from(height) <= pool_len;
                if !fits {
                    resource.post_error(
                        wl_shm::Error::InvalidStride,
                        format!(
                            "buffer {width}x{height} stride {stride} offset {offset} does not fit the {pool_len}-byte pool"
                        ),
                    );
                    return;
                }
                state.buffers.insert(
                    buffer.id(),
                    ShmBuffer {
                        map: pool.map.clone(),
                        offset: offset as usize,
                        width,
                        height,
                        stride,
                        opaque,
                    },
                );
            }
            wl_shm_pool::Request::Resize { size } => {
                let Some(pool) = state.pools.get(&resource.id()) else {
                    return;
                };
                let current = pool.map.borrow().len();
                match usize::try_from(size) {
                    Ok(new) if new >= current => {
                        if let Err(err) = pool.map.borrow_mut().grow(new) {
                            resource.post_error(
                                wl_shm::Error::InvalidFd,
                                format!("cannot grow the pool: {err}"),
                            );
                        }
                    }
                    _ => resource.post_error(wl_shm::Error::InvalidFd, "shrinking a pool"),
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlShmPool, _: &()) {
        state.pools.remove(&resource.id());
    }
}

impl Dispatch<WlBuffer, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlBuffer,
        _: wl_buffer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlBuffer, _: &()) {
        state.buffers.remove(&resource.id());
    }
}

/// Lets the client reuse a buffer.
pub(crate) fn release(buffer: &WlBuffer) {
    if buffer.is_alive() {
        buffer.release();
    }
}

/// Copies every pending buffer through `upload`, then releases it (research R5).
pub(crate) fn upload_pending(
    state: &mut ServerState,
    mut upload: impl FnMut(SurfaceKey, ShmImage<'_>),
) {
    let pending: Vec<_> = state
        .surfaces
        .iter_mut()
        .filter_map(|(id, s)| s.upload.take().map(|b| (id.clone(), s.key, b)))
        .collect();
    for (surface, key, buffer) in pending {
        let Some(shm) = state.buffers.get(&buffer.id()) else {
            continue;
        };
        let (Ok(width), Ok(height), Ok(stride)) = (
            u32::try_from(shm.width),
            u32::try_from(shm.height),
            u32::try_from(shm.stride),
        ) else {
            continue;
        };
        let len = stride as usize * height as usize;
        let result = shm.map.borrow().with_access(shm.offset, len, |bytes| {
            upload(
                key,
                ShmImage {
                    bytes,
                    width,
                    height,
                    stride,
                    opaque: shm.opaque,
                },
            );
        });
        match result {
            Ok(()) => {
                if let Some(s) = state.surfaces.get_mut(&surface) {
                    s.has_content = true;
                }
                release(&buffer);
            }
            Err(_) => buffer.post_error(
                wl_shm::Error::InvalidFd as u32,
                "the buffer's memory shrank while it was read",
            ),
        }
    }
}
