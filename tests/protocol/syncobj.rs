//! `wp_linux_drm_syncobj_v1` per `contracts/protocols.md`, with a `Timelines` double in place of
//! real timelines (research G11): no test here needs a device.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::{self, PipeWriter, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;

use rubickway::canvas::Size;
use rubickway::server::{TimelineId, Timelines, WaitId};
use rubickway::shell::{WindowEvent, WindowId};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;
use wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_manager_v1::{
    self, WpLinuxDrmSyncobjManagerV1,
};
use wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_surface_v1::{
    self, WpLinuxDrmSyncobjSurfaceV1,
};
use wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1;

use crate::dmabuf::{ARGB, dmabuf, gpu_harness, immed, imports_succeed};
use crate::harness::{Harness, solid_buffer, temp_file};
use crate::surfaces::{Window, ack, create_window, globals};

const RED: u32 = 0xffff_0000;

/// Timelines in memory. An empty file cannot be imported. Waits are pipes, written when the test
/// says a point was reached.
#[derive(Default)]
pub struct FakeTimelines {
    next: Cell<u64>,
    reached: RefCell<HashSet<(u64, u64)>>,
    writers: RefCell<HashMap<(u64, u64), Vec<PipeWriter>>>,
    pub signalled: RefCell<Vec<(u64, u64)>>,
    pub forgotten: RefCell<Vec<u64>>,
    /// The error getting a file to wait on gives, if set, as an errno.
    pub wait_error: Cell<Option<i32>>,
}

impl FakeTimelines {
    /// The drawing reached `point` on the timeline imported `id`-th, counting from 1.
    pub fn reach(&self, id: u64, point: u64) {
        self.reached.borrow_mut().insert((id, point));
        for mut writer in self
            .writers
            .borrow_mut()
            .remove(&(id, point))
            .unwrap_or_default()
        {
            // The test drives waits by hand and may have dropped the read end already.
            let _ = writer.write_all(b"x");
        }
    }
}

impl Timelines for FakeTimelines {
    fn import(&self, fd: OwnedFd) -> io::Result<TimelineId> {
        if std::fs::File::from(fd).metadata()?.len() == 0 {
            return Err(io::Error::other("not a timeline"));
        }
        self.next.set(self.next.get() + 1);
        Ok(TimelineId(self.next.get()))
    }

    fn wait_fd(&self, id: TimelineId, point: u64) -> io::Result<OwnedFd> {
        if let Some(errno) = self.wait_error.get() {
            return Err(io::Error::from_raw_os_error(errno));
        }
        let (read, mut write) = io::pipe()?;
        if self.reached.borrow().contains(&(id.0, point)) {
            write.write_all(b"x")?;
        }
        self.writers
            .borrow_mut()
            .entry((id.0, point))
            .or_default()
            .push(write);
        Ok(read.into())
    }

    fn signal(&self, id: TimelineId, point: u64) {
        self.signalled.borrow_mut().push((id.0, point));
    }

    fn forget(&self, id: TimelineId) {
        self.forgotten.borrow_mut().push(id.0);
    }
}

pub fn sync_harness() -> (Harness, Rc<FakeTimelines>) {
    let mut h = gpu_harness();
    let fake = Rc::new(FakeTimelines::default());
    h.server.set_timelines(fake.clone());
    (h, fake)
}

pub struct Sync {
    pub manager: WpLinuxDrmSyncobjManagerV1,
    pub dmabuf: ZwpLinuxDmabufV1,
    pub window: Window,
    pub surface: WpLinuxDrmSyncobjSurfaceV1,
    /// Two timelines: imported first and second, so with the ids 1 and 2.
    pub a: WpLinuxDrmSyncobjTimelineV1,
    pub b: WpLinuxDrmSyncobjTimelineV1,
}

/// A client with a window using explicit synchronisation, not showing anything yet.
pub fn sync_client(h: &mut Harness, ci: usize) -> Sync {
    let g = globals(h.c(ci));
    let dmabuf = dmabuf(h, ci, 5);
    let manager: WpLinuxDrmSyncobjManagerV1 = h.c(ci).bind(1);
    let window = create_window(h, ci, &g);
    ack(h, ci, &window);
    let qh = h.c(ci).qh.clone();
    let surface = manager.get_surface(&window.surface, &qh, ());
    let a = import(h, ci, &manager);
    let b = import(h, ci, &manager);
    h.roundtrip();
    Sync {
        manager,
        dmabuf,
        window,
        surface,
        a,
        b,
    }
}

fn import(
    h: &mut Harness,
    ci: usize,
    manager: &WpLinuxDrmSyncobjManagerV1,
) -> WpLinuxDrmSyncobjTimelineV1 {
    let qh = h.c(ci).qh.clone();
    let file = temp_file(1);
    manager.import_timeline(file.as_fd(), &qh, ())
}

fn hi(point: u64) -> u32 {
    (point >> 32) as u32
}

fn lo(point: u64) -> u32 {
    point as u32
}

/// Commits `buffer` with an acquire point on `a` and a release point on `b`, then does the import
/// step, as one pass of the loop would.
pub fn commit_with_points(
    h: &mut Harness,
    s: &Sync,
    buffer: &WlBuffer,
    acquire: u64,
    release: u64,
) {
    s.surface.set_acquire_point(&s.a, hi(acquire), lo(acquire));
    s.surface.set_release_point(&s.b, hi(release), lo(release));
    s.window.surface.attach(Some(buffer), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    imports_succeed(h);
}

fn mapped(h: &mut Harness) -> Option<WindowId> {
    h.server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { id, .. } => Some(id),
            _ => None,
        })
}

fn one_wait(h: &mut Harness) -> WaitId {
    let waits = h.server.take_waits();
    assert_eq!(waits.len(), 1, "one open wait");
    waits[0].id
}

fn surface_error(h: &mut Harness, ci: usize, error: wp_linux_drm_syncobj_surface_v1::Error) {
    let c = h.c(ci);
    assert_eq!(c.error().map(|e| e.code), Some(error as u32));
}

fn manager_error(h: &mut Harness, ci: usize, error: wp_linux_drm_syncobj_manager_v1::Error) {
    let c = h.c(ci);
    assert_eq!(c.error().map(|e| e.code), Some(error as u32));
}

/// A client's window with points on `a` and `b` set, for the error tests.
fn client(h: &mut Harness) -> (usize, Sync) {
    let c = h.connect();
    let s = sync_client(h, c);
    (c, s)
}

#[test]
fn the_global_appears_only_when_the_gpu_has_timelines() {
    let mut h = gpu_harness();
    let c = h.connect();
    assert!(
        h.c(c).bind_all::<WpLinuxDrmSyncobjManagerV1>(1).is_empty(),
        "without timelines, no global"
    );

    let (mut h, _fake) = sync_harness();
    let c = h.connect();
    let version = h
        .c(c)
        .state
        .globals
        .iter()
        .find(|(_, i, _)| i == "wp_linux_drm_syncobj_manager_v1")
        .map(|(_, _, v)| *v);
    assert_eq!(version, Some(1));
}

// --- The table of errors -----------------------------------------------------------------------

#[test]
fn a_second_object_for_a_surface_is_surface_exists() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let qh = h.c(c).qh.clone();
    let _again = s.manager.get_surface(&s.window.surface, &qh, ());
    h.roundtrip();
    manager_error(
        &mut h,
        c,
        wp_linux_drm_syncobj_manager_v1::Error::SurfaceExists,
    );
}

#[test]
fn after_destroying_the_object_a_surface_can_have_another() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let qh = h.c(c).qh.clone();
    s.surface.destroy();
    let _again = s.manager.get_surface(&s.window.surface, &qh, ());
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn a_file_that_cannot_be_imported_is_invalid_timeline() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let qh = h.c(c).qh.clone();
    let file = temp_file(0);
    let _t = s.manager.import_timeline(file.as_fd(), &qh, ());
    h.roundtrip();
    manager_error(
        &mut h,
        c,
        wp_linux_drm_syncobj_manager_v1::Error::InvalidTimeline,
    );
}

#[test]
fn points_after_the_surface_was_destroyed_are_no_surface() {
    for release in [false, true] {
        let (mut h, _fake) = sync_harness();
        let (c, s) = client(&mut h);
        s.window.toplevel.destroy();
        s.window.xdg.destroy();
        s.window.surface.destroy();
        if release {
            s.surface.set_release_point(&s.b, 0, 1);
        } else {
            s.surface.set_acquire_point(&s.a, 0, 1);
        }
        h.roundtrip();
        surface_error(&mut h, c, wp_linux_drm_syncobj_surface_v1::Error::NoSurface);
    }
}

#[test]
fn points_with_no_buffer_are_no_buffer() {
    for release in [false, true] {
        let (mut h, _fake) = sync_harness();
        let (c, s) = client(&mut h);
        if release {
            s.surface.set_release_point(&s.b, 0, 1);
        } else {
            s.surface.set_acquire_point(&s.a, 0, 1);
        }
        s.window.surface.commit();
        h.roundtrip();
        surface_error(&mut h, c, wp_linux_drm_syncobj_surface_v1::Error::NoBuffer);
    }
}

#[test]
fn points_with_a_null_buffer_are_no_buffer() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    s.surface.set_acquire_point(&s.a, 0, 1);
    s.surface.set_release_point(&s.b, 0, 1);
    s.window.surface.attach(None, 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    surface_error(&mut h, c, wp_linux_drm_syncobj_surface_v1::Error::NoBuffer);
}

#[test]
fn a_commit_with_neither_buffer_nor_points_is_fine() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    s.window.surface.commit();
    s.window.surface.attach(None, 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn a_buffer_with_no_acquire_point_is_no_acquire_point() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 4, 4);
    s.surface.set_release_point(&s.b, 0, 1);
    s.window.surface.attach(Some(&b), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    surface_error(
        &mut h,
        c,
        wp_linux_drm_syncobj_surface_v1::Error::NoAcquirePoint,
    );
}

#[test]
fn a_buffer_with_no_release_point_is_no_release_point() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 4, 4);
    s.surface.set_acquire_point(&s.a, 0, 1);
    s.window.surface.attach(Some(&b), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    surface_error(
        &mut h,
        c,
        wp_linux_drm_syncobj_surface_v1::Error::NoReleasePoint,
    );
}

#[test]
fn a_shared_memory_buffer_with_points_is_unsupported_buffer() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let g = globals(h.c(c));
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 4, 4, RED);
    s.surface.set_acquire_point(&s.a, 0, 1);
    s.surface.set_release_point(&s.b, 0, 1);
    s.window.surface.attach(Some(&buffer), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    surface_error(
        &mut h,
        c,
        wp_linux_drm_syncobj_surface_v1::Error::UnsupportedBuffer,
    );
}

#[test]
fn acquire_not_below_release_on_one_timeline_is_conflicting_points() {
    for (acquire, release) in [(5, 5), (6, 5)] {
        let (mut h, _fake) = sync_harness();
        let (c, s) = client(&mut h);
        let b = immed(&mut h, c, &s.dmabuf, 4, 4);
        s.surface.set_acquire_point(&s.a, 0, acquire);
        s.surface.set_release_point(&s.a, 0, release);
        s.window.surface.attach(Some(&b), 0, 0);
        s.window.surface.commit();
        h.roundtrip();
        surface_error(
            &mut h,
            c,
            wp_linux_drm_syncobj_surface_v1::Error::ConflictingPoints,
        );
    }
}

#[test]
fn acquire_below_release_on_one_timeline_is_fine() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 4, 4);
    s.surface.set_acquire_point(&s.a, 0, 4);
    s.surface.set_release_point(&s.a, 0, 5);
    s.window.surface.attach(Some(&b), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn any_points_on_two_timelines_are_fine() {
    let (mut h, _fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 4, 4);
    commit_with_points(&mut h, &s, &b, 9, 1);
    assert!(!h.c(c).is_disconnected());
}

// --- Behaviour ---------------------------------------------------------------------------------

#[test]
fn the_commit_takes_effect_after_the_acquire_point() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    commit_with_points(&mut h, &s, &b, 7, 1);
    let wait = one_wait(&mut h);
    assert_eq!(mapped(&mut h), None, "not reached yet");
    fake.reach(1, 7);
    h.server.wait_done(wait);
    h.roundtrip();
    assert!(mapped(&mut h).is_some());
}

#[test]
fn a_point_already_reached_applies_at_once() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 7);
    commit_with_points(&mut h, &s, &b, 7, 1);
    assert!(h.server.take_waits().is_empty());
    assert!(mapped(&mut h).is_some());
}

/// `EMFILE` from `eventfd`, as a real timeline would give it (FR-015). The window goes on, showing
/// its image as it is, and the shortage ends once a descriptor can be opened: in this test at once,
/// as only the double ran out.
#[test]
fn running_out_of_descriptors_is_noticed_and_the_window_goes_on() {
    const EMFILE: i32 = 24;
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.wait_error.set(Some(EMFILE));
    commit_with_points(&mut h, &s, &b, 7, 1);
    assert!(h.server.out_of_descriptors());
    assert!(h.server.take_waits().is_empty(), "nothing to wait on");
    assert!(mapped(&mut h).is_some(), "shown as it is");
    assert!(!h.c(c).is_disconnected());

    fake.wait_error.set(None);
    h.server.check_descriptors();
    assert!(!h.server.out_of_descriptors());
}

#[test]
fn another_failure_to_wait_is_not_running_out_of_descriptors() {
    const EIO: i32 = 5;
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.wait_error.set(Some(EIO));
    commit_with_points(&mut h, &s, &b, 7, 1);
    assert!(!h.server.out_of_descriptors());
    assert!(mapped(&mut h).is_some(), "shown as it is");
}

#[test]
fn a_point_is_all_64_bits() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 0x1_0000_0002);
    commit_with_points(&mut h, &s, &b, 0x1_0000_0002, 0x3_0000_0004);
    assert!(mapped(&mut h).is_some(), "acquire read in full");
    let next = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 8);
    commit_with_points(&mut h, &s, &next, 8, 9);
    h.server.release_images(None);
    assert_eq!(*fake.signalled.borrow(), [(2, 0x3_0000_0004)]);
}

#[test]
fn implicit_readiness_is_ignored_with_points() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    // A file that is never readable: without points this would wait for ever.
    let (read, _write) = io::pipe().expect("pipe");
    let qh = h.c(c).qh.clone();
    let p = s.dmabuf.create_params(&qh, ());
    p.add(read.as_fd(), 0, 0, 80, 0, 0);
    let b = p.create_immed(20, 10, ARGB, Flags::empty(), &qh, ());
    fake.reach(1, 7);
    commit_with_points(&mut h, &s, &b, 7, 1);
    assert!(h.server.take_waits().is_empty());
    assert!(mapped(&mut h).is_some());
}

#[test]
fn the_release_point_is_signalled_once_no_frame_in_flight_can_read_the_buffer() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let first = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &first, 1, 11);

    let shown = h.server.frame_drawn();
    let second = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 2);
    commit_with_points(&mut h, &s, &second, 2, 12);
    h.server.release_images(Some(shown));
    assert!(
        fake.signalled.borrow().is_empty(),
        "that frame may still read it"
    );

    h.server.release_images(None);
    assert_eq!(*fake.signalled.borrow(), [(2, 11)], "the second is shown");
    h.roundtrip();
    assert!(
        !h.c(c)
            .events::<wl_buffer::Event>()
            .iter()
            .any(|(id, _)| *id == first.id()),
        "no wl_buffer.release on such a surface"
    );
}

#[test]
fn the_same_image_attached_again_signals_each_commits_point() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let image = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &image, 1, 11);
    fake.reach(1, 2);
    commit_with_points(&mut h, &s, &image, 2, 12);
    h.server.release_images(None);
    assert_eq!(*fake.signalled.borrow(), [(2, 11)]);
}

#[test]
fn a_point_set_before_the_timeline_was_destroyed_stays_valid() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let first = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &first, 1, 11);
    s.b.destroy();
    h.roundtrip();
    assert!(
        !fake.forgotten.borrow().contains(&2),
        "a point still uses it"
    );

    let t = import(&mut h, c, &s.manager);
    let second = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 2);
    s.surface.set_acquire_point(&s.a, 0, 2);
    s.surface.set_release_point(&t, 0, 12);
    s.window.surface.attach(Some(&second), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    h.server.release_images(None);
    assert_eq!(*fake.signalled.borrow(), [(2, 11)]);
    assert!(fake.forgotten.borrow().contains(&2), "and now nothing does");
}

#[test]
fn an_acquire_point_never_reached_keeps_the_last_picture() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let first = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &first, 1, 11);
    let id = mapped(&mut h).expect("mapped");

    let never = immed(&mut h, c, &s.dmabuf, 40, 30);
    commit_with_points(&mut h, &s, &never, 2, 12);
    let _wait = one_wait(&mut h);
    assert_eq!(
        h.server.window_parts(id)[0].rect.size,
        Size::new(20.0, 10.0)
    );
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn a_failed_image_goes_back_through_its_release_point() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    s.surface.set_acquire_point(&s.a, 0, 1);
    s.surface.set_release_point(&s.b, 0, 11);
    s.window.surface.attach(Some(&b), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    h.server.process_imports(|_| Err("refused".to_string()));
    h.roundtrip();
    assert_eq!(*fake.signalled.borrow(), [(2, 11)]);
    assert_eq!(mapped(&mut h), None);
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn on_disconnect_everything_is_dropped_and_release_points_are_not_signalled() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let first = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &first, 1, 11);
    let second = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 2);
    commit_with_points(&mut h, &s, &second, 2, 12);
    let waiting = immed(&mut h, c, &s.dmabuf, 20, 10);
    commit_with_points(&mut h, &s, &waiting, 3, 13);
    let wait = one_wait(&mut h);

    h.clients.remove(c);
    h.server.dispatch().expect("dispatch");
    assert_eq!(h.server.take_cancelled_waits(), [wait]);
    h.server.release_images(None);
    assert!(fake.signalled.borrow().is_empty());
    let mut forgotten = fake.forgotten.borrow().clone();
    forgotten.sort_unstable();
    assert_eq!(forgotten, [1, 2], "no timeline stays imported");
}

#[test]
fn destroying_the_object_drops_points_set_since_the_last_commit() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let b = immed(&mut h, c, &s.dmabuf, 20, 10);
    s.surface.set_acquire_point(&s.a, 0, 7);
    s.surface.destroy();
    h.roundtrip();
    // No points any more and no object: an ordinary commit, finished as far as files go.
    s.window.surface.attach(Some(&b), 0, 0);
    s.window.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    assert!(h.server.take_waits().is_empty());
    assert!(mapped(&mut h).is_some());
    assert!(!h.c(c).is_disconnected());
    assert!(fake.signalled.borrow().is_empty());
}

#[test]
fn a_synchronised_subsurface_keeps_its_points_until_its_parent_commits() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let g = globals(h.c(c));
    let parent = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &parent, 1, 11);
    let id = mapped(&mut h).expect("mapped");

    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let _sub = g
        .subcompositor
        .get_subsurface(&child, &s.window.surface, &qh, ());
    let child_sync = s.manager.get_surface(&child, &qh, ());
    let image = immed(&mut h, c, &s.dmabuf, 5, 5);
    child_sync.set_acquire_point(&s.a, 0, 2);
    child_sync.set_release_point(&s.b, 0, 12);
    child.attach(Some(&image), 0, 0);
    child.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    assert!(h.server.take_waits().is_empty(), "cached");

    s.window.surface.commit();
    h.roundtrip();
    let wait = one_wait(&mut h);
    assert_eq!(h.server.window_parts(id).len(), 1);
    fake.reach(1, 2);
    h.server.wait_done(wait);
    h.roundtrip();
    assert_eq!(h.server.window_parts(id).len(), 2);
}

#[test]
fn a_cached_image_replaced_before_it_applied_goes_back_through_its_point() {
    let (mut h, fake) = sync_harness();
    let (c, s) = client(&mut h);
    let g = globals(h.c(c));
    let parent = immed(&mut h, c, &s.dmabuf, 20, 10);
    fake.reach(1, 1);
    commit_with_points(&mut h, &s, &parent, 1, 11);

    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let _sub = g
        .subcompositor
        .get_subsurface(&child, &s.window.surface, &qh, ());
    let child_sync = s.manager.get_surface(&child, &qh, ());
    for (acquire, release) in [(2, 12), (3, 13)] {
        let image = immed(&mut h, c, &s.dmabuf, 5, 5);
        child_sync.set_acquire_point(&s.a, 0, acquire);
        child_sync.set_release_point(&s.b, 0, release);
        child.attach(Some(&image), 0, 0);
        child.commit();
    }
    h.roundtrip();
    assert_eq!(*fake.signalled.borrow(), [(2, 12)], "never to be read");
}
