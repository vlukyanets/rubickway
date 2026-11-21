//! Pointer lock: the shell's rules for relative motion, locks and confinements (pointer-lock
//! spec, research L4-L9, contracts/input.md). Headless: no Wayland, no devices.

use rubickway::canvas::{Camera, Point, Rect, Size, Vector, ViewerId};
use rubickway::input::bindings::{Bindings, KeyCombo, ModKey, SpawnBind};
use rubickway::input::{Button, InputEvent, Keysym, Modifiers, RawModifiers, ScrollSource};
use rubickway::shell::{
    Adjust, Anchor8, AppCursor, AppId, ClientInput, ConstraintChange, ConstraintEnd,
    ConstraintInfo, ConstraintKind, ConstraintToken, CursorName, CursorShape, IRect, PopupId,
    PopupRules, Shell, SizeLimits, WindowCommand, WindowEvent, WindowId,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const KEY_ESC: u32 = 1;
const KEY_TAB: u32 = 15;
const KEY_0: u32 = 11;
const DISPLAY: Size = Size {
    w: 1000.0,
    h: 800.0,
};

fn w(n: u64) -> WindowId {
    WindowId(n)
}

fn t(n: u64) -> ConstraintToken {
    ConstraintToken(n)
}

fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(Point::new(x, y), Size::new(w, h))
}

fn full(size: Size) -> Vec<Rect> {
    vec![Rect::new(Point::new(0.0, 0.0), size)]
}

fn mapped(n: u64, width: f64, height: f64) -> WindowEvent {
    let size = Size::new(width, height);
    WindowEvent::Mapped {
        id: w(n),
        size,
        input: full(size),
        limits: SizeLimits::default(),
        parent: None,
        app: AppId(n),
    }
}

/// One 1000x800 display looking at the canvas origin at 100%, the keyboard on RubickWay, and
/// window 1 (200x100) mapped in the middle: on screen (400, 350) .. (600, 450). The pointer is
/// in the display's centre, window point (100, 50), and has entered the window.
fn shell() -> Shell {
    let mut s = Shell::new();
    s.add_viewer(ViewerId(1), DISPLAY, true);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    moved(&mut s, 0.0, 0.0);
    s.take_commands();
    s
}

fn send(s: &mut Shell, event: InputEvent) -> Vec<ClientInput> {
    s.handle(&event).1
}

/// A device motion of `(dx, dy)`; the unaccelerated delta is half of it, the time 7 us.
fn moved(s: &mut Shell, dx: f64, dy: f64) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::PointerMoved {
            delta: Vector::new(dx, dy),
            unaccel: Vector::new(dx / 2.0, dy / 2.0),
            time_us: 7,
        },
    )
}

fn button(s: &mut Shell, code: u32, pressed: bool) -> Vec<ClientInput> {
    let button = Button::from_code(code);
    send(
        s,
        InputEvent::PointerButton {
            button,
            code,
            pressed,
        },
    )
}

/// A left click: press and release; returns the press's output.
fn click(s: &mut Shell) -> Vec<ClientInput> {
    let out = button(s, BTN_LEFT, true);
    button(s, BTN_LEFT, false);
    out
}

fn key(s: &mut Shell, keycode: u32, sym: Keysym, pressed: bool) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::Key {
            keycode,
            binding_sym: sym,
            pressed,
            time_ms: 0,
        },
    )
}

/// Sets the held modifiers (Super is Mod).
fn mods(s: &mut Shell, logo: bool, shift: bool) {
    send(
        s,
        InputEvent::Modifiers {
            mods: Modifiers {
                logo,
                shift,
                ..Modifiers::NONE
            },
            raw: RawModifiers::default(),
        },
    );
}

/// Mod + `sym`: press and release with Mod held, then Mod released; returns the press's output.
fn shortcut(s: &mut Shell, keycode: u32, sym: Keysym) -> Vec<ClientInput> {
    mods(s, true, false);
    let out = key(s, keycode, sym, true);
    key(s, keycode, sym, false);
    mods(s, false, false);
    out
}

fn release_shortcut(s: &mut Shell) -> Vec<ClientInput> {
    shortcut(s, KEY_ESC, Keysym::Escape)
}

fn info(token: u64, kind: ConstraintKind, persistent: bool) -> ConstraintInfo {
    ConstraintInfo {
        token: t(token),
        kind,
        persistent,
        area: None,
        hint: None,
    }
}

fn report(s: &mut Shell, window: u64, constraint: Option<ConstraintInfo>) -> Vec<ClientInput> {
    s.apply(WindowEvent::ConstraintChanged {
        id: w(window),
        constraint,
    })
}

/// A lock on window `window` with `token`; `persistent`: lasting.
fn lock(s: &mut Shell, window: u64, token: u64, persistent: bool) -> Vec<ClientInput> {
    report(
        s,
        window,
        Some(info(token, ConstraintKind::Lock, persistent)),
    )
}

fn start(token: u64) -> ClientInput {
    ClientInput::Constraint {
        token: t(token),
        change: ConstraintChange::Start,
    }
}

fn stop(token: u64, why: ConstraintEnd) -> ClientInput {
    ClientInput::Constraint {
        token: t(token),
        change: ConstraintChange::Stop(why),
    }
}

fn decisions(out: &[ClientInput]) -> Vec<ClientInput> {
    out.iter()
        .filter(|i| matches!(i, ClientInput::Constraint { .. }))
        .cloned()
        .collect()
}

fn relative(out: &[ClientInput]) -> Vec<(WindowId, Vector, Vector, u64)> {
    out.iter()
        .filter_map(|i| match i {
            ClientInput::RelativeMotion {
                window,
                delta,
                unaccel,
                time_us,
            } => Some((*window, *delta, *unaccel, *time_us)),
            _ => None,
        })
        .collect()
}

fn near(a: Vector, b: Vector) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

fn set_camera(s: &mut Shell, x: f64, y: f64, zoom: f64) {
    s.viewer_mut().set_camera(Camera {
        center: Point::new(x, y),
        zoom,
    });
}

/// Maps window `n` (100x100) at the top-left of the view, (100, 100) .. (200, 200) on screen,
/// away from the pointer; it gets the focus, as every new window does.
fn map_aside(s: &mut Shell, n: u64) {
    let camera = s.viewer().camera();
    set_camera(s, -350.0, -250.0, 1.0);
    s.apply(mapped(n, 100.0, 100.0));
    s.viewer_mut().set_camera(camera);
}

fn pointer(s: &Shell) -> Point {
    s.pointer().expect("a pointer").1
}

// --- US1: relative motion (FR-001 - FR-003, research L4) ------------------------------------

#[test]
fn every_motion_over_a_window_is_reported_once_before_the_position() {
    let mut s = shell();
    let out = moved(&mut s, 10.0, 5.0);
    assert_eq!(
        relative(&out),
        vec![(w(1), Vector::new(10.0, 5.0), Vector::new(5.0, 2.5), 7)]
    );
    assert!(
        matches!(
            &out[..],
            [
                ClientInput::RelativeMotion { .. },
                ClientInput::PointerMotion { .. },
                ClientInput::PointerFrame { .. }
            ]
        ),
        "{out:?}"
    );
}

#[test]
fn a_held_button_keeps_reporting_to_its_window_outside_it() {
    let mut s = shell();
    button(&mut s, BTN_LEFT, true);
    moved(&mut s, 300.0, 0.0);
    let out = moved(&mut s, 20.0, 0.0);
    assert_eq!(
        relative(&out),
        vec![(w(1), Vector::new(20.0, 0.0), Vector::new(10.0, 0.0), 7)]
    );
}

#[test]
fn the_accelerated_movement_follows_the_zoom_and_the_raw_one_does_not() {
    for zoom in [0.5, 2.0] {
        let mut s = shell();
        set_camera(&mut s, 0.0, 0.0, zoom);
        moved(&mut s, 0.0, 0.0);
        let out = moved(&mut s, 10.0, -4.0);
        let got = relative(&out);
        assert_eq!(got.len(), 1, "{out:?}");
        assert!(
            near(got[0].1, Vector::new(10.0 / zoom, -4.0 / zoom)),
            "{got:?}"
        );
        assert!(near(got[0].2, Vector::new(5.0, -2.0)), "{got:?}");
    }
}

#[test]
fn a_screen_edge_does_not_shorten_the_movement() {
    let mut s = Shell::new();
    s.add_viewer(ViewerId(1), DISPLAY, true);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 1000.0, 800.0));
    moved(&mut s, 0.0, 0.0);
    let out = moved(&mut s, 10_000.0, 0.0);
    assert_eq!(relative(&out)[0].1, Vector::new(10_000.0, 0.0));
    let at_edge = pointer(&s);
    let out = moved(&mut s, 50.0, 0.0);
    assert_eq!(pointer(&s), at_edge, "the pointer stays at the edge");
    assert_eq!(
        relative(&out),
        vec![(w(1), Vector::new(50.0, 0.0), Vector::new(25.0, 0.0), 7)]
    );
}

#[test]
fn nothing_is_reported_over_the_background_or_during_a_gesture() {
    let mut s = shell();
    // The motion leaving the window still belongs to it; the next ones are over the background.
    moved(&mut s, 300.0, 0.0);
    assert!(relative(&moved(&mut s, 10.0, 0.0)).is_empty());
    moved(&mut s, -310.0, 0.0);

    // A pan (Mod + left).
    mods(&mut s, true, false);
    button(&mut s, BTN_LEFT, true);
    assert!(relative(&moved(&mut s, 30.0, 0.0)).is_empty());
    button(&mut s, BTN_LEFT, false);
    // A move (Mod+Shift + left) and a resize (Mod + right).
    mods(&mut s, true, true);
    button(&mut s, BTN_LEFT, true);
    assert!(relative(&moved(&mut s, 30.0, 0.0)).is_empty());
    button(&mut s, BTN_LEFT, false);
    mods(&mut s, true, false);
    button(&mut s, BTN_RIGHT, true);
    assert!(relative(&moved(&mut s, 30.0, 0.0)).is_empty());
    button(&mut s, BTN_RIGHT, false);
    mods(&mut s, false, false);
}

#[test]
fn an_absolute_position_gives_no_relative_motion() {
    let mut s = shell();
    let out = send(
        &mut s,
        InputEvent::PointerMotion {
            pos: Point::new(510.0, 400.0),
        },
    );
    assert!(relative(&out).is_empty(), "{out:?}");
}

// --- US1: locks (FR-004, FR-006 - FR-008, research L5 - L7, L9) -----------------------------

#[test]
fn a_lock_holds_the_pointer_and_leaves_only_relative_motion_buttons_and_scrolling() {
    let mut s = shell();
    let out = lock(&mut s, 1, 1, true);
    assert_eq!(decisions(&out), vec![start(1)]);
    let before = pointer(&s);
    let out = moved(&mut s, 30.0, 0.0);
    assert_eq!(pointer(&s), before);
    assert!(
        matches!(
            &out[..],
            [
                ClientInput::RelativeMotion { window, .. },
                ClientInput::PointerFrame { .. }
            ] if *window == w(1)
        ),
        "{out:?}"
    );
    let out = button(&mut s, BTN_LEFT, true);
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerButton { window, .. } if *window == w(1))),
        "{out:?}"
    );
    button(&mut s, BTN_LEFT, false);
    let out = send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, 10.0),
            value120: Some(Vector::new(0.0, 120.0)),
        },
    );
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::Scroll { window, .. } if *window == w(1))),
        "{out:?}"
    );
}

#[test]
fn a_lock_waits_for_the_focus_and_the_pointer() {
    let mut s = shell();
    map_aside(&mut s, 2);
    // Window 2 has the focus.
    assert!(decisions(&lock(&mut s, 1, 1, true)).is_empty());
    assert!(decisions(&moved(&mut s, 5.0, 0.0)).is_empty());
    // The click that focuses window 1 is delivered to it; then the lock applies.
    assert_eq!(decisions(&click(&mut s)), vec![start(1)]);

    // The pointer outside the area: nothing until it is inside.
    let mut s = shell();
    moved(&mut s, 300.0, 0.0);
    assert!(decisions(&lock(&mut s, 1, 2, false)).is_empty());
    assert_eq!(decisions(&moved(&mut s, -300.0, 0.0)), vec![start(2)]);
}

#[test]
fn a_focus_change_ends_a_single_use_lock_and_pauses_a_lasting_one() {
    for persistent in [false, true] {
        let mut s = shell();
        map_aside(&mut s, 2);
        click(&mut s);
        lock(&mut s, 1, 1, persistent);
        mods(&mut s, true, false);
        let out = key(&mut s, KEY_TAB, Keysym::Tab, true);
        key(&mut s, KEY_TAB, Keysym::Tab, false);
        assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::Focus)]);
        let out = key(&mut s, KEY_TAB, Keysym::Tab, true);
        key(&mut s, KEY_TAB, Keysym::Tab, false);
        mods(&mut s, false, false);
        assert_eq!(s.windows().focus(), Some(w(1)));
        let expected = if persistent { vec![start(1)] } else { vec![] };
        assert_eq!(decisions(&out), expected, "persistent {persistent}");
    }
}

#[test]
fn a_new_window_or_the_window_closing_stops_a_lock() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = s.apply(mapped(2, 50.0, 50.0));
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::Focus)]);

    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::WindowGone)]);
}

#[test]
fn mod_escape_releases_a_lock_and_a_lasting_one_returns_on_a_click_it_receives() {
    let mut s = shell();
    map_aside(&mut s, 2);
    click(&mut s);
    lock(&mut s, 1, 1, true);
    let out = release_shortcut(&mut s);
    assert_eq!(out, vec![stop(1, ConstraintEnd::Shortcut)]);
    // The pointer moves freely now.
    let before = pointer(&s);
    moved(&mut s, 5.0, 0.0);
    assert_ne!(pointer(&s), before);
    // A focus change and return does not bring it back.
    shortcut(&mut s, KEY_TAB, Keysym::Tab);
    let out = shortcut(&mut s, KEY_TAB, Keysym::Tab);
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert!(decisions(&out).is_empty(), "{out:?}");
    // Nor does a Mod + click, which pans.
    mods(&mut s, true, false);
    let out = button(&mut s, BTN_LEFT, true);
    button(&mut s, BTN_LEFT, false);
    mods(&mut s, false, false);
    assert!(decisions(&out).is_empty(), "{out:?}");
    // A click the window receives does.
    assert_eq!(decisions(&click(&mut s)), vec![start(1)]);
}

#[test]
fn after_mod_escape_a_lock_asked_for_again_at_once_waits_for_a_click() {
    // An application that asks for a new single-use lock as soon as one ends (M11, SC-002).
    let mut s = shell();
    lock(&mut s, 1, 1, false);
    assert_eq!(
        release_shortcut(&mut s),
        vec![stop(1, ConstraintEnd::Shortcut)]
    );
    assert!(decisions(&lock(&mut s, 1, 2, false)).is_empty());
    let before = pointer(&s);
    moved(&mut s, 5.0, 0.0);
    assert_ne!(pointer(&s), before, "the pointer is free");
    assert_eq!(decisions(&click(&mut s)), vec![start(2)]);
}

#[test]
fn mod_escape_ends_a_single_use_lock_for_good() {
    let mut s = shell();
    lock(&mut s, 1, 1, false);
    assert_eq!(
        release_shortcut(&mut s),
        vec![stop(1, ConstraintEnd::Shortcut)]
    );
    assert!(decisions(&click(&mut s)).is_empty());
    assert!(release_shortcut(&mut s).is_empty(), "nothing to release");
}

#[test]
fn a_released_lock_moves_the_pointer_to_its_release_point_inside_the_area() {
    let mut s = shell();
    let mut with_hint = info(1, ConstraintKind::Lock, true);
    with_hint.hint = Some(Point::new(20.0, 10.0));
    report(&mut s, 1, Some(with_hint));
    let out = release_shortcut(&mut s);
    assert_eq!(
        out,
        vec![
            stop(1, ConstraintEnd::Shortcut),
            ClientInput::PointerMotion {
                window: w(1),
                pos: Point::new(20.0, 10.0)
            },
            ClientInput::PointerFrame { window: w(1) },
        ]
    );
    assert_eq!(pointer(&s), Point::new(420.0, 360.0));

    // A release point outside the window: the pointer stays.
    let mut s = shell();
    let mut outside = info(1, ConstraintKind::Lock, true);
    outside.hint = Some(Point::new(500.0, 10.0));
    report(&mut s, 1, Some(outside));
    let before = pointer(&s);
    let out = release_shortcut(&mut s);
    assert_eq!(out, vec![stop(1, ConstraintEnd::Shortcut)]);
    assert_eq!(pointer(&s), before);
}

#[test]
fn removing_an_applying_lock_moves_the_pointer_without_a_decision() {
    let mut s = shell();
    let mut with_hint = info(1, ConstraintKind::Lock, true);
    with_hint.hint = Some(Point::new(30.0, 20.0));
    report(&mut s, 1, Some(with_hint));
    let out = report(&mut s, 1, None);
    assert_eq!(
        out,
        vec![
            ClientInput::PointerMotion {
                window: w(1),
                pos: Point::new(30.0, 20.0)
            },
            ClientInput::PointerFrame { window: w(1) },
        ]
    );
    // The pointer moves freely.
    let before = pointer(&s);
    moved(&mut s, 5.0, 0.0);
    assert_ne!(pointer(&s), before);
}

fn menu(at: (i32, i32), size: (i32, i32)) -> PopupRules {
    PopupRules {
        size,
        anchor_rect: IRect::new(at.0, at.1, 0, 0),
        anchor: Anchor8::BottomRight,
        gravity: Anchor8::BottomRight,
        adjust: Adjust::ALL,
        ..PopupRules::default()
    }
}

/// Shows popup `n` of window 1 at window point `at`, `size` large.
fn show_popup(
    s: &mut Shell,
    n: u64,
    at: (i32, i32),
    size: (i32, i32),
    grab: bool,
) -> Vec<ClientInput> {
    let mut out = s.apply(WindowEvent::PopupRequested {
        id: PopupId(n),
        window: w(1),
        parent: None,
        rules: menu(at, size),
        grab,
    });
    let size = Size::new(f64::from(size.0), f64::from(size.1));
    out.extend(s.apply(WindowEvent::PopupMapped {
        id: PopupId(n),
        at,
        size,
        input: full(size),
    }));
    out
}

#[test]
fn a_grabbing_menu_stops_a_lock_and_a_lasting_one_returns_when_it_closes() {
    for persistent in [false, true] {
        let mut s = shell();
        lock(&mut s, 1, 1, persistent);
        let out = show_popup(&mut s, 1, (150, 60), (40, 30), true);
        assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::Focus)]);
        let out = s.apply(WindowEvent::PopupUnmapped { id: PopupId(1) });
        let expected = if persistent { vec![start(1)] } else { vec![] };
        assert_eq!(decisions(&out), expected, "persistent {persistent}");
    }
}

#[test]
fn a_popup_of_the_window_under_the_pointer_does_not_stop_a_lock() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    // A tooltip right under the pointer (window point 100, 50).
    let out = show_popup(&mut s, 1, (90, 40), (40, 40), false);
    assert!(decisions(&out).is_empty(), "{out:?}");
    assert!(decisions(&moved(&mut s, 10.0, 0.0)).is_empty());
}

#[test]
fn a_hidden_window_stops_its_lock_and_a_lasting_one_returns_when_it_is_shown() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::WindowGone)]);
    s.apply(mapped(1, 200.0, 100.0));
    moved(&mut s, 0.0, 0.0);
    assert_eq!(decisions(&lock(&mut s, 1, 1, true)), vec![start(1)]);
}

#[test]
fn a_view_jump_away_from_the_area_stops_a_lock() {
    let mut s = shell();
    // Look a little to the left: the pointer is on window point (40, 50).
    set_camera(&mut s, -60.0, 0.0, 1.0);
    moved(&mut s, 0.0, 0.0);
    let mut left_part = info(1, ConstraintKind::Lock, true);
    left_part.area = Some(vec![r(0.0, 0.0, 60.0, 100.0)]);
    assert_eq!(
        decisions(&report(&mut s, 1, Some(left_part))),
        vec![start(1)]
    );
    // Mod+0: the home view puts window point (100, 50) under the pointer, outside the area.
    let out = shortcut(&mut s, KEY_0, Keysym::_0);
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::PointerLeft)]);
    // Back over the area, the lasting lock applies again.
    assert_eq!(decisions(&moved(&mut s, -70.0, 0.0)), vec![start(1)]);
}

#[test]
fn maximize_keeps_a_lock_if_the_pointer_is_still_over_its_area() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = s.apply(WindowEvent::MaximizeRequested { id: w(1), on: true });
    assert!(decisions(&out).is_empty(), "{out:?}");
    let size = DISPLAY;
    let out = s.apply(WindowEvent::Updated {
        id: w(1),
        size,
        input: full(size),
        limits: SizeLimits::default(),
        configured: true,
    });
    assert!(decisions(&out).is_empty(), "{out:?}");
    let before = pointer(&s);
    moved(&mut s, 40.0, 0.0);
    assert_eq!(pointer(&s), before, "still locked");

    // A lock on the left part only: maximize centres the window under the pointer, at window
    // point (100, 50), outside it.
    let mut s = shell();
    set_camera(&mut s, -60.0, 0.0, 1.0);
    moved(&mut s, 0.0, 0.0);
    let mut left_part = info(1, ConstraintKind::Lock, true);
    left_part.area = Some(vec![r(0.0, 0.0, 60.0, 100.0)]);
    report(&mut s, 1, Some(left_part));
    let out = s.apply(WindowEvent::MaximizeRequested { id: w(1), on: true });
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::PointerLeft)]);
}

#[test]
fn a_console_switch_pauses_a_lasting_lock() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = send(&mut s, InputEvent::FocusLost);
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::Focus)]);
    let out = send(&mut s, InputEvent::FocusGained);
    assert_eq!(decisions(&out), vec![start(1)]);
}

#[test]
fn unplugging_the_monitor_with_the_locked_pointer_stops_the_lock() {
    let a = ViewerId(1);
    let b = ViewerId(2);
    let mut s = Shell::new();
    s.add_viewer(a, DISPLAY, true);
    s.add_viewer(b, DISPLAY, false);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    // On A, under the pointer.
    s.apply(mapped(1, 200.0, 100.0));
    moved(&mut s, 0.0, 0.0);
    assert_eq!(decisions(&lock(&mut s, 1, 1, true)), vec![start(1)]);
    let out = s.remove_viewer(a);
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::PointerLeft)]);
    assert_eq!(s.pointer().map(|(v, _)| v), Some(b));
}

#[test]
fn a_new_constraint_replacing_an_applying_one_is_judged_by_itself() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let out = lock(&mut s, 1, 2, false);
    assert_eq!(decisions(&out), vec![start(2)]);
}

// --- US2: confinement (FR-005, FR-014, research L8) -------------------------------------------

/// A confinement of window 1 to `area` (`None`: the whole window).
fn confine(
    s: &mut Shell,
    token: u64,
    persistent: bool,
    area: Option<Vec<Rect>>,
) -> Vec<ClientInput> {
    let mut i = info(token, ConstraintKind::Confine, persistent);
    i.area = area;
    report(s, 1, Some(i))
}

/// Window `n`'s rectangle on the active viewer, in viewer coordinates.
fn on_screen(s: &Shell, n: u64) -> Rect {
    let win = s.windows().get(w(n)).expect("the window");
    let camera = s.viewer().camera();
    let origin = camera.to_logical(win.origin, s.viewer().size());
    Rect::new(
        origin,
        Size::new(win.size.w * camera.zoom, win.size.h * camera.zoom),
    )
}

/// The pointer in window 1's coordinates, at 100% with the home view.
fn local(s: &Shell) -> Point {
    let p = pointer(s);
    Point::new(p.x - 400.0, p.y - 350.0)
}

fn motions(out: &[ClientInput]) -> Vec<Point> {
    out.iter()
        .filter_map(|i| match i {
            ClientInput::PointerMotion { pos, .. } => Some(*pos),
            _ => None,
        })
        .collect()
}

#[test]
fn a_confinement_starts_once_the_pointer_is_inside_its_area() {
    let mut s = shell();
    let left = Some(vec![r(0.0, 0.0, 50.0, 100.0)]);
    assert!(decisions(&confine(&mut s, 1, true, left)).is_empty());
    assert_eq!(decisions(&moved(&mut s, -60.0, 0.0)), vec![start(1)]);
}

#[test]
fn flicks_never_take_a_confined_pointer_out_of_its_window_at_any_zoom() {
    for zoom in [0.5, 1.0, 2.0] {
        let mut s = shell();
        set_camera(&mut s, 0.0, 0.0, zoom);
        moved(&mut s, 0.0, 0.0);
        assert_eq!(decisions(&confine(&mut s, 1, true, None)), vec![start(1)]);
        let window = on_screen(&s, 1);
        for i in 0..100 {
            let angle = f64::from(i) * 0.7;
            let (dx, dy) = (2000.0 * angle.cos(), 2000.0 * angle.sin());
            let out = moved(&mut s, dx, dy);
            assert!(
                window.contains(pointer(&s)),
                "zoom {zoom}, flick {i}: {:?} outside {window:?}",
                pointer(&s)
            );
            let told = relative(&out);
            assert_eq!(told.len(), 1, "{out:?}");
            assert!(
                near(told[0].1, Vector::new(dx / zoom, dy / zoom)),
                "the full movement: {told:?}"
            );
            assert!(decisions(&out).is_empty(), "{out:?}");
        }
    }
}

#[test]
fn a_confined_pointer_slides_along_the_edge_and_the_window_sees_where_it_is() {
    let mut s = shell();
    confine(&mut s, 1, true, None);
    let out = moved(&mut s, 1000.0, 10.0);
    let at = motions(&out);
    assert_eq!(at.len(), 1, "{out:?}");
    assert!((at[0].x - 200.0).abs() < 1e-3 && at[0].x < 200.0, "{at:?}");
    assert!(
        (at[0].y - 60.0).abs() < 1e-9,
        "slides along the edge: {at:?}"
    );
}

#[test]
fn an_area_of_several_rectangles_holds_the_pointer_in_their_union() {
    let mut s = shell();
    let area = vec![r(20.0, 20.0, 50.0, 30.0), r(70.0, 20.0, 50.0, 30.0)];
    assert!(decisions(&confine(&mut s, 1, true, Some(area.clone()))).is_empty());
    // Window point (100, 40) is in the second rectangle.
    assert_eq!(decisions(&moved(&mut s, 0.0, -10.0)), vec![start(1)]);
    for (dx, dy) in [(-500.0, 0.0), (0.0, 300.0), (800.0, -50.0), (-30.0, -400.0)] {
        moved(&mut s, dx, dy);
        let p = local(&s);
        assert!(area.iter().any(|a| a.contains(p)), "{p:?}");
    }
}

#[test]
fn a_smaller_area_moves_the_pointer_to_its_nearest_point() {
    let mut s = shell();
    confine(&mut s, 1, true, None);
    let out = confine(&mut s, 1, true, Some(vec![r(0.0, 0.0, 50.0, 50.0)]));
    assert!(decisions(&out).is_empty(), "it goes on: {out:?}");
    assert!(relative(&out).is_empty(), "{out:?}");
    let at = motions(&out);
    assert_eq!(at.len(), 1, "{out:?}");
    assert!(
        (at[0].x - 50.0).abs() < 1e-3 && (at[0].y - 50.0).abs() < 1e-3,
        "{at:?}"
    );
    let p = local(&s);
    assert!(r(0.0, 0.0, 50.0, 50.0).contains(p), "{p:?}");
}

#[test]
fn an_empty_area_never_applies() {
    let mut s = shell();
    assert!(decisions(&confine(&mut s, 1, true, Some(Vec::new()))).is_empty());
    assert!(decisions(&moved(&mut s, 5.0, 5.0)).is_empty());
}

#[test]
fn an_area_of_10000_rectangles_counts_as_their_bounding_box() {
    let mut s = shell();
    let many: Vec<Rect> = (0..10_000)
        .map(|i| r(f64::from(i % 200), f64::from(i % 100), 1.0, 1.0))
        .collect();
    // Window point (100, 50) is in none of them, but in their bounding box.
    assert_eq!(
        decisions(&confine(&mut s, 1, true, Some(many))),
        vec![start(1)]
    );
    let before = pointer(&s);
    moved(&mut s, 30.0, 0.0);
    assert_eq!(pointer(&s), Point::new(before.x + 30.0, before.y));
}

#[test]
fn mod_escape_and_a_focus_change_free_a_confined_pointer() {
    let mut s = shell();
    confine(&mut s, 1, true, None);
    assert_eq!(
        release_shortcut(&mut s),
        vec![stop(1, ConstraintEnd::Shortcut)]
    );
    moved(&mut s, 1000.0, 0.0);
    assert!(pointer(&s).x > 600.0, "{:?}", pointer(&s));

    let mut s = shell();
    map_aside(&mut s, 2);
    click(&mut s);
    confine(&mut s, 1, false, None);
    let out = shortcut(&mut s, KEY_TAB, Keysym::Tab);
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::Focus)]);
    moved(&mut s, 1000.0, 0.0);
    assert!(pointer(&s).x > 600.0, "{:?}", pointer(&s));
}

// --- US3: the canvas while the pointer is held (FR-009, FR-013, research L6) -----------------

/// The canvas point under the pointer, in window `n`'s coordinates.
fn window_point(s: &Shell, n: u64) -> Point {
    let win = s.windows().get(w(n)).expect("the window");
    let canvas = s.viewer().camera().to_canvas(pointer(s), s.viewer().size());
    Point::new(canvas.x - win.origin.x, canvas.y - win.origin.y)
}

/// Where window point `at` of window `n` is on the active viewer.
fn screen_of(s: &Shell, n: u64, at: Point) -> Point {
    let win = s.windows().get(w(n)).expect("the window");
    let canvas = Point::new(win.origin.x + at.x, win.origin.y + at.y);
    s.viewer().camera().to_logical(canvas, s.viewer().size())
}

/// Within one logical pixel of the screen (SC-004).
fn same_spot(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= 1.0 && (a.y - b.y).abs() <= 1.0
}

fn applies(kind: ConstraintKind) -> impl Fn(&mut Shell) -> Vec<ClientInput> {
    move |s| report(s, 1, Some(info(1, kind, true)))
}

/// What a gesture's outputs may not contain while a constraint is suspended.
fn silent(out: &[ClientInput]) -> bool {
    out.iter().all(|i| {
        !matches!(
            i,
            ClientInput::Constraint { .. }
                | ClientInput::RelativeMotion { .. }
                | ClientInput::PointerMotion { .. }
        )
    })
}

/// A Mod + left drag by `steps` motions.
fn pan(s: &mut Shell, steps: &[(f64, f64)]) -> Vec<ClientInput> {
    mods(s, true, false);
    let mut out = button(s, BTN_LEFT, true);
    for (dx, dy) in steps {
        out.extend(moved(s, *dx, *dy));
    }
    out.extend(button(s, BTN_LEFT, false));
    mods(s, false, false);
    out
}

#[test]
fn a_pan_suspends_a_constraint_silently_and_it_applies_again_on_the_same_window_point() {
    for kind in [ConstraintKind::Lock, ConstraintKind::Confine] {
        for zoom in [0.5, 1.0, 2.0] {
            let mut s = shell();
            set_camera(&mut s, 0.0, 0.0, zoom);
            moved(&mut s, 0.0, 0.0);
            assert_eq!(decisions(&applies(kind)(&mut s)), vec![start(1)]);
            let held = window_point(&s, 1);
            let out = pan(&mut s, &[(-150.0, 40.0), (-150.0, 40.0)]);
            assert!(silent(&out), "{kind:?} at {zoom}: {out:?}");
            assert_ne!(
                s.viewer().camera().center,
                Point::new(0.0, 0.0),
                "it panned"
            );
            assert!(
                same_spot(pointer(&s), screen_of(&s, 1, held)),
                "{kind:?} at {zoom}: {:?} vs {:?}",
                pointer(&s),
                screen_of(&s, 1, held)
            );
            // It still applies: no decision, and a lock holds the pointer again.
            let before = pointer(&s);
            let out = moved(&mut s, 10.0, 0.0);
            assert!(decisions(&out).is_empty(), "{out:?}");
            if kind == ConstraintKind::Lock {
                assert_eq!(pointer(&s), before);
            }
        }
    }
}

#[test]
fn a_window_move_suspends_a_constraint_silently() {
    for kind in [ConstraintKind::Lock, ConstraintKind::Confine] {
        let mut s = shell();
        applies(kind)(&mut s);
        let held = window_point(&s, 1);
        mods(&mut s, true, true);
        let mut out = button(&mut s, BTN_LEFT, true);
        out.extend(moved(&mut s, 120.0, -60.0));
        out.extend(button(&mut s, BTN_LEFT, false));
        mods(&mut s, false, false);
        assert!(silent(&out), "{kind:?}: {out:?}");
        assert_ne!(
            s.windows().get(w(1)).expect("the window").origin,
            Point::new(-100.0, -50.0),
            "it moved"
        );
        assert!(same_spot(pointer(&s), screen_of(&s, 1, held)));
        assert!(decisions(&moved(&mut s, 5.0, 0.0)).is_empty());
    }
}

#[test]
fn after_a_resize_a_confinement_goes_on_inside_its_area_and_stops_outside_it() {
    // Near the right edge, the resize takes the right edge.
    for (back, expected) in [
        (true, vec![]),
        (false, vec![stop(1, ConstraintEnd::Resize)]),
    ] {
        let mut s = shell();
        moved(&mut s, 90.0, 0.0);
        confine(&mut s, 1, true, None);
        mods(&mut s, true, false);
        let mut out = button(&mut s, BTN_RIGHT, true);
        out.extend(moved(&mut s, 100.0, 0.0));
        if back {
            out.extend(moved(&mut s, -100.0, 0.0));
        }
        assert!(silent(&out), "{out:?}");
        let out = button(&mut s, BTN_RIGHT, false);
        mods(&mut s, false, false);
        assert_eq!(decisions(&out), expected, "back {back}");
    }
}

fn zoom_in(s: &mut Shell) -> Vec<ClientInput> {
    mods(s, true, false);
    let out = send(
        s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, -10.0),
            value120: Some(Vector::new(0.0, -120.0)),
        },
    );
    mods(s, false, false);
    out
}

#[test]
fn a_zoom_keeps_a_constraint_on_the_window_as_now_shown() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    let held = window_point(&s, 1);
    let out = zoom_in(&mut s);
    assert!(decisions(&out).is_empty(), "{out:?}");
    assert!(s.viewer().camera().zoom > 1.0, "it zoomed");
    assert!(same_spot(pointer(&s), screen_of(&s, 1, held)));

    let mut s = shell();
    confine(&mut s, 1, true, None);
    zoom_in(&mut s);
    zoom_in(&mut s);
    let window = on_screen(&s, 1);
    for (dx, dy) in [(3000.0, 0.0), (0.0, 3000.0), (-3000.0, -3000.0)] {
        moved(&mut s, dx, dy);
        assert!(
            window.contains(pointer(&s)),
            "{:?} in {window:?}",
            pointer(&s)
        );
    }
}

const A: ViewerId = ViewerId(1);
const B: ViewerId = ViewerId(2);

/// Two 1000x800 displays, A left of B, home views at canvas (-500, 0) and (500, 0).
fn two_viewers() -> Shell {
    let mut s = Shell::new();
    s.add_viewer(A, DISPLAY, true);
    s.add_viewer(B, DISPLAY, false);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s
}

#[test]
fn a_confined_window_at_50_percent_on_the_second_monitor_keeps_the_pointer_there() {
    let mut s = two_viewers();
    moved(&mut s, 1000.0, 0.0);
    assert_eq!(s.active(), Some(B));
    set_camera(&mut s, 500.0, 0.0, 0.5);
    s.apply(mapped(1, 200.0, 100.0));
    moved(&mut s, 0.0, 0.0);
    assert_eq!(decisions(&confine(&mut s, 1, true, None)), vec![start(1)]);
    let window = on_screen(&s, 1);
    for (dx, dy) in [(-3000.0, 0.0), (0.0, -3000.0), (3000.0, 3000.0)] {
        moved(&mut s, dx, dy);
        let (on, at) = s.pointer().expect("a pointer");
        assert_eq!(on, B, "never crosses to A");
        assert!(window.contains(at), "{at:?} in {window:?}");
    }
}

#[test]
fn a_window_on_two_monitors_confines_to_its_part_on_the_pointers_monitor() {
    let mut s = two_viewers();
    // Map window 1 (400 wide) across the boundary between A and B: canvas (-200, -50) ..
    // (200, 50), on A from screen x 800.
    set_camera(&mut s, 0.0, 0.0, 1.0);
    s.apply(mapped(1, 400.0, 100.0));
    set_camera(&mut s, -500.0, 0.0, 1.0);
    moved(&mut s, 400.0, 0.0);
    assert_eq!(s.active(), Some(A));
    assert_eq!(decisions(&confine(&mut s, 1, true, None)), vec![start(1)]);
    moved(&mut s, 500.0, 0.0);
    let (on, at) = s.pointer().expect("a pointer");
    assert_eq!(on, A, "stays on A");
    assert!(at.x < 1000.0 && at.x > 999.0, "{at:?}");

    // A lock holds the pointer where it is.
    let mut s = two_viewers();
    set_camera(&mut s, 0.0, 0.0, 1.0);
    s.apply(mapped(1, 400.0, 100.0));
    set_camera(&mut s, -500.0, 0.0, 1.0);
    moved(&mut s, 400.0, 0.0);
    lock(&mut s, 1, 1, true);
    let before = s.pointer();
    moved(&mut s, 500.0, 0.0);
    assert_eq!(s.pointer(), before);
}

#[test]
fn a_confinement_whose_window_left_the_monitor_pauses_and_returns() {
    let mut s = shell();
    confine(&mut s, 1, true, None);
    // The view moves away (as another viewer's camera or a display change could do).
    set_camera(&mut s, 3000.0, 0.0, 1.0);
    let out = moved(&mut s, 1.0, 0.0);
    assert_eq!(decisions(&out), vec![stop(1, ConstraintEnd::PointerLeft)]);
    set_camera(&mut s, 0.0, 0.0, 1.0);
    assert_eq!(decisions(&moved(&mut s, -1.0, 0.0)), vec![start(1)]);
}

#[test]
fn rubickway_shortcuts_work_while_a_lock_applies() {
    let logo = ModKey::Super.modifiers();
    let spawn = SpawnBind {
        combo: KeyCombo {
            mods: logo,
            sym: Keysym::Return,
        },
        command: vec!["kitty".to_string()],
    };
    let mut s = Shell::with_bindings(Bindings::new(ModKey::Super, vec![spawn]));
    s.add_viewer(ViewerId(1), DISPLAY, true);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    moved(&mut s, 0.0, 0.0);
    lock(&mut s, 1, 1, true);
    mods(&mut s, true, false);
    let press = |s: &mut Shell, code: u32, sym: Keysym| {
        let effect = s
            .handle(&InputEvent::Key {
                keycode: code,
                binding_sym: sym,
                pressed: true,
                time_ms: 10,
            })
            .0;
        key(s, code, sym, false);
        effect
    };
    assert_eq!(press(&mut s, 28, Keysym::Return).spawn, Some(0));
    press(&mut s, 16, Keysym::q);
    assert!(
        s.take_commands()
            .contains(&WindowCommand::Close { window: w(1) })
    );
    mods(&mut s, true, true);
    press(&mut s, 16, Keysym::q);
    assert!(press(&mut s, 16, Keysym::q).quit, "the two-press quit");
}

/// Firefox ignores `unlocked` and keeps the pointer hidden (quickstart Q2): after Mod+Escape the
/// user still sees the pointer over that window, until the click that may capture it again.
#[test]
fn after_mod_escape_the_pointer_shows_over_the_window_until_a_click() {
    let mut s = shell();
    click(&mut s);
    lock(&mut s, 1, 1, true);
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Hidden,
    });
    assert_eq!(s.cursor(), CursorShape::Hidden);
    release_shortcut(&mut s);
    assert_eq!(s.cursor(), CursorShape::Default);
    // Asking again to hide it changes nothing until the click.
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Hidden,
    });
    moved(&mut s, 5.0, 0.0);
    assert_eq!(s.cursor(), CursorShape::Default);
    // Other shapes the window asks for still show.
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Shape(CursorName::Text),
    });
    assert_eq!(s.cursor(), CursorShape::Named(CursorName::Text));
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Hidden,
    });
    assert_eq!(s.cursor(), CursorShape::Default);
    // The click it receives captures it again, and the window's choice counts again.
    assert_eq!(decisions(&click(&mut s)), vec![start(1)]);
    assert_eq!(s.cursor(), CursorShape::Hidden);
}

#[test]
fn a_gesture_shows_its_own_cursor_and_the_application_hides_it_again_afterwards() {
    let mut s = shell();
    lock(&mut s, 1, 1, true);
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Hidden,
    });
    assert_eq!(s.cursor(), CursorShape::Hidden);
    mods(&mut s, true, false);
    button(&mut s, BTN_LEFT, true);
    moved(&mut s, 30.0, 0.0);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    button(&mut s, BTN_LEFT, false);
    mods(&mut s, false, false);
    // The window got its pointer back and hides it again, as games do on each enter.
    s.apply(WindowEvent::CursorChanged {
        id: w(1),
        cursor: AppCursor::Hidden,
    });
    assert_eq!(s.cursor(), CursorShape::Hidden);
}
