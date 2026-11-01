//! The seven misbehaving-application cases of spec.md Edge Cases (SC-004). Each test checks
//! that only the offending client is disconnected and that a well-behaved client still gets
//! its frame callback afterwards.

use std::os::fd::AsFd;

use rubickway::shell::{WindowEvent, WindowId};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, Proxy};

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
    // Every sync makes the server queue a callback and a delete_id event (~24 bytes).
    for round in 0..200 {
        for _ in 0..500 {
            display.sync(&qh, ());
        }
        let _ = h.c(ci).conn.flush();
        h.roundtrip();
        if round % 20 == 0 && client_gone(&h.c(ci).conn) {
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

fn client_gone(conn: &Connection) -> bool {
    conn.protocol_error().is_some()
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
