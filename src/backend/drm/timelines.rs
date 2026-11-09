//! `Timelines` on the display GPU's render node (research G6). The render node needs no DRM master
//! and is opened for the display GPU only, so the second GPU stays closed (FR-012).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use drm::control::{Device as ControlDevice, syncobj};
use drm::{Device, DriverCapability};

use super::device::Card;
use crate::ffi::dmabuf::eventfd;
use crate::server::{TimelineId, Timelines};

pub(crate) struct RenderNodeTimelines {
    node: Card,
    handles: RefCell<HashMap<u64, syncobj::Handle>>,
    next: Cell<u64>,
}

impl RenderNodeTimelines {
    /// Opens `path`; `None` if its driver has no timelines.
    pub(crate) fn open(path: &Path) -> io::Result<Option<RenderNodeTimelines>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let node = Card(OwnedFd::from(file));
        if node.get_driver_capability(DriverCapability::TimelineSyncObj)? == 0 {
            return Ok(None);
        }
        Ok(Some(RenderNodeTimelines {
            node,
            handles: RefCell::new(HashMap::new()),
            next: Cell::new(0),
        }))
    }

    fn handle(&self, id: TimelineId) -> io::Result<syncobj::Handle> {
        self.handles
            .borrow()
            .get(&id.0)
            .copied()
            .ok_or_else(|| io::Error::other("unknown timeline"))
    }
}

impl Timelines for RenderNodeTimelines {
    fn import(&self, fd: OwnedFd) -> io::Result<TimelineId> {
        let handle = self.node.fd_to_syncobj(fd.as_fd(), false)?;
        self.next.set(self.next.get() + 1);
        let id = self.next.get();
        self.handles.borrow_mut().insert(id, handle);
        Ok(TimelineId(id))
    }

    fn wait_fd(&self, id: TimelineId, point: u64) -> io::Result<OwnedFd> {
        let fd = eventfd()?;
        // `wait_available` false: signalled, not merely with a fence attached (research G6).
        self.node
            .syncobj_eventfd(self.handle(id)?, point, fd.as_fd(), false)?;
        Ok(fd)
    }

    fn signal(&self, id: TimelineId, point: u64) {
        let signalled = self
            .handle(id)
            .and_then(|handle| self.node.syncobj_timeline_signal(&[handle], &[point]));
        if let Err(err) = signalled {
            tracing::warn!(
                "cannot tell an application its buffer is free ({err}): it may stop drawing"
            );
        }
    }

    fn forget(&self, id: TimelineId) {
        if let Some(handle) = self.handles.borrow_mut().remove(&id.0)
            && let Err(err) = self.node.destroy_syncobj(handle)
        {
            tracing::debug!("destroying a timeline: {err}");
        }
    }
}
