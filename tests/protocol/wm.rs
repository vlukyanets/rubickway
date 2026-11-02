//! Window-management requests and configures of `xdg_toplevel` (contracts/protocols.md).

use rubickway::canvas::{Point, Size};
use rubickway::shell::{
    ClientInput, Edges, SizeLimits, States, WindowCommand, WindowEvent, WindowId,
};
use wayland_client::Proxy;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols::xdg::shell::client::xdg_toplevel;

use crate::harness::{Harness, solid_buffer};
use crate::surfaces::{Globals, Window, ack, create_window, globals, open_window};

const RED: u32 = 0xffff_0000;

/// The toplevel configures `w` received, oldest first: (width, height, states).
fn configures(h: &mut Harness, ci: usize, w: &Window) -> Vec<(i32, i32, Vec<u32>)> {
    h.c(ci)
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .filter_map(|(id, e)| match e {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } if id == w.toplevel.id() => Some((*width, *height, decode(states))),
            _ => None,
        })
        .collect()
}

fn decode(states: &[u8]) -> Vec<u32> {
    states
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_ne_bytes(*c))
        .collect()
}

fn state(s: xdg_toplevel::State) -> u32 {
    s as u32
}

fn configure(window: WindowId, w: f64, h: f64, states: States) -> WindowCommand {
    WindowCommand::Configure {
        window,
        size: Some(Size::new(w, h)),
        states,
    }
}

fn maximized() -> States {
    States {
        maximized: true,
        ..States::default()
    }
}

/// Commits the current buffer again (no new content).
fn commit(h: &mut Harness, w: &Window) {
    w.surface.commit();
    h.roundtrip();
}

#[test]
fn configure_carries_the_wanted_size_and_states() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    h.server
        .command(&[configure(id, 300.0, 200.0, maximized())]);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    assert_eq!(got.len(), 1, "{got:?}");
    let (width, height, states) = &got[0];
    assert_eq!((*width, *height), (300, 200));
    assert!(states.contains(&state(xdg_toplevel::State::Maximized)));
    assert!(states.contains(&state(xdg_toplevel::State::Activated)));
    assert!(!states.contains(&state(xdg_toplevel::State::Fullscreen)));
}

#[test]
fn new_sizes_are_coalesced_while_a_configure_is_unacked() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    h.server
        .command(&[configure(id, 100.0, 100.0, States::default())]);
    h.roundtrip();
    h.server
        .command(&[configure(id, 200.0, 200.0, States::default())]);
    h.server
        .command(&[configure(id, 300.0, 300.0, States::default())]);
    h.roundtrip();
    let sizes: Vec<_> = configures(&mut h, c, &w)
        .iter()
        .map(|(w, h, _)| (*w, *h))
        .collect();
    assert_eq!(sizes, vec![(100, 100)], "only one size outstanding");
    ack(&mut h, c, &w);
    h.roundtrip();
    let sizes: Vec<_> = configures(&mut h, c, &w)
        .iter()
        .map(|(w, h, _)| (*w, *h))
        .collect();
    assert_eq!(
        sizes,
        vec![(100, 100), (300, 300)],
        "the newest follows the ack"
    );
}

#[test]
fn a_focus_change_resends_the_wanted_state() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server
        .command(&[configure(id, 300.0, 200.0, maximized())]);
    h.roundtrip();
    ack(&mut h, c, &w);
    h.server.deliver(&[ClientInput::KeyboardFocus(Some(id))], 0);
    h.roundtrip();
    h.c(c).clear_events();
    h.server.deliver(&[ClientInput::KeyboardFocus(None)], 0);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    let (width, height, states) = got.last().expect("a configure");
    assert_eq!((*width, *height), (300, 200));
    assert!(states.contains(&state(xdg_toplevel::State::Maximized)));
    assert!(!states.contains(&state(xdg_toplevel::State::Activated)));
}

#[test]
fn size_none_asks_for_zero() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    h.server.command(&[WindowCommand::Configure {
        window: id,
        size: None,
        states: maximized(),
    }]);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    assert_eq!((got[0].0, got[0].1), (0, 0));
}

#[test]
fn applying_the_shells_configure_reports_configured() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    // An activation-only configure, acked and committed: no event.
    h.server.deliver(&[ClientInput::KeyboardFocus(Some(id))], 0);
    h.server.deliver(&[ClientInput::KeyboardFocus(None)], 0);
    h.roundtrip();
    ack(&mut h, c, &w);
    commit(&mut h, &w);
    assert!(h.server.take_window_events().is_empty());
    // The shell's configure, acked and committed without a size change: configured.
    h.server.command(&[configure(id, 64.0, 64.0, maximized())]);
    h.roundtrip();
    commit(&mut h, &w);
    let before_ack = h.server.take_window_events();
    assert!(
        !before_ack.iter().any(|e| matches!(
            e,
            WindowEvent::Updated {
                configured: true,
                ..
            }
        )),
        "{before_ack:?}"
    );
    ack(&mut h, c, &w);
    commit(&mut h, &w);
    let events = h.server.take_window_events();
    assert!(
        matches!(&events[..], [WindowEvent::Updated { id: i, configured: true, .. }] if *i == id),
        "{events:?}"
    );
    // Nothing new: no further event.
    commit(&mut h, &w);
    assert!(h.server.take_window_events().is_empty());
}

fn limits_of(events: &[WindowEvent]) -> Option<SizeLimits> {
    events.iter().rev().find_map(|e| match e {
        WindowEvent::Mapped { limits, .. } | WindowEvent::Updated { limits, .. } => Some(*limits),
        _ => None,
    })
}

#[test]
fn size_limits_are_double_buffered_and_reported() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    w.toplevel.set_min_size(100, 50);
    w.toplevel.set_max_size(0, 400);
    ack(&mut h, c, &w);
    let (buffer, _pool, _file) = solid_buffer(h.c(c), &g.shm, 64, 64, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    assert_eq!(
        limits_of(&events),
        Some(SizeLimits {
            min: Size::new(100.0, 50.0),
            max: Size::new(0.0, 400.0),
        })
    );
    // Not applied before the next commit.
    w.toplevel.set_min_size(0, 0);
    h.roundtrip();
    assert!(h.server.take_window_events().is_empty());
    // A limits-only change is reported.
    commit(&mut h, &w);
    let events = h.server.take_window_events();
    assert_eq!(
        limits_of(&events),
        Some(SizeLimits {
            min: Size::new(0.0, 0.0),
            max: Size::new(0.0, 400.0),
        })
    );
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn negative_size_limits_are_invalid_size() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, _id) = open_window(&mut h, c, &g, 64, 64, RED);
    w.toplevel.set_max_size(-1, 10);
    h.roundtrip();
    let error = h.c(c).error().expect("disconnected");
    assert_eq!(error.code, xdg_toplevel::Error::InvalidSize as u32);
}

// --- US1: interactive move ---------------------------------------------------------------------

const BTN_LEFT: u32 = 0x110;

/// Delivers a press (or release) of the left button on `window`.
fn press(h: &mut Harness, window: WindowId, pressed: bool) {
    h.server.deliver(
        &[
            ClientInput::PointerEnter {
                window,
                pos: Point::new(5.0, 5.0),
            },
            ClientInput::PointerButton {
                window,
                code: BTN_LEFT,
                pressed,
            },
            ClientInput::PointerFrame { window },
        ],
        0,
    );
    h.roundtrip();
}

/// The serial of the latest `wl_pointer.button` the client received.
fn last_button_serial(h: &mut Harness, ci: usize) -> u32 {
    h.c(ci)
        .events::<wl_pointer::Event>()
        .into_iter()
        .rev()
        .find_map(|(_, e)| match e {
            wl_pointer::Event::Button { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("a button event")
}

fn move_requests(events: &[WindowEvent]) -> Vec<WindowId> {
    events
        .iter()
        .filter_map(|e| match e {
            WindowEvent::MoveRequested { id } => Some(*id),
            _ => None,
        })
        .collect()
}

#[test]
fn a_move_with_the_latest_press_serial_is_reported() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let _pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    press(&mut h, id, true);
    let serial = last_button_serial(&mut h, c);
    w.toplevel._move(&seat, serial);
    h.roundtrip();
    assert_eq!(move_requests(&h.server.take_window_events()), vec![id]);
}

#[test]
fn moves_with_other_serials_are_ignored() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let _pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let _keyboard: WlKeyboard = seat.get_keyboard(&h.c(c).qh, ());
    let (w1, id1) = open_window(&mut h, c, &g, 64, 64, RED);
    let (_w2, id2) = open_window(&mut h, c, &g, 64, 64, RED);
    // An older press.
    press(&mut h, id1, true);
    let old = last_button_serial(&mut h, c);
    press(&mut h, id1, false);
    press(&mut h, id1, true);
    w1.toplevel._move(&seat, old);
    // A keyboard serial (the enter of a focus change).
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(id1))], 0);
    h.roundtrip();
    let key_serial = h
        .c(c)
        .events::<wl_keyboard::Event>()
        .into_iter()
        .rev()
        .find_map(|(_, e)| match e {
            wl_keyboard::Event::Enter { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("keyboard enter");
    w1.toplevel._move(&seat, key_serial);
    // The latest press, but on another window.
    press(&mut h, id1, false);
    press(&mut h, id2, true);
    let on_other = last_button_serial(&mut h, c);
    w1.toplevel._move(&seat, on_other);
    h.roundtrip();
    assert!(move_requests(&h.server.take_window_events()).is_empty());
    assert!(!h.c(c).is_disconnected());
}

// --- US2: interactive resize -------------------------------------------------------------------

fn resize_requests(events: &[WindowEvent]) -> Vec<(WindowId, Edges)> {
    events
        .iter()
        .filter_map(|e| match e {
            WindowEvent::ResizeRequested { id, edges } => Some((*id, *edges)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_resize_with_the_latest_press_serial_is_reported() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let seat: WlSeat = h.c(c).bind(8);
    let _pointer: WlPointer = seat.get_pointer(&h.c(c).qh, ());
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    press(&mut h, id, true);
    let serial = last_button_serial(&mut h, c);
    w.toplevel
        .resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    // `none` is a valid value but means nothing: ignored.
    w.toplevel
        .resize(&seat, serial, xdg_toplevel::ResizeEdge::None);
    h.roundtrip();
    assert_eq!(
        resize_requests(&h.server.take_window_events()),
        vec![(
            id,
            Edges {
                top: false,
                bottom: true,
                left: false,
                right: true,
            }
        )]
    );
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn an_unknown_resize_edge_disconnects_only_that_client() {
    let mut h = Harness::new();
    let bad = h.connect();
    let good = h.connect();
    let g = globals(h.c(bad));
    let g2 = globals(h.c(good));
    let seat: WlSeat = h.c(bad).bind(8);
    let (w, _id) = open_window(&mut h, bad, &g, 64, 64, RED);
    let (w2, id2) = open_window(&mut h, good, &g2, 64, 64, RED);
    // 3 is not in the resize_edge enum (top + bottom).
    w.toplevel
        .send_request(xdg_toplevel::Request::Resize {
            seat: seat.clone(),
            serial: 0,
            edges: wayland_client::WEnum::Unknown(3),
        })
        .expect("send");
    h.roundtrip();
    let error = h.c(bad).error().expect("disconnected");
    assert_eq!(error.code, xdg_toplevel::Error::InvalidResizeEdge as u32);
    // The other client still works.
    h.server
        .command(&[configure(id2, 100.0, 80.0, States::default())]);
    h.roundtrip();
    assert!(!h.c(good).is_disconnected());
    let got = configures(&mut h, good, &w2);
    assert_eq!(got.last().map(|(w, h, _)| (*w, *h)), Some((100, 80)));
}

#[test]
fn configures_during_a_resize_carry_the_resizing_state() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    let states = States {
        resizing: true,
        ..States::default()
    };
    h.server.command(&[configure(id, 120.0, 90.0, states)]);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    assert!(got[0].2.contains(&state(xdg_toplevel::State::Resizing)));
}

#[test]
fn a_client_that_never_acks_gets_one_size_configure() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    let states = States {
        resizing: true,
        ..States::default()
    };
    for i in 0..20 {
        h.server
            .command(&[configure(id, 100.0 + f64::from(i), 100.0, states)]);
        h.roundtrip();
    }
    assert_eq!(configures(&mut h, c, &w).len(), 1);
    assert!(!h.c(c).is_disconnected());
}

// --- US4: close -------------------------------------------------------------------------------

#[test]
fn close_is_sent_and_an_ignoring_client_stays() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.command(&[
        WindowCommand::Close { window: id },
        WindowCommand::Close {
            window: WindowId(9999),
        },
    ]);
    h.roundtrip();
    let closes = h
        .c(c)
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .filter(|(oid, e)| *oid == w.toplevel.id() && matches!(e, xdg_toplevel::Event::Close))
        .count();
    assert_eq!(closes, 1);
    // The client ignores it: still mapped and connected.
    commit(&mut h, &w);
    assert!(!h.c(c).is_disconnected());
    assert!(
        !h.server
            .take_window_events()
            .iter()
            .any(|e| matches!(e, WindowEvent::Unmapped { .. }))
    );
    assert!(!h.server.window_parts(id).is_empty());
}

// --- US5: maximize, fullscreen, initial state, parents -----------------------------------------

#[test]
fn state_requests_after_map_are_reported() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    w.toplevel.set_maximized();
    w.toplevel.unset_maximized();
    w.toplevel.set_fullscreen(None);
    w.toplevel.unset_fullscreen();
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![
            WindowEvent::MaximizeRequested { id, on: true },
            WindowEvent::MaximizeRequested { id, on: false },
            WindowEvent::FullscreenRequested { id, on: true },
            WindowEvent::FullscreenRequested { id, on: false },
        ]
    );
}

/// Creates a toplevel, runs `before` on it, then does the initial commit.
fn create_with(h: &mut Harness, ci: usize, g: &Globals, before: impl FnOnce(&Window)) -> Window {
    let c = h.c(ci);
    let surface = g.compositor.create_surface(&c.qh, ());
    let xdg = g.wm_base.get_xdg_surface(&surface, &c.qh, ());
    let toplevel = xdg.get_toplevel(&c.qh, ());
    let w = Window {
        surface,
        xdg,
        toplevel,
    };
    before(&w);
    w.surface.commit();
    h.roundtrip();
    w
}

/// Acks the latest configure, attaches a `width x height` buffer and commits.
fn map_now(h: &mut Harness, ci: usize, g: &Globals, w: &Window, width: i32, height: i32) {
    ack(h, ci, w);
    let (buffer, _pool, _file) = solid_buffer(h.c(ci), &g.shm, width, height, RED);
    w.surface.attach(Some(&buffer), 0, 0);
    w.surface.commit();
    h.roundtrip();
}

fn mapped_parent(events: &[WindowEvent]) -> Option<(WindowId, Option<WindowId>)> {
    events.iter().find_map(|e| match e {
        WindowEvent::Mapped { id, parent, .. } => Some((*id, *parent)),
        _ => None,
    })
}

#[test]
fn an_initial_state_holds_the_first_configure_for_the_shell() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_with(&mut h, c, &g, |w| w.toplevel.set_maximized());
    let events = h.server.take_window_events();
    let id = match &events[..] {
        [
            WindowEvent::Created {
                id,
                parent: None,
                maximized: true,
                fullscreen: false,
                ..
            },
        ] => *id,
        other => panic!("{other:?}"),
    };
    assert!(configures(&mut h, c, &w).is_empty(), "held for the shell");
    h.server
        .command(&[configure(id, 800.0, 600.0, maximized())]);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].0, got[0].1), (800, 600));
    assert!(got[0].2.contains(&state(xdg_toplevel::State::Maximized)));
    map_now(&mut h, c, &g, &w, 800, 600);
    assert_eq!(
        mapped_parent(&h.server.take_window_events()),
        Some((id, None)),
        "the same id"
    );
}

#[test]
fn a_toplevel_without_initial_state_is_configured_at_once() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    assert!(h.server.take_window_events().is_empty());
    assert_eq!(configures(&mut h, c, &w).len(), 1);
}

#[test]
fn parents_are_reported_and_follow_the_protocol_rules() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (p, pid) = open_window(&mut h, c, &g, 64, 64, RED);
    let (a, aid) = open_window(&mut h, c, &g, 64, 64, RED);
    a.toplevel.set_parent(Some(&p.toplevel));
    // B's parent is A, set while A is mapped.
    let b = create_with(&mut h, c, &g, |b| b.toplevel.set_parent(Some(&a.toplevel)));
    // A unmaps before B maps: B's parent becomes A's parent, P.
    a.surface.attach(None, 0, 0);
    a.surface.commit();
    h.roundtrip();
    let _ = h.server.take_window_events();
    map_now(&mut h, c, &g, &b, 64, 64);
    let (_, parent) = mapped_parent(&h.server.take_window_events()).expect("B mapped");
    assert_eq!(parent, Some(pid));
    // An unmapped parent counts as none.
    let d = create_window(&mut h, c, &g);
    let e = create_with(&mut h, c, &g, |e| e.toplevel.set_parent(Some(&d.toplevel)));
    map_now(&mut h, c, &g, &e, 64, 64);
    let (_, parent) = mapped_parent(&h.server.take_window_events()).expect("E mapped");
    assert_eq!(parent, None);
    assert_ne!(aid, pid);
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn a_toplevel_cannot_be_its_own_parent() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, _id) = open_window(&mut h, c, &g, 64, 64, RED);
    w.toplevel.set_parent(Some(&w.toplevel));
    h.roundtrip();
    let error = h.c(c).error().expect("disconnected");
    assert_eq!(error.code, xdg_toplevel::Error::InvalidParent as u32);
}

#[test]
fn a_parent_cannot_be_a_descendant() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (a, _) = open_window(&mut h, c, &g, 64, 64, RED);
    let (b, _) = open_window(&mut h, c, &g, 64, 64, RED);
    b.toplevel.set_parent(Some(&a.toplevel));
    a.toplevel.set_parent(Some(&b.toplevel));
    h.roundtrip();
    let error = h.c(c).error().expect("disconnected");
    assert_eq!(error.code, xdg_toplevel::Error::InvalidParent as u32);
}

#[test]
fn fullscreen_over_maximized_carries_both_states() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.c(c).clear_events();
    let both = States {
        maximized: true,
        fullscreen: true,
        ..States::default()
    };
    h.server.command(&[configure(id, 800.0, 600.0, both)]);
    h.roundtrip();
    let got = configures(&mut h, c, &w);
    assert!(got[0].2.contains(&state(xdg_toplevel::State::Maximized)));
    assert!(got[0].2.contains(&state(xdg_toplevel::State::Fullscreen)));
}

#[test]
fn wm_capabilities_list_maximize_and_fullscreen() {
    let mut h = Harness::new();
    let c = h.connect();
    let g = globals(h.c(c));
    let w = create_window(&mut h, c, &g);
    let caps: Vec<u32> = h
        .c(c)
        .events::<xdg_toplevel::Event>()
        .into_iter()
        .find_map(|(id, e)| match e {
            xdg_toplevel::Event::WmCapabilities { capabilities } if id == w.toplevel.id() => {
                Some(decode(capabilities))
            }
            _ => None,
        })
        .expect("wm_capabilities");
    let mut caps = caps;
    caps.sort_unstable();
    assert_eq!(
        caps,
        vec![
            xdg_toplevel::WmCapabilities::Maximize as u32,
            xdg_toplevel::WmCapabilities::Fullscreen as u32,
        ]
    );
}
