//! `zwp_relative_pointer_manager_v1` and `zwp_pointer_constraints_v1` (pointer-lock
//! contracts/protocols.md, research L3, L11, L12): what the objects report to the shell and what
//! they send for the shell's decisions. The shell's rules are in `tests/pointer_constraints.rs`.

use rubickway::canvas::{Point, Rect, Size, Vector};
use rubickway::shell::{
    ClientInput, ConstraintChange, ConstraintEnd, ConstraintInfo, ConstraintKind, ConstraintToken,
    WindowEvent, WindowId,
};
use wayland_client::Proxy;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::{
    self, ZwpConfinedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::{
    self, ZwpLockedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::{
    Lifetime, ZwpPointerConstraintsV1,
};
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1;
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::{
    self, ZwpRelativePointerV1,
};

use crate::harness::{Harness, solid_buffer};
use crate::input::enter_pointer;
use crate::surfaces::{Globals, Window, ack, create_window, globals, open_window};

const RED: u32 = 0xffff_0000;

/// A client with a mapped 100x100 window, a pointer, and both managers bound.
pub struct LockClient {
    pub index: usize,
    pub g: Globals,
    pub pointer: WlPointer,
    pub constraints: ZwpPointerConstraintsV1,
    pub relative: ZwpRelativePointerManagerV1,
    pub window: Window,
    pub id: WindowId,
}

pub fn lock_client(h: &mut Harness) -> LockClient {
    let index = h.connect();
    let g = globals(h.c(index));
    let seat: WlSeat = h.c(index).bind(8);
    let qh = h.c(index).qh.clone();
    let pointer = seat.get_pointer(&qh, ());
    let constraints: ZwpPointerConstraintsV1 = h.c(index).bind(1);
    let relative: ZwpRelativePointerManagerV1 = h.c(index).bind(1);
    let (window, id) = open_window(h, index, &g, 100, 100, RED);
    h.server.take_window_events();
    LockClient {
        index,
        g,
        pointer,
        constraints,
        relative,
        window,
        id,
    }
}

impl LockClient {
    pub fn lock(
        &self,
        h: &mut Harness,
        region: Option<&WlRegion>,
        lifetime: Lifetime,
    ) -> ZwpLockedPointerV1 {
        let qh = h.c(self.index).qh.clone();
        self.constraints.lock_pointer(
            &self.window.surface,
            &self.pointer,
            region,
            lifetime,
            &qh,
            (),
        )
    }

    pub fn confine(
        &self,
        h: &mut Harness,
        region: Option<&WlRegion>,
        lifetime: Lifetime,
    ) -> ZwpConfinedPointerV1 {
        let qh = h.c(self.index).qh.clone();
        self.constraints.confine_pointer(
            &self.window.surface,
            &self.pointer,
            region,
            lifetime,
            &qh,
            (),
        )
    }

    /// A region of the given rectangles.
    pub fn region(&self, h: &mut Harness, rects: &[(i32, i32, i32, i32)]) -> WlRegion {
        let qh = h.c(self.index).qh.clone();
        let region = self.g.compositor.create_region(&qh, ());
        for (x, y, w, h) in rects {
            region.add(*x, *y, *w, *h);
        }
        region
    }

    pub fn relative_pointer(&self, h: &mut Harness) -> ZwpRelativePointerV1 {
        let qh = h.c(self.index).qh.clone();
        self.relative.get_relative_pointer(&self.pointer, &qh, ())
    }
}

fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(Point::new(x, y), Size::new(w, h))
}

/// The constraint reports in `events`, in order.
pub fn reports(events: &[WindowEvent]) -> Vec<(WindowId, Option<ConstraintInfo>)> {
    events
        .iter()
        .filter_map(|e| match e {
            WindowEvent::ConstraintChanged { id, constraint } => Some((*id, constraint.clone())),
            _ => None,
        })
        .collect()
}

/// The single report in the window events since the last call.
pub fn one_report(h: &mut Harness) -> (WindowId, Option<ConstraintInfo>) {
    let events = h.server.take_window_events();
    let got = reports(&events);
    assert_eq!(got.len(), 1, "{events:?}");
    got[0].clone()
}

pub fn decide(h: &mut Harness, token: ConstraintToken, change: ConstraintChange) {
    h.server
        .deliver(&[ClientInput::Constraint { token, change }], 1);
    h.roundtrip();
}

pub fn locked_events(h: &mut Harness, ci: usize) -> Vec<String> {
    h.c(ci)
        .events::<zwp_locked_pointer_v1::Event>()
        .iter()
        .map(|(_, e)| format!("{e:?}"))
        .collect()
}

fn confined_events(h: &mut Harness, ci: usize) -> Vec<String> {
    h.c(ci)
        .events::<zwp_confined_pointer_v1::Event>()
        .iter()
        .map(|(_, e)| format!("{e:?}"))
        .collect()
}

#[derive(Debug, PartialEq)]
struct Motion {
    hi: u32,
    lo: u32,
    d: (f64, f64),
    raw: (f64, f64),
}

fn motions(h: &mut Harness, ci: usize, on: &ZwpRelativePointerV1) -> Vec<Motion> {
    h.c(ci)
        .events::<zwp_relative_pointer_v1::Event>()
        .iter()
        .filter(|(id, _)| *id == on.id())
        .filter_map(|(_, e)| match e {
            zwp_relative_pointer_v1::Event::RelativeMotion {
                utime_hi,
                utime_lo,
                dx,
                dy,
                dx_unaccel,
                dy_unaccel,
            } => Some(Motion {
                hi: *utime_hi,
                lo: *utime_lo,
                d: (*dx, *dy),
                raw: (*dx_unaccel, *dy_unaccel),
            }),
            _ => None,
        })
        .collect()
}

fn relative_motion(h: &mut Harness, window: WindowId) {
    h.server.deliver(
        &[
            ClientInput::RelativeMotion {
                window,
                delta: Vector::new(1.5, -2.25),
                unaccel: Vector::new(3.0, 4.5),
                time_us: 0x1_0000_0002,
            },
            ClientInput::PointerFrame { window },
        ],
        1,
    );
    h.roundtrip();
}

fn expected_motion() -> Motion {
    Motion {
        hi: 1,
        lo: 2,
        d: (1.5, -2.25),
        raw: (3.0, 4.5),
    }
}

// --- Relative pointers ----------------------------------------------------------------------

#[test]
fn relative_motion_reaches_every_live_relative_pointer_of_the_window_with_the_pointer() {
    let mut h = Harness::new();
    let a = lock_client(&mut h);
    let b = lock_client(&mut h);
    let first = a.relative_pointer(&mut h);
    let second = a.relative_pointer(&mut h);
    let other = b.relative_pointer(&mut h);
    enter_pointer(&mut h, a.index, a.id);
    relative_motion(&mut h, a.id);
    assert_eq!(motions(&mut h, a.index, &first), vec![expected_motion()]);
    assert_eq!(motions(&mut h, a.index, &second), vec![expected_motion()]);
    assert!(motions(&mut h, b.index, &other).is_empty());

    // The manager going away changes nothing; a destroyed relative pointer gets nothing.
    a.relative.destroy();
    first.destroy();
    h.c(a.index).clear_events();
    relative_motion(&mut h, a.id);
    assert!(motions(&mut h, a.index, &first).is_empty());
    assert_eq!(motions(&mut h, a.index, &second), vec![expected_motion()]);

    // A window the pointer is not on gets nothing.
    h.c(b.index).clear_events();
    relative_motion(&mut h, b.id);
    assert!(motions(&mut h, b.index, &other).is_empty());
    assert!(!h.c(a.index).is_disconnected());
}

// --- Locks: reports ---------------------------------------------------------------------------

#[test]
fn a_lock_is_reported_with_its_token_kind_lifetime_and_area() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let lock = c.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    let (id, info) = one_report(&mut h);
    assert_eq!(id, c.id);
    let info = info.expect("a constraint");
    assert_eq!(info.kind, ConstraintKind::Lock);
    assert!(info.persistent);
    assert_eq!(info.area, None);
    assert_eq!(info.hint, None);

    // Destroyed: reported gone.
    lock.destroy();
    h.roundtrip();
    assert_eq!(one_report(&mut h), (c.id, None));

    // A single-use one with an area, copied when it is made: changing the region later does
    // nothing.
    let region = c.region(&mut h, &[(10, 20, 30, 40)]);
    let _lock = c.lock(&mut h, Some(&region), Lifetime::Oneshot);
    region.add(0, 0, 5, 5);
    h.roundtrip();
    let (_, second) = one_report(&mut h);
    let second = second.expect("a constraint");
    assert!(!second.persistent);
    assert_ne!(second.token, info.token, "tokens are never reused");
    assert_eq!(second.area, Some(vec![r(10.0, 20.0, 30.0, 40.0)]));
}

#[test]
fn the_hint_and_the_region_apply_on_the_next_commit_in_window_coordinates() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let lock = c.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    h.server.take_window_events();
    lock.set_cursor_position_hint(20.0, 10.0);
    let region = c.region(&mut h, &[(0, 0, 50, 50)]);
    lock.set_region(Some(&region));
    h.roundtrip();
    assert!(
        reports(&h.server.take_window_events()).is_empty(),
        "not before the commit"
    );
    c.window.surface.commit();
    h.roundtrip();
    let info = one_report(&mut h).1.expect("a constraint");
    assert_eq!(info.hint, Some(Point::new(20.0, 10.0)));
    assert_eq!(info.area, Some(vec![r(0.0, 0.0, 50.0, 50.0)]));

    // The window geometry starts at (5, 5) of the surface: window coordinates move with it.
    c.window.xdg.set_window_geometry(5, 5, 90, 90);
    c.window.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    let got = reports(&events);
    assert_eq!(got.len(), 1, "{events:?}");
    let info = got[0].1.clone().expect("a constraint");
    assert_eq!(info.hint, Some(Point::new(15.0, 5.0)));
    assert_eq!(info.area, Some(vec![r(-5.0, -5.0, 50.0, 50.0)]));

    // Back to the whole window.
    lock.set_region(None);
    c.window.surface.commit();
    h.roundtrip();
    assert_eq!(one_report(&mut h).1.expect("a constraint").area, None);
}

#[test]
fn a_lock_on_a_window_not_shown_yet_is_reported_after_it_is_mapped() {
    let mut h = Harness::new();
    let mut c = lock_client(&mut h);
    let w = create_window(&mut h, c.index, &c.g);
    c.window = w;
    let _lock = c.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    assert!(reports(&h.server.take_window_events()).is_empty());
    ack(&mut h, c.index, &c.window);
    let (buffer, _pool, _file) = solid_buffer(h.c(c.index), &c.g.shm, 50, 50, RED);
    c.window.surface.attach(Some(&buffer), 0, 0);
    c.window.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    let mapped = events
        .iter()
        .position(|e| matches!(e, WindowEvent::Mapped { .. }))
        .expect("mapped");
    let reported = events
        .iter()
        .position(|e| matches!(e, WindowEvent::ConstraintChanged { .. }))
        .expect("reported");
    assert!(mapped < reported, "{events:?}");
}

#[test]
fn a_constraint_on_a_surface_that_is_not_a_window_is_never_reported() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let qh = h.c(c.index).qh.clone();
    // A surface without a role, and a subsurface of the window.
    let bare: WlSurface = c.g.compositor.create_surface(&qh, ());
    let child: WlSurface = c.g.compositor.create_surface(&qh, ());
    let _sub =
        c.g.subcompositor
            .get_subsurface(&child, &c.window.surface, &qh, ());
    let (buffer, _pool, _file) = solid_buffer(h.c(c.index), &c.g.shm, 10, 10, RED);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    c.window.surface.commit();
    for surface in [&bare, &child] {
        c.constraints
            .lock_pointer(surface, &c.pointer, None, Lifetime::Persistent, &qh, ());
        surface.commit();
    }
    h.roundtrip();
    assert!(reports(&h.server.take_window_events()).is_empty());
    assert!(!h.c(c.index).is_disconnected());
}

// --- Locks: decisions ---------------------------------------------------------------------------

#[test]
fn the_shells_decisions_send_locked_and_unlocked() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let _lock = c.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    let token = one_report(&mut h).1.expect("a constraint").token;
    decide(&mut h, token, ConstraintChange::Start);
    assert_eq!(locked_events(&mut h, c.index), vec!["Locked"]);
    decide(&mut h, token, ConstraintChange::Stop(ConstraintEnd::Focus));
    assert_eq!(locked_events(&mut h, c.index), vec!["Locked", "Unlocked"]);
    // A lasting one can apply again.
    decide(&mut h, token, ConstraintChange::Start);
    assert_eq!(
        locked_events(&mut h, c.index),
        vec!["Locked", "Unlocked", "Locked"]
    );
    // A decision for a token that never existed is ignored.
    decide(&mut h, ConstraintToken(9999), ConstraintChange::Start);
    assert!(!h.c(c.index).is_disconnected());
}

#[test]
fn a_single_use_lock_is_defunct_after_it_stops_and_the_surface_may_get_a_new_one() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let lock = c.lock(&mut h, None, Lifetime::Oneshot);
    h.roundtrip();
    let token = one_report(&mut h).1.expect("a constraint").token;
    decide(&mut h, token, ConstraintChange::Start);
    decide(
        &mut h,
        token,
        ConstraintChange::Stop(ConstraintEnd::Shortcut),
    );
    assert_eq!(locked_events(&mut h, c.index), vec!["Locked", "Unlocked"]);
    // Defunct: decisions and requests do nothing.
    decide(&mut h, token, ConstraintChange::Start);
    lock.set_cursor_position_hint(5.0, 5.0);
    c.window.surface.commit();
    h.roundtrip();
    assert_eq!(locked_events(&mut h, c.index), vec!["Locked", "Unlocked"]);
    assert!(reports(&h.server.take_window_events()).is_empty());
    // A new one on the same surface is accepted (M2).
    let _again = c.lock(&mut h, None, Lifetime::Oneshot);
    h.roundtrip();
    let fresh = one_report(&mut h).1.expect("a constraint").token;
    assert_ne!(fresh, token);
    assert!(!h.c(c.index).is_disconnected());
}

#[test]
fn destroying_the_surface_ends_its_lock_and_later_requests_are_ignored() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let lock = c.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    let token = one_report(&mut h).1.expect("a constraint").token;
    decide(&mut h, token, ConstraintChange::Start);
    c.window.toplevel.destroy();
    c.window.xdg.destroy();
    c.window.surface.destroy();
    h.roundtrip();
    let events = h.server.take_window_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, WindowEvent::Unmapped { id } if *id == c.id)),
        "{events:?}"
    );
    lock.set_cursor_position_hint(1.0, 1.0);
    lock.destroy();
    decide(
        &mut h,
        token,
        ConstraintChange::Stop(ConstraintEnd::WindowGone),
    );
    h.roundtrip();
    assert!(!h.c(c.index).is_disconnected());
}

#[test]
fn a_second_constraint_on_the_same_surface_is_an_error_for_that_client_only() {
    let mut h = Harness::new();
    let a = lock_client(&mut h);
    let b = lock_client(&mut h);
    let _lock = a.lock(&mut h, None, Lifetime::Persistent);
    let _again = a.confine(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    assert_eq!(h.c(a.index).error().map(|e| e.code), Some(1));
    let _ok = b.lock(&mut h, None, Lifetime::Persistent);
    h.roundtrip();
    assert!(!h.c(b.index).is_disconnected());
}

#[test]
fn regions_and_release_points_at_the_limits_are_handled() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let region = c.region(&mut h, &[(i32::MIN, i32::MIN, i32::MAX, i32::MAX)]);
    region.add(0, 0, i32::MAX, i32::MAX);
    let lock = c.lock(&mut h, Some(&region), Lifetime::Persistent);
    lock.set_cursor_position_hint(8_000_000.0, -8_000_000.0);
    c.window.surface.commit();
    h.roundtrip();
    let events = h.server.take_window_events();
    let info = reports(&events)
        .last()
        .and_then(|(_, i)| i.clone())
        .expect("a constraint");
    let min = f64::from(i32::MIN);
    let max = f64::from(i32::MAX);
    assert_eq!(
        info.area,
        Some(vec![r(min, min, max, max), r(0.0, 0.0, max, max)])
    );
    assert_eq!(
        info.hint, None,
        "a release point outside the window is dropped"
    );
    assert!(!h.c(c.index).is_disconnected());
}

// --- Confinements -------------------------------------------------------------------------------

#[test]
fn a_confinement_is_reported_and_its_region_applies_on_the_next_commit() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let region = c.region(&mut h, &[(10, 10, 40, 40)]);
    let confine = c.confine(&mut h, Some(&region), Lifetime::Persistent);
    h.roundtrip();
    let info = one_report(&mut h).1.expect("a constraint");
    assert_eq!(info.kind, ConstraintKind::Confine);
    assert!(info.persistent);
    assert_eq!(info.area, Some(vec![r(10.0, 10.0, 40.0, 40.0)]));
    let bigger = c.region(&mut h, &[(0, 0, 80, 80)]);
    confine.set_region(Some(&bigger));
    h.roundtrip();
    assert!(
        reports(&h.server.take_window_events()).is_empty(),
        "not before the commit"
    );
    c.window.surface.commit();
    h.roundtrip();
    assert_eq!(
        one_report(&mut h).1.expect("a constraint").area,
        Some(vec![r(0.0, 0.0, 80.0, 80.0)])
    );
    confine.set_region(None);
    c.window.surface.commit();
    h.roundtrip();
    assert_eq!(one_report(&mut h).1.expect("a constraint").area, None);
    // Destroyed: reported gone; the shell decides nothing for it.
    confine.destroy();
    h.roundtrip();
    assert_eq!(one_report(&mut h), (c.id, None));
}

#[test]
fn the_shells_decisions_send_confined_and_unconfined_and_end_a_single_use_one() {
    let mut h = Harness::new();
    let c = lock_client(&mut h);
    let confine = c.confine(&mut h, None, Lifetime::Oneshot);
    h.roundtrip();
    let token = one_report(&mut h).1.expect("a constraint").token;
    decide(&mut h, token, ConstraintChange::Start);
    decide(
        &mut h,
        token,
        ConstraintChange::Stop(ConstraintEnd::PointerLeft),
    );
    assert_eq!(
        confined_events(&mut h, c.index),
        vec!["Confined", "Unconfined"]
    );
    // Defunct.
    decide(&mut h, token, ConstraintChange::Start);
    confine.set_region(None);
    c.window.surface.commit();
    h.roundtrip();
    assert_eq!(
        confined_events(&mut h, c.index),
        vec!["Confined", "Unconfined"]
    );
    assert!(reports(&h.server.take_window_events()).is_empty());
    assert!(!h.c(c.index).is_disconnected());
}
