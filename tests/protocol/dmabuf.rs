//! `zwp_linux_dmabuf_v1` per `contracts/protocols.md`, with an `ImageFiles` double in place of
//! real dma-bufs (research G11): no test here needs a device.

use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::FileTypeExt;

use rubickway::render::formats::{self, FormatPair};
use rubickway::server::{GpuImage, ImageFiles};
use rubickway::shell::{WindowEvent, WindowId};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback;
use wayland_client::protocol::wl_surface;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::{
    self, Flags, ZwpLinuxBufferParamsV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_feedback_v1::{
    self, ZwpLinuxDmabufFeedbackV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::{
    self, ZwpLinuxDmabufV1,
};

use crate::harness::{Client, Harness, solid_buffer, temp_file};
use crate::surfaces::{Globals, Window, ack, create_window, globals, open_window};

pub const ARGB: u32 = u32::from_le_bytes(*b"AR24");
pub const NV12: u32 = u32::from_le_bytes(*b"NV12");
pub const LINEAR: u64 = 0;
pub const UNOFFERED: u64 = 0x0100_0000_0000_0001;
const MAIN_DEVICE: u64 = 0xe280;
const PAIRS: [FormatPair; 2] = [(ARGB, LINEAR), (NV12, LINEAR)];
const RED: u32 = 0xffff_0000;

/// Accepts any file as a GPU image, sized as it is. A pipe has no size and is big enough for any
/// layout: it is how a test makes an image that is not finished, as polling it says "not readable"
/// until something is written.
pub struct AnyFile;

impl ImageFiles for AnyFile {
    fn describe(&self, fd: BorrowedFd<'_>) -> Option<u64> {
        let file = File::from(fd.try_clone_to_owned().ok()?);
        let metadata = file.metadata().ok()?;
        Some(if metadata.file_type().is_fifo() {
            u64::MAX
        } else {
            metadata.len()
        })
    }
}

/// A server offering GPU images, whose file checks accept any file.
pub fn gpu_harness() -> Harness {
    let mut h = Harness::new();
    h.server.set_image_files(Box::new(AnyFile));
    h.server
        .set_gpu_offer(MAIN_DEVICE, PAIRS.to_vec())
        .expect("offer");
    h
}

/// Binds the global at `version`.
pub fn dmabuf(h: &mut Harness, ci: usize, version: u32) -> ZwpLinuxDmabufV1 {
    let d = h.c(ci).bind(version);
    h.roundtrip();
    d
}

/// New parameters with one plane of a file big enough for `width x height`.
pub fn params(
    h: &mut Harness,
    ci: usize,
    d: &ZwpLinuxDmabufV1,
    width: i32,
    height: i32,
) -> (ZwpLinuxBufferParamsV1, File) {
    let c = h.c(ci);
    let p = d.create_params(&c.qh, ());
    let file = temp_file((width * height * 4) as usize);
    p.add(file.as_fd(), 0, 0, (width * 4) as u32, 0, 0);
    (p, file)
}

/// A `wl_buffer` from `create_immed`.
pub fn immed(
    h: &mut Harness,
    ci: usize,
    d: &ZwpLinuxDmabufV1,
    width: i32,
    height: i32,
) -> WlBuffer {
    let (p, _file) = params(h, ci, d, width, height);
    let qh = h.c(ci).qh.clone();
    p.create_immed(width, height, ARGB, Flags::empty(), &qh, ())
}

/// The import step of a pass of the event loop, with every import succeeding.
pub fn imports_succeed(h: &mut Harness) {
    h.server.process_imports(|_| Ok(()));
    h.roundtrip();
}

fn error_code(h: &mut Harness, ci: usize) -> Option<u32> {
    h.c(ci).error().map(|e| e.code)
}

fn param_error(h: &mut Harness, ci: usize) -> Option<u32> {
    h.c(ci)
        .error()
        .filter(|e| e.object_interface == "zwp_linux_buffer_params_v1")
        .map(|e| e.code)
}

fn failed(c: &Client) -> bool {
    c.events::<zwp_linux_buffer_params_v1::Event>()
        .iter()
        .any(|(_, e)| matches!(e, zwp_linux_buffer_params_v1::Event::Failed))
}

fn released(c: &Client, buffer: &WlBuffer) -> bool {
    c.events::<wl_buffer::Event>()
        .iter()
        .any(|(id, e)| *id == buffer.id() && matches!(e, wl_buffer::Event::Release))
}

fn mapped_id(h: &mut Harness) -> Option<WindowId> {
    h.server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { id, .. } => Some(id),
            _ => None,
        })
}

fn ready_window(h: &mut Harness, ci: usize, g: &Globals) -> Window {
    let w = create_window(h, ci, g);
    ack(h, ci, &w);
    w
}

// --- The global --------------------------------------------------------------------------------

#[test]
fn the_global_appears_only_with_an_offer_at_version_5() {
    let mut h = Harness::new();
    let c = h.connect();
    assert!(
        !h.c(c)
            .state
            .globals
            .iter()
            .any(|(_, i, _)| i == "zwp_linux_dmabuf_v1"),
        "no offer, no global"
    );

    let mut h = gpu_harness();
    let c = h.connect();
    let version = h
        .c(c)
        .state
        .globals
        .iter()
        .find(|(_, i, _)| i == "zwp_linux_dmabuf_v1")
        .map(|(_, _, v)| *v);
    assert_eq!(version, Some(5));
}

#[test]
fn old_clients_get_formats_at_bind_and_new_ones_do_not() {
    let mut h = gpu_harness();
    let c = h.connect();
    dmabuf(&mut h, c, 3);
    let modifiers = h.c(c).events::<zwp_linux_dmabuf_v1::Event>().iter().filter(
        |(_, e)| matches!(e, zwp_linux_dmabuf_v1::Event::Modifier { format, .. } if *format == ARGB),
    ).count();
    assert_eq!(modifiers, 1);

    let c = h.connect();
    dmabuf(&mut h, c, 4);
    assert!(h.c(c).events::<zwp_linux_dmabuf_v1::Event>().is_empty());
}

fn feedback_events(c: &Client, f: &ZwpLinuxDmabufFeedbackV1) -> Vec<&'static str> {
    c.events::<zwp_linux_dmabuf_feedback_v1::Event>()
        .iter()
        .filter(|(id, _)| *id == f.id())
        .map(|(_, e)| match e {
            zwp_linux_dmabuf_feedback_v1::Event::FormatTable { .. } => "format_table",
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { .. } => "main_device",
            zwp_linux_dmabuf_feedback_v1::Event::TrancheTargetDevice { .. } => "target_device",
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFlags { .. } => "flags",
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFormats { .. } => "formats",
            zwp_linux_dmabuf_feedback_v1::Event::TrancheDone => "tranche_done",
            zwp_linux_dmabuf_feedback_v1::Event::Done => "done",
            _ => "other",
        })
        .collect()
}

const FEEDBACK: [&str; 7] = [
    "format_table",
    "main_device",
    "target_device",
    "flags",
    "formats",
    "tranche_done",
    "done",
];

#[test]
fn feedback_tells_the_device_and_table_once_in_order() {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let f = d.get_default_feedback(&qh, ());
    h.roundtrip();
    assert_eq!(feedback_events(h.c(c), &f), FEEDBACK);
    h.roundtrip();
    assert_eq!(feedback_events(h.c(c), &f), FEEDBACK, "sent once");
    let events = h.c(c).events::<zwp_linux_dmabuf_feedback_v1::Event>();
    for (_, e) in events {
        match e {
            zwp_linux_dmabuf_feedback_v1::Event::FormatTable { size, .. } => {
                assert_eq!(*size as usize, formats::table_bytes(&PAIRS).len());
            }
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device }
            | zwp_linux_dmabuf_feedback_v1::Event::TrancheTargetDevice { device } => {
                assert_eq!(device.as_slice(), MAIN_DEVICE.to_ne_bytes());
            }
            zwp_linux_dmabuf_feedback_v1::Event::TrancheFormats { indices } => {
                assert_eq!(
                    indices.len(),
                    PAIRS.len() * 2,
                    "16-bit indices, all entries"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn surface_feedback_is_the_same_and_inert_for_a_destroyed_surface() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let surface = g.compositor.create_surface(&qh, ());
    let f = d.get_surface_feedback(&surface, &qh, ());
    h.roundtrip();
    assert_eq!(feedback_events(h.c(c), &f), FEEDBACK);
    surface.destroy();
    h.roundtrip();
    assert_eq!(feedback_events(h.c(c), &f), FEEDBACK);
    assert!(!h.c(c).is_disconnected());
}

// --- Parameters --------------------------------------------------------------------------------

#[test]
fn create_answers_created_after_the_import() {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let (p, _file) = params(&mut h, c, &d, 4, 4);
    p.create(4, 4, ARGB, Flags::empty());
    h.roundtrip();
    assert!(
        h.c(c)
            .events::<zwp_linux_buffer_params_v1::Event>()
            .is_empty(),
        "nothing before the import"
    );
    imports_succeed(&mut h);
    let created = h
        .c(c)
        .events::<zwp_linux_buffer_params_v1::Event>()
        .iter()
        .any(|(_, e)| matches!(e, zwp_linux_buffer_params_v1::Event::Created { .. }));
    assert!(created);
}

#[test]
fn a_refused_import_answers_failed_and_ends_nobody() {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let (p, _file) = params(&mut h, c, &d, 4, 4);
    p.create(4, 4, ARGB, Flags::empty());
    h.roundtrip();
    h.server.process_imports(|_| Err("refused".to_string()));
    h.roundtrip();
    assert!(failed(h.c(c)));
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn the_import_gets_the_description() {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let p = d.create_params(&qh, ());
    let file = temp_file(64 * 64 * 4);
    p.add(file.as_fd(), 0, 16, 256, 0, 0);
    let _b = p.create_immed(64, 8, ARGB, Flags::YInvert, &qh, ());
    h.roundtrip();
    let mut seen = Vec::new();
    h.server.process_imports(|image: GpuImage<'_>| {
        seen.push((
            image.width,
            image.height,
            image.fourcc,
            image.modifier,
            image.planes.len(),
            image.planes[0].offset,
            image.planes[0].stride,
        ));
        Ok(())
    });
    assert_eq!(seen, vec![(64, 8, ARGB, LINEAR, 1, 16, 256)]);
}

/// `add` then `create_immed` with `change` applied first; returns the params error, if any.
fn with_params(
    change: impl FnOnce(&Client, &ZwpLinuxBufferParamsV1, &File) -> (i32, i32, u32, Flags),
) -> (Option<u32>, bool) {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let p = d.create_params(&qh, ());
    let file = temp_file(64);
    let (w, ht, format, flags) = change(h.c(c), &p, &file);
    let _b = p.create_immed(w, ht, format, flags, &qh, ());
    h.roundtrip();
    let error = param_error(&mut h, c);
    (error, failed(h.c(c)))
}

fn one_plane(p: &ZwpLinuxBufferParamsV1, file: &File) {
    p.add(file.as_fd(), 0, 0, 16, 0, 0);
}

#[test]
fn plane_index_out_of_range_is_plane_idx() {
    let (error, _) = with_params(|_, p, f| {
        p.add(f.as_fd(), 4, 0, 16, 0, 0);
        (4, 4, ARGB, Flags::empty())
    });
    assert_eq!(
        error,
        Some(zwp_linux_buffer_params_v1::Error::PlaneIdx as u32)
    );
}

#[test]
fn the_same_plane_twice_is_plane_set() {
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        one_plane(p, f);
        (4, 4, ARGB, Flags::empty())
    });
    assert_eq!(
        error,
        Some(zwp_linux_buffer_params_v1::Error::PlaneSet as u32)
    );
}

#[test]
fn different_modifiers_across_planes_are_invalid_format() {
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        p.add(f.as_fd(), 1, 0, 16, 1, 0);
        (4, 4, ARGB, Flags::empty())
    });
    assert_eq!(
        error,
        Some(zwp_linux_buffer_params_v1::Error::InvalidFormat as u32)
    );
}

#[test]
fn no_plane_or_a_gap_or_too_few_are_incomplete() {
    let incomplete = Some(zwp_linux_buffer_params_v1::Error::Incomplete as u32);
    let (error, _) = with_params(|_, _, _| (4, 4, ARGB, Flags::empty()));
    assert_eq!(error, incomplete, "no plane");
    let (error, _) = with_params(|_, p, f| {
        p.add(f.as_fd(), 1, 0, 16, 0, 0);
        (4, 4, ARGB, Flags::empty())
    });
    assert_eq!(error, incomplete, "a gap at plane 0");
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        (4, 4, NV12, Flags::empty())
    });
    assert_eq!(error, incomplete, "NV12 needs two");
}

#[test]
fn a_zero_or_negative_size_is_invalid_dimensions() {
    let invalid = Some(zwp_linux_buffer_params_v1::Error::InvalidDimensions as u32);
    for (w, ht) in [(0, 4), (4, 0), (-1, 4), (4, -1)] {
        let (error, _) = with_params(|_, p, f| {
            one_plane(p, f);
            (w, ht, ARGB, Flags::empty())
        });
        assert_eq!(error, invalid, "{w}x{ht}");
    }
}

#[test]
fn a_layout_beyond_the_file_or_overflowing_is_out_of_bounds() {
    let oob = Some(zwp_linux_buffer_params_v1::Error::OutOfBounds as u32);
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        (4, 5, ARGB, Flags::empty())
    });
    assert_eq!(error, oob, "the 64-byte file has 4 rows of 16");
    let (error, _) = with_params(|_, p, f| {
        p.add(f.as_fd(), 0, u32::MAX, u32::MAX, 0, 0);
        (i32::MAX, i32::MAX, ARGB, Flags::empty())
    });
    assert_eq!(error, oob, "arithmetic that would overflow 32 bits");
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        p.add(f.as_fd(), 1, 65, 16, 0, 0);
        (4, 4, NV12, Flags::empty())
    });
    assert_eq!(error, oob, "plane 1 starts beyond its file");
}

#[test]
fn a_pair_never_offered_is_invalid_format() {
    let (error, _) = with_params(|_, p, f| {
        p.add(
            f.as_fd(),
            0,
            0,
            16,
            (UNOFFERED >> 32) as u32,
            UNOFFERED as u32,
        );
        (4, 4, ARGB, Flags::empty())
    });
    assert_eq!(
        error,
        Some(zwp_linux_buffer_params_v1::Error::InvalidFormat as u32)
    );
    let (error, _) = with_params(|_, p, f| {
        one_plane(p, f);
        (4, 4, 0x1234, Flags::empty())
    });
    assert_eq!(
        error,
        Some(zwp_linux_buffer_params_v1::Error::InvalidFormat as u32)
    );
}

#[test]
fn unsupported_flags_fail_only_that_image() {
    let (error, was_failed) = with_params(|_, p, f| {
        one_plane(p, f);
        (4, 4, ARGB, Flags::Interlaced)
    });
    assert_eq!(error, None);
    assert!(was_failed);
}

#[test]
fn parameters_are_used_once() {
    let mut h = gpu_harness();
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let (p, _file) = params(&mut h, c, &d, 4, 4);
    let _b = p.create_immed(4, 4, ARGB, Flags::empty(), &qh, ());
    p.create(4, 4, ARGB, Flags::empty());
    h.roundtrip();
    assert_eq!(
        param_error(&mut h, c),
        Some(zwp_linux_buffer_params_v1::Error::AlreadyUsed as u32)
    );
}

#[test]
fn a_file_that_is_not_a_gpu_image_fails_only_that_image() {
    // The real check, no double: a temporary file is not a dma-buf (research G3).
    let mut h = Harness::new();
    h.server
        .set_gpu_offer(MAIN_DEVICE, PAIRS.to_vec())
        .expect("offer");
    let c = h.connect();
    let d = dmabuf(&mut h, c, 5);
    let qh = h.c(c).qh.clone();
    let (p, _file) = params(&mut h, c, &d, 4, 4);
    let _b = p.create_immed(4, 4, ARGB, Flags::empty(), &qh, ());
    h.roundtrip();
    assert!(failed(h.c(c)));
    assert!(!h.c(c).is_disconnected());
}

// --- Commits -----------------------------------------------------------------------------------

/// What Mesa sends in one swap: `create_immed`, `attach`, `commit` (research G7).
#[test]
fn a_commit_waits_for_the_import_of_its_image() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let w = ready_window(&mut h, c, &g);
    let b = immed(&mut h, c, &d, 20, 10);
    w.surface.attach(Some(&b), 0, 0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(mapped_id(&mut h), None, "not before the import");
    imports_succeed(&mut h);
    let mapped = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { size, .. } => Some(size),
            _ => None,
        });
    assert_eq!(mapped.map(|s| (s.w, s.h)), Some((20.0, 10.0)));
}

#[test]
fn a_failed_import_changes_nothing_about_the_window() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, id) = open_window(&mut h, c, &g, 30, 30, RED);
    let parts = h.server.window_parts(id);
    h.server.take_window_events();
    let qh = h.c(c).qh.clone();
    let cb = w.surface.frame(&qh, ());
    let b = immed(&mut h, c, &d, 20, 10);
    w.surface.set_buffer_scale(2);
    w.surface.attach(Some(&b), 0, 0);
    w.surface.commit();
    h.roundtrip();
    h.server.process_imports(|_| Err("refused".to_string()));
    h.roundtrip();
    assert!(failed(h.c(c)));
    assert_eq!(
        h.server.window_parts(id),
        parts,
        "same size, scale, picture"
    );
    assert!(h.server.take_window_events().is_empty());
    assert!(released(h.c(c), &b), "the buffer goes back");
    assert!(!h.c(c).is_disconnected());
    // Its frame callback completes with the next frame.
    h.server.presented(5, &[id]);
    h.roundtrip();
    let done = h
        .c(c)
        .events::<wl_callback::Event>()
        .iter()
        .any(|(i, e)| *i == cb.id() && matches!(e, wl_callback::Event::Done { .. }));
    assert!(done);
}

#[test]
fn an_image_goes_back_when_no_frame_in_flight_can_read_it() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let w = ready_window(&mut h, c, &g);
    let first = immed(&mut h, c, &d, 20, 10);
    w.surface.attach(Some(&first), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);

    // A frame showing `first` is in flight when `second` replaces it.
    let shown = h.server.frame_drawn();
    let second = immed(&mut h, c, &d, 20, 10);
    w.surface.attach(Some(&second), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    h.server.release_images(Some(shown));
    h.roundtrip();
    assert!(!released(h.c(c), &first), "that frame may still read it");
    h.server.release_images(None);
    h.roundtrip();
    assert!(released(h.c(c), &first));
    assert!(!released(h.c(c), &second), "still shown");
}

#[test]
fn the_renderer_hears_what_each_surface_shows_and_which_images_are_gone() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let w = ready_window(&mut h, c, &g);
    let qh = h.c(c).qh.clone();
    let (p, _file) = params(&mut h, c, &d, 20, 10);
    let b = p.create_immed(20, 10, ARGB, Flags::YInvert, &qh, ());
    w.surface.attach(Some(&b), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let mut imported = Vec::new();
    h.server.process_imports(|image| {
        imported.push(image.key);
        Ok(())
    });
    let shown = h.server.take_shown_images();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].image, imported[0]);
    assert!(shown[0].y_invert);
    assert!(h.server.take_destroyed_images().is_empty());
    b.destroy();
    h.roundtrip();
    assert_eq!(h.server.take_destroyed_images(), imported);
}

#[test]
fn an_image_above_the_size_limit_ends_the_application_on_commit() {
    let mut h = gpu_harness();
    h.server.set_max_texture_side(16);
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let w = ready_window(&mut h, c, &g);
    let b = immed(&mut h, c, &d, 17, 4);
    w.surface.attach(Some(&b), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    assert_eq!(
        error_code(&mut h, c),
        Some(wl_surface::Error::InvalidSize as u32)
    );
}

/// Named in the log when file descriptors run out (FR-015).
#[test]
fn the_application_holding_the_most_images_can_be_named() {
    let mut h = gpu_harness();
    assert_eq!(h.server.application_holding_most_images(), None);
    let one = h.connect();
    let two = h.connect();
    let d_one = dmabuf(&mut h, one, 5);
    let d_two = dmabuf(&mut h, two, 5);
    let _first = immed(&mut h, one, &d_one, 20, 10);
    let _second = immed(&mut h, two, &d_two, 20, 10);
    let third = immed(&mut h, two, &d_two, 20, 10);
    let g = globals(h.c(one));
    // Shared memory does not count.
    let (_pixels, _pool, _file) = solid_buffer(h.c(one), &g.shm, 8, 8, RED);
    let (_pixels2, _pool2, _file2) = solid_buffer(h.c(one), &g.shm, 8, 8, RED);
    h.roundtrip();
    let (holder, count) = h.server.application_holding_most_images().expect("some");
    assert_eq!(count, 2);

    // Its images going away changes who it is.
    third.destroy();
    h.roundtrip();
    let (tied, count) = h.server.application_holding_most_images().expect("some");
    assert_eq!(count, 1);
    assert!(tied < holder, "a tie goes to the lowest number");
}

// --- Without GPU images, and next to them (US3) ---------------------------------------------------
// A shared-memory buffer with synchronisation points is in `syncobj.rs`.

/// The sizes of the shared-memory pictures waiting to be copied.
fn uploads(h: &mut Harness) -> Vec<(u32, u32)> {
    let mut seen = Vec::new();
    h.server
        .upload_pending(|_, image| seen.push((image.width, image.height)));
    seen
}

fn part_size(h: &Harness, id: WindowId) -> (f64, f64) {
    let size = h.server.window_parts(id)[0].rect.size;
    (size.w, size.h)
}

fn callback_done(c: &Client, callback: &wl_callback::WlCallback) -> bool {
    c.events::<wl_callback::Event>()
        .iter()
        .any(|(id, e)| *id == callback.id() && matches!(e, wl_callback::Event::Done { .. }))
}

#[test]
fn with_no_offer_there_are_no_gpu_globals_and_shared_memory_works() {
    let mut h = Harness::new();
    let c = h.connect();
    for name in ["zwp_linux_dmabuf_v1", "wp_linux_drm_syncobj_manager_v1"] {
        assert!(
            !h.c(c).state.globals.iter().any(|(_, i, _)| i == name),
            "{name}"
        );
    }
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 30, 30, RED);
    assert_eq!(uploads(&mut h), vec![(30, 30)]);
    assert!(h.server.window_parts(id)[0].has_content);

    let qh = h.c(c).qh.clone();
    let callback = w.surface.frame(&qh, ());
    w.surface.commit();
    h.roundtrip();
    h.server.presented(5, &[id]);
    h.roundtrip();
    assert!(callback_done(h.c(c), &callback));
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn one_window_can_take_turns_between_shared_memory_and_gpu_images() {
    let mut h = gpu_harness();
    let c = h.connect();
    let g = globals(h.c(c));
    let d = dmabuf(&mut h, c, 5);
    let (w, id) = open_window(&mut h, c, &g, 30, 30, RED);
    assert_eq!(uploads(&mut h), vec![(30, 30)]);
    assert!(h.server.take_shown_images().is_empty());

    // Over to a GPU image.
    let image = immed(&mut h, c, &d, 20, 10);
    w.surface.attach(Some(&image), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    let shown = h.server.take_shown_images();
    assert_eq!(shown.len(), 1);
    let key = shown[0].image;
    assert!(uploads(&mut h).is_empty(), "nothing to copy");
    assert_eq!(part_size(&h, id), (20.0, 10.0));

    // Back to shared memory: the picture is copied, and the image goes back.
    let (pixels, _pool, _file) = solid_buffer(h.c(c), &g.shm, 40, 40, RED);
    w.surface.attach(Some(&pixels), 0, 0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(uploads(&mut h), vec![(40, 40)]);
    assert!(h.server.take_shown_images().is_empty());
    assert_eq!(part_size(&h, id), (40.0, 40.0));
    h.server.release_images(None);
    h.roundtrip();
    assert!(released(h.c(c), &image));

    // And the same image again, with no second import.
    w.surface.attach(Some(&image), 0, 0);
    w.surface.commit();
    h.roundtrip();
    h.server
        .process_imports(|_| Err("it was imported already".to_string()));
    h.roundtrip();
    let shown = h.server.take_shown_images();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].image, key);
    assert_eq!(part_size(&h, id), (20.0, 10.0));
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn a_shared_memory_client_next_to_a_gpu_one_is_unaffected() {
    let mut h = gpu_harness();
    let gpu = h.connect();
    let plain = h.connect();
    let gpu_globals = globals(h.c(gpu));
    let plain_globals = globals(h.c(plain));
    let d = dmabuf(&mut h, gpu, 5);
    let (plain_window, plain_id) = open_window(&mut h, plain, &plain_globals, 30, 30, RED);
    assert_eq!(uploads(&mut h), vec![(30, 30)]);

    // The GPU client's commit waits for its import...
    let w = ready_window(&mut h, gpu, &gpu_globals);
    let image = immed(&mut h, gpu, &d, 20, 10);
    w.surface.attach(Some(&image), 0, 0);
    w.surface.commit();
    h.roundtrip();

    // ...and the other one's does not.
    let (pixels, _pool, _file) = solid_buffer(h.c(plain), &plain_globals.shm, 40, 40, RED);
    let qh = h.c(plain).qh.clone();
    let callback = plain_window.surface.frame(&qh, ());
    plain_window.surface.attach(Some(&pixels), 0, 0);
    plain_window.surface.commit();
    h.roundtrip();
    assert_eq!(uploads(&mut h), vec![(40, 40)]);

    // Its image being refused changes nothing for the other one either.
    h.server.process_imports(|_| Err("refused".to_string()));
    h.roundtrip();
    assert!(failed(h.c(gpu)));
    h.server.presented(5, &[plain_id]);
    h.roundtrip();
    assert!(callback_done(h.c(plain), &callback));
    assert_eq!(part_size(&h, plain_id), (40.0, 40.0));
    assert!(!h.c(plain).is_disconnected());
    assert!(!h.c(gpu).is_disconnected());
}
