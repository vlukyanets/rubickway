//! The shell: window events from the server, visibility, and (US2/US3) input routing.

use rubickway::canvas::{Point, Rect, Size, Vector};
use rubickway::input::{Button, InputEvent, Keysym, Modifiers, RawModifiers, ScrollSource};
use rubickway::shell::{ClientInput, Shell, WindowEvent, WindowId};

fn full(size: Size) -> Vec<Rect> {
    vec![Rect::new(Point::new(0.0, 0.0), size)]
}

fn mapped(n: u64, w: f64, h: f64) -> WindowEvent {
    let size = Size::new(w, h);
    WindowEvent::Mapped {
        id: WindowId(n),
        size,
        input: full(size),
    }
}

/// A shell with a 1000x800 viewer at the origin, 100%.
fn shell() -> Shell {
    let mut shell = Shell::new();
    shell.viewer_mut().configure(Size::new(1000.0, 800.0));
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

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
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

    let out = mods(&mut s, alt(), 8);
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
        ..alt()
    };
    mods(&mut s, shift_alt, 9);
    let (effect, out) = s.handle(&InputEvent::Key {
        keycode: KEY_Q,
        binding_sym: Keysym::q,
        pressed: true,
    });
    assert!(effect.quit);
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn a_new_window_takes_focus_even_while_typing() {
    let mut s = shell_with_window();
    key(&mut s, KEY_A, Keysym::a, true);
    let out = s.apply(mapped(2, 50.0, 50.0));
    assert_eq!(out, vec![ClientInput::KeyboardFocus(Some(w(2)))]);
}

#[test]
fn focus_moves_to_the_topmost_window_when_the_focused_one_goes() {
    let mut s = shell_with_window();
    s.apply(mapped(2, 50.0, 50.0));
    let out = s.apply(WindowEvent::Unmapped { id: w(2) });
    assert_eq!(out, vec![ClientInput::KeyboardFocus(Some(w(1)))]);
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
    assert_eq!(out, vec![ClientInput::KeyboardFocus(Some(w(1)))]);
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
fn mod_drag_over_a_window_pans_without_input_focus_or_raise() {
    let mut s = shell_with_window();
    s.apply(mapped(2, 50.0, 50.0)); // focused and on top, at the centre
    motion(&mut s, 410.0, 360.0); // over window 1 only
    mods(&mut s, alt(), 8);
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
    mods(&mut s, alt(), 8);
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
