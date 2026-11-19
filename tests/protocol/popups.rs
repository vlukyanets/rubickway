//! Popups (popups contracts/protocols.md): the life of a popup, placement answers, the grab, input
//! during a grab, and the misbehaving cases tied to them.

use rubickway::canvas::{Point, Rect, Size};
use rubickway::server::OutputDesc;
use rubickway::shell::{
    Adjust, Anchor8, AppCursor, ClientInput, CursorName, IRect, PopupId, PopupRules, WindowCommand,
    WindowEvent, WindowId,
};
use wayland_client::Proxy;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, wp_fractional_scale_v1,
};
use wayland_protocols::xdg::shell::client::xdg_popup::{self, XdgPopup};
use wayland_protocols::xdg::shell::client::xdg_positioner::{
    self, ConstraintAdjustment, Gravity, XdgPositioner,
};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::{xdg_toplevel, xdg_wm_base};

use crate::harness::{Client, Harness, solid_buffer};
use crate::surfaces::{Globals, Window, globals, open_window};

const RED: u32 = 0xffff_0000;
const BLUE: u32 = 0xff00_00ff;
const BTN_RIGHT: u32 = 0x111;
const KEY_A: u32 = 30;

// --- helpers (tasks T009) -------------------------------------------------------------------

/// A client with a seat, pointer and keyboard and a mapped 200x100 window.
pub struct PopupClient {
    pub index: usize,
    pub g: Globals,
    pub seat: WlSeat,
    pub pointer: WlPointer,
    pub keyboard: WlKeyboard,
    pub window: Window,
    pub id: WindowId,
}

pub fn popup_client(h: &mut Harness) -> PopupClient {
    let index = h.connect();
    let g = globals(h.c(index));
    let seat: WlSeat = h.c(index).bind(8);
    let qh = h.c(index).qh.clone();
    let pointer = seat.get_pointer(&qh, ());
    let keyboard = seat.get_keyboard(&qh, ());
    let (window, id) = open_window(h, index, &g, 200, 100, RED);
    h.server.take_window_events();
    PopupClient {
        index,
        g,
        seat,
        pointer,
        keyboard,
        window,
        id,
    }
}

/// A complete positioner: `size`, the anchor rectangle `(x, y, w, h)`, anchor bottom-right,
/// gravity bottom-right, no adjustment.
pub fn positioner(
    c: &Client,
    g: &Globals,
    size: (i32, i32),
    rect: (i32, i32, i32, i32),
) -> XdgPositioner {
    let p = g.wm_base.create_positioner(&c.qh, ());
    p.set_size(size.0, size.1);
    p.set_anchor_rect(rect.0, rect.1, rect.2, rect.3);
    p.set_anchor(xdg_positioner::Anchor::BottomRight);
    p.set_gravity(Gravity::BottomRight);
    p
}

/// The rules `positioner` gives for `size` and `rect`.
pub fn rules_of(size: (i32, i32), rect: (i32, i32, i32, i32)) -> PopupRules {
    PopupRules {
        size,
        anchor_rect: IRect::new(rect.0, rect.1, rect.2, rect.3),
        anchor: Anchor8::BottomRight,
        gravity: Anchor8::BottomRight,
        ..PopupRules::default()
    }
}

/// A popup's objects.
pub struct Popup {
    pub surface: WlSurface,
    pub xdg: XdgSurface,
    pub popup: XdgPopup,
}

/// Creates a popup on `parent` with `positioner`, asks for a grab with `grab` if given, and
/// makes the initial commit.
pub fn create_popup(
    h: &mut Harness,
    ci: usize,
    g: &Globals,
    parent: Option<&XdgSurface>,
    positioner: &XdgPositioner,
    grab: Option<(&WlSeat, u32)>,
) -> Popup {
    let qh = h.c(ci).qh.clone();
    let surface = g.compositor.create_surface(&qh, ());
    let xdg = g.wm_base.get_xdg_surface(&surface, &qh, ());
    let popup = xdg.get_popup(parent, positioner, &qh, ());
    if let Some((seat, serial)) = grab {
        popup.grab(seat, serial);
    }
    surface.commit();
    h.roundtrip();
    Popup {
        surface,
        xdg,
        popup,
    }
}

/// The `PopupRequested` among the server's window events, if any; the other events are dropped.
pub fn requested(h: &mut Harness) -> Option<WindowEvent> {
    h.server
        .take_window_events()
        .into_iter()
        .find(|e| matches!(e, WindowEvent::PopupRequested { .. }))
}

pub fn requested_id(h: &mut Harness) -> PopupId {
    match requested(h) {
        Some(WindowEvent::PopupRequested { id, .. }) => id,
        other => panic!("no PopupRequested: {other:?}"),
    }
}

/// Places popup `id` at `at` and lets the client hear of it.
pub fn place(h: &mut Harness, id: PopupId, at: IRect) {
    h.server.command(&[WindowCommand::PlacePopup {
        id,
        at,
        token: None,
    }]);
    h.roundtrip();
}

/// The serial of the latest `xdg_surface.configure` of `xdg`.
pub fn latest_configure(c: &Client, xdg: &XdgSurface) -> u32 {
    c.events::<xdg_surface::Event>()
        .iter()
        .rev()
        .find_map(|(id, e)| match e {
            xdg_surface::Event::Configure { serial } if *id == xdg.id() => Some(*serial),
            _ => None,
        })
        .expect("a configure")
}

/// Acks the latest configure of `p` and commits a `w x h` buffer.
pub fn show(
    h: &mut Harness,
    ci: usize,
    g: &Globals,
    p: &Popup,
    w: i32,
    hgt: i32,
) -> (WlBuffer, WlShmPool) {
    let serial = latest_configure(h.c(ci), &p.xdg);
    p.xdg.ack_configure(serial);
    let (buffer, pool, _file) = solid_buffer(h.c(ci), &g.shm, w, hgt, BLUE);
    p.surface.attach(Some(&buffer), 0, 0);
    p.surface.commit();
    h.roundtrip();
    (buffer, pool)
}

/// A popup on the client's window, placed at `at` and shown with `at`'s size; returns its id.
pub fn shown_popup(
    h: &mut Harness,
    pc: &PopupClient,
    at: IRect,
    grab: Option<u32>,
) -> (Popup, PopupId) {
    let pos = positioner(h.c(pc.index), &pc.g, (at.w, at.h), (at.x, at.y, 0, 0));
    let p = create_popup(
        h,
        pc.index,
        &pc.g,
        Some(&pc.window.xdg),
        &pos,
        grab.map(|s| (&pc.seat, s)),
    );
    let id = requested_id(h);
    place(h, id, at);
    show(h, pc.index, &pc.g, &p, at.w, at.h);
    (p, id)
}

/// The pointer enters the client's window at `pos` and presses the right button; returns the
/// serial of the `wl_pointer.button` event.
pub fn press(h: &mut Harness, pc: &PopupClient, pos: Point) -> u32 {
    h.c(pc.index).clear_events();
    h.server.deliver(
        &[
            ClientInput::PointerEnter { window: pc.id, pos },
            ClientInput::PointerFrame { window: pc.id },
            ClientInput::PointerButton {
                window: pc.id,
                code: BTN_RIGHT,
                pressed: true,
            },
            ClientInput::PointerFrame { window: pc.id },
        ],
        1,
    );
    h.roundtrip();
    h.c(pc.index)
        .events::<wl_pointer::Event>()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            wl_pointer::Event::Button { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("a button event")
}

pub fn release(h: &mut Harness, pc: &PopupClient) {
    h.server.deliver(
        &[
            ClientInput::PointerButton {
                window: pc.id,
                code: BTN_RIGHT,
                pressed: false,
            },
            ClientInput::PointerFrame { window: pc.id },
        ],
        2,
    );
    h.roundtrip();
}

fn popup_done(c: &Client, p: &XdgPopup) -> bool {
    c.events::<xdg_popup::Event>()
        .iter()
        .any(|(id, e)| *id == p.id() && matches!(e, xdg_popup::Event::PopupDone))
}

fn error_code(h: &mut Harness, ci: usize) -> Option<(String, u32)> {
    h.c(ci)
        .error()
        .map(|e| (e.object_interface.clone(), e.code))
}

fn popup_events(h: &mut Harness) -> Vec<WindowEvent> {
    h.server
        .take_window_events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                WindowEvent::PopupRequested { .. }
                    | WindowEvent::PopupRepositionRequested { .. }
                    | WindowEvent::PopupMapped { .. }
                    | WindowEvent::PopupUpdated { .. }
                    | WindowEvent::PopupUnmapped { .. }
            )
        })
        .collect()
}

fn full(w: f64, h: f64) -> Vec<Rect> {
    vec![Rect::new(Point::new(0.0, 0.0), Size::new(w, h))]
}

// --- the life of a popup (tasks T010) -------------------------------------------------------

#[test]
fn the_initial_commit_asks_the_shell_with_the_positioners_rules() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 5, 6));
    pos.set_offset(3, -4);
    pos.set_constraint_adjustment(ConstraintAdjustment::FlipX | ConstraintAdjustment::SlideY);
    pos.set_reactive();
    pos.set_parent_size(200, 100);
    let _p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    let expected = PopupRules {
        offset: (3, -4),
        adjust: Adjust::FLIP_X | Adjust::SLIDE_Y,
        reactive: true,
        parent_size: Some((200, 100)),
        ..rules_of((30, 40), (10, 20, 5, 6))
    };
    match requested(&mut h) {
        Some(WindowEvent::PopupRequested {
            window,
            parent,
            rules,
            grab,
            ..
        }) => {
            assert_eq!(window, pc.id);
            assert_eq!(parent, None);
            assert_eq!(rules, expected);
            assert!(!grab);
        }
        other => panic!("{other:?}"),
    }
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_placement_is_sent_as_popup_configure_then_surface_configure() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    let id = requested_id(&mut h);
    h.c(pc.index).clear_events();
    place(&mut h, id, IRect::new(10, 20, 30, 40));
    let c = h.c(pc.index);
    let popup_at = c.state.events.iter().position(|r| {
        r.object == p.popup.id()
            && matches!(
                r.event.downcast_ref::<xdg_popup::Event>(),
                Some(xdg_popup::Event::Configure {
                    x: 10,
                    y: 20,
                    width: 30,
                    height: 40
                })
            )
    });
    let surface_at = c.state.events.iter().position(|r| {
        r.object == p.xdg.id()
            && matches!(
                r.event.downcast_ref::<xdg_surface::Event>(),
                Some(xdg_surface::Event::Configure { .. })
            )
    });
    assert!(popup_at.is_some() && surface_at.is_some());
    assert!(popup_at < surface_at);
}

#[test]
fn a_buffer_before_the_ack_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    let id = requested_id(&mut h);
    place(&mut h, id, IRect::new(10, 20, 30, 40));
    let (buffer, _pool, _file) = solid_buffer(h.c(pc.index), &pc.g.shm, 30, 40, BLUE);
    p.surface.attach(Some(&buffer), 0, 0);
    p.surface.commit();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_surface".into(),
            xdg_surface::Error::UnconfiguredBuffer as u32
        ))
    );
}

#[test]
fn a_shown_popup_is_drawn_after_its_window_at_its_place() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    let id = requested_id(&mut h);
    place(&mut h, id, IRect::new(190, 20, 30, 40));
    show(&mut h, pc.index, &pc.g, &p, 30, 40);
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupMapped {
            id,
            at: (190, 20),
            size: Size::new(30.0, 40.0),
            input: full(30.0, 40.0),
        }]
    );
    let parts = h.server.window_parts(pc.id);
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[1].rect,
        Rect::new(Point::new(190.0, 20.0), Size::new(30.0, 40.0))
    );
}

#[test]
fn new_sizes_update_and_destroying_unmaps() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    let (buffer, _pool, _file) = solid_buffer(h.c(pc.index), &pc.g.shm, 50, 40, BLUE);
    p.surface.attach(Some(&buffer), 0, 0);
    p.surface.commit();
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupUpdated {
            id,
            at: (10, 20),
            size: Size::new(50.0, 40.0),
            input: full(50.0, 40.0),
        }]
    );
    p.popup.destroy();
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupUnmapped { id }]
    );
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_commit_without_a_buffer_ends_the_popup_for_good() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    p.surface.attach(None, 0, 0);
    p.surface.commit();
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupUnmapped { id }]
    );
    assert!(popup_done(h.c(pc.index), &p.popup));
    let (buffer, _pool, _file) = solid_buffer(h.c(pc.index), &pc.g.shm, 30, 40, BLUE);
    p.surface.attach(Some(&buffer), 0, 0);
    p.surface.commit();
    h.roundtrip();
    assert!(popup_events(&mut h).is_empty());
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn destroying_the_surface_first_ends_the_popup_without_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    p.surface.destroy();
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupUnmapped { id }]
    );
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
    assert!(
        !h.c(pc.index).is_disconnected(),
        "{:?}",
        h.c(pc.index).error()
    );
}

#[test]
fn a_dismissed_popup_is_never_shown_again() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    h.server.command(&[WindowCommand::DismissPopup { id }]);
    h.roundtrip();
    assert!(popup_done(h.c(pc.index), &p.popup));
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
    let (buffer, _pool, _file) = solid_buffer(h.c(pc.index), &pc.g.shm, 30, 40, BLUE);
    p.surface.attach(Some(&buffer), 0, 0);
    p.surface.commit();
    h.roundtrip();
    assert!(popup_events(&mut h).is_empty());
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
}

#[test]
fn popup_surfaces_get_frame_callbacks_with_their_window() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, _id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    let qh = h.c(pc.index).qh.clone();
    let cb: WlCallback = p.surface.frame(&qh, ());
    p.surface.commit();
    h.roundtrip();
    assert!(h.server.has_pending_callbacks(&[pc.id]));
    h.server.presented(5, &[pc.id]);
    h.roundtrip();
    assert!(
        h.c(pc.index)
            .events::<wl_callback::Event>()
            .iter()
            .any(|(id, _)| *id == cb.id())
    );
}

fn laptop() -> OutputDesc {
    OutputDesc {
        name: "eDP-1".into(),
        make: "NCP".into(),
        model: "LM156LF1L03".into(),
        physical_mm: (344, 194),
        mode: (1920, 1080, 60_000),
        position: (0, 0),
    }
}

#[test]
fn popup_surfaces_enter_their_windows_outputs_and_get_the_scale() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let pc = popup_client(&mut h);
    let outputs: Vec<WlOutput> = h.c(pc.index).bind_all(4);
    let fractional: WpFractionalScaleManagerV1 = h.c(pc.index).bind(1);
    h.server.set_window_outputs(pc.id, &[1]);
    h.roundtrip();
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    let qh = h.c(pc.index).qh.clone();
    let _fs = fractional.get_fractional_scale(&p.surface, &qh, ());
    let id = requested_id(&mut h);
    h.c(pc.index).clear_events();
    place(&mut h, id, IRect::new(10, 20, 30, 40));
    show(&mut h, pc.index, &pc.g, &p, 30, 40);
    let c = h.c(pc.index);
    assert!(
        c.events::<wl_surface::Event>()
            .iter()
            .any(|(sid, e)| *sid == p.surface.id()
                && matches!(e, wl_surface::Event::Enter { output } if *output == outputs[0])),
        "the popup surface enters the window's output"
    );
    assert!(
        c.events::<wp_fractional_scale_v1::Event>()
            .iter()
            .all(|(_, e)| matches!(
                e,
                wp_fractional_scale_v1::Event::PreferredScale { scale: 120 }
            ))
    );
}

#[test]
fn a_popup_without_a_parent_is_dismissed_at_its_initial_commit() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, None, &pos, None);
    assert!(requested(&mut h).is_none());
    assert!(popup_done(h.c(pc.index), &p.popup));
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_popup_of_an_unmapped_window_is_dismissed_at_its_initial_commit() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    pc.window.surface.attach(None, 0, 0);
    pc.window.surface.commit();
    h.roundtrip();
    h.server.take_window_events();
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    assert!(requested(&mut h).is_none());
    assert!(popup_done(h.c(pc.index), &p.popup));
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_parent_without_a_role_is_an_invalid_popup_parent() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let qh = h.c(pc.index).qh.clone();
    let bare = pc.g.compositor.create_surface(&qh, ());
    let bare_xdg = pc.g.wm_base.get_xdg_surface(&bare, &qh, ());
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let _p = create_popup(&mut h, pc.index, &pc.g, Some(&bare_xdg), &pos, None);
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_wm_base".into(),
            xdg_wm_base::Error::InvalidPopupParent as u32
        ))
    );
}

#[test]
fn destroying_the_xdg_surface_before_the_popup_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, _id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    p.xdg.destroy();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_surface".into(),
            xdg_surface::Error::DefunctRoleObject as u32
        ))
    );
}

// --- the grab and input (tasks T012) --------------------------------------------------------

fn grab_requested(h: &mut Harness) -> Option<bool> {
    match requested(h) {
        Some(WindowEvent::PopupRequested { grab, .. }) => Some(grab),
        _ => None,
    }
}

#[test]
fn a_grab_with_the_latest_press_serial_counts_even_after_the_release() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    create_popup(
        &mut h,
        pc.index,
        &pc.g,
        Some(&pc.window.xdg),
        &pos,
        Some((&pc.seat, serial)),
    );
    assert_eq!(grab_requested(&mut h), Some(true));
    release(&mut h, &pc);
    create_popup(
        &mut h,
        pc.index,
        &pc.g,
        Some(&pc.window.xdg),
        &pos,
        Some((&pc.seat, serial)),
    );
    assert_eq!(grab_requested(&mut h), Some(true));
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_grab_with_the_latest_key_serial_counts() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    h.server.deliver(
        &[
            ClientInput::KeyboardFocus(Some(pc.id)),
            ClientInput::Key {
                window: pc.id,
                keycode: KEY_A,
                pressed: true,
            },
        ],
        1,
    );
    h.roundtrip();
    let key_serial = |h: &mut Harness| {
        h.c(pc.index)
            .events::<wl_keyboard::Event>()
            .iter()
            .rev()
            .find_map(|(_, e)| match e {
                wl_keyboard::Event::Key { serial, .. } => Some(*serial),
                _ => None,
            })
            .expect("a key")
    };
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let serial = key_serial(&mut h);
    create_popup(
        &mut h,
        pc.index,
        &pc.g,
        Some(&pc.window.xdg),
        &pos,
        Some((&pc.seat, serial)),
    );
    assert_eq!(grab_requested(&mut h), Some(true));
    // The release counts too.
    h.server.deliver(
        &[ClientInput::Key {
            window: pc.id,
            keycode: KEY_A,
            pressed: false,
        }],
        2,
    );
    h.roundtrip();
    let serial = key_serial(&mut h);
    create_popup(
        &mut h,
        pc.index,
        &pc.g,
        Some(&pc.window.xdg),
        &pos,
        Some((&pc.seat, serial)),
    );
    assert_eq!(grab_requested(&mut h), Some(true));
}

#[test]
fn a_grab_with_a_wrong_serial_is_dismissed_without_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let other = popup_client(&mut h);
    let theirs = press(&mut h, &other, Point::new(10.0, 10.0));
    let mine = press(&mut h, &pc, Point::new(10.0, 10.0));
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    for serial in [0, mine.wrapping_sub(1), theirs] {
        let p = create_popup(
            &mut h,
            pc.index,
            &pc.g,
            Some(&pc.window.xdg),
            &pos,
            Some((&pc.seat, serial)),
        );
        assert!(requested(&mut h).is_none(), "serial {serial}");
        assert!(popup_done(h.c(pc.index), &p.popup), "serial {serial}");
    }
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_grab_after_the_initial_commit_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let pos = positioner(h.c(pc.index), &pc.g, (30, 40), (10, 20, 0, 0));
    let p = create_popup(&mut h, pc.index, &pc.g, Some(&pc.window.xdg), &pos, None);
    p.popup.grab(&pc.seat, serial);
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, pc.index),
        Some(("xdg_popup".into(), xdg_popup::Error::InvalidGrab as u32))
    );
}

/// A keyboard event in short: which surface, which key.
#[derive(Debug, PartialEq)]
enum Kb {
    Leave(WlSurface),
    Enter(WlSurface, Vec<u8>),
    Modifiers,
    Key(u32),
    Other,
}

fn keyboard_events(h: &mut Harness, pc: &PopupClient) -> Vec<Kb> {
    h.c(pc.index)
        .events::<wl_keyboard::Event>()
        .into_iter()
        .filter(|(id, _)| *id == pc.keyboard.id())
        .map(|(_, e)| match e {
            wl_keyboard::Event::Leave { surface, .. } => Kb::Leave(surface.clone()),
            wl_keyboard::Event::Enter { surface, keys, .. } => {
                Kb::Enter(surface.clone(), keys.clone())
            }
            wl_keyboard::Event::Modifiers { .. } => Kb::Modifiers,
            wl_keyboard::Event::Key { key, .. } => Kb::Key(*key),
            _ => Kb::Other,
        })
        .collect()
}

#[test]
fn the_keyboard_moves_to_a_popup_and_back_without_changing_activated() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(pc.id))], 1);
    h.roundtrip();
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), Some(serial));
    h.server.deliver(
        &[ClientInput::Key {
            window: pc.id,
            keycode: KEY_A,
            pressed: true,
        }],
        2,
    );
    h.roundtrip();
    h.c(pc.index).clear_events();
    h.server.deliver(
        &[
            ClientInput::PopupKeyboard {
                window: pc.id,
                popup: Some(id),
            },
            ClientInput::Key {
                window: pc.id,
                keycode: KEY_A,
                pressed: false,
            },
        ],
        3,
    );
    h.roundtrip();
    assert_eq!(
        keyboard_events(&mut h, &pc),
        vec![
            Kb::Leave(pc.window.surface.clone()),
            Kb::Enter(p.surface.clone(), KEY_A.to_ne_bytes().to_vec()),
            Kb::Modifiers,
            Kb::Key(KEY_A),
        ]
    );
    assert!(
        h.c(pc.index).events::<xdg_toplevel::Event>().is_empty(),
        "activated did not change"
    );
    h.c(pc.index).clear_events();
    h.server.deliver(
        &[ClientInput::PopupKeyboard {
            window: pc.id,
            popup: None,
        }],
        4,
    );
    h.roundtrip();
    assert_eq!(
        keyboard_events(&mut h, &pc),
        vec![
            Kb::Leave(p.surface.clone()),
            Kb::Enter(pc.window.surface.clone(), Vec::new()),
            Kb::Modifiers,
        ]
    );
}

fn entered_surface(h: &mut Harness, pc: &PopupClient) -> Option<WlSurface> {
    h.c(pc.index)
        .events::<wl_pointer::Event>()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            wl_pointer::Event::Enter { surface, .. } => Some(surface.clone()),
            _ => None,
        })
}

fn motion(h: &mut Harness, pc: &PopupClient, pos: Point) {
    h.server.deliver(
        &[
            ClientInput::PointerMotion { window: pc.id, pos },
            ClientInput::PointerFrame { window: pc.id },
        ],
        5,
    );
    h.roundtrip();
}

#[test]
fn with_a_grab_the_pointer_moves_onto_the_popup_while_a_button_is_held() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (p, _id) = shown_popup(&mut h, &pc, IRect::new(150, 50, 100, 100), Some(serial));
    h.c(pc.index).clear_events();
    motion(&mut h, &pc, Point::new(160.0, 60.0));
    assert_eq!(entered_surface(&mut h, &pc), Some(p.surface.clone()));
}

#[test]
fn without_a_grab_the_pointer_stays_on_the_pressed_surface() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (_p, _id) = shown_popup(&mut h, &pc, IRect::new(150, 50, 100, 100), None);
    press(&mut h, &pc, Point::new(10.0, 10.0));
    h.c(pc.index).clear_events();
    motion(&mut h, &pc, Point::new(160.0, 60.0));
    assert_eq!(entered_surface(&mut h, &pc), None);
}

#[test]
fn a_shape_named_over_a_popup_counts_for_its_window() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let manager: WpCursorShapeManagerV1 = h.c(pc.index).bind(2);
    let qh = h.c(pc.index).qh.clone();
    let device = manager.get_pointer(&pc.pointer, &qh, ());
    let (p, _id) = shown_popup(&mut h, &pc, IRect::new(150, 50, 100, 100), None);
    h.c(pc.index).clear_events();
    h.server.deliver(
        &[
            ClientInput::PointerEnter {
                window: pc.id,
                pos: Point::new(160.0, 60.0),
            },
            ClientInput::PointerFrame { window: pc.id },
        ],
        1,
    );
    h.roundtrip();
    assert_eq!(entered_surface(&mut h, &pc), Some(p.surface.clone()));
    let serial = h
        .c(pc.index)
        .events::<wl_pointer::Event>()
        .iter()
        .find_map(|(_, e)| match e {
            wl_pointer::Event::Enter { serial, .. } => Some(*serial),
            _ => None,
        })
        .expect("enter");
    h.server.take_window_events();
    device.set_shape(serial, Shape::Pointer);
    h.roundtrip();
    assert_eq!(
        h.server.take_window_events(),
        vec![WindowEvent::CursorChanged {
            id: pc.id,
            cursor: AppCursor::Shape(CursorName::Pointer),
        }]
    );
}

// --- nested popups and reposition (tasks T022) ----------------------------------------------

/// A popup on popup `parent` asking for `size` at `(x, y)` of the parent, with a grab if
/// `grab` is given; the initial commit is made.
fn child_popup(
    h: &mut Harness,
    pc: &PopupClient,
    parent: &Popup,
    at: IRect,
    grab: Option<u32>,
) -> Popup {
    let pos = positioner(h.c(pc.index), &pc.g, (at.w, at.h), (at.x, at.y, 0, 0));
    create_popup(
        h,
        pc.index,
        &pc.g,
        Some(&parent.xdg),
        &pos,
        grab.map(|s| (&pc.seat, s)),
    )
}

/// A child popup placed at `at` and shown; returns it and its id.
fn shown_child(
    h: &mut Harness,
    pc: &PopupClient,
    parent: &Popup,
    at: IRect,
    grab: Option<u32>,
) -> (Popup, PopupId) {
    let child = child_popup(h, pc, parent, at, grab);
    let id = requested_id(h);
    place(h, id, at);
    show(h, pc.index, &pc.g, &child, at.w, at.h);
    (child, id)
}

/// The `xdg_popup` objects that got `popup_done`, in order.
fn done_order(c: &Client) -> Vec<wayland_client::backend::ObjectId> {
    c.events::<xdg_popup::Event>()
        .into_iter()
        .filter(|(_, e)| matches!(e, xdg_popup::Event::PopupDone))
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn a_grabbing_submenu_is_asked_for_on_its_parent_and_drawn_above_it() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (menu, menu_id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    h.server.take_window_events();
    let at = IRect::new(50, 10, 60, 60);
    let sub = child_popup(&mut h, &pc, &menu, at, Some(serial));
    let sub_id = match requested(&mut h) {
        Some(WindowEvent::PopupRequested {
            id,
            window,
            parent,
            grab,
            ..
        }) => {
            assert_eq!(window, pc.id);
            assert_eq!(parent, Some(menu_id));
            assert!(grab);
            id
        }
        other => panic!("{other:?}"),
    };
    place(&mut h, sub_id, at);
    show(&mut h, pc.index, &pc.g, &sub, 60, 60);
    let parts = h.server.window_parts(pc.id);
    assert_eq!(parts.len(), 3);
    // The submenu's position is relative to the menu at window (10, 20).
    assert_eq!(
        parts[2].rect,
        Rect::new(Point::new(60.0, 30.0), Size::new(60.0, 60.0))
    );
    h.c(pc.index).clear_events();
    // (70, 40) is on both popups: the submenu is on top.
    motion(&mut h, &pc, Point::new(70.0, 40.0));
    assert_eq!(entered_surface(&mut h, &pc), Some(sub.surface.clone()));
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_grabbing_popup_mapped_while_its_parent_is_not_the_topmost_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (menu, _) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    let (_sub, _) = shown_child(&mut h, &pc, &menu, IRect::new(50, 10, 60, 60), Some(serial));
    // A second grabbing child of the menu: the first one is the topmost.
    let other = child_popup(&mut h, &pc, &menu, IRect::new(50, 40, 60, 60), Some(serial));
    let id = requested_id(&mut h);
    place(&mut h, id, IRect::new(50, 40, 60, 60));
    show(&mut h, pc.index, &pc.g, &other, 60, 60);
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_wm_base".into(),
            xdg_wm_base::Error::NotTheTopmostPopup as u32
        ))
    );
}

#[test]
fn destroying_a_popup_with_a_shown_child_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (menu, _) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    let (_sub, _) = shown_child(&mut h, &pc, &menu, IRect::new(50, 10, 60, 60), Some(serial));
    menu.popup.destroy();
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_wm_base".into(),
            xdg_wm_base::Error::NotTheTopmostPopup as u32
        ))
    );
}

#[test]
fn a_grabbing_child_of_a_popup_without_a_grab_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (tooltip, _) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), None);
    child_popup(
        &mut h,
        &pc,
        &tooltip,
        IRect::new(50, 10, 60, 60),
        Some(serial),
    );
    assert_eq!(
        error_code(&mut h, pc.index),
        Some(("xdg_popup".into(), xdg_popup::Error::InvalidGrab as u32))
    );
}

#[test]
fn a_grabbing_child_of_a_dismissed_menu_is_dismissed_at_once() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (menu, menu_id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    h.server
        .command(&[WindowCommand::DismissPopup { id: menu_id }]);
    h.roundtrip();
    h.server.take_window_events();
    let sub = child_popup(&mut h, &pc, &menu, IRect::new(50, 10, 60, 60), Some(serial));
    assert!(popup_done(h.c(pc.index), &sub.popup));
    assert!(popup_events(&mut h).is_empty(), "the shell is not asked");
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn hiding_a_menu_ends_its_submenu_first() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let serial = press(&mut h, &pc, Point::new(10.0, 10.0));
    let (menu, menu_id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    let (sub, sub_id) = shown_child(&mut h, &pc, &menu, IRect::new(50, 10, 60, 60), Some(serial));
    h.server.take_window_events();
    h.c(pc.index).clear_events();
    menu.surface.attach(None, 0, 0);
    menu.surface.commit();
    h.roundtrip();
    assert_eq!(
        done_order(h.c(pc.index)),
        vec![sub.popup.id(), menu.popup.id()]
    );
    assert_eq!(
        popup_events(&mut h),
        vec![
            WindowEvent::PopupUnmapped { id: sub_id },
            WindowEvent::PopupUnmapped { id: menu_id },
        ]
    );
    assert_eq!(h.server.window_parts(pc.id).len(), 1);
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_reposition_is_answered_with_its_token_and_applies_after_the_ack() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    let pos = positioner(h.c(pc.index), &pc.g, (50, 60), (5, 6, 0, 0));
    pos.set_parent_size(200, 100);
    p.popup.reposition(&pos, 7);
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupRepositionRequested {
            id,
            rules: PopupRules {
                parent_size: Some((200, 100)),
                ..rules_of((50, 60), (5, 6, 0, 0))
            },
            token: 7,
        }]
    );
    h.c(pc.index).clear_events();
    h.server.command(&[WindowCommand::PlacePopup {
        id,
        at: IRect::new(5, 6, 50, 60),
        token: Some(7),
    }]);
    h.roundtrip();
    let c = h.c(pc.index);
    let order: Vec<&str> = c
        .state
        .events
        .iter()
        .filter_map(|r| {
            if r.object == p.popup.id() {
                match r.event.downcast_ref::<xdg_popup::Event>() {
                    Some(xdg_popup::Event::Repositioned { token: 7 }) => Some("repositioned"),
                    Some(xdg_popup::Event::Configure {
                        x: 5,
                        y: 6,
                        width: 50,
                        height: 60,
                    }) => Some("configure"),
                    _ => Some("other"),
                }
            } else if r.object == p.xdg.id() {
                Some("surface configure")
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        order,
        vec!["repositioned", "configure", "surface configure"]
    );
    // The old position stays until the commit after the ack.
    assert_eq!(
        h.server.window_parts(pc.id)[1].rect,
        Rect::new(Point::new(10.0, 20.0), Size::new(30.0, 40.0))
    );
    show(&mut h, pc.index, &pc.g, &p, 50, 60);
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupUpdated {
            id,
            at: (5, 6),
            size: Size::new(50.0, 60.0),
            input: full(50.0, 60.0),
        }]
    );
    assert_eq!(
        h.server.window_parts(pc.id)[1].rect,
        Rect::new(Point::new(5.0, 6.0), Size::new(50.0, 60.0))
    );
}

#[test]
fn a_flood_of_repositions_asks_the_shell_once_with_the_newest_token() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    h.server.take_window_events();
    let pos = positioner(h.c(pc.index), &pc.g, (50, 60), (5, 6, 0, 0));
    for token in 0..10_000 {
        p.popup.reposition(&pos, token);
    }
    h.roundtrip();
    assert_eq!(
        popup_events(&mut h),
        vec![WindowEvent::PopupRepositionRequested {
            id,
            rules: rules_of((50, 60), (5, 6, 0, 0)),
            token: 9_999,
        }]
    );
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_reposition_with_an_incomplete_positioner_is_an_error() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    let (p, _id) = shown_popup(&mut h, &pc, IRect::new(10, 20, 30, 40), None);
    let qh = h.c(pc.index).qh.clone();
    let pos = pc.g.wm_base.create_positioner(&qh, ());
    pos.set_size(10, 10);
    p.popup.reposition(&pos, 1);
    h.roundtrip();
    assert_eq!(
        error_code(&mut h, pc.index),
        Some((
            "xdg_wm_base".into(),
            xdg_wm_base::Error::InvalidPositioner as u32
        ))
    );
}

// --- a window going away (tasks T028) --------------------------------------------------------

/// A client with a grabbing menu and a grabbing submenu shown on its window.
fn client_with_menus(h: &mut Harness) -> (PopupClient, Popup, Popup) {
    let pc = popup_client(h);
    let serial = press(h, &pc, Point::new(10.0, 10.0));
    let (menu, _) = shown_popup(h, &pc, IRect::new(10, 20, 100, 100), Some(serial));
    let (sub, _) = shown_child(h, &pc, &menu, IRect::new(50, 10, 60, 60), Some(serial));
    assert_eq!(h.server.window_parts(pc.id).len(), 3);
    h.c(pc.index).clear_events();
    h.server.take_window_events();
    (pc, menu, sub)
}

#[test]
fn a_window_unmapping_ends_its_popups_innermost_first() {
    let mut h = Harness::new();
    let (pc, menu, sub) = client_with_menus(&mut h);
    pc.window.surface.attach(None, 0, 0);
    pc.window.surface.commit();
    h.roundtrip();
    assert_eq!(
        done_order(h.c(pc.index)),
        vec![sub.popup.id(), menu.popup.id()]
    );
    assert!(h.server.window_parts(pc.id).is_empty());
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_window_destroyed_ends_its_popups_innermost_first() {
    let mut h = Harness::new();
    let (pc, menu, sub) = client_with_menus(&mut h);
    pc.window.toplevel.destroy();
    h.roundtrip();
    assert_eq!(
        done_order(h.c(pc.index)),
        vec![sub.popup.id(), menu.popup.id()]
    );
    assert!(h.server.window_parts(pc.id).is_empty());
    assert!(!h.c(pc.index).is_disconnected());
}

#[test]
fn a_client_gone_with_a_grab_open_leaves_no_popup_behind() {
    let mut h = Harness::new();
    let other = popup_client(&mut h);
    let (pc, _menu, _sub) = client_with_menus(&mut h);
    let gone = pc.id;
    let client = h.clients.remove(pc.index);
    drop(pc);
    drop(client);
    h.roundtrip();
    assert!(h.server.window_parts(gone).is_empty());
    assert!(
        h.server
            .take_window_events()
            .contains(&WindowEvent::Unmapped { id: gone })
    );
    // The other application keeps working.
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(other.id))], 9);
    h.roundtrip();
    assert!(
        keyboard_events(&mut h, &other)
            .contains(&Kb::Enter(other.window.surface.clone(), Vec::new())),
    );
    assert_eq!(h.server.window_parts(other.id).len(), 1);
}

// --- tooltips (tasks T033) --------------------------------------------------------------------

#[test]
fn a_tooltip_gets_the_pointer_but_not_the_keyboard() {
    let mut h = Harness::new();
    let pc = popup_client(&mut h);
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(pc.id))], 1);
    h.roundtrip();
    let (tip, _) = shown_popup(&mut h, &pc, IRect::new(150, 50, 100, 40), None);
    h.c(pc.index).clear_events();
    h.server.deliver(
        &[
            ClientInput::PointerEnter {
                window: pc.id,
                pos: Point::new(160.0, 60.0),
            },
            ClientInput::PointerFrame { window: pc.id },
            ClientInput::Key {
                window: pc.id,
                keycode: KEY_A,
                pressed: true,
            },
        ],
        2,
    );
    h.roundtrip();
    assert_eq!(entered_surface(&mut h, &pc), Some(tip.surface.clone()));
    assert_eq!(keyboard_events(&mut h, &pc), vec![Kb::Key(KEY_A)]);
    let key_on = h.c(pc.index).events::<wl_keyboard::Event>().iter().any(
        |(_, e)| matches!(e, wl_keyboard::Event::Enter { surface, .. } if *surface == tip.surface),
    );
    assert!(!key_on, "the keyboard never enters the tooltip");
}
