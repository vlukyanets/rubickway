//! Input delivery to applications (research R11): keymap, focus, keys, pointer, scrolling.

use std::os::unix::fs::FileExt;

use rubickway::canvas::{Point, Vector};
use rubickway::input::{RawModifiers, ScrollSource};
use rubickway::shell::{AppCursor, ClientInput, CursorName, WindowEvent, WindowId};
use wayland_client::Proxy;
use wayland_client::WEnum;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols::xdg::shell::client::xdg_toplevel;

use crate::harness::{Harness, solid_buffer};
use crate::surfaces::{globals, open_window};

const RED: u32 = 0xffff_0000;
const KEYMAP: &str = "xkb_keymap { a test keymap };";

fn activated(states: &[u8]) -> bool {
    let a = (xdg_toplevel::State::Activated as u32).to_ne_bytes();
    states.chunks(4).any(|s| s == a)
}

#[test]
fn keyboards_get_the_host_keymap_and_repeat_info() {
    let mut h = Harness::new();
    h.server.set_keymap(KEYMAP);
    let c = h.connect();
    let seat: WlSeat = h.c(c).bind(8);
    let _keyboard: WlKeyboard = seat.get_keyboard(&h.c(c).qh, ());
    h.roundtrip();
    h.server.set_repeat(30, 400);
    h.roundtrip();
    let client = h.c(c);
    let events = client.events::<wl_keyboard::Event>();
    let (fd, size) = events
        .iter()
        .find_map(|(_, e)| match e {
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => Some((fd.try_clone().expect("dup keymap fd"), *size)),
            _ => None,
        })
        .expect("keymap event");
    assert_eq!(size as usize, KEYMAP.len() + 1);
    let mut text = vec![0u8; size as usize];
    std::fs::File::from(fd)
        .read_exact_at(&mut text, 0)
        .expect("read keymap");
    assert_eq!(&text[..KEYMAP.len()], KEYMAP.as_bytes());
    let repeats: Vec<(i32, i32)> = events
        .iter()
        .filter_map(|(_, e)| match e {
            wl_keyboard::Event::RepeatInfo { rate, delay } => Some((*rate, *delay)),
            _ => None,
        })
        .collect();
    assert_eq!(repeats, vec![(25, 600), (30, 400)]);
}

#[test]
fn focus_keys_and_modifiers_reach_the_focused_window() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let keyboard: WlKeyboard = seat.get_keyboard(&h.c(c).qh, ());
    let (w, id) = open_window(&mut h, c, &g, 50, 50, RED);
    h.c(c).clear_events();

    h.server.deliver(
        &[
            ClientInput::Key {
                window: id,
                keycode: 30,
                pressed: true,
            },
            ClientInput::KeyboardFocus(Some(id)),
            ClientInput::Modifiers {
                window: id,
                raw: RawModifiers {
                    depressed: 8,
                    latched: 0,
                    locked: 2,
                    group: 1,
                },
            },
            ClientInput::Key {
                window: id,
                keycode: 30,
                pressed: false,
            },
        ],
        100,
    );
    h.roundtrip();
    let client = h.c(c);
    let events: Vec<_> = client
        .events::<wl_keyboard::Event>()
        .into_iter()
        .filter(|(oid, _)| *oid == keyboard.id())
        .map(|(_, e)| e)
        .collect();
    assert!(
        events.iter().any(
            |e| matches!(e, wl_keyboard::Event::Enter { surface, keys, .. }
            if *surface == w.surface && keys == &30u32.to_ne_bytes().to_vec())
        ),
        "enter with the pressed key"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        wl_keyboard::Event::Modifiers {
            mods_depressed: 8,
            mods_locked: 2,
            group: 1,
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        wl_keyboard::Event::Key {
            key: 30,
            state: WEnum::Value(wl_keyboard::KeyState::Released),
            time: 100,
            ..
        }
    )));
    // Focus moved to the window: its configure says activated (it already was at map time,
    // so look at the latest configure).
    let last = client
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .filter_map(|(_, e)| match e {
            xdg_toplevel::Event::Configure { states, .. } => Some(states.clone()),
            _ => None,
        })
        .next_back();
    assert!(last.is_none_or(|s| activated(&s)));

    h.server.deliver(&[ClientInput::KeyboardFocus(None)], 200);
    h.roundtrip();
    let client = h.c(c);
    assert!(
        client
            .events::<wl_keyboard::Event>()
            .iter()
            .any(|(_, e)| matches!(e, wl_keyboard::Event::Leave { .. }))
    );
    let last = client
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .filter_map(|(_, e)| match e {
            xdg_toplevel::Event::Configure { states, .. } => Some(states.clone()),
            _ => None,
        })
        .next_back()
        .expect("a configure after losing focus");
    assert!(!activated(&last), "deactivated after losing focus");
}

#[test]
fn pointer_events_use_surface_local_coordinates() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let _pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let (w, id) = open_window(&mut h, c, &g, 100, 100, RED);
    // A 20x20 subsurface at (60, 60).
    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    sub.set_position(60, 60);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 20, 20, RED);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    w.surface.commit();
    h.roundtrip();
    h.c(c).clear_events();

    let frame = ClientInput::PointerFrame { window: id };
    h.server.deliver(
        &[
            ClientInput::PointerEnter {
                window: id,
                pos: Point::new(10.0, 20.0),
            },
            frame.clone(),
            ClientInput::PointerButton {
                window: id,
                code: 0x110,
                pressed: true,
            },
            frame.clone(),
            ClientInput::Scroll {
                window: id,
                source: ScrollSource::Wheel,
                delta: Vector::new(0.0, 15.0),
                value120: Some(Vector::new(0.0, 120.0)),
            },
            frame.clone(),
            ClientInput::PointerButton {
                window: id,
                code: 0x110,
                pressed: false,
            },
            frame.clone(),
            // Into the subsurface: leave the parent, enter the child at (5, 5).
            ClientInput::PointerMotion {
                window: id,
                pos: Point::new(65.0, 65.0),
            },
            frame.clone(),
            ClientInput::PointerMotion {
                window: id,
                pos: Point::new(70.0, 66.0),
            },
            frame,
        ],
        500,
    );
    h.roundtrip();
    let events: Vec<_> = h
        .c(c)
        .events::<wl_pointer::Event>()
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    let pos = |e: &wl_pointer::Event| match e {
        wl_pointer::Event::Enter {
            surface,
            surface_x,
            surface_y,
            ..
        } => Some((surface.clone(), *surface_x, *surface_y)),
        _ => None,
    };
    let enters: Vec<_> = events.iter().filter_map(|e| pos(e)).collect();
    assert_eq!(enters.len(), 2, "{events:?}");
    assert_eq!(enters[0], (w.surface.clone(), 10.0, 20.0));
    assert_eq!(enters[1], (child.clone(), 5.0, 5.0));
    assert!(events.iter().any(|e| matches!(
        e,
        wl_pointer::Event::Button {
            button: 0x110,
            time: 500,
            state: WEnum::Value(wl_pointer::ButtonState::Pressed),
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        wl_pointer::Event::AxisSource {
            axis_source: WEnum::Value(wl_pointer::AxisSource::Wheel)
        }
    )));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, wl_pointer::Event::AxisValue120 { value120: 120, .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, wl_pointer::Event::Axis { value, .. } if *value == 15.0))
    );
    assert!(events.iter().any(|e| matches!(
        e,
        wl_pointer::Event::Motion { surface_x, surface_y, .. } if *surface_x == 10.0 && *surface_y == 6.0
    )));
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, wl_pointer::Event::Frame))
            .count()
            >= 6
    );
}

#[test]
fn a_disconnecting_focused_client_leaves_input_free() {
    let mut h = Harness::new();
    let c1 = h.connect();
    let g1 = globals(h.c(c1));
    let seat1: WlSeat = h.c(c1).bind(8);
    let _k1: WlKeyboard = seat1.get_keyboard(&h.c(c1).qh, ());
    let _p1: WlPointer = seat1.get_pointer(&h.c(c1).qh, ());
    let (_w1, id1) = open_window(&mut h, c1, &g1, 50, 50, RED);
    h.server.deliver(
        &[
            ClientInput::KeyboardFocus(Some(id1)),
            ClientInput::PointerEnter {
                window: id1,
                pos: Point::new(5.0, 5.0),
            },
            ClientInput::PointerButton {
                window: id1,
                code: 0x110,
                pressed: true,
            },
        ],
        1,
    );
    h.roundtrip();
    let gone = h.clients.remove(c1);
    drop(gone);
    h.roundtrip();

    let c2 = h.connect();
    let g2 = globals(h.c(c2));
    let seat2: WlSeat = h.c(c2).bind(8);
    let k2: WlKeyboard = seat2.get_keyboard(&h.c(c2).qh, ());
    let (_w2, id2) = open_window(&mut h, c2, &g2, 50, 50, RED);
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(id2))], 2);
    h.roundtrip();
    let client = h.c(c2);
    assert!(!client.is_disconnected());
    assert!(
        client
            .events::<wl_keyboard::Event>()
            .iter()
            .any(|(oid, e)| *oid == k2.id() && matches!(e, wl_keyboard::Event::Enter { .. }))
    );
}

// --- Standalone: applications hiding the pointer (FR-008) ------------------------------------

/// Enters window `id` at (10, 10) and returns the serial of the `wl_pointer.enter` client `c` got.
pub fn enter_pointer(h: &mut Harness, c: usize, id: WindowId) -> u32 {
    h.c(c).clear_events();
    h.server.deliver(
        &[
            ClientInput::PointerEnter {
                window: id,
                pos: Point::new(10.0, 10.0),
            },
            ClientInput::PointerFrame { window: id },
        ],
        1,
    );
    h.roundtrip();
    h.c(c)
        .events::<wl_pointer::Event>()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            wl_pointer::Event::Enter { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("an enter")
}

fn cursor_changed(id: WindowId, cursor: AppCursor) -> WindowEvent {
    WindowEvent::CursorChanged { id, cursor }
}

#[test]
fn set_cursor_without_a_surface_hides_the_pointer_over_the_clients_windows() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let (_w, id) = open_window(&mut h, c, &g, 50, 50, RED);
    h.server.take_window_events();

    // Before any enter: not counted (cursor-shapes research C2).
    pointer.set_cursor(0, None, 0, 0);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());

    let serial = enter_pointer(&mut h, c, id);
    h.server.take_window_events();
    pointer.set_cursor(serial, None, 0, 0);
    h.roundtrip();
    let events = h.server.take_window_events();
    assert_eq!(events, vec![cursor_changed(id, AppCursor::Hidden)]);

    // Asking again changes nothing; a cursor image shows the arrow again.
    pointer.set_cursor(serial, None, 0, 0);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());
    let image = g.compositor.create_surface(&h.c(c).qh, ());
    pointer.set_cursor(serial, Some(&image), 0, 0);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![cursor_changed(id, AppCursor::Shape(CursorName::Default))]
    );
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn set_cursor_counts_only_with_the_latest_enter_serial_while_the_pointer_is_there() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let (_w, id) = open_window(&mut h, c, &g, 50, 50, RED);
    let old = enter_pointer(&mut h, c, id);
    let serial = enter_pointer(&mut h, c, id);
    h.server.take_window_events();

    // A stale or made-up serial: ignored.
    pointer.set_cursor(old, None, 0, 0);
    pointer.set_cursor(serial.wrapping_add(1), None, 0, 0);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());

    // Hidden, then a new enter: the arrow again until the application asks.
    pointer.set_cursor(serial, None, 0, 0);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![cursor_changed(id, AppCursor::Hidden)]
    );
    let serial = enter_pointer(&mut h, c, id);
    assert_eq!(
        h.server.take_window_events(),
        vec![cursor_changed(id, AppCursor::Shape(CursorName::Default))]
    );

    // After the pointer left: ignored, even with the latest serial.
    h.server
        .deliver(&[ClientInput::PointerLeave { window: id }], 2);
    h.roundtrip();
    pointer.set_cursor(serial, None, 0, 0);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());

    // A surface with another role is still an error.
    let (w2, _id2) = open_window(&mut h, c, &g, 20, 20, RED);
    pointer.set_cursor(serial, Some(&w2.surface), 0, 0);
    h.roundtrip();
    assert!(h.c(c).is_disconnected());
}
