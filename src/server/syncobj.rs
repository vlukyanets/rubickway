//! `wp_linux_drm_syncobj_v1`: explicit synchronisation (research G6, G9, contracts/protocols.md).
//!
//! An acquire point becomes one more `Wait`, and a release point one more way for a replaced image
//! to go back. The timelines themselves are behind a trait, so that tests need no device (G11).

use std::fmt;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;

use wayland_protocols::wp::linux_drm_syncobj::v1::server::wp_linux_drm_syncobj_manager_v1::{
    self, WpLinuxDrmSyncobjManagerV1,
};
use wayland_protocols::wp::linux_drm_syncobj::v1::server::wp_linux_drm_syncobj_surface_v1::{
    self, WpLinuxDrmSyncobjSurfaceV1,
};
use wayland_protocols::wp::linux_drm_syncobj::v1::server::wp_linux_drm_syncobj_timeline_v1::{
    self, WpLinuxDrmSyncobjTimelineV1,
};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_buffer::WlBuffer;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::ServerState;
use super::shm::{self, BufferKind};
use crate::ffi::dmabuf as files;

/// A timeline as `Timelines` knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimelineId(pub u64);

/// DRM synchronisation timelines, behind a trait so that tests need no device (research G11).
pub trait Timelines {
    /// Takes in a timeline an application handed over. An error is the application's fault.
    fn import(&self, fd: OwnedFd) -> io::Result<TimelineId>;
    /// A file that becomes readable once `point` is reached, also if it already was.
    fn wait_fd(&self, id: TimelineId, point: u64) -> io::Result<OwnedFd>;
    /// Reaches `point`, for the application waiting on it.
    fn signal(&self, id: TimelineId, point: u64);
    /// Nothing uses this timeline any more.
    fn forget(&self, id: TimelineId);
}

/// An imported timeline: it stays until the last point on it is gone, also after the application
/// destroyed its object (contracts/protocols.md).
pub(crate) struct Timeline {
    id: TimelineId,
    owner: Rc<dyn Timelines>,
}

impl fmt::Debug for Timeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timeline({})", self.id.0)
    }
}

impl Drop for Timeline {
    fn drop(&mut self) {
        self.owner.forget(self.id);
    }
}

/// A point on a timeline.
#[derive(Debug, Clone)]
pub(crate) struct SyncPoint {
    timeline: Rc<Timeline>,
    point: u64,
}

impl SyncPoint {
    /// A file to wait on, if the point is not reached yet. One that cannot be had counts as
    /// reached: the image is shown as it is rather than the window stopping for good.
    pub(crate) fn unfinished(&self) -> std::io::Result<Option<OwnedFd>> {
        let timeline = &self.timeline;
        let fd = timeline.owner.wait_fd(timeline.id, self.point)?;
        Ok((!files::is_readable(fd.as_fd())).then_some(fd))
    }

    pub(crate) fn signal(&self) {
        self.timeline.owner.signal(self.timeline.id, self.point);
    }
}

/// How a replaced image goes back to its application (research G9).
pub(crate) enum ToRelease {
    Buffer(WlBuffer),
    /// Not signalled if the application has gone in the meantime.
    Point(SyncPoint, ClientId),
}

impl ToRelease {
    pub(crate) fn is_buffer(&self, buffer: &WlBuffer) -> bool {
        matches!(self, ToRelease::Buffer(b) if b.id() == buffer.id())
    }
}

/// Sends `entry` back to its application.
pub(crate) fn release(state: &ServerState, entry: ToRelease) {
    match entry {
        ToRelease::Buffer(buffer) => shm::release(&buffer),
        ToRelease::Point(point, client) => {
            if state
                .dh
                .backend_handle()
                .get_client_credentials(client)
                .is_ok()
            {
                point.signal();
            }
        }
    }
}

pub(crate) fn set_timelines(state: &mut ServerState, timelines: Rc<dyn Timelines>) {
    state.timelines = Some(timelines);
    state
        .dh
        .create_global::<ServerState, WpLinuxDrmSyncobjManagerV1, ()>(1, ());
}

/// Checks a commit on surface `id` against the protocol's rules; false if the client was
/// disconnected for breaking them.
pub(crate) fn check_commit(state: &ServerState, id: &ObjectId) -> bool {
    use wp_linux_drm_syncobj_surface_v1::Error;
    let Some(surface) = state.surfaces.get(id) else {
        return true;
    };
    let Some(object) = &surface.syncobj else {
        return true;
    };
    let pending = &surface.pending;
    let buffer = match &pending.attach {
        Some(Some(buffer)) => Some(buffer),
        _ => None,
    };
    let error = match (buffer, &pending.acquire, &pending.release) {
        (None, None, None) => return true,
        (None, _, _) => (Error::NoBuffer, "points with no buffer"),
        (Some(_), None, _) => (Error::NoAcquirePoint, "a buffer with no acquire point"),
        (Some(_), _, None) => (Error::NoReleasePoint, "a buffer with no release point"),
        (Some(buffer), Some(acquire), Some(release)) => {
            if Rc::ptr_eq(&acquire.timeline, &release.timeline) && acquire.point >= release.point {
                (
                    Error::ConflictingPoints,
                    "the acquire point is not below the release point",
                )
            } else if matches!(state.buffers.get(&buffer.id()), Some(BufferKind::Shm(_))) {
                (
                    Error::UnsupportedBuffer,
                    "shared memory cannot use explicit synchronisation",
                )
            } else {
                return true;
            }
        }
    };
    object.post_error(error.0, error.1);
    false
}

impl GlobalDispatch<WpLinuxDrmSyncobjManagerV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpLinuxDrmSyncobjManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpLinuxDrmSyncobjManagerV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WpLinuxDrmSyncobjManagerV1,
        request: wp_linux_drm_syncobj_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use wp_linux_drm_syncobj_manager_v1::{Error, Request};
        match request {
            Request::GetSurface { id, surface } => {
                let key = surface.id();
                let object = data_init.init(id, key.clone());
                let Some(surface) = state.surfaces.get_mut(&key) else {
                    return;
                };
                if surface.syncobj.is_some() {
                    resource.post_error(
                        Error::SurfaceExists,
                        "the surface already has a synchronisation object",
                    );
                } else {
                    surface.syncobj = Some(object);
                }
            }
            Request::ImportTimeline { id, fd } => {
                let object = data_init.init(id, ());
                let Some(owner) = state.timelines.clone() else {
                    return;
                };
                match owner.import(fd) {
                    Ok(id) => {
                        let timeline = Rc::new(Timeline { id, owner });
                        state.timeline_objects.insert(object.id(), timeline);
                    }
                    Err(err) => resource.post_error(
                        Error::InvalidTimeline,
                        format!("the timeline cannot be imported: {err}"),
                    ),
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpLinuxDrmSyncobjTimelineV1, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WpLinuxDrmSyncobjTimelineV1,
        _: wp_linux_drm_syncobj_timeline_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WpLinuxDrmSyncobjTimelineV1, _: &()) {
        state.timeline_objects.remove(&resource.id());
    }
}

impl Dispatch<WpLinuxDrmSyncobjSurfaceV1, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WpLinuxDrmSyncobjSurfaceV1,
        request: wp_linux_drm_syncobj_surface_v1::Request,
        surface: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        use wp_linux_drm_syncobj_surface_v1::Request;
        let (timeline, point_hi, point_lo, is_acquire) = match request {
            Request::SetAcquirePoint {
                timeline,
                point_hi,
                point_lo,
            } => (timeline, point_hi, point_lo, true),
            Request::SetReleasePoint {
                timeline,
                point_hi,
                point_lo,
            } => (timeline, point_hi, point_lo, false),
            _ => return,
        };
        let Some(timeline) = state.timeline_objects.get(&timeline.id()).cloned() else {
            return;
        };
        let Some(surface) = state.surfaces.get_mut(surface) else {
            resource.post_error(
                wp_linux_drm_syncobj_surface_v1::Error::NoSurface,
                "the surface was destroyed",
            );
            return;
        };
        let point = SyncPoint {
            timeline,
            point: (u64::from(point_hi) << 32) | u64::from(point_lo),
        };
        if is_acquire {
            surface.pending.acquire = Some(point);
        } else {
            surface.pending.release = Some(point);
        }
    }

    fn destroyed(
        state: &mut Self,
        _: ClientId,
        _: &WpLinuxDrmSyncobjSurfaceV1,
        surface: &ObjectId,
    ) {
        if let Some(surface) = state.surfaces.get_mut(surface) {
            surface.syncobj = None;
            surface.pending.acquire = None;
            surface.pending.release = None;
        }
    }
}
