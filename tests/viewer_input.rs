//! Viewer behaviour driven by backend-neutral input events.

use rubickway::canvas::{Camera, Size};
use rubickway::input::{InputEvent, Modifiers, RawModifiers};
use rubickway::viewer::Viewer;

fn modifiers(mods: Modifiers) -> InputEvent {
    InputEvent::Modifiers {
        mods,
        raw: RawModifiers::default(),
    }
}

// --- User Story 1: viewer basics -------------------------------------------------------------

#[test]
fn new_viewer_shows_origin_at_100_percent_and_needs_a_frame() {
    let mut viewer = Viewer::new();
    assert_eq!(viewer.camera(), Camera::default());
    assert!(viewer.take_dirty());
    assert!(!viewer.take_dirty(), "take_dirty clears the flag");
}

#[test]
fn resize_keeps_centre_and_zoom() {
    let mut viewer = Viewer::new();
    viewer.take_dirty();
    let before = viewer.camera();
    viewer.configure(Size::new(800.0, 600.0));
    assert_eq!(viewer.camera(), before);
    assert_eq!(viewer.size(), Size::new(800.0, 600.0));
    assert!(viewer.take_dirty());
}

#[test]
fn invalid_scales_are_ignored() {
    let mut viewer = Viewer::new();
    viewer.set_scale(1.5);
    assert_eq!(viewer.scale(), 1.5);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        viewer.set_scale(bad);
        assert_eq!(viewer.scale(), 1.5, "scale {bad} must be ignored");
    }
}

#[test]
fn buffer_size_rounds_and_is_at_least_one_pixel() {
    let mut viewer = Viewer::new();
    viewer.configure(Size::new(801.0, 601.0));
    viewer.set_scale(1.5);
    assert_eq!(viewer.buffer_size(), (1202, 902)); // 1201.5 and 901.5 round away from zero
    viewer.configure(Size::new(0.2, 0.2));
    viewer.set_scale(1.0);
    assert_eq!(viewer.buffer_size(), (1, 1));
}

#[test]
fn huge_sizes_do_not_overflow_and_keep_the_logical_size() {
    let mut viewer = Viewer::new();
    viewer.configure(Size::new(2.0e9, 1.5e9));
    viewer.set_scale(2.0);
    assert_eq!(
        viewer.size(),
        Size::new(2.0e9, 1.5e9),
        "logical size is not limited"
    );
    let (w, h) = viewer.buffer_size();
    assert!(
        i32::try_from(w).is_ok() && i32::try_from(h).is_ok(),
        "{w}x{h}"
    );
}

// --- User Story 2: pan and zoom --------------------------------------------------------------

use rubickway::canvas::{Point, Vector};
use rubickway::input::ScrollSource;

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

fn ready_viewer(pointer: Point) -> Viewer {
    let mut viewer = Viewer::new();
    viewer.handle(&InputEvent::PointerEnter { pos: pointer });
    viewer.take_dirty();
    viewer
}

/// The pan button pressed or released, as the shell drives the viewer (FR-032).
fn button(viewer: &mut Viewer, pressed: bool) {
    if pressed {
        viewer.begin_pan();
    } else {
        viewer.end_pan();
    }
}

fn motion(viewer: &mut Viewer, pos: Point) {
    viewer.handle(&InputEvent::PointerMotion { pos });
}

fn wheel(viewer: &mut Viewer, value120_y: f64) {
    viewer.handle(&InputEvent::Scroll {
        source: ScrollSource::Wheel,
        delta: Vector::new(0.0, value120_y / 8.0),
        value120: Some(Vector::new(0.0, value120_y)),
    });
}

fn finger(viewer: &mut Viewer, source: ScrollSource, delta: Vector) {
    viewer.handle(&InputEvent::Scroll {
        source,
        delta,
        value120: None,
    });
}

/// Mod (Super by default) held.
fn super_mod() -> Modifiers {
    Modifiers {
        logo: true,
        ..Modifiers::NONE
    }
}

#[test]
fn drag_keeps_the_grabbed_point_under_the_pointer() {
    let start = Point::new(100.0, 100.0);
    let mut viewer = ready_viewer(start);
    let anchor = viewer.camera().to_canvas(start, viewer.size());
    button(&mut viewer, true);
    assert!(viewer.is_panning());
    for pos in [
        Point::new(150.0, 80.0),
        Point::new(-500.0, 2000.0),
        Point::new(3000.0, -40.0),
    ] {
        motion(&mut viewer, pos);
        assert!(
            close(viewer.camera().to_logical(anchor, viewer.size()), pos),
            "{pos:?}"
        );
    }
    assert!(viewer.take_dirty(), "a pan needs a frame");
}

#[test]
fn mod_drag_pans_too() {
    let start = Point::new(400.0, 300.0);
    let mut viewer = ready_viewer(start);
    viewer.handle(&modifiers(super_mod()));
    let anchor = viewer.camera().to_canvas(start, viewer.size());
    button(&mut viewer, true);
    motion(&mut viewer, Point::new(420.0, 330.0));
    assert!(close(
        viewer.camera().to_logical(anchor, viewer.size()),
        Point::new(420.0, 330.0)
    ));
}

#[test]
fn release_focus_loss_and_leave_end_the_drag() {
    let ends: [fn(&mut Viewer); 3] = [
        |v| button(v, false),
        |v| {
            v.handle(&InputEvent::FocusLost);
        },
        |v| {
            v.handle(&InputEvent::PointerLeave);
        },
    ];
    for end in ends {
        let mut viewer = ready_viewer(Point::new(100.0, 100.0));
        button(&mut viewer, true);
        motion(&mut viewer, Point::new(120.0, 100.0));
        end(&mut viewer);
        assert!(!viewer.is_panning());
        let camera = viewer.camera();
        motion(&mut viewer, Point::new(300.0, 300.0));
        assert_eq!(
            viewer.camera(),
            camera,
            "camera must stay after the drag ends"
        );
    }
}

#[test]
fn scrolling_is_ignored_during_a_drag() {
    let mut viewer = ready_viewer(Point::new(100.0, 100.0));
    button(&mut viewer, true);
    let camera = viewer.camera();
    wheel(&mut viewer, -120.0);
    finger(&mut viewer, ScrollSource::Finger, Vector::new(10.0, 10.0));
    assert_eq!(viewer.camera(), camera);
}

#[test]
fn wheel_notch_zooms_ten_percent_around_the_pointer() {
    let p = Point::new(200.0, 150.0);
    let mut viewer = ready_viewer(p);
    let under = viewer.camera().to_canvas(p, viewer.size());
    wheel(&mut viewer, -120.0);
    assert!((viewer.camera().zoom - 1.1).abs() < 1e-12);
    assert!(close(viewer.camera().to_canvas(p, viewer.size()), under));
    assert!(viewer.take_dirty());
    wheel(&mut viewer, 120.0);
    assert!((viewer.camera().zoom - 1.0).abs() < 1e-12);
}

#[test]
fn high_resolution_wheel_zooms_by_fractions_of_a_step() {
    let mut viewer = ready_viewer(Point::new(640.0, 400.0));
    wheel(&mut viewer, -60.0);
    assert!((viewer.camera().zoom - 1.1f64.powf(0.5)).abs() < 1e-12);
}

#[test]
fn mod_scroll_zooms_one_step_per_50_px() {
    for source in [ScrollSource::Finger, ScrollSource::Continuous] {
        let p = Point::new(300.0, 300.0);
        let mut viewer = ready_viewer(p);
        viewer.handle(&modifiers(super_mod()));
        let under = viewer.camera().to_canvas(p, viewer.size());
        finger(&mut viewer, source, Vector::new(0.0, -50.0));
        assert!((viewer.camera().zoom - 1.1).abs() < 1e-12, "{source:?}");
        assert!(close(viewer.camera().to_canvas(p, viewer.size()), under));
    }
}

#[test]
fn finger_and_continuous_scroll_pan_like_a_document() {
    for source in [ScrollSource::Finger, ScrollSource::Continuous] {
        let mut viewer = ready_viewer(Point::new(300.0, 300.0));
        finger(&mut viewer, source, Vector::new(10.0, 20.0));
        assert_eq!(viewer.camera().center, Point::new(10.0, 20.0), "{source:?}");
        assert!(viewer.take_dirty());
    }
}

#[test]
fn horizontal_wheel_notch_pans_50_px() {
    let mut viewer = ready_viewer(Point::new(300.0, 300.0));
    viewer.handle(&InputEvent::Scroll {
        source: ScrollSource::Wheel,
        delta: Vector::new(15.0, 0.0),
        value120: Some(Vector::new(120.0, 0.0)),
    });
    assert_eq!(viewer.camera().center, Point::new(50.0, 0.0));
    assert_eq!(viewer.camera().zoom, 1.0);
}

#[test]
fn pinch_zooms_relative_to_the_begin() {
    let p = Point::new(500.0, 200.0);
    let mut viewer = ready_viewer(p);
    let under = viewer.camera().to_canvas(p, viewer.size());
    viewer.handle(&InputEvent::PinchBegin);
    viewer.handle(&InputEvent::PinchUpdate { scale: 1.5 });
    viewer.handle(&InputEvent::PinchUpdate { scale: 2.0 });
    assert!((viewer.camera().zoom - 2.0).abs() < 1e-12);
    assert!(close(viewer.camera().to_canvas(p, viewer.size()), under));
    viewer.handle(&InputEvent::PinchEnd { cancelled: false });
    viewer.handle(&InputEvent::PinchUpdate { scale: 3.0 });
    assert!(
        (viewer.camera().zoom - 2.0).abs() < 1e-12,
        "updates after the end are ignored"
    );
}

#[test]
fn zooming_at_a_limit_needs_no_frame() {
    let mut viewer = ready_viewer(Point::new(640.0, 400.0));
    for _ in 0..100 {
        wheel(&mut viewer, -120.0);
    }
    assert_eq!(viewer.camera().zoom, 10.0);
    viewer.take_dirty();
    wheel(&mut viewer, -120.0);
    assert!(!viewer.take_dirty());
}
