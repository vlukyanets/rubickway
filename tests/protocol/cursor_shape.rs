//! `wp_cursor_shape_manager_v1` (cursor-shapes contracts/protocols.md, research C1-C4): which
//! shape requests count, and the objects' lifecycles. Protocol errors are in `misbehaving.rs`.

use rubickway::canvas::Point;
use rubickway::shell::{AppCursor, ClientInput, CursorName, WindowEvent, WindowId};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Proxy, WEnum};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    self, Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;

use crate::harness::{Harness, solid_buffer};
use crate::input::enter_pointer;
use crate::surfaces::{Globals, Window, globals, open_window};

const RED: u32 = 0xffff_0000;

/// A client with a 100x100 window, a pointer and a cursor shape device for it.
pub struct ShapeClient {
    pub index: usize,
    pub g: Globals,
    pub manager: WpCursorShapeManagerV1,
    pub pointer: WlPointer,
    pub device: WpCursorShapeDeviceV1,
    pub window: Window,
    pub id: WindowId,
}

pub fn shape_client(h: &mut Harness, version: u32) -> ShapeClient {
    let index = h.connect();
    let g = globals(h.c(index));
    let seat: WlSeat = h.c(index).bind(8);
    let qh = h.c(index).qh.clone();
    let pointer = seat.get_pointer(&qh, ());
    let manager: WpCursorShapeManagerV1 = h.c(index).bind(version);
    let device = manager.get_pointer(&pointer, &qh, ());
    let (window, id) = open_window(h, index, &g, 100, 100, RED);
    h.server.take_window_events();
    ShapeClient {
        index,
        g,
        manager,
        pointer,
        device,
        window,
        id,
    }
}

pub fn shape(id: WindowId, name: CursorName) -> WindowEvent {
    WindowEvent::CursorChanged {
        id,
        cursor: AppCursor::Shape(name),
    }
}

/// Sends `set_shape` with a raw value, which may be outside the enum.
pub fn set_raw_shape(device: &WpCursorShapeDeviceV1, serial: u32, value: u32) {
    device
        .send_request(wp_cursor_shape_device_v1::Request::SetShape {
            serial,
            shape: WEnum::Unknown(value),
        })
        .expect("send");
}

#[test]
fn the_global_is_offered_at_version_2() {
    let mut h = Harness::new();
    let c = h.connect();
    h.roundtrip();
    let globals = &h.c(c).state.globals;
    assert!(
        globals
            .iter()
            .any(|(_, name, v)| name == "wp_cursor_shape_manager_v1" && *v == 2),
        "{globals:?}"
    );
}

#[test]
fn a_shape_named_with_the_enter_serial_reaches_the_shell_once() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.device.set_shape(serial, Shape::Text);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(s.id, CursorName::Text)]
    );
    s.device.set_shape(serial, Shape::Text);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty(), "no change");
    assert!(!h.c(s.index).is_disconnected());
}

#[test]
fn every_protocol_value_names_its_shape() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    // From 2 up, then 1 (the arrow is what an enter starts with, so 1 first would change nothing).
    for value in (2..=36).chain([1]) {
        set_raw_shape(&s.device, serial, value);
        h.roundtrip();
        let expected = CursorName::ALL[value as usize - 1];
        assert_eq!(
            h.server.take_window_events(),
            vec![shape(s.id, expected)],
            "value {value}"
        );
    }
    assert!(!h.c(s.index).is_disconnected());
}

#[test]
fn requests_without_the_pointer_or_with_a_stale_serial_are_ignored() {
    let mut h = Harness::new();
    let a = shape_client(&mut h, 2);
    let b = shape_client(&mut h, 2);
    let old = enter_pointer(&mut h, a.index, a.id);
    let serial = enter_pointer(&mut h, a.index, a.id);
    h.server.take_window_events();

    // A stale serial.
    a.device.set_shape(old, Shape::Text);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());

    // Another client, while the pointer is over the first one's window, even with that serial.
    a.device.set_shape(serial, Shape::Pointer);
    b.device.set_shape(serial, Shape::Wait);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(a.id, CursorName::Pointer)]
    );

    // After the pointer left.
    h.server
        .deliver(&[ClientInput::PointerLeave { window: a.id }], 2);
    h.roundtrip();
    a.device.set_shape(serial, Shape::Help);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());
    assert!(!h.c(a.index).is_disconnected());
    assert!(!h.c(b.index).is_disconnected());
}

#[test]
fn a_device_whose_pointer_is_gone_is_inert() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.pointer.release();
    h.roundtrip();
    s.device.set_shape(serial, Shape::Text);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());
    assert!(!h.c(s.index).is_disconnected());
}

#[test]
fn destroying_the_device_or_the_manager_keeps_what_was_asked_for() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.device.set_shape(serial, Shape::Text);
    s.manager.destroy();
    s.device.destroy();
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(s.id, CursorName::Text)],
        "the shape stays"
    );
    assert!(!h.c(s.index).is_disconnected());
}

#[test]
fn a_device_made_before_its_manager_was_destroyed_keeps_working() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    s.manager.destroy();
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.device.set_shape(serial, Shape::Crosshair);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(s.id, CursorName::Crosshair)]
    );
}

#[test]
fn the_latest_request_counts_across_devices_and_set_cursor() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    let qh = h.c(s.index).qh.clone();
    let second: WpCursorShapeManagerV1 = h.c(s.index).bind(2);
    let other = second.get_pointer(&s.pointer, &qh, ());
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.device.set_shape(serial, Shape::Text);
    other.set_shape(serial, Shape::Pointer);
    s.pointer.set_cursor(serial, None, 0, 0);
    s.device.set_shape(serial, Shape::ZoomIn);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![
            shape(s.id, CursorName::Text),
            shape(s.id, CursorName::Pointer),
            WindowEvent::CursorChanged {
                id: s.id,
                cursor: AppCursor::Hidden
            },
            shape(s.id, CursorName::ZoomIn),
        ]
    );
}

#[test]
fn entering_a_subsurface_of_the_same_window_starts_again_from_the_arrow() {
    let mut h = Harness::new();
    let s = shape_client(&mut h, 2);
    // A 20x20 subsurface at (60, 60).
    let qh = h.c(s.index).qh.clone();
    let parent = &s.window.surface;
    let child = s.g.compositor.create_surface(&qh, ());
    let sub = s.g.subcompositor.get_subsurface(&child, parent, &qh, ());
    sub.set_position(60, 60);
    let (buffer, _pool, _file) = solid_buffer(h.c(s.index), &s.g.shm, 20, 20, RED);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    parent.commit();
    h.roundtrip();
    let serial = enter_pointer(&mut h, s.index, s.id);
    h.server.take_window_events();
    s.device.set_shape(serial, Shape::Text);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(s.id, CursorName::Text)]
    );
    h.server.deliver(
        &[
            ClientInput::PointerMotion {
                window: s.id,
                pos: Point::new(65.0, 65.0),
            },
            ClientInput::PointerFrame { window: s.id },
        ],
        3,
    );
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![shape(s.id, CursorName::Default)]
    );
}
