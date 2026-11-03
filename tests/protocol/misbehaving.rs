//! The seven misbehaving-application cases of spec.md Edge Cases (SC-004). Each test checks
//! that only the offending client is disconnected and that a well-behaved client still gets
//! its frame callback afterwards.

use std::os::fd::AsFd;

use rubickway::canvas::Size;
use rubickway::shell::{States, WindowCommand, WindowEvent, WindowId};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, Proxy};
use wayland_protocols::xdg::shell::client::xdg_toplevel;

use crate::harness::{Harness, solid_buffer, temp_file};
use crate::surfaces::{Window, globals, open_window};

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
