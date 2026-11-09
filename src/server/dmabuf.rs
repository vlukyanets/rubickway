//! `zwp_linux_dmabuf_v1`: GPU images from applications (research G2, G3, G8, contracts/protocols.md).
//!
//! The server holds no GPU context, so it never imports anything itself: a description becomes a
//! `GpuBuffer` in `Pending`, `process_imports` hands it to whoever owns the renderer, right after
//! the requests of a pass of the loop were read, and commits waiting for it then go on (G7).

use std::collections::HashSet;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_buffer_params_v1::{
    self, ZwpLinuxBufferParamsV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1::{
    self, ZwpLinuxDmabufFeedbackV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_v1::{
    self, ZwpLinuxDmabufV1,
};
use wayland_server::backend::ClientId;
use wayland_server::protocol::wl_buffer::WlBuffer;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use super::commit_queue::WaitId;
use super::shm::BufferKind;
use super::{ClientState, ServerState, compositor};
use crate::ffi::dmabuf as files;
use crate::ffi::shm_map::sealed_memfd_bytes;
use crate::render::formats::{self, FormatPair};

/// Names an imported image for the renderer; never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferKey(pub u64);

/// How the server learns what a client's file is. Real dma-bufs need a device, so the protocol
/// tests bring a double (research G11).
pub trait ImageFiles {
    /// The file's size if it is a GPU image, `None` otherwise.
    fn describe(&self, fd: BorrowedFd<'_>) -> Option<u64>;
}

/// The real check: recognition first, then the size (research G3).
pub(crate) struct DmaBufFiles;

impl ImageFiles for DmaBufFiles {
    fn describe(&self, fd: BorrowedFd<'_>) -> Option<u64> {
        files::size_of(fd)
    }
}

/// One plane of an image to import.
#[derive(Debug, Clone, Copy)]
pub struct GpuPlane<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

/// An image to import, as `Server::process_imports` hands it out.
#[derive(Debug)]
pub struct GpuImage<'a> {
    pub key: BufferKey,
    pub width: i32,
    pub height: i32,
    pub fourcc: u32,
    pub modifier: u64,
    pub planes: &'a [GpuPlane<'a>],
}

/// What a surface shows from now on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownImage {
    pub surface: super::SurfaceKey,
    pub image: BufferKey,
    pub y_invert: bool,
}

/// The GPU and what it reads, told to every application (FR-002). It does not change while
/// RubickWay runs.
pub(crate) struct GpuOffer {
    main_device: u64,
    pairs: Vec<FormatPair>,
    table: OwnedFd,
    table_size: u32,
}

pub(crate) struct Plane {
    fd: OwnedFd,
    offset: u32,
    stride: u32,
}

/// The import outcome of a `GpuBuffer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportState {
    Pending(WaitId),
    Ready,
    Failed,
}

/// A GPU image behind a `wl_buffer`. Its plane files stay open for its life.
/// A file that becomes readable when something a commit waits for is finished: an image, or a
/// synchronisation point (research G6).
pub struct Wait {
    pub id: WaitId,
    pub fd: OwnedFd,
}

pub(crate) struct GpuBuffer {
    pub key: BufferKey,
    pub width: i32,
    pub height: i32,
    fourcc: u32,
    modifier: u64,
    planes: Vec<Plane>,
    pub y_invert: bool,
    pub state: ImportState,
}

impl GpuBuffer {
    /// Copies of the files of planes still being drawn; most images are finished by the commit, so
    /// most of the time none (research G6). One that cannot be copied is an error for the caller:
    /// the image is then shown as it is, as running out of descriptors must not stop a window for
    /// good.
    pub(crate) fn unfinished_planes(&self) -> Vec<std::io::Result<OwnedFd>> {
        self.planes
            .iter()
            .filter(|plane| !files::is_readable(plane.fd.as_fd()))
            .map(|plane| plane.fd.try_clone())
            .collect()
    }

    fn with_description<R>(&self, f: impl FnOnce(GpuImage<'_>) -> R) -> R {
        let planes: Vec<GpuPlane<'_>> = self
            .planes
            .iter()
            .map(|p| GpuPlane {
                fd: p.fd.as_fd(),
                offset: p.offset,
                stride: p.stride,
            })
            .collect();
        f(GpuImage {
            key: self.key,
            width: self.width,
            height: self.height,
            fourcc: self.fourcc,
            modifier: self.modifier,
            planes: &planes,
        })
    }
}

/// `zwp_linux_buffer_params_v1`'s state.
#[derive(Default)]
pub(crate) struct BufferParams {
    planes: [Option<Plane>; 4],
    modifier: Option<u64>,
    used: bool,
}

/// An import to do at the next `process_imports`.
pub(crate) enum PendingImport {
    /// `create`: the `wl_buffer` exists only if the import works.
    Create {
        params: ZwpLinuxBufferParamsV1,
        buffer: GpuBuffer,
    },
    /// `create_immed`: the `wl_buffer` exists; commits may be waiting for it.
    Immed {
        params: ZwpLinuxBufferParamsV1,
        resource: WlBuffer,
        wait: WaitId,
    },
}

/// Offers GPU images: called at most once, before applications connect.
pub(crate) fn set_offer(
    state: &mut ServerState,
    main_device: u64,
    pairs: Vec<FormatPair>,
) -> io::Result<()> {
    let (table, table_size) = sealed_memfd_bytes(&formats::table_bytes(&pairs))?;
    state.gpu_offer = Some(GpuOffer {
        main_device,
        pairs,
        table,
        table_size,
    });
    state
        .dh
        .create_global::<ServerState, ZwpLinuxDmabufV1, ()>(5, ());
    Ok(())
}

impl GlobalDispatch<ZwpLinuxDmabufV1, ()> for ServerState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpLinuxDmabufV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let dmabuf = data_init.init(resource, ());
        // From version 4 on applications get the table through feedback instead.
        if dmabuf.version() >= 4 {
            return;
        }
        let Some(offer) = &state.gpu_offer else {
            return;
        };
        let mut seen = HashSet::new();
        for (format, modifier) in &offer.pairs {
            if seen.insert(*format) {
                dmabuf.format(*format);
            }
            if dmabuf.version() >= 3 {
                dmabuf.modifier(*format, (*modifier >> 32) as u32, *modifier as u32);
            }
        }
    }
}

fn send_feedback(state: &ServerState, feedback: &ZwpLinuxDmabufFeedbackV1) {
    let Some(offer) = &state.gpu_offer else {
        return;
    };
    let device = offer.main_device.to_ne_bytes().to_vec();
    feedback.format_table(offer.table.as_fd(), offer.table_size);
    feedback.main_device(device.clone());
    feedback.tranche_target_device(device);
    feedback.tranche_flags(zwp_linux_dmabuf_feedback_v1::TrancheFlags::empty());
    let indices: Vec<u8> = (0..offer.pairs.len())
        .flat_map(|i| (i as u16).to_ne_bytes())
        .collect();
    feedback.tranche_formats(indices);
    feedback.tranche_done();
    feedback.done();
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpLinuxDmabufV1,
        request: zwp_linux_dmabuf_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_dmabuf_v1::Request::CreateParams { params_id } => {
                let params = data_init.init(params_id, ());
                state
                    .buffer_params
                    .insert(params.id(), BufferParams::default());
            }
            zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { id }
            | zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { id, .. } => {
                let feedback = data_init.init(id, ());
                send_feedback(state, &feedback);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwpLinuxDmabufFeedbackV1,
        _: zwp_linux_dmabuf_feedback_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

/// A valid description, ready to import.
struct Valid {
    planes: Vec<Plane>,
    modifier: u64,
}

/// Checks `create` or `create_immed`. `Err(None)`: a protocol error was posted, the client is
/// gone. `Err(Some(reason))`: only this image fails (FR-016).
fn validate(
    state: &mut ServerState,
    resource: &ZwpLinuxBufferParamsV1,
    width: i32,
    height: i32,
    format: u32,
    flags: &WEnum<zwp_linux_buffer_params_v1::Flags>,
) -> Result<Valid, Option<&'static str>> {
    use zwp_linux_buffer_params_v1::Error;
    let post = |code: Error, message: String| {
        resource.post_error(code, message);
        None
    };
    let Some(params) = state.buffer_params.get_mut(&resource.id()) else {
        return Err(None);
    };
    if params.used {
        return Err(post(
            Error::AlreadyUsed,
            "the parameters were used already".into(),
        ));
    }
    params.used = true;
    let count = params.planes.iter().take_while(|p| p.is_some()).count();
    let total = params.planes.iter().filter(|p| p.is_some()).count();
    if count != total || count < formats::min_planes(format).max(1) {
        return Err(post(
            Error::Incomplete,
            format!("{count} contiguous planes of {total} for format {format:#x}"),
        ));
    }
    if width <= 0 || height <= 0 {
        return Err(post(
            Error::InvalidDimensions,
            format!("size {width}x{height}"),
        ));
    }
    let modifier = params.modifier.unwrap_or(formats::MOD_INVALID);
    let offered = state
        .gpu_offer
        .as_ref()
        .is_some_and(|o| o.pairs.contains(&(format, modifier)));
    if !offered {
        return Err(post(
            Error::InvalidFormat,
            format!("format {format:#x} modifier {modifier:#x} was not offered"),
        ));
    }
    let known = zwp_linux_buffer_params_v1::Flags::YInvert.bits();
    let supported = match flags {
        WEnum::Value(f) => f.bits() & !known == 0,
        WEnum::Unknown(_) => false,
    };
    if !supported {
        return Err(Some("unsupported flags"));
    }
    for (index, plane) in params.planes.iter().flatten().enumerate() {
        let Some(size) = state.image_files.describe(plane.fd.as_fd()) else {
            return Err(Some("a file that is not a GPU image"));
        };
        let offset = u64::from(plane.offset);
        // Other planes may be subsampled: how far they reach is the driver's to check on import.
        let needed = if index == 0 {
            u64::from(plane.stride)
                .checked_mul(height as u64)
                .and_then(|bytes| bytes.checked_add(offset))
        } else {
            Some(offset)
        };
        if needed.is_none_or(|end| end > size) {
            return Err(post(
                Error::OutOfBounds,
                format!("plane {index} does not fit its {size}-byte file"),
            ));
        }
    }
    let planes = params.planes.iter_mut().filter_map(Option::take).collect();
    Ok(Valid { planes, modifier })
}

fn new_gpu_buffer(
    state: &mut ServerState,
    width: i32,
    height: i32,
    format: u32,
    flags: &WEnum<zwp_linux_buffer_params_v1::Flags>,
    valid: Valid,
    wait: WaitId,
) -> GpuBuffer {
    state.next_buffer += 1;
    GpuBuffer {
        key: BufferKey(state.next_buffer),
        width,
        height,
        fourcc: format,
        modifier: valid.modifier,
        planes: valid.planes,
        y_invert: matches!(flags, WEnum::Value(f) if f.contains(zwp_linux_buffer_params_v1::Flags::YInvert)),
        state: ImportState::Pending(wait),
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwp_linux_buffer_params_v1::{Error, Request};
        match request {
            Request::Add {
                fd,
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            } => {
                let Some(params) = state.buffer_params.get_mut(&resource.id()) else {
                    return;
                };
                if params.used {
                    resource.post_error(Error::AlreadyUsed, "the parameters were used already");
                    return;
                }
                let Some(slot) = params.planes.get_mut(plane_idx as usize) else {
                    resource.post_error(Error::PlaneIdx, format!("plane {plane_idx}"));
                    return;
                };
                if slot.is_some() {
                    resource.post_error(Error::PlaneSet, format!("plane {plane_idx} again"));
                    return;
                }
                let modifier = (u64::from(modifier_hi) << 32) | u64::from(modifier_lo);
                if params.modifier.is_some_and(|m| m != modifier) {
                    resource.post_error(Error::InvalidFormat, "different modifiers across planes");
                    return;
                }
                params.modifier = Some(modifier);
                *slot = Some(Plane { fd, offset, stride });
            }
            Request::Create {
                width,
                height,
                format,
                flags,
            } => match validate(state, resource, width, height, format, &flags) {
                Ok(valid) => {
                    let wait = state.new_wait();
                    let buffer = new_gpu_buffer(state, width, height, format, &flags, valid, wait);
                    state.pending_imports.push(PendingImport::Create {
                        params: resource.clone(),
                        buffer,
                    });
                }
                Err(Some(reason)) => {
                    log_failure(state, client_number(client), reason);
                    resource.failed();
                }
                Err(None) => {}
            },
            Request::CreateImmed {
                buffer_id,
                width,
                height,
                format,
                flags,
            } => {
                let buffer = data_init.init(buffer_id, ());
                let outcome = validate(state, resource, width, height, format, &flags);
                match outcome {
                    Ok(valid) => {
                        let wait = state.new_wait();
                        let gpu = new_gpu_buffer(state, width, height, format, &flags, valid, wait);
                        state.buffers.insert(buffer.id(), BufferKind::Gpu(gpu));
                        state.pending_imports.push(PendingImport::Immed {
                            params: resource.clone(),
                            resource: buffer,
                            wait,
                        });
                    }
                    Err(reason) => {
                        // Answered `failed`; the buffer exists and shows nothing (research G8).
                        state.buffers.insert(
                            buffer.id(),
                            BufferKind::Gpu(GpuBuffer {
                                key: BufferKey(0),
                                width: 1,
                                height: 1,
                                fourcc: format,
                                modifier: formats::MOD_INVALID,
                                planes: Vec::new(),
                                y_invert: false,
                                state: ImportState::Failed,
                            }),
                        );
                        if let Some(reason) = reason {
                            log_failure(state, client_number(client), reason);
                            resource.failed();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &ZwpLinuxBufferParamsV1, _: &()) {
        state.buffer_params.remove(&resource.id());
    }
}

/// The application's number in the log.
pub(crate) fn client_number(client: &Client) -> u64 {
    client.get_data::<ClientState>().map_or(0, |c| c.id)
}

/// One line per application and cause (FR-015).
pub(crate) fn log_failure(state: &mut ServerState, application: u64, reason: &str) {
    if state
        .logged_failures
        .insert((application, reason.to_string()))
    {
        tracing::warn!("application {application}: an image failed to import ({reason})");
    }
}

/// As [`log_failure`], for what the import step refused; `reason` names the GPU.
fn log_refusal(state: &mut ServerState, application: u64, reason: &str) {
    if state
        .logged_failures
        .insert((application, reason.to_string()))
    {
        tracing::warn!("application {application}: {reason}; its window keeps its last picture");
    }
}

/// Does the imports asked for since the last call through `import`, answers applications, and lets
/// the commits waiting for them go on (research G7, G8). An import failure ends nobody.
pub(crate) fn process_imports(
    state: &mut ServerState,
    mut import: impl FnMut(GpuImage<'_>) -> Result<(), String>,
) {
    for pending in std::mem::take(&mut state.pending_imports) {
        match pending {
            PendingImport::Create { params, mut buffer } => {
                let result = buffer.with_description(&mut import);
                match result {
                    Ok(()) => {
                        let dh = state.dh.clone();
                        let Some(client) = params.client() else {
                            continue;
                        };
                        let Ok(resource) =
                            client.create_resource::<WlBuffer, (), ServerState>(&dh, 1, ())
                        else {
                            continue;
                        };
                        buffer.state = ImportState::Ready;
                        state.buffers.insert(resource.id(), BufferKind::Gpu(buffer));
                        params.created(&resource);
                    }
                    Err(reason) => {
                        report_failed(state, &params, &reason);
                    }
                }
            }
            PendingImport::Immed {
                params,
                resource,
                wait,
            } => {
                // The application may have destroyed the buffer already.
                if let Some(BufferKind::Gpu(buffer)) = state.buffers.get_mut(&resource.id()) {
                    let result = buffer.with_description(&mut import);
                    buffer.state = match result {
                        Ok(()) => ImportState::Ready,
                        Err(_) => ImportState::Failed,
                    };
                    if let Err(reason) = result {
                        report_failed(state, &params, &reason);
                    }
                }
                compositor::wait_done(state, wait);
            }
        }
    }
}

fn report_failed(state: &mut ServerState, params: &ZwpLinuxBufferParamsV1, reason: &str) {
    let application = params.client().map_or(0, |c| client_number(&c));
    log_refusal(state, application, reason);
    if params.is_alive() {
        params.failed();
    }
}
