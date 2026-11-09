//! Commit order per `contracts/protocols.md`: an update takes effect when its images are finished,
//! in order per surface, and holds up nothing else (research G7). Images that are not finished are
//! made from pipes, and waits are driven by hand as the event loop would (research G11).

use std::io::{PipeReader, PipeWriter};
use std::os::fd::AsFd;

use rubickway::canvas::Size;
use rubickway::server::{Wait, WaitId};
use rubickway::shell::{WindowEvent, WindowId};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;

use crate::dmabuf::{ARGB, dmabuf, gpu_harness, immed, imports_succeed};
use crate::harness::Harness;
use crate::surfaces::{Globals, Window, ack, create_window, globals};

/// An image still being drawn. Its pipe stays open as long as this lives.
pub struct Unfinished {
    pub buffer: WlBuffer,
    _reader: PipeReader,
    _writer: PipeWriter,
}

pub fn unfinished(
    h: &mut Harness,
    ci: usize,
    d: &ZwpLinuxDmabufV1,
    width: i32,
    height: i32,
) -> Unfinished {
    let (reader, writer) = std::io::pipe().expect("pipe");
    let qh = h.c(ci).qh.clone();
    let p = d.create_params(&qh, ());
    p.add(reader.as_fd(), 0, 0, (width * 4) as u32, 0, 0);
    let buffer = p.create_immed(width, height, ARGB, Flags::empty(), &qh, ());
    Unfinished {
        buffer,
        _reader: reader,
        _writer: writer,
    }
}

/// Attaches and commits, then does the import step, as one pass of the loop would.
fn commit(h: &mut Harness, surface: &WlSurface, buffer: &WlBuffer) {
    surface.attach(Some(buffer), 0, 0);
    surface.commit();
    h.roundtrip();
    imports_succeed(h);
}

/// Opens a window showing a finished `width x height` image.
fn shown_window(
    h: &mut Harness,
    ci: usize,
    g: &Globals,
    d: &ZwpLinuxDmabufV1,
    width: i32,
    height: i32,
) -> (Window, WlBuffer, WindowId) {
    let w = create_window(h, ci, g);
    ack(h, ci, &w);
    let image = immed(h, ci, d, width, height);
    commit(h, &w.surface, &image);
    let id = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { id, .. } => Some(id),
            _ => None,
        })
        .expect("mapped");
    (w, image, id)
}

fn size_of(h: &Harness, id: WindowId) -> Size {
    h.server.window_parts(id)[0].rect.size
}

fn one_wait(h: &mut Harness) -> WaitId {
    let waits: Vec<Wait> = h.server.take_waits();
    assert_eq!(waits.len(), 1, "one open wait");
    waits[0].id
}

fn released(h: &Harness, ci: usize, buffer: &WlBuffer) -> bool {
    h.clients[ci]
        .events::<wl_buffer::Event>()
        .iter()
        .any(|(id, e)| *id == buffer.id() && matches!(e, wl_buffer::Event::Release))
}

fn called_back(h: &Harness, ci: usize, callback: &WlCallback) -> bool {
    h.clients[ci]
        .events::<wl_callback::Event>()
        .iter()
        .any(|(id, _)| *id == callback.id())
}

#[test]
fn a_finished_image_applies_at_once_and_opens_no_wait() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (_w, _image, id) = shown_window(&mut h, c, &g, &d, 20, 10);
    assert_eq!(size_of(&h, id), Size::new(20.0, 10.0));
    assert!(h.server.take_waits().is_empty());
}

#[test]
fn the_window_keeps_its_last_picture_while_a_wait_is_open() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let next = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &next.buffer);
    let wait = one_wait(&mut h);
    assert_eq!(size_of(&h, id), Size::new(20.0, 10.0), "not finished yet");

    h.server.wait_done(wait);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(40.0, 30.0));
}

#[test]
fn a_new_window_does_not_open_until_its_image_is_finished() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let w = create_window(&mut h, c, &g);
    ack(&mut h, c, &w);
    let image = unfinished(&mut h, c, &d, 20, 10);
    commit(&mut h, &w.surface, &image.buffer);
    let wait = one_wait(&mut h);
    assert!(h.server.take_window_events().is_empty());
    h.server.wait_done(wait);
    h.roundtrip();
    assert!(matches!(
        h.server.take_window_events()[..],
        [WindowEvent::Mapped { .. }]
    ));
}

#[test]
fn a_later_image_finishing_first_does_not_overtake_an_earlier_one() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let earlier = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &earlier.buffer);
    let earlier_wait = one_wait(&mut h);
    let later = unfinished(&mut h, c, &d, 60, 50);
    commit(&mut h, &w.surface, &later.buffer);
    let later_wait = one_wait(&mut h);

    h.server.wait_done(later_wait);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(20.0, 10.0), "behind the earlier");
    h.server.wait_done(earlier_wait);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(60.0, 50.0), "both, in order");
}

#[test]
fn a_finished_image_behind_an_unfinished_one_waits_its_turn() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let slow = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &slow.buffer);
    let wait = one_wait(&mut h);
    let ready = immed(&mut h, c, &d, 60, 50);
    commit(&mut h, &w.surface, &ready);
    assert_eq!(size_of(&h, id), Size::new(20.0, 10.0));
    h.server.wait_done(wait);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(60.0, 50.0));
}

#[test]
fn synchronised_subsurfaces_go_with_their_parent() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, first, id) = shown_window(&mut h, c, &g, &d, 20, 10);
    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let _sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    h.roundtrip();

    let image = unfinished(&mut h, c, &d, 5, 5);
    commit(&mut h, &child, &image.buffer);
    assert!(
        h.server.take_waits().is_empty(),
        "cached: it never waits by itself"
    );

    // The parent's own image is finished, its child's is not: neither shows yet.
    commit(&mut h, &w.surface, &first);
    let wait = one_wait(&mut h);
    assert_eq!(h.server.window_parts(id).len(), 1);
    h.server.wait_done(wait);
    h.roundtrip();
    assert_eq!(h.server.window_parts(id).len(), 2);
}

#[test]
fn a_desynchronised_subsurface_has_its_own_queue() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);
    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    sub.set_desync();
    h.roundtrip();

    let image = unfinished(&mut h, c, &d, 5, 5);
    commit(&mut h, &child, &image.buffer);
    let wait = one_wait(&mut h);

    // The parent does not wait for it.
    let next = immed(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &next);
    assert_eq!(size_of(&h, id), Size::new(40.0, 30.0));
    assert!(h.server.take_waits().is_empty());
    h.server.wait_done(wait);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(40.0, 30.0));
}

#[test]
fn frame_callbacks_complete_only_after_the_update_took_effect_and_was_shown() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let qh = h.c(c).qh.clone();
    let callback = w.surface.frame(&qh, ());
    let next = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &next.buffer);
    let wait = one_wait(&mut h);

    h.server.presented(1, &[id]);
    h.roundtrip();
    assert!(!called_back(&h, c, &callback), "not in effect yet");

    h.server.wait_done(wait);
    h.roundtrip();
    assert!(!called_back(&h, c, &callback), "in effect, not shown yet");

    h.server.presented(2, &[id]);
    h.roundtrip();
    assert!(called_back(&h, c, &callback));
}

#[test]
fn geometry_comes_from_the_commit_and_not_from_requests_after_it() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 100, 80);

    w.xdg.set_window_geometry(0, 0, 50, 40);
    let next = unfinished(&mut h, c, &d, 100, 80);
    commit(&mut h, &w.surface, &next.buffer);
    let wait = one_wait(&mut h);
    assert!(h.server.take_window_events().is_empty(), "nothing yet");

    // Asked for after the commit: it belongs to the next one.
    w.xdg.set_window_geometry(0, 0, 10, 10);
    h.roundtrip();
    h.server.wait_done(wait);
    h.roundtrip();
    let events = h.server.take_window_events();
    assert!(
        matches!(
            &events[..],
            [WindowEvent::Updated { id: i, size, .. }]
                if *i == id && *size == Size::new(50.0, 40.0)
        ),
        "{events:?}"
    );
}

#[test]
fn a_wait_holds_up_no_other_surface() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (slow, _a, slow_id) = shown_window(&mut h, c, &g, &d, 20, 10);
    let (other, _b, other_id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let image = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &slow.surface, &image.buffer);
    let _wait = one_wait(&mut h);

    let next = immed(&mut h, c, &d, 60, 50);
    commit(&mut h, &other.surface, &next);
    assert_eq!(size_of(&h, other_id), Size::new(60.0, 50.0));
    assert_eq!(size_of(&h, slow_id), Size::new(20.0, 10.0));
}

#[test]
fn the_shown_image_goes_back_only_once_the_next_one_replaced_it() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, first, _id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let next = unfinished(&mut h, c, &d, 20, 10);
    commit(&mut h, &w.surface, &next.buffer);
    let wait = one_wait(&mut h);
    h.server.release_images(None);
    h.roundtrip();
    assert!(!released(&h, c, &first), "it is still what is shown");

    h.server.wait_done(wait);
    h.roundtrip();
    h.server.release_images(None);
    h.roundtrip();
    assert!(released(&h, c, &first));
    assert!(!released(&h, c, &next.buffer));
}

#[test]
fn an_image_with_several_unfinished_planes_waits_for_all() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let (read_y, _write_y) = std::io::pipe().expect("pipe");
    let (read_uv, _write_uv) = std::io::pipe().expect("pipe");
    let qh = h.c(c).qh.clone();
    let p = d.create_params(&qh, ());
    p.add(read_y.as_fd(), 0, 0, 40, 0, 0);
    p.add(read_uv.as_fd(), 1, 0, 40, 0, 0);
    let image = p.create_immed(40, 30, crate::dmabuf::NV12, Flags::empty(), &qh, ());
    commit(&mut h, &w.surface, &image);
    let waits = h.server.take_waits();
    assert_eq!(waits.len(), 2);

    h.server.wait_done(waits[0].id);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(20.0, 10.0));
    h.server.wait_done(waits[1].id);
    h.roundtrip();
    assert_eq!(size_of(&h, id), Size::new(40.0, 30.0));
}

#[test]
fn a_destroyed_surface_cancels_its_waits() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, _first, _id) = shown_window(&mut h, c, &g, &d, 20, 10);

    let next = unfinished(&mut h, c, &d, 40, 30);
    commit(&mut h, &w.surface, &next.buffer);
    let wait = one_wait(&mut h);
    w.toplevel.destroy();
    w.xdg.destroy();
    w.surface.destroy();
    h.roundtrip();
    assert_eq!(h.server.take_cancelled_waits(), [wait]);
    // Its file could still fire before the event loop hears: nothing happens.
    h.server.wait_done(wait);
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
}
