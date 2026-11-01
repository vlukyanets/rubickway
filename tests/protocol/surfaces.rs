//! Globals, surfaces, subsurfaces, xdg-shell, output, viewport, clipboard stub and frame pacing
//! (contracts/protocols.md, research R6, R9, R10, R12).

use rubickway::canvas::{Point, Rect, Size};
use rubickway::shell::{WindowEvent, WindowId};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_popup;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use crate::harness::{Client, Harness, solid_buffer};

const RED: u32 = 0xffff_0000;

/// The globals a test needs, bound at the versions of contracts/protocols.md.
pub struct Globals {
    pub compositor: WlCompositor,
    pub subcompositor: WlSubcompositor,
    pub shm: WlShm,
    pub wm_base: XdgWmBase,
}

pub fn globals(c: &Client) -> Globals {
    Globals {
        compositor: c.bind(6),
        subcompositor: c.bind(1),
        shm: c.bind(1),
        wm_base: c.bind(6),
    }
}

/// A toplevel window's objects.
pub struct Window {
    pub surface: WlSurface,
    pub xdg: XdgSurface,
    pub toplevel: XdgToplevel,
}

/// Creates a toplevel and does the initial commit; the configure is not acked yet.
pub fn create_window(h: &mut Harness, ci: usize, g: &Globals) -> Window {
    let c = h.c(ci);
    let surface = g.compositor.create_surface(&c.qh, ());
    let xdg = g.wm_base.get_xdg_surface(&surface, &c.qh, ());
    let toplevel = xdg.get_toplevel(&c.qh, ());
    surface.commit();
    h.roundtrip();
    Window {
        surface,
        xdg,
        toplevel,
    }
}

/// Acks the latest configure of `w`.
pub fn ack(h: &mut Harness, ci: usize, w: &Window) {
    let c = h.c(ci);
    let serial = c
        .events::<xdg_surface::Event>()
        .iter()
        .rev()
        .find_map(|(id, e)| match e {
            xdg_surface::Event::Configure { serial } if *id == w.xdg.id() => Some(*serial),
            _ => None,
        })
        .expect("a configure");
    w.xdg.ack_configure(serial);
}

/// Opens a mapped `width x height` window filled with `argb` and returns it with its id.
pub fn open_window(
    h: &mut Harness,
    ci: usize,
    g: &Globals,
    width: i32,
    height: i32,
    argb: u32,
) -> (Window, WindowId) {
    let w = create_window(h, ci, g);
    ack(h, ci, &w);
    let c = h.c(ci);
    let (buffer, _pool, _file) = solid_buffer(c, &g.shm, width, height, argb);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let id = h
        .server
        .take_window_events()
        .into_iter()
        .find_map(|e| match e {
            WindowEvent::Mapped { id, .. } => Some(id),
            _ => None,
        })
        .expect("window mapped");
    (w, id)
}

fn error_code(h: &mut Harness, ci: usize) -> Option<u32> {
    h.c(ci).error().map(|e| e.code)
}

// --- T014: globals and core surfaces -------------------------------------------------------

#[test]
fn globals_and_versions_match_the_contract() {
    let mut h = Harness::new();
    let c = h.connect();
    let mut globals: Vec<(String, u32)> = h
        .c(c)
        .state
        .globals
        .iter()
        .map(|(_, i, v)| (i.clone(), *v))
        .collect();
    globals.sort();
    let expected = [
        ("wl_compositor", 6),
        ("wl_data_device_manager", 3),
        ("wl_output", 4),
        ("wl_seat", 8),
        ("wl_shm", 1),
        ("wl_subcompositor", 1),
        ("wp_fractional_scale_manager_v1", 1),
        ("wp_viewporter", 1),
        ("xdg_wm_base", 6),
    ];
    let expected: Vec<(String, u32)> = expected.iter().map(|(i, v)| (i.to_string(), *v)).collect();
    assert_eq!(globals, expected);
}

#[test]
fn surface_state_is_double_buffered() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    ack(&mut h, c, &w);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 64, 32, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    h.roundtrip();
    assert!(
        h.server.take_window_events().is_empty(),
        "attach alone maps"
    );
    w.surface.commit();
    h.roundtrip();
    assert_eq!(h.server.take_window_events().len(), 1);
}

#[test]
fn invalid_scale_is_an_error() {
    // (An invalid transform cannot be sent through the typed client API; the server rejects
    // unknown values with `invalid_transform`.)
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let surface = g.compositor.create_surface(&h.c(c).qh, ());
    surface.set_buffer_transform(wl_output::Transform::Flipped180);
    surface.set_buffer_scale(0);
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, c),
        Some(wl_surface::Error::InvalidScale as u32)
    );
}

#[test]
fn buffer_not_divisible_by_scale_is_invalid_size() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    ack(&mut h, c, &w);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 63, 32, RED);
    w.surface.set_buffer_scale(2);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, c),
        Some(wl_surface::Error::InvalidSize as u32)
    );
}

#[test]
fn viewport_destination_sets_the_size_and_bad_source_is_an_error() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let viewporter: WpViewporter = h.c(c).bind(1);
    let w = create_window(&mut h, c, &g);
    ack(&mut h, c, &w);
    let viewport = viewporter.get_viewport(&w.surface, &h.c(c).qh, ());
    viewport.set_destination(320, 200);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 64, 40, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    assert!(
        matches!(&events[..], [WindowEvent::Mapped { size, .. }] if *size == Size::new(320.0, 200.0)),
        "{events:?}"
    );

    viewport.set_source(0.0, 0.0, 100.0, 10.0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, c),
        Some(wayland_protocols::wp::viewporter::client::wp_viewport::Error::OutOfBuffer as u32)
    );
}

#[test]
fn synchronised_subsurfaces_apply_on_the_parent_commit() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 100, 100, RED);
    let child = g.compositor.create_surface(&h.c(c).qh, ());
    let sub = g
        .subcompositor
        .get_subsurface(&child, &w.surface, &h.c(c).qh, ());
    sub.set_position(10, 20);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 30, 30, RED);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    h.roundtrip();
    assert_eq!(
        h.server.window_parts(id).len(),
        1,
        "applied before parent commit"
    );
    w.surface.commit();
    h.roundtrip();
    let parts = h.server.window_parts(id);
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[1].rect,
        Rect::new(Point::new(10.0, 20.0), Size::new(30.0, 30.0))
    );

    // Desynchronised: applies at once.
    sub.set_desync();
    child.attach(None, 0, 0);
    child.commit();
    h.roundtrip();
    assert_eq!(h.server.window_parts(id).len(), 1);
}

#[test]
fn place_below_draws_the_child_first() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 100, 100, RED);
    let child = g.compositor.create_surface(&h.c(c).qh, ());
    let sub = g
        .subcompositor
        .get_subsurface(&child, &w.surface, &h.c(c).qh, ());
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 30, 30, RED);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    sub.place_below(&w.surface);
    w.surface.commit();
    h.roundtrip();
    let parts = h.server.window_parts(id);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].rect.size, Size::new(30.0, 30.0), "child first");
}

#[test]
fn a_parent_cannot_be_its_own_descendant() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let a = g.compositor.create_surface(&h.c(c).qh, ());
    let b = g.compositor.create_surface(&h.c(c).qh, ());
    let _sub = g.subcompositor.get_subsurface(&b, &a, &h.c(c).qh, ());
    let _bad = g.subcompositor.get_subsurface(&a, &b, &h.c(c).qh, ());
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, c),
        Some(wayland_client::protocol::wl_subcompositor::Error::BadParent as u32)
    );
}

// --- T015: xdg-shell and output ------------------------------------------------------------

#[test]
fn first_configure_is_zero_sized_and_activated() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let _w = create_window(&mut h, c, &g);
    let configures = h.c(c).events::<xdg_toplevel::Event>();
    let (width, height, states) = configures
        .iter()
        .find_map(|(_, e)| match e {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => Some((*width, *height, states.clone())),
            _ => None,
        })
        .expect("toplevel configure");
    assert_eq!((width, height), (0, 0));
    let activated = (xdg_toplevel::State::Activated as u32).to_ne_bytes();
    assert!(states.chunks(4).any(|s| s == activated));
}

#[test]
fn a_buffer_before_the_ack_is_unconfigured_buffer() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 64, 32, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, c),
        Some(xdg_surface::Error::UnconfiguredBuffer as u32)
    );
}

#[test]
fn mapped_size_uses_window_geometry_and_updates_keep_running() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    ack(&mut h, c, &w);
    w.xdg.set_window_geometry(10, 10, 80, 60);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 100, 80, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    let id = match &events[..] {
        [WindowEvent::Mapped { id, size, .. }] => {
            assert_eq!(*size, Size::new(80.0, 60.0));
            *id
        }
        other => panic!("{other:?}"),
    };
    // The surface starts at -10, -10 in window coordinates.
    assert_eq!(
        h.server.window_parts(id)[0].rect,
        Rect::new(Point::new(-10.0, -10.0), Size::new(100.0, 80.0))
    );

    w.xdg.set_window_geometry(0, 0, 200, 150);
    let (bigger, _pool2, _file2) = solid_buffer(h.c(c), &g.shm, 200, 150, RED);
    w.surface.attach(Some(&bigger), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    assert!(
        matches!(&events[..], [WindowEvent::Updated { id: i, size, .. }] if *i == id && *size == Size::new(200.0, 150.0)),
        "{events:?}"
    );
}

#[test]
fn null_buffer_and_destroy_unmap() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    w.surface.attach(None, 0, 0);
    w.surface.commit();
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![WindowEvent::Unmapped { id }]
    );

    let (w2, id2) = open_window(&mut h, c, &g, 64, 64, RED);
    w2.toplevel.destroy();
    w2.xdg.destroy();
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![WindowEvent::Unmapped { id: id2 }]
    );
    assert_ne!(id, id2, "window ids are not reused");
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn popups_are_dismissed_and_wm_requests_ignored() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, _id) = open_window(&mut h, c, &g, 64, 64, RED);
    let qh = h.c(c).qh.clone();
    let positioner = g.wm_base.create_positioner(&qh, ());
    positioner.set_size(10, 10);
    positioner.set_anchor_rect(0, 0, 1, 1);
    let popup_surface = g.compositor.create_surface(&qh, ());
    let popup_xdg = g.wm_base.get_xdg_surface(&popup_surface, &qh, ());
    let _popup = popup_xdg.get_popup(Some(&w.xdg), &positioner, &qh, ());
    w.toplevel.set_maximized();
    w.toplevel.set_fullscreen(None);
    w.toplevel.set_minimized();
    w.toplevel.set_title("t".into());
    w.toplevel.set_app_id("a".into());
    h.roundtrip();
    assert!(!h.c(c).is_disconnected(), "{:?}", h.c(c).error());
    assert!(
        h.c(c)
            .events::<xdg_popup::Event>()
            .iter()
            .any(|(_, e)| matches!(e, xdg_popup::Event::PopupDone))
    );
}

#[test]
fn output_reports_the_viewer_and_follows_scale_changes() {
    let mut h = Harness::new();
    h.server.set_viewer((1600, 1000), 1.25);
    let c = h.connect();
    let output: WlOutput = h.c(c).bind(4);
    h.roundtrip();
    let events = h.c(c).events::<wl_output::Event>();
    assert!(
        events
            .iter()
            .any(|(_, e)| matches!(e, wl_output::Event::Name { name } if name == "RUBICKWAY-1"))
    );
    assert!(events.iter().any(|(_, e)| matches!(
        e,
        wl_output::Event::Mode {
            width: 1600,
            height: 1000,
            ..
        }
    )));
    assert!(
        events
            .iter()
            .any(|(_, e)| matches!(e, wl_output::Event::Scale { factor: 2 }))
    );

    // A surface with fractional scale hears about changes.
    let g = globals(h.c(c));
    let fractional: WpFractionalScaleManagerV1 = h.c(c).bind(1);
    let (w, _id) = open_window(&mut h, c, &g, 64, 64, RED);
    let _fs = fractional.get_fractional_scale(&w.surface, &h.c(c).qh, ());
    h.roundtrip();
    h.c(c).clear_events();
    h.server.set_viewer((2000, 1200), 1.5);
    h.roundtrip();
    let c_ref = h.c(c);
    assert!(
        c_ref
            .events::<wp_fractional_scale_v1::Event>()
            .iter()
            .any(|(_, e)| matches!(
                e,
                wp_fractional_scale_v1::Event::PreferredScale { scale: 180 }
            ))
    );
    assert!(
        c_ref
            .events::<wl_surface::Event>()
            .iter()
            .any(|(_, e)| { matches!(e, wl_surface::Event::PreferredBufferScale { factor: 2 }) })
    );
    assert!(
        c_ref
            .events::<wl_output::Event>()
            .iter()
            .any(|(_, e)| matches!(e, wl_output::Event::Mode { width: 2000, .. }))
    );
    let _ = output;
}

// --- T016: clipboard stub ------------------------------------------------------------------

#[test]
fn clipboard_stub_accepts_everything_and_offers_nothing() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let manager: WlDataDeviceManager = h.c(c).bind(3);
    let seat: WlSeat = h.c(c).bind(8);
    let qh = h.c(c).qh.clone();
    let device = manager.get_data_device(&seat, &qh, ());
    let source = manager.create_data_source(&qh, ());
    source.offer("text/plain".into());
    device.set_selection(Some(&source), 1);
    let drag_source: WlDataSource = manager.create_data_source(&qh, ());
    let origin = g.compositor.create_surface(&qh, ());
    device.start_drag(Some(&drag_source), &origin, None, 1);
    h.roundtrip();
    let c_ref = h.c(c);
    assert!(!c_ref.is_disconnected(), "{:?}", c_ref.error());
    assert!(
        c_ref
            .events::<wayland_client::protocol::wl_data_device::Event>()
            .is_empty()
    );
    assert!(
        c_ref
            .events::<wl_data_source::Event>()
            .iter()
            .any(|(id, e)| *id == drag_source.id()
                && matches!(e, wl_data_source::Event::Cancelled))
    );
    source.destroy();
    device.release();
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
}

// --- T017: frame pacing --------------------------------------------------------------------

#[test]
fn new_content_is_uploaded_for_the_next_frame() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 4, 2, RED);
    h.server.take_dirty();
    let mut seen = Vec::new();
    h.server
        .upload_pending(|_, image| seen.push(image.bytes.to_vec()));
    assert_eq!(seen.len(), 1);
    assert_eq!(&seen[0][..4], &RED.to_le_bytes());

    let blue = 0xff00_00ff_u32;
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 4, 2, blue);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.damage_buffer(0, 0, 4, 2);
    w.surface.commit();
    h.roundtrip();
    assert!(h.server.take_dirty());
    let mut seen = Vec::new();
    h.server
        .upload_pending(|_, image| seen.push(image.bytes.to_vec()));
    assert_eq!(&seen[0][..4], &blue.to_le_bytes());
    assert!(h.server.window_parts(id)[0].has_content);
    // The buffer was released after the copy.
    h.roundtrip();
    assert!(
        h.c(c)
            .events::<wl_buffer::Event>()
            .iter()
            .any(|(bid, e)| *bid == buffer.id() && matches!(e, wl_buffer::Event::Release))
    );
}

#[test]
fn frame_callbacks_complete_only_for_presented_windows() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w1, id1) = open_window(&mut h, c, &g, 10, 10, RED);
    let (w2, id2) = open_window(&mut h, c, &g, 10, 10, RED);
    let qh = h.c(c).qh.clone();
    let cb1: WlCallback = w1.surface.frame(&qh, ());
    let cb2: WlCallback = w2.surface.frame(&qh, ());
    w1.surface.commit();
    w2.surface.commit();
    h.roundtrip();
    assert!(h.server.has_pending_callbacks(&[id1]));

    // Not presented yet (viewer hidden): nothing completes.
    h.roundtrip();
    assert!(h.c(c).events::<wl_callback::Event>().is_empty());

    // Only window 1 was in view.
    h.server.presented(1234, &[id1]);
    h.roundtrip();
    let done: Vec<_> = h
        .c(c)
        .events::<wl_callback::Event>()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(done, vec![cb1.id()]);
    assert!(!h.server.has_pending_callbacks(&[id1]));
    assert!(h.server.has_pending_callbacks(&[id2]));
    let _ = cb2;
}
