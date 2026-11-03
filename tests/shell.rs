//! The shell: window events from the server, visibility, input routing and window management.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers look up windows the test just mapped; clippy.toml only exempts #[test] functions"
)]

use proptest::prelude::*;
use rubickway::canvas::{Camera, Point, Rect, Size, Vector, ViewerId};
use rubickway::input::bindings::{
    Bindings, KeyCombo, ModKey, PointerAction, PointerBind, PointerCombo, SpawnBind,
};
use rubickway::input::{Button, InputEvent, Keysym, Modifiers, RawModifiers, ScrollSource};
use rubickway::shell::{
    ClientInput, CursorShape, Edges, Mode, Shell, SizeLimits, States, WindowCommand, WindowEvent,
    WindowId,
};

fn full(size: Size) -> Vec<Rect> {
    vec![Rect::new(Point::new(0.0, 0.0), size)]
}

fn mapped(n: u64, w: f64, h: f64) -> WindowEvent {
    let size = Size::new(w, h);
    WindowEvent::Mapped {
        id: WindowId(n),
        size,
        input: full(size),
        limits: SizeLimits::default(),
        parent: None,
    }
}

/// A shell with one display (viewer 1) of 1000x800 looking at the origin, 100%; the pointer
/// starts at its centre.
fn shell() -> Shell {
    let mut shell = Shell::new();
    shell.add_viewer(ViewerId(1), Size::new(1000.0, 800.0), true);
    shell.place_home_views();
    shell.take_dirty();
    shell
}

#[test]
fn mapped_windows_are_centred_in_the_view_and_mark_dirty() {
    let mut s = shell();
    s.apply(mapped(1, 200.0, 100.0));
    let win = s.windows().get(WindowId(1)).unwrap();
    assert_eq!(win.origin, Point::new(-100.0, -50.0));
    assert!(s.take_dirty());
    assert!(!s.take_dirty());
}

#[test]
fn updated_and_unmapped_change_the_stack_and_mark_dirty() {
    let mut s = shell();
    s.apply(mapped(1, 200.0, 100.0));
    s.take_dirty();
    let size = Size::new(400.0, 300.0);
    s.apply(WindowEvent::Updated {
        id: WindowId(1),
        size,
        input: full(size),
        limits: SizeLimits::default(),
        configured: false,
    });
    assert_eq!(s.windows().get(WindowId(1)).unwrap().size, size);
    assert_eq!(
        s.windows().get(WindowId(1)).unwrap().origin,
        Point::new(-100.0, -50.0)
    );
    assert!(s.take_dirty());
    s.apply(WindowEvent::Unmapped { id: WindowId(1) });
    assert!(s.windows().get(WindowId(1)).is_none());
    assert!(s.take_dirty());
}

#[test]
fn visible_follows_the_camera() {
    let mut s = shell();
    s.apply(mapped(1, 100.0, 100.0));
    // The view covers [-500, 500) x [-400, 400): window 1 is in it.
    assert_eq!(s.visible(), vec![WindowId(1)]);
    // Look 2000 units to the right: window 1 is out of view.
    let mut camera = s.viewer().camera();
    camera.center = Point::new(2000.0, 0.0);
    s.viewer_mut().set_camera(camera);
    assert!(s.visible().is_empty());
}

#[test]
fn view_rect_matches_zoom() {
    let mut s = shell();
    let mut camera = s.viewer().camera();
    camera.zoom = 2.0;
    s.viewer_mut().set_camera(camera);
    assert_eq!(
        s.view_rect(),
        Rect::new(Point::new(-250.0, -200.0), Size::new(500.0, 400.0))
    );
}

// --- US2: routing input to windows -----------------------------------------------------------

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const KEY_A: u32 = 30;
const KEY_Q: u32 = 16;
const KEY_0: u32 = 11;

/// A focused shell with window 1 (200x100) mapped at the view centre: on screen it covers
/// (400, 350) .. (600, 450) at 100%.
fn shell_with_window() -> Shell {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    s
}

fn send(s: &mut Shell, event: InputEvent) -> Vec<ClientInput> {
    s.handle(&event).1
}

fn motion(s: &mut Shell, x: f64, y: f64) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::PointerMotion {
            pos: Point::new(x, y),
        },
    )
}

fn button(s: &mut Shell, button: Button, code: u32, pressed: bool) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::PointerButton {
            button,
            code,
            pressed,
        },
    )
}

fn left(s: &mut Shell, pressed: bool) -> Vec<ClientInput> {
    button(s, Button::Left, BTN_LEFT, pressed)
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

fn mods(s: &mut Shell, mods: Modifiers, depressed: u32) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::Modifiers {
            mods,
            raw: RawModifiers {
                depressed,
                ..RawModifiers::default()
            },
        },
    )
}

/// Mod (Super by default) held.
fn super_mod() -> Modifiers {
    Modifiers {
        logo: true,
        ..Modifiers::NONE
    }
}

fn w(n: u64) -> WindowId {
    WindowId(n)
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

#[test]
fn motion_over_a_window_enters_it_in_window_coordinates() {
    let mut s = shell_with_window();
    let out = motion(&mut s, 450.0, 375.0);
    assert!(
        matches!(&out[..], [ClientInput::PointerEnter { window, pos }, ClientInput::PointerFrame { .. }]
            if *window == w(1) && near(*pos, Point::new(50.0, 25.0))),
        "{out:?}"
    );
    let out = motion(&mut s, 460.0, 380.0);
    assert!(
        matches!(&out[..], [ClientInput::PointerMotion { window, pos }, ClientInput::PointerFrame { .. }]
            if *window == w(1) && near(*pos, Point::new(60.0, 30.0))),
        "{out:?}"
    );
    let out = motion(&mut s, 10.0, 10.0);
    assert!(
        matches!(&out[..], [ClientInput::PointerLeave { window }, ClientInput::PointerFrame { .. }] if *window == w(1)),
        "{out:?}"
    );
}

#[test]
fn window_coordinates_do_not_depend_on_zoom() {
    for zoom in [0.5, 2.0] {
        let mut s = shell_with_window();
        let mut camera = s.viewer().camera();
        camera.zoom = zoom;
        s.viewer_mut().set_camera(camera);
        // Canvas point (-50, -25) is window point (50, 25); on screen it is at
        // (500, 400) + (-50, -25) * zoom.
        let out = motion(&mut s, 500.0 - 50.0 * zoom, 400.0 - 25.0 * zoom);
        assert!(
            out.iter().any(|i| matches!(i, ClientInput::PointerEnter { pos, .. } if near(*pos, Point::new(50.0, 25.0)))),
            "zoom {zoom}: {out:?}"
        );
    }
}

#[test]
fn a_press_on_an_unfocused_window_focuses_raises_and_is_delivered() {
    let mut s = shell_with_window();
    s.apply(mapped(2, 50.0, 50.0)); // centred, on top, focused
    // Click window 1 outside window 2: (410, 360).
    motion(&mut s, 410.0, 360.0);
    let out = left(&mut s, true);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
    assert!(
        out.contains(&ClientInput::PointerButton {
            window: w(1),
            code: BTN_LEFT,
            pressed: true
        }),
        "{out:?}"
    );
    let order: Vec<_> = s.windows().iter().map(|win| win.id).collect();
    assert_eq!(order, vec![w(2), w(1)]);
    assert_eq!(s.windows().focus(), Some(w(1)));
    left(&mut s, false);
    // Right clicks focus too: window 3 opens with focus, a right click moves it to window 1.
    s.apply(mapped(3, 20.0, 20.0));
    motion(&mut s, 410.0, 360.0);
    let out = button(&mut s, Button::Right, BTN_RIGHT, true);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
}

#[test]
fn a_press_grabs_the_pointer_until_the_last_release() {
    let mut s = shell_with_window();
    motion(&mut s, 450.0, 375.0);
    left(&mut s, true);
    // Far outside the window: still delivered, in window coordinates.
    let out = motion(&mut s, 900.0, 700.0);
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerMotion { window, pos }
            if *window == w(1) && near(*pos, Point::new(500.0, 350.0)))),
        "{out:?}"
    );
    // Scrolling during the grab goes to the grabbing window.
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
    assert_eq!(s.viewer().camera().zoom, 1.0, "the canvas did not zoom");
    let out = left(&mut s, false);
    assert!(out.contains(&ClientInput::PointerButton {
        window: w(1),
        code: BTN_LEFT,
        pressed: false
    }));
    // Released outside: the pointer leaves the window.
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerLeave { window } if *window == w(1))),
        "{out:?}"
    );
}

#[test]
fn a_host_pointer_leave_ends_the_grab() {
    let mut s = shell_with_window();
    motion(&mut s, 450.0, 375.0);
    left(&mut s, true);
    send(&mut s, InputEvent::PointerLeave);
    send(
        &mut s,
        InputEvent::PointerEnter {
            pos: Point::new(10.0, 10.0),
        },
    );
    let out = motion(&mut s, 20.0, 20.0);
    assert!(
        !out.iter()
            .any(|i| matches!(i, ClientInput::PointerMotion { .. })),
        "{out:?}"
    );
}

#[test]
fn a_press_outside_the_input_region_falls_through() {
    let mut s = shell_with_window();
    // Window 2 on top, accepting input only in its left half.
    let size = Size::new(200.0, 100.0);
    s.apply(WindowEvent::Mapped {
        id: w(2),
        size,
        input: vec![Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 100.0))],
        limits: SizeLimits::default(),
        parent: None,
    });
    motion(&mut s, 550.0, 400.0); // right half: window 1 below
    let out = left(&mut s, true);
    assert!(
        out.contains(&ClientInput::PointerButton {
            window: w(1),
            code: BTN_LEFT,
            pressed: true
        }),
        "{out:?}"
    );
}

/// The modifiers the client of `window` is told before it gets the keyboard, if any.
fn modifiers_before_focus(out: &[ClientInput], window: WindowId) -> Option<RawModifiers> {
    let focus = out
        .iter()
        .position(|i| *i == ClientInput::KeyboardFocus(Some(window)))?;
    out[..focus].iter().rev().find_map(|i| match i {
        ClientInput::Modifiers { window: w, raw } if *w == window => Some(*raw),
        _ => None,
    })
}

#[test]
fn a_new_window_gets_the_current_modifiers_even_if_they_changed_without_focus() {
    // Super+Q closes the last window while Super is held; Super is released with no window
    // focused; the next window must not believe Super is still down.
    let mut s = shell_with_window();
    mods(&mut s, super_mod(), 64);
    s.apply(WindowEvent::Unmapped { id: w(1) });
    assert!(
        mods(&mut s, Modifiers::NONE, 0).is_empty(),
        "no window to tell"
    );
    let out = s.apply(mapped(2, 100.0, 50.0));
    assert_eq!(
        modifiers_before_focus(&out, w(2)),
        Some(RawModifiers::default()),
        "{out:?}"
    );
    // The same when the keyboard comes back after a console switch.
    mods(&mut s, Modifiers::NONE, 2);
    send(&mut s, InputEvent::FocusLost);
    let out = send(&mut s, InputEvent::FocusGained);
    assert_eq!(
        modifiers_before_focus(&out, w(2)).map(|r| r.depressed),
        Some(2),
        "{out:?}"
    );
}

#[test]
fn keys_go_to_the_focused_window_and_shortcuts_are_consumed() {
    let mut s = shell_with_window();
    let out = key(&mut s, KEY_A, Keysym::a, true);
    assert_eq!(
        out,
        vec![ClientInput::Key {
            window: w(1),
            keycode: KEY_A,
            pressed: true
        }]
    );
    key(&mut s, KEY_A, Keysym::a, false);

    let out = mods(&mut s, super_mod(), 8);
    assert!(
        out.iter().any(|i| matches!(i, ClientInput::Modifiers { window, raw } if *window == w(1) && raw.depressed == 8)),
        "{out:?}"
    );
    let mut camera = s.viewer().camera();
    camera.center = Point::new(300.0, 0.0);
    s.viewer_mut().set_camera(camera);
    let out = key(&mut s, KEY_0, Keysym::_0, true);
    assert!(out.is_empty(), "Mod+0 press delivered: {out:?}");
    assert_eq!(s.viewer().camera().center, Point::new(0.0, 0.0), "reset");
    let out = key(&mut s, KEY_0, Keysym::_0, false);
    assert!(out.is_empty(), "Mod+0 release delivered: {out:?}");

    let shift_alt = Modifiers {
        shift: true,
        ..super_mod()
    };
    mods(&mut s, shift_alt, 9);
    // Quitting takes two presses (FR-031); neither is delivered.
    let (effect, out) = quit_press(&mut s, 0);
    assert!(!effect.quit);
    assert!(out.is_empty(), "{out:?}");
    let (effect, out) = quit_press(&mut s, 300);
    assert!(effect.quit);
    assert!(out.is_empty(), "{out:?}");
}

/// One Mod+Shift+Q press and release at `t` ms (Mod+Shift must be held already).
fn quit_press(s: &mut Shell, t: u32) -> (rubickway::shell::Effect, Vec<ClientInput>) {
    let pressed = s.handle(&InputEvent::Key {
        keycode: KEY_Q,
        binding_sym: Keysym::q,
        pressed: true,
        time_ms: t,
    });
    let released = s.handle(&InputEvent::Key {
        keycode: KEY_Q,
        binding_sym: Keysym::q,
        pressed: false,
        time_ms: t + 50,
    });
    let mut out = pressed.1;
    out.extend(released.1);
    (pressed.0, out)
}

/// What giving window `n` the keyboard sends, with no modifier held: the modifiers first,
/// then the focus.
fn focus_to(n: u64) -> Vec<ClientInput> {
    vec![
        ClientInput::Modifiers {
            window: w(n),
            raw: RawModifiers::default(),
        },
        ClientInput::KeyboardFocus(Some(w(n))),
    ]
}

#[test]
fn a_new_window_takes_focus_even_while_typing() {
    let mut s = shell_with_window();
    key(&mut s, KEY_A, Keysym::a, true);
    let out = s.apply(mapped(2, 50.0, 50.0));
    assert_eq!(out, focus_to(2));
}

#[test]
fn focus_moves_to_the_topmost_window_when_the_focused_one_goes() {
    let mut s = shell_with_window();
    s.apply(mapped(2, 50.0, 50.0));
    let out = s.apply(WindowEvent::Unmapped { id: w(2) });
    assert_eq!(out, focus_to(1));
    let out = s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(out, vec![ClientInput::KeyboardFocus(None)]);
}

#[test]
fn background_presses_never_change_focus() {
    let mut s = shell_with_window();
    motion(&mut s, 10.0, 10.0);
    for (b, code) in [(Button::Left, BTN_LEFT), (Button::Right, BTN_RIGHT)] {
        let out = button(&mut s, b, code, true);
        assert!(out.is_empty(), "{out:?}");
        let out = button(&mut s, b, code, false);
        assert!(out.is_empty(), "{out:?}");
    }
    assert_eq!(s.windows().focus(), Some(w(1)));
}

#[test]
fn the_viewer_losing_and_regaining_focus_is_passed_on() {
    let mut s = shell_with_window();
    let out = send(&mut s, InputEvent::FocusLost);
    assert!(out.contains(&ClientInput::KeyboardFocus(None)), "{out:?}");
    let out = send(&mut s, InputEvent::FocusGained);
    assert_eq!(out, focus_to(1));
}

#[test]
fn unmapping_the_grabbing_window_ends_the_grab_and_drops_its_keys() {
    let mut s = shell_with_window();
    motion(&mut s, 450.0, 375.0);
    left(&mut s, true);
    key(&mut s, KEY_A, Keysym::a, true);
    s.apply(WindowEvent::Unmapped { id: w(1) });
    // The release of the held key goes nowhere; the pointer is free again.
    let out = key(&mut s, KEY_A, Keysym::a, false);
    assert!(out.is_empty(), "{out:?}");
    s.apply(mapped(2, 200.0, 100.0));
    let out = motion(&mut s, 460.0, 380.0);
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerEnter { window, .. } if *window == w(2))),
        "{out:?}"
    );
}

// --- US3: navigating over windows ------------------------------------------------------------

/// No pointer or keyboard input for any window, apart from the pointer leaving.
fn only_leaves(out: &[ClientInput]) -> bool {
    out.iter().all(|i| {
        matches!(
            i,
            ClientInput::PointerLeave { .. } | ClientInput::PointerFrame { .. }
        )
    })
}

#[test]
fn mod_shift_drag_over_a_window_pans_without_input_focus_or_raise() {
    let mut s = shell_with_window();
    s.apply(mapped(2, 50.0, 50.0)); // focused and on top, at the centre
    motion(&mut s, 410.0, 360.0); // over window 1 only
    mods(&mut s, super_shift(), 65);
    let anchor = s
        .viewer()
        .camera()
        .to_canvas(Point::new(410.0, 360.0), s.viewer().size());
    let out = left(&mut s, true);
    assert!(only_leaves(&out), "{out:?}");
    let out = motion(&mut s, 300.0, 200.0);
    assert!(out.is_empty(), "{out:?}");
    let grabbed = s.viewer().camera().to_logical(anchor, s.viewer().size());
    assert!(
        near(grabbed, Point::new(300.0, 200.0)),
        "the canvas followed"
    );
    // Releasing Mod mid-drag keeps the pan going.
    mods(&mut s, Modifiers::NONE, 0);
    let out = motion(&mut s, 250.0, 150.0);
    assert!(out.is_empty(), "{out:?}");
    let grabbed = s.viewer().camera().to_logical(anchor, s.viewer().size());
    assert!(near(grabbed, Point::new(250.0, 150.0)));
    left(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(2)), "focus unchanged");
    let order: Vec<_> = s.windows().iter().map(|win| win.id).collect();
    assert_eq!(order, vec![w(1), w(2)], "not raised");
}

#[test]
fn mod_scroll_over_a_window_zooms_the_canvas() {
    let mut s = shell_with_window();
    motion(&mut s, 450.0, 375.0);
    mods(&mut s, super_mod(), 8);
    let out = send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, -10.0),
            value120: Some(Vector::new(0.0, -120.0)),
        },
    );
    assert!(
        !out.iter().any(|i| matches!(i, ClientInput::Scroll { .. })),
        "{out:?}"
    );
    assert!((s.viewer().camera().zoom - 1.1).abs() < 1e-9);
}

#[test]
fn a_pinch_over_a_window_zooms_the_canvas() {
    let mut s = shell_with_window();
    motion(&mut s, 450.0, 375.0);
    let mut out = send(&mut s, InputEvent::PinchBegin);
    out.extend(send(&mut s, InputEvent::PinchUpdate { scale: 2.0 }));
    out.extend(send(&mut s, InputEvent::PinchEnd { cancelled: false }));
    assert!(out.is_empty(), "{out:?}");
    assert!((s.viewer().camera().zoom - 2.0).abs() < 1e-9);
}

#[test]
fn after_a_canvas_pan_the_pointer_enters_the_window_under_it() {
    let mut s = shell_with_window();
    motion(&mut s, 10.0, 10.0); // background
    left(&mut s, true);
    motion(&mut s, 20.0, 20.0);
    // Pan ends with the pointer over a window: it enters.
    s.viewer_mut()
        .set_camera(rubickway::canvas::Camera::default());
    let mut camera = s.viewer().camera();
    // Canvas (0, 0), inside window 1, now under screen (20, 20).
    camera.center = Point::new(480.0, 380.0);
    s.viewer_mut().set_camera(camera);
    let out = left(&mut s, false);
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerEnter { window, .. } if *window == w(1))),
        "{out:?}"
    );
}

#[test]
fn background_drag_and_scroll_still_navigate() {
    let mut s = shell_with_window();
    motion(&mut s, 10.0, 10.0);
    let anchor = s
        .viewer()
        .camera()
        .to_canvas(Point::new(10.0, 10.0), s.viewer().size());
    left(&mut s, true);
    let out = motion(&mut s, 60.0, 40.0);
    assert!(out.is_empty(), "{out:?}");
    left(&mut s, false);
    let grabbed = s.viewer().camera().to_logical(anchor, s.viewer().size());
    assert!(near(grabbed, Point::new(60.0, 40.0)));
    let out = send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, -10.0),
            value120: Some(Vector::new(0.0, -120.0)),
        },
    );
    assert!(out.is_empty(), "{out:?}");
    assert!((s.viewer().camera().zoom - 1.1).abs() < 1e-9);
}

// --- Window management: foundation -----------------------------------------------------------

#[test]
fn the_cursor_is_grabbing_only_while_panning() {
    let mut s = shell_with_window();
    motion(&mut s, 10.0, 10.0);
    assert_eq!(s.cursor(), CursorShape::Default);
    left(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    left(&mut s, false);
    assert_eq!(s.cursor(), CursorShape::Default);
}

#[test]
fn commands_come_out_of_the_queue_once() {
    let mut s = shell();
    s.apply(WindowEvent::Created {
        id: w(7),
        parent: None,
        limits: SizeLimits::default(),
        maximized: false,
        fullscreen: false,
    });
    assert_eq!(
        s.take_commands(),
        vec![WindowCommand::Configure {
            window: w(7),
            size: None,
            states: States::default(),
        }]
    );
    assert!(s.take_commands().is_empty());
}

// --- US1: moving windows -----------------------------------------------------------------------

/// Mod and Shift held.
fn super_shift() -> Modifiers {
    Modifiers {
        logo: true,
        shift: true,
        ..Modifiers::NONE
    }
}

fn origin(s: &Shell, n: u64) -> Point {
    s.windows().get(w(n)).unwrap().origin
}

fn stack(s: &Shell) -> Vec<WindowId> {
    s.windows().iter().map(|win| win.id).collect()
}

fn has_button(out: &[ClientInput]) -> bool {
    out.iter()
        .any(|i| matches!(i, ClientInput::PointerButton { .. }))
}

/// Window 1 (200x100) with window 2 (100x50) on top, both centred: window 1 is exposed at
/// screen (410, 360); window 2 is focused.
fn shell_with_two_windows() -> Shell {
    let mut s = shell_with_window();
    s.apply(mapped(2, 100.0, 50.0));
    s
}

/// Starts a Mod + left move (the default `move-window` binding) at screen (x, y).
fn start_move(s: &mut Shell, x: f64, y: f64) -> Vec<ClientInput> {
    motion(s, x, y);
    mods(s, super_mod(), 64);
    left(s, true)
}

#[test]
fn the_move_gesture_focuses_raises_and_hides_the_press() {
    let mut s = shell_with_two_windows();
    let out = start_move(&mut s, 410.0, 360.0);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
    assert!(
        out.contains(&ClientInput::PointerLeave { window: w(1) }),
        "{out:?}"
    );
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(stack(&s), vec![w(2), w(1)]);
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(s.cursor(), CursorShape::Move);
    assert!(!s.viewer().is_panning());
}

#[test]
fn a_move_follows_the_pointer_and_ends_on_release() {
    let mut s = shell_with_two_windows();
    start_move(&mut s, 410.0, 360.0);
    let out = motion(&mut s, 510.0, 400.0);
    assert!(out.is_empty(), "{out:?}");
    assert!(near(origin(&s, 1), Point::new(0.0, -10.0)));
    let camera = s.viewer().camera();
    let out = left(&mut s, false);
    assert!(!has_button(&out), "{out:?}");
    assert!(
        out.contains(&ClientInput::PointerEnter {
            window: w(1),
            pos: Point::new(10.0, 10.0)
        }),
        "{out:?}"
    );
    assert_eq!(s.cursor(), CursorShape::Default);
    motion(&mut s, 600.0, 450.0);
    assert!(
        near(origin(&s, 1), Point::new(0.0, -10.0)),
        "the move ended"
    );
    assert_eq!(s.viewer().camera(), camera, "no pan either");
}

#[test]
fn a_host_pointer_leave_ends_a_move() {
    let mut s = shell_with_window();
    start_move(&mut s, 410.0, 360.0);
    send(&mut s, InputEvent::PointerLeave);
    send(
        &mut s,
        InputEvent::PointerEnter {
            pos: Point::new(450.0, 400.0),
        },
    );
    motion(&mut s, 500.0, 420.0);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    assert_eq!(s.cursor(), CursorShape::Default);
}

#[test]
fn unmapping_the_moved_window_ends_the_move() {
    let mut s = shell_with_two_windows();
    start_move(&mut s, 410.0, 360.0);
    s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(s.cursor(), CursorShape::Default);
    motion(&mut s, 700.0, 700.0);
    assert!(near(origin(&s, 2), Point::new(-50.0, -25.0)));
    left(&mut s, false);
}

#[test]
fn losing_the_keyboard_or_releasing_mod_does_not_end_a_move() {
    let mut s = shell_with_window();
    start_move(&mut s, 410.0, 360.0);
    send(&mut s, InputEvent::FocusLost);
    mods(&mut s, Modifiers::NONE, 0);
    motion(&mut s, 460.0, 380.0);
    assert!(near(origin(&s, 1), Point::new(-50.0, -30.0)));
    assert_eq!(s.cursor(), CursorShape::Move);
}

#[test]
fn mod_left_on_the_background_pans() {
    let mut s = shell_with_window();
    let anchor = s
        .viewer()
        .camera()
        .to_canvas(Point::new(10.0, 10.0), s.viewer().size());
    let out = start_move(&mut s, 10.0, 10.0);
    assert!(out.is_empty(), "{out:?}");
    assert!(s.viewer().is_panning());
    motion(&mut s, 60.0, 40.0);
    let grabbed = s.viewer().camera().to_logical(anchor, s.viewer().size());
    assert!(near(grabbed, Point::new(60.0, 40.0)));
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
}

#[test]
fn mod_scroll_during_a_move_keeps_the_window_under_the_pointer() {
    let mut s = shell_with_window();
    start_move(&mut s, 410.0, 360.0);
    let grabbed_local = Point::new(10.0, 10.0);
    for (x, y) in [(300.0, 300.0), (700.0, 100.0)] {
        motion(&mut s, x, y);
        send(
            &mut s,
            InputEvent::Scroll {
                source: ScrollSource::Wheel,
                delta: Vector::new(0.0, 10.0),
                value120: Some(Vector::new(0.0, 360.0)),
            },
        );
        let camera = s.viewer().camera();
        assert!(camera.zoom < 1.0);
        let o = origin(&s, 1);
        let p = camera.to_logical(
            Point::new(o.x + grabbed_local.x, o.y + grabbed_local.y),
            s.viewer().size(),
        );
        assert!(near(p, Point::new(x, y)), "{p:?} vs ({x}, {y})");
    }
    send(&mut s, InputEvent::PinchBegin);
    send(&mut s, InputEvent::PinchUpdate { scale: 3.0 });
    let camera = s.viewer().camera();
    let o = origin(&s, 1);
    let p = camera.to_logical(
        Point::new(o.x + grabbed_local.x, o.y + grabbed_local.y),
        s.viewer().size(),
    );
    assert!(near(p, Point::new(700.0, 100.0)), "{p:?}");
}

#[test]
fn other_input_during_a_move() {
    let mut s = shell_with_window();
    start_move(&mut s, 410.0, 360.0);
    // A further press is swallowed and the move goes on.
    let out = button(&mut s, Button::Right, BTN_RIGHT, true);
    assert!(out.is_empty(), "{out:?}");
    let out = button(&mut s, Button::Right, BTN_RIGHT, false);
    assert!(out.is_empty(), "{out:?}");
    motion(&mut s, 420.0, 370.0);
    assert!(near(origin(&s, 1), Point::new(-90.0, -40.0)));
    // Scroll without Mod does nothing.
    mods(&mut s, Modifiers::NONE, 0);
    let camera = s.viewer().camera();
    let out = send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Finger,
            delta: Vector::new(0.0, 30.0),
            value120: None,
        },
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(s.viewer().camera(), camera);
    // Mod+0 is consumed and does nothing.
    mods(&mut s, super_mod(), 8);
    let mut moved = s.viewer().camera();
    moved.center = Point::new(5.0, 5.0);
    s.viewer_mut().set_camera(moved);
    let out = key(&mut s, KEY_0, Keysym::_0, true);
    assert!(out.is_empty(), "{out:?}");
    assert!(key(&mut s, KEY_0, Keysym::_0, false).is_empty());
    assert_eq!(s.viewer().camera(), moved);
    // Keys go to the focused window.
    mods(&mut s, Modifiers::NONE, 0);
    let out = key(&mut s, KEY_A, Keysym::a, true);
    assert!(out.contains(&ClientInput::Key {
        window: w(1),
        keycode: KEY_A,
        pressed: true
    }));
    key(&mut s, KEY_A, Keysym::a, false);
    // Mod+Shift+Q still quits, with the same two presses as anywhere (FR-031).
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 0).0.quit);
    assert!(quit_press(&mut s, 500).0.quit);
    assert_eq!(s.cursor(), CursorShape::Move);
}

#[test]
fn a_move_request_during_a_press_starts_a_move() {
    let mut s = shell_with_window();
    motion(&mut s, 410.0, 360.0);
    left(&mut s, true);
    let out = s.apply(WindowEvent::MoveRequested { id: w(1) });
    assert!(
        out.contains(&ClientInput::PointerLeave { window: w(1) }),
        "{out:?}"
    );
    assert_eq!(s.cursor(), CursorShape::Move);
    let out = motion(&mut s, 450.0, 380.0);
    assert!(out.is_empty(), "{out:?}");
    assert!(near(origin(&s, 1), Point::new(-60.0, -30.0)));
    let out = left(&mut s, false);
    assert!(
        !has_button(&out),
        "the release belongs to the move: {out:?}"
    );
}

#[test]
fn move_requests_without_a_held_press_are_ignored() {
    let mut s = shell_with_two_windows();
    motion(&mut s, 410.0, 360.0);
    // No press at all.
    assert!(s.apply(WindowEvent::MoveRequested { id: w(1) }).is_empty());
    motion(&mut s, 450.0, 380.0);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    // A press on window 2, a request for window 1.
    motion(&mut s, 500.0, 400.0);
    left(&mut s, true);
    s.apply(WindowEvent::MoveRequested { id: w(1) });
    motion(&mut s, 520.0, 410.0);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    assert!(near(origin(&s, 2), Point::new(-50.0, -25.0)));
    left(&mut s, false);
    // After the release.
    s.apply(WindowEvent::MoveRequested { id: w(2) });
    motion(&mut s, 560.0, 430.0);
    assert!(near(origin(&s, 2), Point::new(-50.0, -25.0)));
    assert_eq!(s.cursor(), CursorShape::Default);
}

proptest! {
    #[test]
    fn the_grabbed_point_stays_under_the_pointer_at_any_zoom(
        zoom in 0.1f64..=10.0,
        fx in 0.05f64..0.95, fy in 0.05f64..0.95,
        path in prop::collection::vec((0.0f64..1000.0, 0.0f64..800.0), 1..6),
    ) {
        let mut s = shell_with_window();
        let mut camera = s.viewer().camera();
        camera.zoom = zoom;
        s.viewer_mut().set_camera(camera);
        let size = s.viewer().size();
        // A start point inside both the window and the viewer.
        let win = s.windows().get(w(1)).unwrap().rect();
        let tl = camera.to_logical(win.origin, size);
        let br = camera.to_logical(win.end(), size);
        let (x0, x1) = (tl.x.max(1.0), br.x.min(999.0));
        let (y0, y1) = (tl.y.max(1.0), br.y.min(799.0));
        let start = Point::new(x0 + fx * (x1 - x0), y0 + fy * (y1 - y0));
        let grabbed = camera.to_canvas(start, size);
        let offset = Point::new(grabbed.x - win.origin.x, grabbed.y - win.origin.y);
        start_move(&mut s, start.x, start.y);
        for (x, y) in path {
            motion(&mut s, x, y);
            let o = origin(&s, 1);
            let p = s.viewer().camera().to_logical(
                Point::new(o.x + offset.x, o.y + offset.y),
                size,
            );
            prop_assert!((p.x - x).abs() <= 1.0 && (p.y - y).abs() <= 1.0, "{:?} vs ({}, {})", p, x, y);
        }
        // The window moved; the canvas did not pan.
        prop_assert_eq!(s.viewer().camera(), camera);
    }
}

// --- US3: focus cycling ------------------------------------------------------------------------

const KEY_TAB: u32 = 15;

/// Five 200x100 windows centred at canvas (n * 5000, 0), the camera back at the origin: all
/// outside the view. Window 5 is focused; the history is 5, 4, 3, 2, 1.
fn shell_with_five_far_windows() -> Shell {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    for n in 1..=5 {
        let mut camera = s.viewer().camera();
        camera.center = Point::new(n as f64 * 5000.0, 0.0);
        s.viewer_mut().set_camera(camera);
        s.apply(mapped(n, 200.0, 100.0));
    }
    let mut camera = s.viewer().camera();
    camera.center = Point::new(0.0, 0.0);
    s.viewer_mut().set_camera(camera);
    s
}

/// Mod+Tab (or Mod+Shift+Tab with `shift`), press and release, Mod still held afterwards.
fn mod_tab(s: &mut Shell, shift: bool) -> Vec<ClientInput> {
    let held = if shift { super_shift() } else { super_mod() };
    mods(s, held, if shift { 9 } else { 8 });
    let mut out = key(s, KEY_TAB, Keysym::Tab, true);
    out.extend(key(s, KEY_TAB, Keysym::Tab, false));
    out
}

fn release_mod(s: &mut Shell) {
    mods(s, Modifiers::NONE, 0);
}

fn camera_center(s: &Shell) -> Point {
    s.viewer().camera().center
}

#[test]
fn mod_tab_walks_the_history_and_brings_each_window_into_view() {
    let mut s = shell_with_five_far_windows();
    for n in [4, 3, 2, 1, 5, 4] {
        let out = mod_tab(&mut s, false);
        assert!(
            out.contains(&ClientInput::KeyboardFocus(Some(w(n)))),
            "{out:?}"
        );
        assert!(
            !out.iter().any(|i| matches!(i, ClientInput::Key { .. })),
            "{out:?}"
        );
        assert_eq!(s.windows().focus(), Some(w(n)));
        assert_eq!(stack(&s).last(), Some(&w(n)), "raised");
        assert!(near(camera_center(&s), Point::new(n as f64 * 5000.0, 0.0)));
        assert_eq!(s.viewer().camera().zoom, 1.0);
    }
}

#[test]
fn releasing_mod_commits_the_order() {
    let mut s = shell_with_five_far_windows();
    mod_tab(&mut s, false);
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(3)));
    release_mod(&mut s);
    // Two short presses switch back and forth.
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(5)));
    release_mod(&mut s);
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(3)));
    release_mod(&mut s);
}

#[test]
fn losing_the_keyboard_commits_the_order() {
    let mut s = shell_with_five_far_windows();
    mod_tab(&mut s, false);
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(3)));
    send(&mut s, InputEvent::FocusLost);
    send(&mut s, InputEvent::FocusGained);
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(5)));
}

#[test]
fn mod_shift_tab_walks_the_other_way() {
    let mut s = shell_with_five_far_windows();
    mod_tab(&mut s, true);
    assert_eq!(s.windows().focus(), Some(w(1)));
    mod_tab(&mut s, true);
    assert_eq!(s.windows().focus(), Some(w(2)));
    assert!(near(camera_center(&s), Point::new(10_000.0, 0.0)));
}

#[test]
fn a_window_too_large_for_the_view_is_shown_whole() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    let mut camera = s.viewer().camera();
    camera.center = Point::new(9000.0, 0.0);
    s.viewer_mut().set_camera(camera);
    s.apply(mapped(1, 4000.0, 100.0));
    camera.center = Point::new(0.0, 0.0);
    s.viewer_mut().set_camera(camera);
    s.apply(mapped(2, 100.0, 100.0));
    mod_tab(&mut s, false);
    let c = s.viewer().camera();
    assert!(near(c.center, Point::new(9000.0, 0.0)));
    assert!((c.zoom - 0.9 * 1000.0 / 4000.0).abs() < 1e-12);
}

#[test]
fn cycling_with_no_or_one_visible_window_keeps_the_view() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    let camera = s.viewer().camera();
    assert!(mod_tab(&mut s, false).is_empty());
    assert_eq!(s.viewer().camera(), camera);
    let mut s = shell_with_window();
    let camera = s.viewer().camera();
    mod_tab(&mut s, false);
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(s.viewer().camera(), camera);
}

#[test]
fn cycling_is_ignored_during_a_move() {
    let mut s = shell_with_two_windows();
    start_move(&mut s, 410.0, 360.0);
    let camera = s.viewer().camera();
    let out = mod_tab(&mut s, false);
    assert!(out.is_empty(), "{out:?}");
    let out = mod_tab(&mut s, true);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(s.viewer().camera(), camera);
    assert_eq!(s.cursor(), CursorShape::Move);
}

// --- US4: closing windows ----------------------------------------------------------------------

#[test]
fn mod_q_asks_the_focused_window_to_close() {
    let mut s = shell_with_two_windows();
    s.take_commands();
    mods(&mut s, super_mod(), 8);
    let mut out = key(&mut s, KEY_Q, Keysym::q, true);
    out.extend(key(&mut s, KEY_Q, Keysym::q, false));
    assert!(
        !out.iter().any(|i| matches!(i, ClientInput::Key { .. })),
        "{out:?}"
    );
    assert_eq!(
        s.take_commands(),
        vec![WindowCommand::Close { window: w(2) }]
    );
}

#[test]
fn mod_q_without_a_focused_window_does_nothing() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    mods(&mut s, super_mod(), 8);
    key(&mut s, KEY_Q, Keysym::q, true);
    key(&mut s, KEY_Q, Keysym::q, false);
    assert!(s.take_commands().is_empty());
}

#[test]
fn mod_shift_q_still_quits_instead_of_closing() {
    let mut s = shell_with_window();
    s.take_commands();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 0).0.quit);
    assert!(quit_press(&mut s, 100).0.quit);
    assert!(s.take_commands().is_empty());
}

#[test]
fn mod_q_during_a_move_closes_the_moved_window() {
    let mut s = shell_with_two_windows();
    start_move(&mut s, 410.0, 360.0);
    s.take_commands();
    mods(&mut s, super_mod(), 8);
    key(&mut s, KEY_Q, Keysym::q, true);
    assert_eq!(
        s.take_commands(),
        vec![WindowCommand::Close { window: w(1) }]
    );
    assert_eq!(s.cursor(), CursorShape::Move);
    s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(s.cursor(), CursorShape::Default);
}

// --- US5: maximize and fullscreen --------------------------------------------------------------

fn maximized() -> States {
    States {
        maximized: true,
        ..States::default()
    }
}

fn fullscreen() -> States {
    States {
        fullscreen: true,
        ..States::default()
    }
}

fn camera_at(x: f64, y: f64, zoom: f64) -> Camera {
    Camera {
        center: Point::new(x, y),
        zoom,
    }
}

fn request_maximize(s: &mut Shell, n: u64, on: bool) -> Vec<ClientInput> {
    s.apply(WindowEvent::MaximizeRequested { id: w(n), on })
}

fn request_fullscreen(s: &mut Shell, n: u64, on: bool) -> Vec<ClientInput> {
    s.apply(WindowEvent::FullscreenRequested {
        id: w(n),
        on,
        output: None,
    })
}

/// A fullscreen request naming the output of viewer `output`.
fn request_fullscreen_on(s: &mut Shell, n: u64, output: ViewerId) -> Vec<ClientInput> {
    s.apply(WindowEvent::FullscreenRequested {
        id: w(n),
        on: true,
        output: Some(output),
    })
}

fn mode(s: &Shell, n: u64) -> Mode {
    s.windows().get(w(n)).unwrap().mode
}

/// Window 1 maximized and drawn at the viewer size: it covers (-500, -400)..(500, 400) and
/// the camera is at the origin, 100%.
fn shell_with_maximized_window() -> Shell {
    let mut s = shell_with_window();
    request_maximize(&mut s, 1, true);
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    s.take_commands();
    s
}

#[test]
fn maximize_jumps_to_the_window_and_asks_for_the_viewer_size() {
    let mut s = shell_with_window();
    s.viewer_mut().set_camera(camera_at(300.0, 200.0, 2.0));
    s.take_commands();
    request_maximize(&mut s, 1, true);
    assert_eq!(s.viewer().camera(), camera_at(0.0, 0.0, 1.0));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), maximized())]
    );
    assert!(matches!(mode(&s, 1), Mode::Maximized { saved: Some(_) }));
    // Whatever size the application draws stays centred on the window's old centre.
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    assert!(near(origin(&s, 1), Point::new(-500.0, -400.0)));
    answer(&mut s, 1, Size::new(600.0, 400.0), false);
    assert!(near(origin(&s, 1), Point::new(-300.0, -200.0)));
    answer(&mut s, 1, Size::new(1200.0, 900.0), false);
    assert!(near(origin(&s, 1), Point::new(-600.0, -450.0)));
}

#[test]
fn a_maximized_window_stays_while_the_view_moves_and_others_can_be_raised() {
    let mut s = shell_with_two_windows();
    // Focus window 1 with Mod+Tab, then maximize it.
    mod_tab(&mut s, false);
    release_mod(&mut s);
    request_maximize(&mut s, 1, true);
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    assert_eq!(stack(&s).last(), Some(&w(1)), "raised");
    s.take_commands();
    // Pan with Mod+Shift + left drag, then zoom.
    motion(&mut s, 100.0, 100.0);
    mods(&mut s, super_shift(), 65);
    left(&mut s, true);
    motion(&mut s, 300.0, 200.0);
    left(&mut s, false);
    send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, 10.0),
            value120: Some(Vector::new(0.0, 120.0)),
        },
    );
    assert!(matches!(mode(&s, 1), Mode::Maximized { .. }));
    assert!(near(origin(&s, 1), Point::new(-500.0, -400.0)));
    assert!(s.take_commands().is_empty());
    // Raising another window draws it over the maximized one.
    release_mod(&mut s);
    mod_tab(&mut s, false);
    assert_eq!(stack(&s).last(), Some(&w(2)));
    assert!(matches!(mode(&s, 1), Mode::Maximized { .. }));
}

#[test]
fn unmaximize_restores_the_old_place_and_size() {
    let mut s = shell_with_maximized_window();
    request_maximize(&mut s, 1, false);
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    assert_eq!(s.viewer().camera(), camera_at(0.0, 0.0, 1.0), "view stays");
    answer(&mut s, 1, Size::new(200.0, 100.0), true);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), None, States::default())]
    );
}

#[test]
fn resizing_a_maximized_window_continues_from_its_old_size() {
    let mut s = shell_with_maximized_window();
    // (990, 400) is the right middle cell of the maximized window.
    start_resize(&mut s, 990.0, 400.0);
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    motion(&mut s, 1010.0, 400.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(220.0, 100.0)), resizing())]
    );
}

#[test]
fn moving_a_maximized_window_restores_its_size_under_the_pointer() {
    let mut s = shell_with_maximized_window();
    // Grabbed at 3/4 of the width: x offset 750 of 1000 becomes 150 of 200; y offset 400 stays.
    start_move(&mut s, 750.0, 400.0);
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert!(near(origin(&s, 1), Point::new(100.0, -400.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    motion(&mut s, 760.0, 400.0);
    assert!(near(origin(&s, 1), Point::new(110.0, -400.0)));
    // A move request from the application does the same.
    let mut s = shell_with_maximized_window();
    motion(&mut s, 750.0, 400.0);
    left(&mut s, true);
    s.apply(WindowEvent::MoveRequested { id: w(1) });
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert!(near(origin(&s, 1), Point::new(100.0, -400.0)));
}

#[test]
fn a_new_viewer_size_resizes_a_maximized_window() {
    let mut s = shell_with_maximized_window();
    s.add_viewer(ViewerId(1), Size::new(800.0, 600.0), true);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(800.0, 600.0)), maximized())]
    );
}

#[test]
fn repeated_or_unfocused_maximize_requests_change_nothing() {
    let mut s = shell_with_maximized_window();
    request_maximize(&mut s, 1, true);
    assert!(s.take_commands().is_empty());
    let mut s = shell_with_window();
    s.take_commands();
    request_maximize(&mut s, 1, false);
    assert!(s.take_commands().is_empty());
    // Window 2 is focused: window 1 may not maximize (FR-019).
    let mut s = shell_with_two_windows();
    s.take_commands();
    let camera = s.viewer().camera();
    request_maximize(&mut s, 1, true);
    request_fullscreen(&mut s, 1, true);
    assert!(s.take_commands().is_empty());
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.viewer().camera(), camera);
}

#[test]
fn cycling_to_a_maximized_window_shows_its_maximize_view() {
    let mut s = shell_with_five_far_windows();
    request_maximize(&mut s, 5, true);
    answer(&mut s, 5, Size::new(1000.0, 800.0), true);
    s.viewer_mut().set_camera(camera_at(0.0, 0.0, 0.5));
    mod_tab(&mut s, false);
    release_mod(&mut s);
    assert_eq!(s.windows().focus(), Some(w(4)));
    mod_tab(&mut s, false);
    release_mod(&mut s);
    assert_eq!(s.windows().focus(), Some(w(5)));
    assert_eq!(s.viewer().camera(), camera_at(25_000.0, 0.0, 1.0));
}

#[test]
fn fullscreen_shows_only_the_window_and_jumps_to_it() {
    let mut s = shell_with_two_windows();
    s.viewer_mut().set_camera(camera_at(300.0, 200.0, 2.0));
    s.take_commands();
    request_fullscreen(&mut s, 2, true);
    assert_eq!(s.fullscreen(), Some(w(2)));
    assert_eq!(s.visible(), vec![w(2)]);
    assert_eq!(s.viewer().camera(), camera_at(0.0, 0.0, 1.0));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(2), Some(Size::new(1000.0, 800.0)), fullscreen())]
    );
    request_fullscreen(&mut s, 2, false);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.viewer().camera(), camera_at(300.0, 200.0, 2.0));
    assert!(near(origin(&s, 2), Point::new(-50.0, -25.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(2), Some(Size::new(100.0, 50.0)), States::default())]
    );
}

/// Each navigation input leaves fullscreen, restores the view and is consumed.
#[test]
fn navigation_leaves_fullscreen_and_is_consumed() {
    let saved = camera_at(300.0, 200.0, 2.0);
    let scroll = |s: &mut Shell| {
        send(
            s,
            InputEvent::Scroll {
                source: ScrollSource::Wheel,
                delta: Vector::new(0.0, 10.0),
                value120: Some(Vector::new(0.0, 120.0)),
            },
        )
    };
    type Gesture = Box<dyn Fn(&mut Shell) -> Vec<ClientInput>>;
    let cases: Vec<(&str, Gesture)> = vec![
        (
            "Mod + Shift + left (pan) on the window",
            Box::new(|s: &mut Shell| {
                motion(s, 500.0, 400.0);
                mods(s, super_shift(), 65);
                left(s, true)
            }),
        ),
        (
            "Mod + left (move) on the background",
            Box::new(|s: &mut Shell| {
                motion(s, 10.0, 10.0);
                mods(s, super_mod(), 64);
                left(s, true)
            }),
        ),
        (
            "left on the background",
            Box::new(|s: &mut Shell| {
                motion(s, 10.0, 10.0);
                left(s, true)
            }),
        ),
        (
            "Mod + scroll",
            Box::new(move |s: &mut Shell| {
                motion(s, 500.0, 400.0);
                mods(s, super_mod(), 8);
                scroll(s)
            }),
        ),
        (
            "scroll on the background",
            Box::new(move |s: &mut Shell| {
                motion(s, 10.0, 10.0);
                scroll(s)
            }),
        ),
        (
            "pinch",
            Box::new(|s: &mut Shell| send(s, InputEvent::PinchBegin)),
        ),
        (
            "Mod+0",
            Box::new(|s: &mut Shell| {
                mods(s, super_mod(), 8);
                key(s, KEY_0, Keysym::_0, true)
            }),
        ),
    ];
    for (what, gesture) in cases {
        let mut s = shell_with_window();
        s.viewer_mut().set_camera(saved);
        request_fullscreen(&mut s, 1, true);
        s.take_commands();
        let out = gesture(&mut s);
        assert_eq!(s.fullscreen(), None, "{what}");
        assert_eq!(s.viewer().camera(), saved, "{what}");
        assert!(!s.viewer().is_panning(), "{what}");
        assert!(!has_button(&out), "{what}: {out:?}");
        assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)), "{what}");
        assert_eq!(
            configures(&s.take_commands()),
            vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())],
            "{what}"
        );
    }
}

#[test]
fn cycling_leaves_fullscreen_and_goes_straight_to_the_next_window() {
    let mut s = shell_with_five_far_windows();
    request_fullscreen(&mut s, 5, true);
    mod_tab(&mut s, false);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.windows().focus(), Some(w(4)));
    assert_eq!(s.viewer().camera(), camera_at(20_000.0, 0.0, 1.0));
}

#[test]
fn mod_q_closes_the_fullscreen_window_and_its_end_restores_the_view() {
    let mut s = shell_with_window();
    let saved = camera_at(300.0, 200.0, 2.0);
    s.viewer_mut().set_camera(saved);
    request_fullscreen(&mut s, 1, true);
    s.take_commands();
    mods(&mut s, super_mod(), 8);
    key(&mut s, KEY_Q, Keysym::q, true);
    assert_eq!(
        s.take_commands(),
        vec![WindowCommand::Close { window: w(1) }]
    );
    assert_eq!(s.fullscreen(), Some(w(1)));
    s.apply(WindowEvent::Unmapped { id: w(1) });
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.viewer().camera(), saved);
}

#[test]
fn fullscreen_over_maximized_returns_to_maximized() {
    let mut s = shell_with_maximized_window();
    request_fullscreen(&mut s, 1, true);
    let both = States {
        maximized: true,
        fullscreen: true,
        ..States::default()
    };
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), both)]
    );
    s.viewer_mut().set_camera(camera_at(0.0, 0.0, 1.0));
    request_fullscreen(&mut s, 1, false);
    assert!(matches!(mode(&s, 1), Mode::Maximized { saved: Some(_) }));
    assert!(near(origin(&s, 1), Point::new(-500.0, -400.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), maximized())]
    );
    request_maximize(&mut s, 1, false);
    assert!(near(origin(&s, 1), Point::new(-100.0, -50.0)));
}

#[test]
fn moving_a_fullscreen_window_leaves_fullscreen_under_the_pointer() {
    let mut s = shell_with_window();
    request_fullscreen(&mut s, 1, true);
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    s.take_commands();
    start_move(&mut s, 750.0, 400.0);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.cursor(), CursorShape::Move);
    assert!(near(origin(&s, 1), Point::new(100.0, -400.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
}

#[test]
fn resizing_a_fullscreen_window_only_leaves_fullscreen() {
    let mut s = shell_with_window();
    request_fullscreen(&mut s, 1, true);
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    s.take_commands();
    let out = start_resize(&mut s, 990.0, 400.0);
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.cursor(), CursorShape::Default);
    motion(&mut s, 1010.0, 400.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
}

#[test]
fn a_new_viewer_size_resizes_a_fullscreen_window() {
    let mut s = shell_with_window();
    request_fullscreen(&mut s, 1, true);
    s.take_commands();
    s.add_viewer(ViewerId(1), Size::new(800.0, 600.0), true);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(800.0, 600.0)), fullscreen())]
    );
}

fn created(n: u64, parent: Option<u64>, maximized: bool, fullscreen: bool) -> WindowEvent {
    WindowEvent::Created {
        id: w(n),
        parent: parent.map(w),
        limits: SizeLimits::default(),
        maximized,
        fullscreen,
    }
}

fn mapped_with_parent(n: u64, size: Size, parent: Option<u64>) -> WindowEvent {
    WindowEvent::Mapped {
        id: w(n),
        size,
        input: full(size),
        limits: SizeLimits::default(),
        parent: parent.map(w),
    }
}

#[test]
fn a_window_can_open_maximized() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    s.viewer_mut().set_camera(camera_at(300.0, 200.0, 2.0));
    s.apply(created(1, None, true, false));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), maximized())]
    );
    s.apply(mapped_with_parent(1, Size::new(1000.0, 800.0), None));
    assert_eq!(mode(&s, 1), Mode::Maximized { saved: None });
    assert_eq!(s.viewer().camera(), camera_at(300.0, 200.0, 1.0));
    assert!(near(origin(&s, 1), Point::new(-200.0, -200.0)));
    // Leaving it lets the application choose, keeping the centre.
    request_maximize(&mut s, 1, false);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), None, States::default())]
    );
    answer(&mut s, 1, Size::new(300.0, 200.0), true);
    assert!(near(origin(&s, 1), Point::new(150.0, 100.0)));
}

#[test]
fn a_window_can_open_fullscreen_over_maximized() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    s.apply(created(1, None, true, true));
    let both = States {
        maximized: true,
        fullscreen: true,
        ..States::default()
    };
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), both)]
    );
    s.apply(mapped_with_parent(1, Size::new(1000.0, 800.0), None));
    assert_eq!(s.fullscreen(), Some(w(1)));
    assert_eq!(mode(&s, 1), Mode::Maximized { saved: None });
}

#[test]
fn an_initial_state_follows_a_resized_viewer() {
    let mut s = shell();
    s.apply(created(1, None, true, false));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(1000.0, 800.0)), maximized())]
    );
    s.add_viewer(ViewerId(1), Size::new(800.0, 600.0), true);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(800.0, 600.0)), maximized())]
    );
}

/// Window 1 fullscreen; the view before it was at (300, 200), 100%.
fn shell_with_fullscreen_window() -> Shell {
    let mut s = shell_with_window();
    s.viewer_mut().set_camera(camera_at(300.0, 200.0, 1.0));
    request_fullscreen(&mut s, 1, true);
    s.take_commands();
    s
}

#[test]
fn a_new_window_during_fullscreen_opens_behind_without_focus() {
    let mut s = shell_with_fullscreen_window();
    s.apply(created(2, None, true, false));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(2), None, States::default())]
    );
    let out = s.apply(mapped_with_parent(2, Size::new(100.0, 50.0), None));
    assert!(
        !out.iter()
            .any(|i| matches!(i, ClientInput::KeyboardFocus(_))),
        "{out:?}"
    );
    assert_eq!(s.fullscreen(), Some(w(1)));
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(stack(&s).last(), Some(&w(2)));
    assert_eq!(s.windows().mru(), &[w(1), w(2)]);
    assert_eq!(mode(&s, 2), Mode::Normal);
    // Centred in the view fullscreen returns to.
    assert!(near(origin(&s, 2), Point::new(250.0, 175.0)));
    assert_eq!(s.visible(), vec![w(1)]);
}

#[test]
fn a_dialog_of_the_fullscreen_window_ends_fullscreen() {
    let mut s = shell_with_fullscreen_window();
    let out = s.apply(mapped_with_parent(2, Size::new(100.0, 50.0), Some(1)));
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(2)))),
        "{out:?}"
    );
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.viewer().camera(), camera_at(300.0, 200.0, 1.0));
    assert!(near(origin(&s, 2), Point::new(250.0, 175.0)));
}

#[test]
fn a_dialog_asking_for_fullscreen_takes_it_over() {
    let mut s = shell_with_fullscreen_window();
    s.apply(created(2, Some(1), false, true));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(2), Some(Size::new(1000.0, 800.0)), fullscreen())]
    );
    s.apply(mapped_with_parent(2, Size::new(1000.0, 800.0), Some(1)));
    assert_eq!(s.fullscreen(), Some(w(2)));
    request_fullscreen(&mut s, 2, false);
    assert_eq!(s.viewer().camera(), camera_at(300.0, 200.0, 1.0));
}

#[test]
fn an_initial_state_is_dropped_if_the_window_opens_without_focus() {
    let mut s = shell_with_window();
    s.apply(created(2, None, true, false));
    s.take_commands();
    request_fullscreen(&mut s, 1, true);
    s.take_commands();
    s.apply(mapped_with_parent(2, Size::new(1000.0, 800.0), None));
    assert_eq!(mode(&s, 2), Mode::Normal);
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(2), None, States::default())]
    );
}

// --- Convergence ------------------------------------------------------------------------------

#[test]
fn a_host_pointer_leave_ends_a_resize_like_a_release() {
    let mut s = shell_with_window();
    start_resize(&mut s, 590.0, 400.0);
    motion(&mut s, 620.0, 400.0);
    s.take_commands();
    send(&mut s, InputEvent::PointerLeave);
    assert_eq!(s.cursor(), CursorShape::Default);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(230.0, 100.0)), States::default())]
    );
    answer(&mut s, 1, Size::new(230.0, 100.0), true);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), None, States::default())]
    );
    answer(&mut s, 1, Size::new(300.0, 100.0), true);
    assert!(
        near(origin(&s, 1), Point::new(-100.0, -50.0)),
        "top-left kept"
    );
}

/// Window 1 fullscreen and drawn at the viewer size, the view before at the origin, 100%.
fn shell_with_answered_fullscreen() -> Shell {
    let mut s = shell_with_window();
    request_fullscreen(&mut s, 1, true);
    answer(&mut s, 1, Size::new(1000.0, 800.0), true);
    s.take_commands();
    s
}

#[test]
fn a_move_request_from_the_fullscreen_window_leaves_fullscreen() {
    let mut s = shell_with_answered_fullscreen();
    motion(&mut s, 750.0, 400.0);
    left(&mut s, true);
    s.apply(WindowEvent::MoveRequested { id: w(1) });
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.cursor(), CursorShape::Move);
    assert!(near(origin(&s, 1), Point::new(100.0, -400.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    motion(&mut s, 760.0, 400.0);
    assert!(near(origin(&s, 1), Point::new(110.0, -400.0)));
}

#[test]
fn a_resize_request_from_the_fullscreen_window_leaves_fullscreen() {
    let mut s = shell_with_answered_fullscreen();
    motion(&mut s, 990.0, 400.0);
    left(&mut s, true);
    s.apply(WindowEvent::ResizeRequested {
        id: w(1),
        edges: edges(false, false, false, true),
    });
    assert_eq!(s.fullscreen(), None);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    motion(&mut s, 1010.0, 400.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(220.0, 100.0)), resizing())]
    );
}

#[test]
fn a_resize_click_without_motion_on_a_maximized_window_settles() {
    let mut s = shell_with_maximized_window();
    start_resize(&mut s, 990.0, 400.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
    right(&mut s, false);
    assert!(s.take_commands().is_empty());
    answer(&mut s, 1, Size::new(200.0, 100.0), true);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), None, States::default())]
    );
}

// --- US2: resizing windows ---------------------------------------------------------------------

fn edges(top: bool, bottom: bool, left: bool, right: bool) -> Edges {
    Edges {
        top,
        bottom,
        left,
        right,
    }
}

fn right(s: &mut Shell, pressed: bool) -> Vec<ClientInput> {
    button(s, Button::Right, BTN_RIGHT, pressed)
}

/// Starts a Mod + right resize at screen (x, y).
fn start_resize(s: &mut Shell, x: f64, y: f64) -> Vec<ClientInput> {
    motion(s, x, y);
    mods(s, super_mod(), 8);
    right(s, true)
}

/// The application answers with `size`; `configured`: it applied the newest configure.
fn answer(s: &mut Shell, n: u64, size: Size, configured: bool) -> Vec<ClientInput> {
    let limits = s.windows().get(w(n)).unwrap().limits;
    s.apply(WindowEvent::Updated {
        id: w(n),
        size,
        input: full(size),
        limits,
        configured,
    })
}

fn configures(cmds: &[WindowCommand]) -> Vec<(WindowId, Option<Size>, States)> {
    cmds.iter()
        .filter_map(|c| match c {
            WindowCommand::Configure {
                window,
                size,
                states,
            } => Some((*window, *size, *states)),
            WindowCommand::Close { .. } => None,
        })
        .collect()
}

fn resizing() -> States {
    States {
        resizing: true,
        ..States::default()
    }
}

#[test]
fn mod_right_resizes_from_the_nearest_edge() {
    let mut s = shell_with_two_windows();
    // Window 1 covers screen (400, 350)..(600, 450); (410, 400) is its left middle cell.
    let out = start_resize(&mut s, 410.0, 400.0);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
    assert!(
        out.contains(&ClientInput::PointerLeave { window: w(1) }),
        "{out:?}"
    );
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(stack(&s), vec![w(2), w(1)]);
    assert_eq!(
        s.cursor(),
        CursorShape::Resize(edges(false, false, true, false))
    );
    s.take_commands();
    let out = motion(&mut s, 380.0, 420.0);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(230.0, 100.0)), resizing())]
    );
    // The application answers; the right edge (canvas x 100) stays.
    answer(&mut s, 1, Size::new(230.0, 100.0), true);
    assert!(near(origin(&s, 1), Point::new(-130.0, -50.0)));
    // It picks another size (cell snapping): still anchored on the right.
    answer(&mut s, 1, Size::new(226.0, 100.0), false);
    assert!(near(origin(&s, 1), Point::new(-126.0, -50.0)));
    // The release asks for the last size without `resizing`.
    let out = right(&mut s, false);
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(230.0, 100.0)), States::default())]
    );
    assert_eq!(s.cursor(), CursorShape::Default);
    // The final size arrives: still anchored, then the anchor goes back to the top-left.
    answer(&mut s, 1, Size::new(230.0, 100.0), true);
    assert!(near(origin(&s, 1), Point::new(-130.0, -50.0)));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), None, States::default())]
    );
    answer(&mut s, 1, Size::new(300.0, 120.0), true);
    assert!(
        near(origin(&s, 1), Point::new(-130.0, -50.0)),
        "top-left kept"
    );
}

#[test]
fn mod_shift_right_and_the_centre_cell_resize_from_a_corner() {
    let mut s = shell_with_window();
    // Exact centre of window 1: bottom-right on a tie.
    motion(&mut s, 500.0, 400.0);
    mods(&mut s, super_shift(), 9);
    right(&mut s, true);
    assert_eq!(
        s.cursor(),
        CursorShape::Resize(edges(false, true, false, true))
    );
    s.take_commands();
    motion(&mut s, 520.0, 410.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(220.0, 110.0)), resizing())]
    );
    right(&mut s, false);
    // Near the top-left of the centre cell.
    start_resize(&mut s, 480.0, 390.0);
    assert_eq!(
        s.cursor(),
        CursorShape::Resize(edges(true, false, true, false))
    );
}

#[test]
fn mod_right_on_the_background_does_nothing() {
    let mut s = shell_with_window();
    let out = start_resize(&mut s, 10.0, 10.0);
    assert!(out.is_empty(), "{out:?}");
    motion(&mut s, 40.0, 40.0);
    assert!(s.take_commands().is_empty());
    assert_eq!(s.cursor(), CursorShape::Default);
    assert!(!s.viewer().is_panning());
}

#[test]
fn resize_requests_during_a_press_start_a_resize() {
    let mut s = shell_with_window();
    motion(&mut s, 590.0, 440.0);
    left(&mut s, true);
    let out = s.apply(WindowEvent::ResizeRequested {
        id: w(1),
        edges: edges(false, true, false, true),
    });
    assert!(
        out.contains(&ClientInput::PointerLeave { window: w(1) }),
        "{out:?}"
    );
    s.take_commands();
    motion(&mut s, 610.0, 450.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(220.0, 110.0)), resizing())]
    );
    let out = left(&mut s, false);
    assert!(!has_button(&out), "{out:?}");
}

#[test]
fn resize_requests_without_a_press_or_edges_are_ignored() {
    let mut s = shell_with_window();
    motion(&mut s, 590.0, 440.0);
    s.apply(WindowEvent::ResizeRequested {
        id: w(1),
        edges: edges(false, true, false, true),
    });
    assert_eq!(s.cursor(), CursorShape::Default);
    left(&mut s, true);
    s.apply(WindowEvent::ResizeRequested {
        id: w(1),
        edges: Edges::NONE,
    });
    assert_eq!(s.cursor(), CursorShape::Default);
    s.take_commands();
    motion(&mut s, 610.0, 450.0);
    assert!(s.take_commands().is_empty());
}

#[test]
fn resizes_respect_the_size_limits_and_the_largest_window() {
    let mut s = shell();
    s.handle(&InputEvent::FocusGained);
    let size = Size::new(200.0, 100.0);
    s.apply(WindowEvent::Mapped {
        id: w(1),
        size,
        input: full(size),
        limits: SizeLimits {
            min: Size::new(150.0, 80.0),
            max: Size::new(0.0, 120.0),
        },
        parent: None,
    });
    s.set_largest_window(Size::new(260.0, 4096.0));
    start_resize(&mut s, 590.0, 440.0);
    s.take_commands();
    motion(&mut s, 700.0, 500.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(260.0, 120.0)), resizing())]
    );
    motion(&mut s, 400.0, 360.0);
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(150.0, 80.0)), resizing())]
    );
}

#[test]
fn zooming_during_a_resize_keeps_the_dragged_edge_under_the_pointer() {
    let mut s = shell_with_window();
    // Right middle cell: dragging the right edge (canvas x 100), grabbed 10 left of it.
    start_resize(&mut s, 590.0, 400.0);
    s.take_commands();
    send(
        &mut s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, 10.0),
            value120: Some(Vector::new(0.0, 360.0)),
        },
    );
    let p = s
        .viewer()
        .camera()
        .to_canvas(Point::new(590.0, 400.0), s.viewer().size());
    // The wanted right edge is the pointer's canvas x plus the 10 units grabbed inside.
    let wanted = p.x + 10.0 - (-100.0);
    let cmds = configures(&s.take_commands());
    let (_, size, _) = cmds.last().copied().expect("a configure after the zoom");
    let size = size.expect("a size");
    assert!((size.w - wanted).abs() < 1e-6, "{} vs {wanted}", size.w);
    assert!((size.h - 100.0).abs() < 1e-6);
}

#[test]
fn other_input_during_a_resize() {
    let mut s = shell_with_window();
    start_resize(&mut s, 590.0, 400.0);
    let out = left(&mut s, true);
    assert!(out.is_empty(), "{out:?}");
    left(&mut s, false);
    assert_eq!(
        s.cursor(),
        CursorShape::Resize(edges(false, false, false, true))
    );
    let out = right(&mut s, false);
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(s.cursor(), CursorShape::Default);
}

// --- Standalone foundation: viewers keyed by id (research S9, S14) ---------------------------

const DISPLAY: Size = Size {
    w: 1000.0,
    h: 800.0,
};

/// A shell with one display-backed viewer (id 1) of 1000x800, its home view placed.
fn shell_with_viewer() -> Shell {
    let mut s = Shell::new();
    s.add_viewer(ViewerId(1), DISPLAY, true);
    s.place_home_views();
    s.take_dirty();
    s
}

fn moved(s: &mut Shell, dx: f64, dy: f64) -> Vec<ClientInput> {
    send(
        s,
        InputEvent::PointerMoved {
            delta: Vector::new(dx, dy),
        },
    )
}

#[test]
fn a_new_shell_has_no_viewers() {
    let s = Shell::new();
    assert_eq!(s.viewers().count(), 0);
    assert_eq!(s.active(), None);
    assert_eq!(s.pointer(), None);
}

#[test]
fn an_added_viewer_is_active_with_its_home_view_and_the_pointer_in_its_centre() {
    let s = shell_with_viewer();
    assert_eq!(s.active(), Some(ViewerId(1)));
    assert_eq!(
        s.viewers().map(|(id, _)| id).collect::<Vec<_>>(),
        [ViewerId(1)]
    );
    let viewer = s.viewer_of(ViewerId(1)).expect("the viewer");
    assert_eq!(viewer.size(), DISPLAY);
    assert_eq!(viewer.camera(), Camera::default());
    assert_eq!(s.pointer(), Some((ViewerId(1), Point::new(500.0, 400.0))));
}

#[test]
fn relative_motion_reaches_windows_through_the_layout() {
    let mut s = shell_with_viewer();
    s.handle(&InputEvent::FocusGained);
    // A 200x100 window centred in the view covers logical (400..600, 350..450); the pointer
    // starts at the centre (500, 400), window point (100, 50).
    s.apply(mapped(1, 200.0, 100.0));
    let out = moved(&mut s, 10.0, 5.0);
    assert!(
        out.iter().any(|i| matches!(
            i,
            ClientInput::PointerEnter { window: WindowId(1), pos } if *pos == Point::new(110.0, 55.0)
        )),
        "{out:?}"
    );
    let out = moved(&mut s, 1.0, 0.0);
    assert!(
        out.iter().any(|i| matches!(
            i,
            ClientInput::PointerMotion { window: WindowId(1), pos } if *pos == Point::new(111.0, 55.0)
        )),
        "{out:?}"
    );
    assert_eq!(s.pointer(), Some((ViewerId(1), Point::new(511.0, 405.0))));
}

#[test]
fn relative_motion_pans_like_absolute_motion() {
    let mut s = shell_with_viewer();
    left(&mut s, true);
    moved(&mut s, -100.0, 0.0);
    left(&mut s, false);
    // Dragging the canvas 100 px to the left moves the camera 100 units to the right.
    assert_eq!(
        s.viewer_of(ViewerId(1)).expect("viewer").camera().center,
        Point::new(100.0, 0.0)
    );
}

#[test]
fn removing_the_last_viewer_leaves_none_active() {
    let mut s = shell_with_viewer();
    s.remove_viewer(ViewerId(1));
    assert_eq!(s.viewers().count(), 0);
    assert_eq!(s.active(), None);
    assert_eq!(s.pointer(), None);
}

// --- User Story 1: two-press quit (FR-031) ---------------------------------------------------

const KEY_LEFTSHIFT: u32 = 42;

fn key_at(s: &mut Shell, keycode: u32, sym: Keysym, pressed: bool, t: u32) {
    s.handle(&InputEvent::Key {
        keycode,
        binding_sym: sym,
        pressed,
        time_ms: t,
    });
}

#[test]
fn one_quit_press_does_nothing_and_is_not_delivered() {
    let mut s = shell_with_window();
    mods(&mut s, super_shift(), 9);
    let (effect, out) = quit_press(&mut s, 1000);
    assert!(!effect.quit);
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn a_second_press_within_2000_ms_quits() {
    let mut s = shell();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 1000).0.quit);
    assert!(quit_press(&mut s, 3000).0.quit);
}

#[test]
fn a_second_press_after_2000_ms_starts_over() {
    let mut s = shell();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 1000).0.quit);
    assert!(
        !quit_press(&mut s, 3001).0.quit,
        "too late: a new first press"
    );
    assert!(
        quit_press(&mut s, 4000).0.quit,
        "within 2 s of the new first press"
    );
}

#[test]
fn modifier_keys_in_between_do_not_cancel() {
    let mut s = shell();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 0).0.quit);
    key_at(&mut s, KEY_LEFTSHIFT, Keysym::Shift_L, false, 100);
    mods(&mut s, super_mod(), 8);
    key_at(&mut s, KEY_LEFTSHIFT, Keysym::Shift_L, true, 200);
    mods(&mut s, super_shift(), 9);
    assert!(quit_press(&mut s, 400).0.quit);
}

#[test]
fn another_key_or_focus_loss_cancels() {
    let mut s = shell_with_window();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 0).0.quit);
    key_at(&mut s, KEY_A, Keysym::a, true, 100);
    key_at(&mut s, KEY_A, Keysym::a, false, 150);
    assert!(!quit_press(&mut s, 200).0.quit, "the A press cancelled");
    s.handle(&InputEvent::FocusLost);
    s.handle(&InputEvent::FocusGained);
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 300).0.quit, "focus loss cancelled");
    assert!(quit_press(&mut s, 400).0.quit);
}

#[test]
fn quit_times_wrap_around() {
    let mut s = shell();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, u32::MAX - 500).0.quit);
    assert!(
        quit_press(&mut s, 500).0.quit,
        "1001 ms later across the wrap"
    );
}

// --- User Story 2: spawn bindings (FR-014) ---------------------------------------------------

const KEY_ENTER: u32 = 28;
const KEY_B: u32 = 48;

/// Mod+Return runs kitty (binding 0), Mod+B runs firefox (binding 1).
fn spawn_shell(mod_key: ModKey) -> Shell {
    let bind = |sym, program: &str| SpawnBind {
        combo: KeyCombo {
            mods: mod_key.modifiers(),
            sym,
        },
        command: vec![program.into()],
    };
    let bindings = Bindings::new(
        mod_key,
        vec![bind(Keysym::Return, "kitty"), bind(Keysym::b, "firefox")],
    );
    let mut s = Shell::with_bindings(bindings);
    s.add_viewer(ViewerId(1), Size::new(1000.0, 800.0), true);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    s
}

#[test]
fn a_spawn_binding_spawns_and_is_not_delivered() {
    let mut s = spawn_shell(ModKey::Super);
    mods(&mut s, ModKey::Super.modifiers(), 64);
    let (effect, out) = s.handle(&InputEvent::Key {
        keycode: KEY_ENTER,
        binding_sym: Keysym::Return,
        pressed: true,
        time_ms: 0,
    });
    assert_eq!(effect.spawn, Some(0));
    assert!(!effect.quit);
    assert!(
        !out.iter().any(|i| matches!(i, ClientInput::Key { .. })),
        "{out:?}"
    );
    let (effect, out) = s.handle(&InputEvent::Key {
        keycode: KEY_ENTER,
        binding_sym: Keysym::Return,
        pressed: false,
        time_ms: 10,
    });
    assert_eq!(effect.spawn, None);
    assert!(
        !out.iter().any(|i| matches!(i, ClientInput::Key { .. })),
        "release swallowed"
    );
    // The second binding gives its own index.
    let (effect, _) = s.handle(&InputEvent::Key {
        keycode: KEY_B,
        binding_sym: Keysym::b,
        pressed: true,
        time_ms: 20,
    });
    assert_eq!(effect.spawn, Some(1));
}

#[test]
fn return_without_mod_reaches_the_window() {
    let mut s = spawn_shell(ModKey::Super);
    let (effect, out) = s.handle(&InputEvent::Key {
        keycode: KEY_ENTER,
        binding_sym: Keysym::Return,
        pressed: true,
        time_ms: 0,
    });
    assert_eq!(effect.spawn, None);
    assert!(out.contains(&ClientInput::Key {
        window: w(1),
        keycode: KEY_ENTER,
        pressed: true
    }));
}

#[test]
fn with_mod_ctrl_ctrl_tab_cycles_windows() {
    let mut s = spawn_shell(ModKey::Ctrl);
    s.apply(mapped(2, 100.0, 100.0));
    mods(&mut s, ModKey::Ctrl.modifiers(), 4);
    let out = key(&mut s, KEY_TAB, Keysym::Tab, true);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "focus moved to the other window: {out:?}"
    );
}

// --- User Story 4: console switch away and back (FR-011, research S12) -----------------------

/// What the DRM backend sends when the session pauses.
fn pause(s: &mut Shell) -> Vec<ClientInput> {
    let mut out = send(s, InputEvent::PointerLeave);
    out.extend(send(s, InputEvent::FocusLost));
    out
}

#[test]
fn a_pause_ends_a_move_and_leaves_pointer_and_keyboard() {
    let mut s = shell_with_window();
    start_move(&mut s, 450.0, 380.0);
    assert_eq!(s.cursor(), CursorShape::Move);
    let out = pause(&mut s);
    assert_eq!(s.cursor(), CursorShape::Default, "the move ended");
    assert!(out.contains(&ClientInput::KeyboardFocus(None)), "{out:?}");
    // Motion after the pause moves the window no more.
    let before = s.windows().get(w(1)).expect("window").origin;
    motion(&mut s, 600.0, 500.0);
    assert_eq!(s.windows().get(w(1)).expect("window").origin, before);
}

#[test]
fn a_pause_ends_a_resize_and_a_pan() {
    let mut s = shell_with_window();
    start_resize(&mut s, 590.0, 400.0);
    pause(&mut s);
    assert_eq!(s.cursor(), CursorShape::Default);

    let mut s = shell();
    motion(&mut s, 100.0, 100.0);
    left(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    pause(&mut s);
    let camera = s.viewer().camera();
    motion(&mut s, 300.0, 300.0);
    assert_eq!(s.viewer().camera(), camera, "the pan ended");
}

#[test]
fn a_pause_cancels_a_pending_quit() {
    let mut s = shell();
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 0).0.quit);
    pause(&mut s);
    send(&mut s, InputEvent::FocusGained);
    mods(&mut s, super_shift(), 9);
    assert!(!quit_press(&mut s, 100).0.quit, "a first press again");
}

#[test]
fn after_resuming_focus_returns_and_the_pointer_re_enters() {
    let mut s = shell_with_viewer();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    pause(&mut s);
    let out = send(&mut s, InputEvent::FocusGained);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
    // The pointer is still at the centre, over the window: the next motion re-enters it.
    let out = moved(&mut s, 1.0, 0.0);
    assert!(
        out.iter()
            .any(|i| matches!(i, ClientInput::PointerEnter { window, .. } if *window == w(1))),
        "{out:?}"
    );
}

// --- User Story 2: applications hiding the pointer (FR-008) ----------------------------------

#[test]
fn the_pointer_is_hidden_only_over_a_window_that_hid_it_and_without_a_gesture() {
    let mut s = shell_with_window();
    // Window 1 (200x100) covers logical (400..600, 350..450); window 2 opens there too, on top,
    // and is moved away so the two do not overlap.
    s.apply(WindowEvent::CursorHidden {
        id: w(1),
        hidden: true,
    });
    motion(&mut s, 500.0, 400.0);
    assert_eq!(s.cursor(), CursorShape::Hidden);
    // A compositor gesture shows its own shape: Mod+Shift + left drag pans everywhere.
    mods(&mut s, super_shift(), 65);
    left(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    left(&mut s, false);
    mods(&mut s, Modifiers::NONE, 0);
    // Over the background: the default arrow.
    motion(&mut s, 10.0, 10.0);
    assert_eq!(s.cursor(), CursorShape::Default);
    // Shown again when the application asks for a cursor image.
    s.apply(WindowEvent::CursorHidden {
        id: w(1),
        hidden: false,
    });
    motion(&mut s, 500.0, 400.0);
    assert_eq!(s.cursor(), CursorShape::Default);
}

// --- User Story 3: every monitor is a viewer (FR-018 - FR-023, research S14) -----------------

const A: ViewerId = ViewerId(1);
const B: ViewerId = ViewerId(2);

/// Two 1000x800 displays side by side, A (built-in) left of B, focused. Their home views look
/// at (-500, 0) and (500, 0) at 100%; the pointer starts at A's centre, canvas (-500, 0).
fn two_viewers() -> Shell {
    let mut s = Shell::new();
    s.add_viewer(A, DISPLAY, true);
    s.add_viewer(B, DISPLAY, false);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.take_dirty();
    s
}

fn cam(s: &Shell, id: ViewerId) -> Camera {
    s.viewer_of(id).unwrap().camera()
}

/// From A's centre to B's centre.
fn to_b(s: &mut Shell) {
    moved(s, 1000.0, 0.0);
}

/// From B's centre to A's centre.
fn to_a(s: &mut Shell) {
    moved(s, -1000.0, 0.0);
}

fn zoom_in(s: &mut Shell) {
    send(
        s,
        InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta: Vector::new(0.0, -10.0),
            value120: Some(Vector::new(0.0, -120.0)),
        },
    );
}

fn reset_view(s: &mut Shell) {
    key(s, KEY_0, Keysym::_0, true);
    key(s, KEY_0, Keysym::_0, false);
}

#[test]
fn two_viewers_sit_side_by_side_and_the_pointer_crosses() {
    let mut s = two_viewers();
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0));
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
    assert_eq!(s.active(), Some(A));
    to_b(&mut s);
    assert_eq!(s.active(), Some(B));
    assert_eq!(s.pointer(), Some((B, Point::new(500.0, 400.0))));
}

#[test]
fn pan_zoom_and_reset_act_only_on_the_viewer_under_the_pointer() {
    let mut s = two_viewers();
    to_b(&mut s);
    left(&mut s, true);
    moved(&mut s, -100.0, 0.0);
    left(&mut s, false);
    assert_eq!(cam(&s, B), camera_at(600.0, 0.0, 1.0));
    mods(&mut s, super_mod(), 8);
    zoom_in(&mut s);
    assert!((cam(&s, B).zoom - 1.1).abs() < 1e-9);
    reset_view(&mut s);
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0), "A never moved");
}

#[test]
fn a_pan_keeps_acting_on_the_viewer_it_started_on() {
    let mut s = two_viewers();
    left(&mut s, true);
    moved(&mut s, 1000.0, 0.0);
    assert_eq!(s.active(), Some(A), "still A while the pan runs");
    left(&mut s, false);
    assert_eq!(cam(&s, A), camera_at(-1500.0, 0.0, 1.0));
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
    moved(&mut s, 1.0, 0.0);
    assert_eq!(s.active(), Some(B));
}

#[test]
fn the_viewer_under_the_pointer_is_active_right_after_a_pan_ends() {
    let mut s = two_viewers();
    left(&mut s, true);
    moved(&mut s, 1000.0, 0.0);
    left(&mut s, false);
    assert_eq!(s.active(), Some(B), "no motion needed after the release");
    mods(&mut s, super_mod(), 8);
    zoom_in(&mut s);
    assert!((cam(&s, B).zoom - 1.1).abs() < 1e-9);
    assert_eq!(cam(&s, A).zoom, 1.0, "A only panned");
}

#[test]
fn a_window_carried_across_jumps_to_the_region_the_new_viewer_shows() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.viewer_mut().set_camera(camera_at(5000.0, 0.0, 2.0));
    to_a(&mut s);
    // Centred in A at (-500, 0): origin (-600, -50); the pointer grabs window point (100, 50).
    s.apply(mapped(1, 200.0, 100.0));
    mods(&mut s, super_mod(), 64);
    left(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Move);
    moved(&mut s, 1000.0, 0.0);
    // B's centre shows canvas (5000, 0): the grabbed point stays under the pointer.
    assert_eq!(origin(&s, 1), Point::new(4900.0, -50.0));
    left(&mut s, false);
    assert_eq!(origin(&s, 1), Point::new(4900.0, -50.0));
}

#[test]
fn new_windows_open_centred_in_the_active_viewer() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    assert_eq!(origin(&s, 1), Point::new(400.0, -50.0));
    assert_eq!(s.visible_in(B), vec![w(1)]);
    assert!(s.visible_in(A).is_empty());
}

#[test]
fn cycling_to_a_window_inside_another_viewer_moves_no_view() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(2, 200.0, 100.0));
    to_a(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    mod_tab(&mut s, false);
    release_mod(&mut s);
    assert_eq!(s.windows().focus(), Some(w(2)));
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0));
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
}

#[test]
fn cycling_to_a_window_outside_every_viewer_moves_the_active_viewer() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(2, 200.0, 100.0));
    s.viewer_mut().set_camera(camera_at(5000.0, 0.0, 1.0));
    to_a(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    mod_tab(&mut s, false);
    release_mod(&mut s);
    assert_eq!(s.windows().focus(), Some(w(2)));
    assert!(s.visible_in(A).contains(&w(2)), "A moved to window 2");
    assert_eq!(cam(&s, B), camera_at(5000.0, 0.0, 1.0));
}

// Maximize and fullscreen with several viewers (FR-022).

#[test]
fn maximize_uses_the_viewer_showing_the_window_and_moves_only_its_camera() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    s.viewer_mut().set_camera(camera_at(550.0, 0.0, 1.0));
    to_a(&mut s);
    // A shows nothing of window 1; B does.
    request_maximize(&mut s, 1, true);
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(DISPLAY), maximized())]
    );
}

#[test]
fn maximize_of_a_window_no_viewer_shows_uses_the_active_viewer() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    s.viewer_mut().set_camera(camera_at(5000.0, 0.0, 1.0));
    to_a(&mut s);
    request_maximize(&mut s, 1, true);
    assert_eq!(cam(&s, A), camera_at(500.0, 0.0, 1.0));
    assert_eq!(cam(&s, B), camera_at(5000.0, 0.0, 1.0));
}

/// Window 1 fullscreen on A (it was centred there); the pointer is back on A.
fn fullscreen_on_a() -> Shell {
    let mut s = two_viewers();
    s.apply(mapped(1, 200.0, 100.0));
    request_fullscreen(&mut s, 1, true);
    s.take_commands();
    s
}

#[test]
fn each_viewer_keeps_its_own_fullscreen_window() {
    let mut s = fullscreen_on_a();
    to_b(&mut s);
    // B has no fullscreen window: a new window opens normally there, focused.
    let out = s.apply(mapped(2, 200.0, 100.0));
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(2)))),
        "{out:?}"
    );
    assert_eq!(origin(&s, 2), Point::new(400.0, -50.0));
    request_fullscreen(&mut s, 2, true);
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
    assert_eq!(s.fullscreen_in(B), Some(w(2)));
    assert_eq!(s.visible_in(A), vec![w(1)]);
    assert_eq!(s.visible_in(B), vec![w(2)]);
}

#[test]
fn navigation_on_another_viewer_leaves_a_fullscreen_window_alone() {
    let mut s = fullscreen_on_a();
    let camera_a = cam(&s, A);
    to_b(&mut s);
    left(&mut s, true);
    moved(&mut s, -100.0, 0.0);
    left(&mut s, false);
    mods(&mut s, super_mod(), 8);
    zoom_in(&mut s);
    reset_view(&mut s);
    release_mod(&mut s);
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
    assert_eq!(cam(&s, A), camera_a);
}

#[test]
fn cycling_leaves_fullscreen_only_on_the_active_viewer() {
    let mut s = fullscreen_on_a();
    let camera_a = cam(&s, A);
    to_b(&mut s);
    s.apply(mapped(2, 200.0, 100.0));
    request_fullscreen(&mut s, 2, true);
    mod_tab(&mut s, false);
    release_mod(&mut s);
    // B's fullscreen ended and its view returned; A's fullscreen window got the focus and no
    // view moved for it.
    assert_eq!(s.fullscreen_in(B), None);
    assert_eq!(cam(&s, B), camera_at(500.0, 0.0, 1.0));
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
    assert_eq!(cam(&s, A), camera_a);
}

#[test]
fn a_fullscreen_request_naming_an_output_uses_its_viewer() {
    let mut s = two_viewers();
    s.apply(mapped(1, 200.0, 100.0));
    request_fullscreen_on(&mut s, 1, B);
    assert_eq!(s.fullscreen_in(B), Some(w(1)));
    assert_eq!(s.fullscreen_in(A), None);
    assert_eq!(cam(&s, B), camera_at(-500.0, 0.0, 1.0));
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0));
    // An unknown output counts as none: the window's viewer, A.
    request_fullscreen(&mut s, 1, false);
    request_fullscreen_on(&mut s, 1, ViewerId(99));
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
    assert_eq!(s.fullscreen_in(B), None);
}

#[test]
fn the_new_window_rule_during_fullscreen_applies_only_on_that_viewer() {
    let mut s = fullscreen_on_a();
    // On A: a new window opens behind, unfocused.
    s.apply(created(2, None, false, false));
    s.apply(mapped(2, 100.0, 50.0));
    assert_eq!(s.windows().focus(), Some(w(1)));
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
    // On B: normally, focused; A's fullscreen stays.
    to_b(&mut s);
    s.apply(mapped(3, 100.0, 50.0));
    assert_eq!(s.windows().focus(), Some(w(3)));
    assert_eq!(s.fullscreen_in(A), Some(w(1)));
}

// Removing viewers, and no viewer at all (FR-021, spec Edge Cases).

#[test]
fn removing_a_viewer_ends_its_fullscreen_with_the_old_place_and_size() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    request_fullscreen(&mut s, 1, true);
    answer(&mut s, 1, DISPLAY, true);
    s.take_commands();
    s.remove_viewer(B);
    assert_eq!(s.fullscreen_in(B), None);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(origin(&s, 1), Point::new(400.0, -50.0));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(200.0, 100.0)), States::default())]
    );
}

#[test]
fn removing_a_viewer_keeps_its_maximized_window_as_it_is() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    request_maximize(&mut s, 1, true);
    answer(&mut s, 1, DISPLAY, true);
    s.take_commands();
    let rect = s.windows().get(w(1)).unwrap().rect();
    s.remove_viewer(B);
    assert!(matches!(mode(&s, 1), Mode::Maximized { .. }));
    assert_eq!(s.windows().get(w(1)).unwrap().rect(), rect);
    assert!(configures(&s.take_commands()).is_empty());
}

#[test]
fn removing_the_viewer_under_the_pointer_ends_a_pan_and_moves_the_pointer() {
    let mut s = two_viewers();
    to_b(&mut s);
    left(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    s.remove_viewer(B);
    assert_eq!(s.cursor(), CursorShape::Default);
    assert_eq!(s.active(), Some(A));
    assert_eq!(s.pointer(), Some((A, Point::new(500.0, 400.0))));
    moved(&mut s, 100.0, 0.0);
    left(&mut s, false);
    assert_eq!(cam(&s, A), camera_at(-500.0, 0.0, 1.0));
}

#[test]
fn removing_the_viewer_under_the_pointer_ends_a_move_where_the_window_is() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    mods(&mut s, super_mod(), 64);
    left(&mut s, true);
    moved(&mut s, 50.0, 0.0);
    assert_eq!(origin(&s, 1), Point::new(450.0, -50.0));
    s.remove_viewer(B);
    assert_eq!(s.cursor(), CursorShape::Default);
    moved(&mut s, 100.0, 0.0);
    left(&mut s, false);
    assert_eq!(origin(&s, 1), Point::new(450.0, -50.0));
}

#[test]
fn removing_the_viewer_under_the_pointer_ends_a_resize() {
    let mut s = two_viewers();
    to_b(&mut s);
    s.apply(mapped(1, 200.0, 100.0));
    s.take_commands();
    // B's window covers logical (400..600, 350..450): grab its right edge.
    start_resize(&mut s, 590.0, 400.0);
    motion(&mut s, 640.0, 400.0);
    s.take_commands();
    s.remove_viewer(B);
    assert_eq!(s.cursor(), CursorShape::Default);
    // The last size asked for is asked for once more without `resizing`.
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(1), Some(Size::new(250.0, 100.0)), States::default())]
    );
}

#[test]
fn with_no_viewer_windows_open_at_the_origin_and_states_are_not_granted() {
    let mut s = two_viewers();
    s.remove_viewer(A);
    s.remove_viewer(B);
    s.apply(mapped(1, 200.0, 100.0));
    assert_eq!(origin(&s, 1), Point::new(-100.0, -50.0));
    s.apply(mapped(2, 200.0, 100.0));
    s.take_commands();
    // Focus cycling focuses and raises; there is no view to move.
    mod_tab(&mut s, false);
    release_mod(&mut s);
    assert_eq!(s.windows().focus(), Some(w(1)));
    // Maximize and fullscreen are ignored, also as initial states.
    request_maximize(&mut s, 1, true);
    request_fullscreen(&mut s, 1, true);
    assert_eq!(mode(&s, 1), Mode::Normal);
    assert_eq!(s.fullscreen(), None);
    assert!(s.take_commands().is_empty());
    s.apply(created(3, None, true, true));
    assert_eq!(
        configures(&s.take_commands()),
        vec![(w(3), None, States::default())]
    );
    assert!(s.visible_any().is_empty());
}

// --- Pointer bindings (T085, FR-032, research S24) -------------------------------------------

const BTN_MIDDLE: u32 = 0x112;

fn middle(s: &mut Shell, pressed: bool) -> Vec<ClientInput> {
    button(s, Button::Middle, BTN_MIDDLE, pressed)
}

/// Like `shell_with_two_windows` (window 1 exposed at screen (410, 360), window 2 focused on
/// top), with the pointer table of quickstart Q24 minus resize: Mod + left moves, Mod+Shift +
/// left pans, Mod + middle pans; Mod + right is not bound.
fn shell_with_pointer_binds() -> Shell {
    let bind = |mods, button, action| PointerBind {
        combo: PointerCombo { mods, button },
        action,
    };
    let table = vec![
        bind(super_mod(), Button::Left, PointerAction::MoveWindow),
        bind(super_shift(), Button::Left, PointerAction::PanCanvas),
        bind(super_mod(), Button::Middle, PointerAction::PanCanvas),
    ];
    let mut s = Shell::with_bindings(Bindings::default().with_pointer(table));
    s.add_viewer(ViewerId(1), Size::new(1000.0, 800.0), true);
    s.place_home_views();
    s.handle(&InputEvent::FocusGained);
    s.apply(mapped(1, 200.0, 100.0));
    s.apply(mapped(2, 100.0, 50.0));
    s.take_dirty();
    s
}

#[test]
fn a_bound_move_window_moves_focuses_and_raises() {
    let mut s = shell_with_pointer_binds();
    motion(&mut s, 410.0, 360.0);
    mods(&mut s, super_mod(), 64);
    let out = left(&mut s, true);
    assert!(
        out.contains(&ClientInput::KeyboardFocus(Some(w(1)))),
        "{out:?}"
    );
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(s.cursor(), CursorShape::Move);
    assert_eq!(stack(&s), vec![w(2), w(1)]);
    let before = origin(&s, 1);
    motion(&mut s, 460.0, 380.0);
    assert_eq!(origin(&s, 1), before + Vector::new(50.0, 20.0));
    left(&mut s, false);
    assert_eq!(s.cursor(), CursorShape::Default);
}

#[test]
fn a_bound_pan_canvas_pans_over_a_window_without_focus() {
    let mut s = shell_with_pointer_binds();
    motion(&mut s, 410.0, 360.0);
    mods(&mut s, super_shift(), 65);
    let out = left(&mut s, true);
    assert!(!has_button(&out), "{out:?}");
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    motion(&mut s, 310.0, 360.0);
    left(&mut s, false);
    assert_eq!(camera_center(&s), Point::new(100.0, 0.0));
    assert_eq!(s.windows().focus(), Some(w(2)), "focus unchanged");
}

#[test]
fn a_pan_on_the_middle_button_ends_only_with_its_release() {
    let mut s = shell_with_pointer_binds();
    motion(&mut s, 10.0, 10.0);
    mods(&mut s, super_mod(), 64);
    middle(&mut s, true);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    // A left press and release during the pan change nothing.
    left(&mut s, true);
    left(&mut s, false);
    assert_eq!(s.cursor(), CursorShape::Grabbing);
    motion(&mut s, 60.0, 10.0);
    middle(&mut s, false);
    assert_eq!(s.cursor(), CursorShape::Default);
    assert_eq!(camera_center(&s), Point::new(-50.0, 0.0));
    motion(&mut s, 100.0, 10.0);
    assert_eq!(camera_center(&s), Point::new(-50.0, 0.0), "the pan ended");
}

#[test]
fn an_unbound_mod_click_is_an_ordinary_click() {
    let mut s = shell_with_pointer_binds();
    motion(&mut s, 410.0, 360.0);
    mods(&mut s, super_mod(), 64);
    let out = right(&mut s, true);
    assert!(
        out.contains(&ClientInput::PointerButton {
            window: w(1),
            code: BTN_RIGHT,
            pressed: true,
        }),
        "{out:?}"
    );
    assert_eq!(s.cursor(), CursorShape::Default);
}

#[test]
fn a_resize_binding_over_the_background_does_nothing() {
    let mut s = shell_with_window();
    motion(&mut s, 10.0, 10.0);
    mods(&mut s, super_mod(), 64);
    let camera = s.viewer().camera();
    let out = right(&mut s, true);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(s.cursor(), CursorShape::Default);
    motion(&mut s, 60.0, 10.0);
    right(&mut s, false);
    assert_eq!(s.viewer().camera(), camera);
}

#[test]
fn a_bound_pan_with_a_fullscreen_window_leaves_fullscreen() {
    let mut s = shell_with_pointer_binds();
    request_fullscreen(&mut s, 2, true);
    assert_eq!(s.fullscreen(), Some(w(2)));
    motion(&mut s, 10.0, 10.0);
    mods(&mut s, super_mod(), 64);
    middle(&mut s, true);
    assert_eq!(s.fullscreen(), None);
    assert_eq!(s.cursor(), CursorShape::Default, "consumed, no pan");
    middle(&mut s, false);
}
