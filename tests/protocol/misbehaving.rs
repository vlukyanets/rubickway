//! The seven misbehaving-application cases of spec.md Edge Cases (SC-004). Each test checks
//! that only the offending client is disconnected and that a well-behaved client still gets
//! its frame callback afterwards.

use std::fs::File;
use std::os::fd::AsFd;

use rubickway::canvas::Size;
use rubickway::shell::{ClientInput, States, WindowCommand, WindowEvent, WindowId};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_keyboard;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, Proxy};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::{
    self, Flags, ZwpLinuxBufferParamsV1,
};
use wayland_protocols::xdg::shell::client::xdg_toplevel;

use rubickway::shell::CursorName;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use crate::commits::unfinished;
use crate::cursor_shape::{set_raw_shape, shape, shape_client};
use crate::dmabuf::{ARGB, dmabuf, gpu_harness, immed, imports_succeed};
use crate::harness::{Harness, solid_buffer, temp_file};
use crate::input::enter_pointer;
use crate::surfaces::{Window, globals, open_window};
use crate::syncobj::{Sync, sync_client, sync_harness};

const RED: u32 = 0xffff_0000;

/// A well-behaved client with a mapped window.
struct Good {
    index: usize,
    window: Window,
    id: WindowId,
}

fn good_client(h: &mut Harness) -> Good {
    let index = h.connect();
    let g = globals(h.c(index));
    let (window, id) = open_window(h, index, &g, 20, 20, RED);
    Good { index, window, id }
}

/// Asserts that the good client is still connected and still gets frame callbacks.
fn assert_good_client_works(h: &mut Harness, good: &Good) {
    let qh = h.c(good.index).qh.clone();
    let cb: WlCallback = good.window.surface.frame(&qh, ());
    good.window.surface.commit();
    h.roundtrip();
    h.server.presented(1, &[good.id]);
    h.roundtrip();
    let c = h.c(good.index);
    assert!(!c.is_disconnected(), "good client: {:?}", c.error());
    assert!(
        c.events::<wl_callback::Event>()
            .iter()
            .any(|(id, _)| *id == cb.id()),
        "good client got no frame callback"
    );
}

fn shm_error(h: &mut Harness, ci: usize) -> Option<u32> {
    h.c(ci).error().map(|e| e.code)
}

// Case 1: invalid requests.
#[test]
fn case_1_invalid_buffer_parameters_disconnect_only_the_offender() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    type Bad = fn(
        &wayland_client::protocol::wl_shm_pool::WlShmPool,
        &wayland_client::QueueHandle<crate::harness::Recorder>,
    );
    let cases: [(Bad, u32); 4] = [
        (
            |pool, qh| {
                pool.create_buffer(0, -4, 4, 16, wl_shm::Format::Argb8888, qh, ());
            },
            wl_shm::Error::InvalidStride as u32,
        ),
        (
            |pool, qh| {
                pool.create_buffer(0, 4, i32::MAX, i32::MAX, wl_shm::Format::Argb8888, qh, ());
            },
            wl_shm::Error::InvalidStride as u32,
        ),
        (
            |pool, qh| {
                pool.create_buffer(4096, 4, 4, 16, wl_shm::Format::Argb8888, qh, ());
            },
            wl_shm::Error::InvalidStride as u32,
        ),
        (
            |pool, qh| {
                pool.create_buffer(0, 4, 4, 16, wl_shm::Format::Rgb565, qh, ());
            },
            wl_shm::Error::InvalidFormat as u32,
        ),
    ];
    for (bad, code) in cases {
        let ci = h.connect();
        let shm: WlShm = h.c(ci).bind(1);
        let file = temp_file(4096);
        let qh = h.c(ci).qh.clone();
        let pool = shm.create_pool(file.as_fd(), 4096, &qh, ());
        bad(&pool, &qh);
        h.roundtrip();
        assert_eq!(shm_error(&mut h, ci), Some(code));
    }
    assert_good_client_works(&mut h, &good);
}

// Case 2: shared-memory misuse.
#[test]
fn case_2_truncated_pool_file_disconnects_with_invalid_fd() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    h.server.upload_pending(|_, _| {});
    let ci = h.connect();
    let g = globals(h.c(ci));
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let (buffer, _pool, file) = solid_buffer(h.c(ci), &g.shm, 64, 64, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    file.set_len(0).expect("truncate");
    // The read happens when RubickWay copies the content.
    h.server.upload_pending(|_, image| {
        std::hint::black_box(image.bytes.iter().map(|&b| u64::from(b)).sum::<u64>());
    });
    h.roundtrip();
    assert_eq!(shm_error(&mut h, ci), Some(wl_shm::Error::InvalidFd as u32));
    assert_good_client_works(&mut h, &good);
}

#[test]
fn case_2_shrinking_a_pool_is_an_error() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let shm: WlShm = h.c(ci).bind(1);
    let file = temp_file(8192);
    let pool = shm.create_pool(file.as_fd(), 8192, &h.c(ci).qh, ());
    pool.resize(4096);
    h.roundtrip();
    assert_eq!(shm_error(&mut h, ci), Some(wl_shm::Error::InvalidFd as u32));
    assert_good_client_works(&mut h, &good);
}

// Case 3: disconnecting at any moment.
#[test]
fn case_3_disconnect_while_mapped_unmaps_and_releases() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let (w, id) = open_window(&mut h, ci, &g, 30, 30, RED);
    // Mid-commit: new content attached, then the connection drops.
    let (buffer, _pool, _file) = solid_buffer(h.c(ci), &g.shm, 30, 30, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    let _ = h.c(ci).conn.flush();
    let gone = h.clients.remove(ci);
    drop(gone);
    h.roundtrip();
    assert!(
        h.server
            .take_window_events()
            .contains(&WindowEvent::Unmapped { id })
    );
    assert!(h.server.window_parts(id).is_empty());
    assert_good_client_works(&mut h, &good);
}

// Case 4: a client that never reads.
#[test]
fn case_4_a_client_that_never_reads_is_disconnected() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    h.c(ci).paused = true;
    let display = h.c(ci).conn.display();
    let qh = h.c(ci).qh.clone();
    // Every sync makes the server queue a callback and a delete_id event (~24 bytes). The
    // client library buffers requests without limit, so only the server can end this.
    for _ in 0..200 {
        for _ in 0..500 {
            display.sync(&qh, ());
        }
        let _ = h.c(ci).conn.flush();
        h.roundtrip();
        if client_gone(&h.c(ci).conn) {
            break;
        }
    }
    h.c(ci).paused = false;
    for _ in 0..50 {
        h.roundtrip();
    }
    assert!(
        h.c(ci).is_disconnected(),
        "the stuck client is still connected"
    );
    assert_good_client_works(&mut h, &good);
}

/// Whether the server has dropped the client: a protocol error, or the socket closed under it
/// (what the pure-Rust client backend reports; a full socket is not "gone").
fn client_gone(conn: &Connection) -> bool {
    use std::io::ErrorKind;
    use wayland_client::backend::WaylandError;
    conn.protocol_error().is_some()
        || matches!(
            conn.flush(),
            Err(WaylandError::Io(e))
                if matches!(e.kind(), ErrorKind::BrokenPipe | ErrorKind::ConnectionReset)
        )
}

// Case 5: flooding.
#[test]
fn case_5_a_flooding_client_does_not_starve_others() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let surface = g.compositor.create_surface(&h.c(ci).qh, ());
    for _ in 0..100_000 {
        surface.damage(0, 0, 1, 1);
    }
    let _ = h.c(ci).conn.flush();
    assert_good_client_works(&mut h, &good);
}

// Case 6: never drawing.
#[test]
fn case_6_a_client_that_never_draws_shows_nothing() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let _ = h.server.take_window_events();
    let ci = h.connect();
    let g = globals(h.c(ci));
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());
    assert!(!h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

// GPU images (gpu-buffers spec, Edge Cases 1-6 and 9-11; each row of the contract is also tested
// in `dmabuf.rs`). Each leaves RubickWay and another client running.

/// A misuse of one description: what a client sends on new parameters, given a 64-byte file.
type Misuse = fn(&ZwpLinuxBufferParamsV1, &File);

const LINEAR_HI: u32 = 0;
const LINEAR_LO: u32 = 0;

fn add_plane(p: &ZwpLinuxBufferParamsV1, file: &File, plane: u32) {
    p.add(file.as_fd(), plane, 0, 16, LINEAR_HI, LINEAR_LO);
}

/// Runs each misuse on a new client of one server, next to a good client.
fn each_misuse(cases: &[(&str, Misuse)], check: impl Fn(&mut Harness, usize, &str)) {
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    for (what, misuse) in cases {
        let ci = h.connect();
        let d = dmabuf(&mut h, ci, 5);
        let p = d.create_params(&h.c(ci).qh, ());
        let file = temp_file(64);
        misuse(&p, &file);
        h.roundtrip();
        check(&mut h, ci, what);
        assert_good_client_works(&mut h, &good);
    }
}

fn ended(h: &mut Harness, ci: usize, what: &str) {
    assert!(h.c(ci).is_disconnected(), "{what}");
}

/// Whether the client was told `failed` and stays connected.
fn only_the_image_failed(h: &mut Harness, ci: usize, what: &str) {
    let c = h.c(ci);
    assert!(!c.is_disconnected(), "{what}: {:?}", c.error());
    let failed = c
        .events::<zwp_linux_buffer_params_v1::Event>()
        .iter()
        .any(|(_, e)| matches!(e, zwp_linux_buffer_params_v1::Event::Failed));
    assert!(failed, "{what}");
}

#[test]
fn gpu_case_1_invalid_descriptions_end_only_that_client() {
    each_misuse(
        &[
            ("zero size", |p, f| {
                add_plane(p, f, 0);
                p.create(0, 4, ARGB, Flags::empty());
            }),
            ("negative size", |p, f| {
                add_plane(p, f, 0);
                p.create(4, -1, ARGB, Flags::empty());
            }),
            ("plane out of range", |p, f| add_plane(p, f, 4)),
            ("the same plane twice", |p, f| {
                add_plane(p, f, 0);
                add_plane(p, f, 0);
            }),
            ("no plane", |p, _| p.create(4, 4, ARGB, Flags::empty())),
            ("a plane missing in between", |p, f| {
                add_plane(p, f, 1);
                p.create(4, 4, ARGB, Flags::empty());
            }),
            ("arithmetic that overflows", |p, f| {
                p.add(f.as_fd(), 0, u32::MAX, u32::MAX, LINEAR_HI, LINEAR_LO);
                p.create(i32::MAX, i32::MAX, ARGB, Flags::empty());
            }),
        ],
        ended,
    );
}

#[test]
fn gpu_case_2_a_description_used_twice_or_changed_after_use_ends_only_that_client() {
    each_misuse(
        &[
            ("used twice", |p, f| {
                add_plane(p, f, 0);
                p.create(4, 4, ARGB, Flags::empty());
                p.create(4, 4, ARGB, Flags::empty());
            }),
            ("changed after use", |p, f| {
                add_plane(p, f, 0);
                p.create(4, 4, ARGB, Flags::empty());
                add_plane(p, f, 1);
            }),
        ],
        ended,
    );
}

#[test]
fn gpu_case_3_a_file_that_is_not_an_image_fails_only_that_image() {
    // The real check of files, not the double accepting any.
    let mut h = Harness::new();
    h.server.set_gpu_offer(1, vec![(ARGB, 0)]).expect("offer");
    let good = good_client(&mut h);
    let ci = h.connect();
    let d = dmabuf(&mut h, ci, 5);
    let p = d.create_params(&h.c(ci).qh, ());
    let file = temp_file(64);
    add_plane(&p, &file, 0);
    p.create(4, 4, ARGB, Flags::empty());
    h.roundtrip();
    only_the_image_failed(&mut h, ci, "an ordinary file");
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_3_a_file_too_small_for_its_layout_ends_only_that_client() {
    each_misuse(
        &[("a file of 64 bytes for 4 x 8", |p, f| {
            add_plane(p, f, 0);
            p.create(4, 8, ARGB, Flags::empty());
        })],
        ended,
    );
}

#[test]
fn gpu_case_4_what_was_not_offered_ends_only_that_client() {
    each_misuse(
        &[
            ("a format never offered", |p, f| {
                add_plane(p, f, 0);
                p.create(4, 4, 0x2020_2020, Flags::empty());
            }),
            ("a modifier never offered", |p, f| {
                p.add(f.as_fd(), 0, 0, 16, 0x0100_0000, 1);
                p.create(4, 4, ARGB, Flags::empty());
            }),
            ("different modifiers across planes", |p, f| {
                add_plane(p, f, 0);
                p.add(f.as_fd(), 1, 0, 16, 0x0100_0000, 1);
            }),
        ],
        ended,
    );
}

#[test]
fn gpu_case_4_unsupported_flags_fail_only_that_image() {
    // More planes than the driver accepts fail the same way, in the import: see case 5 here and
    // the quickstart for a real driver.
    each_misuse(
        &[("interlaced", |p, f| {
            add_plane(p, f, 0);
            p.create(4, 4, ARGB, Flags::Interlaced);
        })],
        only_the_image_failed,
    );
}

#[test]
fn gpu_case_5_a_refused_import_fails_only_that_image() {
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    let ci = h.connect();
    let d = dmabuf(&mut h, ci, 5);
    let _b = immed(&mut h, ci, &d, 4, 4);
    h.roundtrip();
    h.server.process_imports(|_| Err("refused".to_string()));
    h.roundtrip();
    only_the_image_failed(&mut h, ci, "refused by the GPU");
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_6_an_image_above_the_limit_ends_only_that_client() {
    let mut h = gpu_harness();
    h.server.set_max_texture_side(32);
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let d = dmabuf(&mut h, ci, 5);
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let b = immed(&mut h, ci, &d, 33, 4);
    w.surface.attach(Some(&b), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    assert!(h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_7_an_image_never_finished_stops_only_its_own_window() {
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    let seat: WlSeat = h.c(good.index).bind(8);
    let _keyboard = seat.get_keyboard(&h.c(good.index).qh, ());
    h.roundtrip();

    let ci = h.connect();
    let g = globals(h.c(ci));
    let d = dmabuf(&mut h, ci, 5);
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let never = unfinished(&mut h, ci, &d, 20, 10);
    w.surface.attach(Some(&never.buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    let waits = h.server.take_waits();
    assert_eq!(
        waits.len(),
        1,
        "it is left to the event loop: nothing blocks"
    );

    // Later commits wait behind it, however many.
    let later = immed(&mut h, ci, &d, 8, 8);
    for _ in 0..3 {
        w.surface.attach(Some(&later), 0, 0);
        w.surface.commit();
    }
    h.roundtrip();
    imports_succeed(&mut h);
    assert!(
        !h.server
            .take_window_events()
            .iter()
            .any(|e| matches!(e, WindowEvent::Mapped { id, .. } if *id != good.id)),
        "its window waits for its image"
    );

    // The other client keeps getting frames and input.
    assert_good_client_works(&mut h, &good);
    h.c(good.index).clear_events();
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(good.id))], 5);
    h.roundtrip();
    assert!(
        h.c(good.index)
            .events::<wl_keyboard::Event>()
            .iter()
            .any(|(_, e)| matches!(e, wl_keyboard::Event::Enter { .. }))
    );
    assert!(!h.c(ci).is_disconnected());
}

#[test]
fn gpu_case_8_no_limit_ends_a_client_creating_many_objects() {
    // Their number is not limited, as in other compositors (spec Assumptions).
    let (mut h, _fake) = sync_harness();
    let good = good_client(&mut h);
    let ci = h.connect();
    let s = sync_client(&mut h, ci);
    let qh = h.c(ci).qh.clone();
    let file = temp_file(64);
    for _ in 0..300 {
        let _params = s.dmabuf.create_params(&qh, ());
        let _timeline = s.manager.import_timeline(file.as_fd(), &qh, ());
    }
    h.roundtrip();
    // Fewer than the objects above: each holds files of the test process too.
    for i in 0..100 {
        let image = immed(&mut h, ci, &s.dmabuf, 4, 4);
        s.surface.set_acquire_point(&s.a, 0, i + 1);
        s.surface.set_release_point(&s.b, 0, i + 1);
        s.window.surface.attach(Some(&image), 0, 0);
        s.window.surface.commit();
    }
    h.roundtrip();
    imports_succeed(&mut h);
    assert_eq!(h.server.take_waits().len(), 100, "all of them wait");
    assert!(!h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_12_misusing_explicit_synchronisation_ends_only_that_client() {
    type SyncMisuse = fn(&mut Harness, usize, &Sync);
    let cases: [(&str, SyncMisuse); 8] = [
        ("a second object for a surface", |h, ci, s| {
            let qh = h.c(ci).qh.clone();
            let _again = s.manager.get_surface(&s.window.surface, &qh, ());
        }),
        ("a file that is not a timeline", |h, ci, s| {
            let qh = h.c(ci).qh.clone();
            let file = temp_file(0);
            let _t = s.manager.import_timeline(file.as_fd(), &qh, ());
        }),
        ("points with no image", |_, _, s| {
            s.surface.set_acquire_point(&s.a, 0, 1);
            s.surface.set_release_point(&s.b, 0, 1);
            s.window.surface.commit();
        }),
        ("an image with no acquire point", |h, ci, s| {
            let b = immed(h, ci, &s.dmabuf, 4, 4);
            s.surface.set_release_point(&s.b, 0, 1);
            s.window.surface.attach(Some(&b), 0, 0);
            s.window.surface.commit();
        }),
        ("an image with no release point", |h, ci, s| {
            let b = immed(h, ci, &s.dmabuf, 4, 4);
            s.surface.set_acquire_point(&s.a, 0, 1);
            s.window.surface.attach(Some(&b), 0, 0);
            s.window.surface.commit();
        }),
        ("an image with no points", |h, ci, s| {
            let b = immed(h, ci, &s.dmabuf, 4, 4);
            s.window.surface.attach(Some(&b), 0, 0);
            s.window.surface.commit();
        }),
        ("the same point for ready and finished", |h, ci, s| {
            let b = immed(h, ci, &s.dmabuf, 4, 4);
            s.surface.set_acquire_point(&s.a, 0, 5);
            s.surface.set_release_point(&s.a, 0, 5);
            s.window.surface.attach(Some(&b), 0, 0);
            s.window.surface.commit();
        }),
        ("shared memory with points", |h, ci, s| {
            let g = globals(h.c(ci));
            let (b, _pool, _file) = solid_buffer(h.c(ci), &g.shm, 4, 4, RED);
            s.surface.set_acquire_point(&s.a, 0, 1);
            s.surface.set_release_point(&s.b, 0, 1);
            s.window.surface.attach(Some(&b), 0, 0);
            s.window.surface.commit();
        }),
    ];
    let (mut h, _fake) = sync_harness();
    let good = good_client(&mut h);
    for (what, misuse) in cases {
        let ci = h.connect();
        let s = sync_client(&mut h, ci);
        misuse(&mut h, ci, &s);
        h.roundtrip();
        assert!(h.c(ci).is_disconnected(), "{what}");
        assert_good_client_works(&mut h, &good);
    }
}

#[test]
fn gpu_case_9_destroying_an_image_while_shown_or_waiting_keeps_the_picture() {
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let d = dmabuf(&mut h, ci, 5);
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let shown = immed(&mut h, ci, &d, 20, 10);
    w.surface.attach(Some(&shown), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    let id = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { id, .. } => Some(id),
            _ => None,
        })
        .expect("mapped");
    let parts = h.server.window_parts(id);
    assert!(parts[0].has_content);

    // Destroyed while shown.
    shown.destroy();
    h.roundtrip();
    assert_eq!(h.server.window_parts(id), parts);

    // Destroyed while its commit waits for the import.
    let waiting = immed(&mut h, ci, &d, 8, 8);
    w.surface.attach(Some(&waiting), 0, 0);
    w.surface.commit();
    waiting.destroy();
    h.roundtrip();
    imports_succeed(&mut h);
    assert_eq!(
        h.server.window_parts(id),
        parts,
        "the window keeps its picture"
    );
    assert!(!h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_10_disconnecting_with_images_waiting_or_shown_releases_them() {
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let d = dmabuf(&mut h, ci, 5);
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let shown = immed(&mut h, ci, &d, 20, 10);
    w.surface.attach(Some(&shown), 0, 0);
    w.surface.commit();
    h.roundtrip();
    imports_succeed(&mut h);
    let waiting = immed(&mut h, ci, &d, 20, 10);
    w.surface.attach(Some(&waiting), 0, 0);
    w.surface.commit();
    h.roundtrip();
    h.clients.remove(ci);
    // Let the server see the connection go, with an import still pending.
    h.server.dispatch().expect("dispatch");
    imports_succeed(&mut h);
    h.server.release_images(None);
    assert_eq!(
        h.server.take_destroyed_images().len(),
        2,
        "both images are gone"
    );
    assert_good_client_works(&mut h, &good);
}

#[test]
fn gpu_case_11_feedback_for_a_surface_destroyed_right_after_is_inert() {
    // A client library cannot name a destroyed object in a request, so the closest a test can get
    // is both in one batch: the server meets the request first and the surface is gone by the
    // time anything is answered.
    let mut h = gpu_harness();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let d = dmabuf(&mut h, ci, 5);
    let qh = h.c(ci).qh.clone();
    let surface = g.compositor.create_surface(&qh, ());
    let _f = d.get_surface_feedback(&surface, &qh, ());
    surface.destroy();
    h.roundtrip();
    let c = h.c(ci);
    assert!(!c.is_disconnected(), "{:?}", c.error());
    assert_good_client_works(&mut h, &good);
}

// Case 7: a window too large to display.
#[test]
fn case_7_a_buffer_above_the_texture_limit_disconnects() {
    let mut h = Harness::new();
    h.server.set_max_texture_side(64);
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let w = crate::surfaces::create_window(&mut h, ci, &g);
    crate::surfaces::ack(&mut h, ci, &w);
    let (buffer, _pool, _file) = solid_buffer(h.c(ci), &g.shm, 65, 10, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let error = h.c(ci).error().expect("disconnected with an error");
    assert_eq!(error.code, wl_surface::Error::InvalidSize as u32);
    assert_eq!(error.object_interface, "wl_surface");
    assert_good_client_works(&mut h, &good);
}

// --- Window management (window-management spec Edge Cases, SC-006) ----------------------------
//
// Shell-level cases (requests without a press, windows disappearing mid-operation or while
// maximized or fullscreen, requests from unfocused windows, limits above the largest window)
// are in tests/shell.rs; stale serials and unknown resize edges in tests/protocol/wm.rs.

fn configure(window: WindowId, w: f64, h: f64) -> WindowCommand {
    WindowCommand::Configure {
        window,
        size: Some(Size::new(w, h)),
        states: States::default(),
    }
}

#[test]
fn contradictory_size_limits_are_reported_as_sent() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let (w, _id) = open_window(&mut h, ci, &g, 20, 20, RED);
    w.toplevel.set_min_size(300, 300);
    w.toplevel.set_max_size(100, 100);
    w.surface.commit();
    h.roundtrip();
    let limits = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Updated { limits, .. } => Some(limits),
            _ => None,
        })
        .expect("limits reported");
    // The shell resolves the conflict (SizeLimits::clamp); the server keeps the values.
    assert_eq!(limits.min, Size::new(300.0, 300.0));
    assert_eq!(limits.max, Size::new(100.0, 100.0));
    assert!(!h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

#[test]
fn a_client_that_never_acks_keeps_one_configure_outstanding() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let (w, id) = open_window(&mut h, ci, &g, 20, 20, RED);
    h.c(ci).clear_events();
    for i in 0..50 {
        h.server
            .command(&[configure(id, 100.0 + f64::from(i), 100.0)]);
        h.roundtrip();
    }
    let sent = h
        .c(ci)
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .filter(|(oid, e)| {
            *oid == w.toplevel.id() && matches!(e, xdg_toplevel::Event::Configure { .. })
        })
        .count();
    assert_eq!(sent, 1);
    assert_good_client_works(&mut h, &good);
}

#[test]
fn a_toplevel_destroyed_while_its_initial_configure_is_held_is_forgotten() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let qh = h.c(ci).qh.clone();
    let surface = g.compositor.create_surface(&qh, ());
    let xdg = g.wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_fullscreen(None);
    surface.commit();
    h.roundtrip();
    let id = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Created { id, .. } => Some(id),
            _ => None,
        })
        .expect("created");
    toplevel.destroy();
    xdg.destroy();
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![WindowEvent::Unmapped { id }]
    );
    // The shell's late answer is ignored.
    h.server.command(&[configure(id, 100.0, 100.0)]);
    h.roundtrip();
    assert!(!h.c(ci).is_disconnected());
    assert_good_client_works(&mut h, &good);
}

#[test]
fn a_client_gone_with_a_configure_queued_harms_no_one() {
    let mut h = Harness::new();
    let good = good_client(&mut h);
    let ci = h.connect();
    let g = globals(h.c(ci));
    let (_w, id) = open_window(&mut h, ci, &g, 20, 20, RED);
    h.server.command(&[configure(id, 100.0, 100.0)]);
    h.server.command(&[configure(id, 200.0, 200.0)]);
    let gone = h.clients.remove(ci);
    drop(gone);
    h.roundtrip();
    assert!(
        h.server
            .take_window_events()
            .contains(&WindowEvent::Unmapped { id })
    );
    h.server.command(&[configure(id, 300.0, 300.0)]);
    h.roundtrip();
    assert_good_client_works(&mut h, &good);
}

// --- Standalone: activity while RubickWay is paused (spec Edge Cases, research S22) ----------

#[test]
fn a_client_flooding_frames_while_paused_gets_no_callback_until_resumed() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 10, 10, RED);
    h.server.set_paused(true);
    let qh = h.c(c).qh.clone();
    let mut callbacks = Vec::new();
    for i in 0..100u32 {
        let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 10, 10, RED ^ i);
        w.surface.attach(Some(&buffer), 0, 0);
        callbacks.push(w.surface.frame(&qh, ()));
        w.surface.commit();
    }
    h.roundtrip();
    // Nothing is shown while paused: presenting completes nothing and nothing is pending.
    h.server.presented(10, &[id]);
    h.roundtrip();
    assert!(h.c(c).events::<wl_callback::Event>().is_empty());
    assert!(!h.server.has_pending_callbacks(&[id]));
    // Only the latest content is kept for the window.
    let mut uploads = 0;
    h.server.upload_pending(|_, _| uploads += 1);
    assert_eq!(uploads, 1);

    h.server.set_paused(false);
    assert!(h.server.has_pending_callbacks(&[id]));
    h.server.presented(20, &[id]);
    h.roundtrip();
    assert_eq!(h.c(c).events::<wl_callback::Event>().len(), callbacks.len());
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn windows_opened_and_closed_while_paused_are_reported_in_order() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (gone, gone_id) = open_window(&mut h, c, &g, 10, 10, RED);
    h.server.set_paused(true);
    let (_new, new_id) = open_window(&mut h, c, &g, 20, 20, RED);
    gone.toplevel.destroy();
    gone.xdg.destroy();
    gone.surface.destroy();
    h.roundtrip();
    h.server.set_paused(false);
    let events = h.server.take_window_events();
    assert!(
        events.contains(&WindowEvent::Unmapped { id: gone_id }),
        "{events:?}"
    );
    assert_ne!(gone_id, new_id);
    assert!(!h.c(c).is_disconnected());
}

// --- Cursor shapes (cursor-shapes contracts/protocols.md "Misbehaving applications") ----------

const INVALID_SHAPE: u32 = 1;

#[test]
fn shape_values_outside_the_enum_disconnect_only_the_offender() {
    for value in [0, 37, u32::MAX] {
        let mut h = Harness::new();
        let good = good_client(&mut h);
        let bad = shape_client(&mut h, 2);
        let serial = enter_pointer(&mut h, bad.index, bad.id);
        set_raw_shape(&bad.device, serial, value);
        h.roundtrip();
        let error = h.c(bad.index).error().expect("disconnected with an error");
        assert_eq!(error.code, INVALID_SHAPE, "value {value}");
        assert_eq!(error.object_interface, "wp_cursor_shape_device_v1");
        assert_good_client_works(&mut h, &good);
    }
}

#[test]
fn version_2_shapes_on_a_version_1_device_are_an_error() {
    for value in [35, 36] {
        let mut h = Harness::new();
        let good = good_client(&mut h);
        let bad = shape_client(&mut h, 1);
        let serial = enter_pointer(&mut h, bad.index, bad.id);
        // The newest version 1 shape works.
        set_raw_shape(&bad.device, serial, 34);
        h.roundtrip();
        assert!(!h.c(bad.index).is_disconnected());
        set_raw_shape(&bad.device, serial, value);
        h.roundtrip();
        let error = h.c(bad.index).error().expect("disconnected with an error");
        assert_eq!(error.code, INVALID_SHAPE, "value {value}");
        assert_good_client_works(&mut h, &good);
    }
}

#[test]
fn a_flood_of_shape_requests_reports_only_changes() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    for _ in 0..10_000 {
        set_raw_shape(&s.device, serial, 9); // text
    }
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events().len(),
        1,
        "repeats change nothing"
    );
    for i in 0..10_000u32 {
        set_raw_shape(&s.device, serial, if i % 2 == 0 { 4 } else { 9 });
    }
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events().len(),
        10_000,
        "one per change"
    );
    assert!(!h.c(s.index).is_disconnected());
}

#[test]
fn a_client_gone_with_its_shape_shown_leaves_nothing_behind() {
    let mut h = Harness::new();
    let next = shape_client(&mut h, 2);
    let gone = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, gone.index, gone.id);
    gone.device.set_shape(serial, Shape::Text);
    h.roundtrip();
    let id = gone.id;
    drop(gone);
    let client = h.clients.remove(1);
    drop(client);
    h.roundtrip();
    assert!(
        h.server
            .take_window_events()
            .contains(&WindowEvent::Unmapped { id })
    );
    // The next window starts from the arrow.
    let serial = enter_pointer(&mut h, next.index, next.id);
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(next.id, CursorName::Default)]
    );
    next.device.set_shape(serial, Shape::Pointer);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(next.id, CursorName::Pointer)]
    );
}
