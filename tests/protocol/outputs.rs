//! `wl_output` per display: description, add and remove, enter and leave
//! (contracts/protocols.md).

use rubickway::canvas::ViewerId;
use rubickway::server::OutputDesc;
use rubickway::shell::WindowEvent;
use wayland_client::Proxy;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_surface;
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, wp_fractional_scale_v1,
};

use crate::harness::Harness;
use crate::surfaces::{globals, open_window};

const RED: u32 = 0xffff_0000;

fn laptop() -> OutputDesc {
    OutputDesc {
        name: "eDP-1".into(),
        make: "NCP".into(),
        model: "LM156LF1L03".into(),
        physical_mm: (344, 194),
        mode: (1920, 1080, 60_002),
        position: (0, 0),
    }
}

/// How many wl_output events of a client match `f`.
fn count(h: &mut Harness, c: usize, f: impl Fn(&wl_output::Event) -> bool) -> usize {
    h.c(c)
        .events::<wl_output::Event>()
        .iter()
        .filter(|(_, e)| f(e))
        .count()
}

/// Whether the client's last wl_output event is `done`.
fn ends_with_done(h: &mut Harness, c: usize) -> bool {
    h.c(c)
        .events::<wl_output::Event>()
        .last()
        .is_some_and(|(_, e)| matches!(e, wl_output::Event::Done))
}

#[test]
fn a_display_is_described_as_it_is() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let _output: WlOutput = h.c(c).bind(4);
    h.roundtrip();
    assert_eq!(
        count(&mut h, c, |e| matches!(
            e,
            wl_output::Event::Geometry {
                x: 0,
                y: 0,
                physical_width: 344,
                physical_height: 194,
                make,
                model,
                ..
            } if make == "NCP" && model == "LM156LF1L03"
        )),
        1
    );
    assert_eq!(
        count(&mut h, c, |e| matches!(
            e,
            wl_output::Event::Mode {
                width: 1920,
                height: 1080,
                refresh: 60_002,
                ..
            }
        )),
        1
    );
    assert_eq!(
        count(&mut h, c, |e| matches!(
            e,
            wl_output::Event::Scale { factor: 1 }
        )),
        1
    );
    assert_eq!(
        count(
            &mut h,
            c,
            |e| matches!(e, wl_output::Event::Name { name } if name == "eDP-1")
        ),
        1
    );
    assert_eq!(
        count(&mut h, c, |e| matches!(
            e,
            wl_output::Event::Description { description } if description == "NCP LM156LF1L03 (eDP-1)"
        )),
        1
    );
    assert!(ends_with_done(&mut h, c));
}

fn output_globals(h: &mut Harness, c: usize) -> usize {
    h.c(c)
        .state
        .globals
        .iter()
        .filter(|(_, i, _)| i == "wl_output")
        .count()
}

#[test]
fn there_is_one_output_per_display_and_none_without_displays() {
    let mut h = Harness::new();
    let c = h.connect();
    assert_eq!(output_globals(&mut h, c), 0);
    h.server.add_output(1, laptop());
    h.roundtrip();
    assert_eq!(output_globals(&mut h, c), 1);
    let _output: WlOutput = h.c(c).bind(4);
    h.roundtrip();
    assert_eq!(
        count(
            &mut h,
            c,
            |e| matches!(e, wl_output::Event::Name { name } if name == "eDP-1")
        ),
        1
    );
    h.server.remove_output(1);
    h.roundtrip();
    assert_eq!(output_globals(&mut h, c), 0);
}

#[test]
fn binding_an_output_many_times_describes_it_each_time() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let outputs: Vec<WlOutput> = (0..5).map(|_| h.c(c).bind(4)).collect();
    h.roundtrip();
    let names = count(&mut h, c, |e| matches!(e, wl_output::Event::Name { .. }));
    assert_eq!(names, outputs.len());
    assert!(!h.c(c).is_disconnected());
}

#[test]
fn surfaces_prefer_scale_1_on_the_hardware() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let g = globals(h.c(c));
    let fractional: WpFractionalScaleManagerV1 = h.c(c).bind(1);
    let (w, _id) = open_window(&mut h, c, &g, 64, 64, RED);
    let _fs = fractional.get_fractional_scale(&w.surface, &h.c(c).qh, ());
    h.roundtrip();
    let c_ref = h.c(c);
    assert!(c_ref.events::<wl_surface::Event>().iter().all(
        |(_, e)| !matches!(e, wl_surface::Event::PreferredBufferScale { factor } if *factor != 1)
    ));
    assert!(
        c_ref
            .events::<wp_fractional_scale_v1::Event>()
            .iter()
            .all(|(_, e)| matches!(
                e,
                wp_fractional_scale_v1::Event::PreferredScale { scale: 120 }
            ))
    );
}

// --- User Story 3: several displays (T069) ---------------------------------------------------

fn monitor() -> OutputDesc {
    OutputDesc {
        name: "HDMI-A-1".into(),
        make: "Dell".into(),
        model: "P2419H".into(),
        physical_mm: (527, 296),
        mode: (1920, 1080, 60_000),
        position: (1920, 0),
    }
}

/// The surface events of a client, as (enter: true / leave: false, output) pairs.
fn crossings(h: &mut Harness, c: usize) -> Vec<(bool, ObjectId)> {
    h.c(c)
        .events::<wl_surface::Event>()
        .iter()
        .filter_map(|(_, e)| match e {
            wl_surface::Event::Enter { output } => Some((true, output.id())),
            wl_surface::Event::Leave { output } => Some((false, output.id())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_removed_output_is_inert_and_counts_as_no_output_for_fullscreen() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    h.server.add_output(2, monitor());
    let c = h.connect();
    let g = globals(h.c(c));
    let outputs: Vec<WlOutput> = h.c(c).bind_all(4);
    assert_eq!(outputs.len(), 2);
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.take_window_events();

    h.server.remove_output(2);
    h.roundtrip();
    assert_eq!(
        h.c(c)
            .state
            .globals
            .iter()
            .filter(|(_, i, _)| i == "wl_output")
            .count(),
        1
    );
    // Using the removed output is no error; as a fullscreen output it names nothing.
    w.toplevel.set_fullscreen(Some(&outputs[1]));
    w.toplevel.set_fullscreen(Some(&outputs[0]));
    outputs[1].release();
    h.roundtrip();
    assert!(!h.c(c).is_disconnected());
    let requested: Vec<Option<ViewerId>> = h
        .server
        .take_window_events()
        .into_iter()
        .filter_map(|e| match e {
            WindowEvent::FullscreenRequested {
                id: i,
                on: true,
                output,
            } if i == id => Some(output),
            _ => None,
        })
        .collect();
    assert_eq!(requested, vec![None, Some(ViewerId(1))]);
}

#[test]
fn window_outputs_are_sent_as_enter_and_leave_changes() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    h.server.add_output(2, monitor());
    let c = h.connect();
    let g = globals(h.c(c));
    let outputs: Vec<WlOutput> = h.c(c).bind_all(4);
    let (_w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.roundtrip();
    // Shown nowhere yet.
    assert!(crossings(&mut h, c).is_empty());

    h.server.set_window_outputs(id, &[1]);
    h.roundtrip();
    assert_eq!(crossings(&mut h, c), vec![(true, outputs[0].id())]);
    h.c(c).clear_events();

    h.server.set_window_outputs(id, &[1, 2]);
    h.server.set_window_outputs(id, &[1, 2]);
    h.roundtrip();
    assert_eq!(crossings(&mut h, c), vec![(true, outputs[1].id())]);
    h.c(c).clear_events();

    h.server.set_window_outputs(id, &[2]);
    h.roundtrip();
    assert_eq!(crossings(&mut h, c), vec![(false, outputs[0].id())]);
    h.c(c).clear_events();

    // Removing the output the window is on sends a leave.
    h.server.remove_output(2);
    h.roundtrip();
    assert_eq!(crossings(&mut h, c), vec![(false, outputs[1].id())]);
}

#[test]
fn subsurfaces_enter_and_leave_with_their_window() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let g = globals(h.c(c));
    let output: WlOutput = h.c(c).bind(4);
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let _sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    w.surface.commit();
    h.roundtrip();
    h.c(c).clear_events();

    h.server.set_window_outputs(id, &[1]);
    h.roundtrip();
    let entered: Vec<ObjectId> = h
        .c(c)
        .events::<wl_surface::Event>()
        .iter()
        .filter(
            |(_, e)| matches!(e, wl_surface::Event::Enter { output: o } if o.id() == output.id()),
        )
        .map(|(surface, _)| surface.clone())
        .collect();
    assert_eq!(entered, vec![w.surface.id(), child.id()]);
    h.c(c).clear_events();
    h.server.set_window_outputs(id, &[]);
    h.roundtrip();
    assert_eq!(
        crossings(&mut h, c),
        vec![(false, output.id()), (false, output.id())]
    );
}

#[test]
fn a_subsurface_added_later_enters_the_outputs_of_its_window() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let g = globals(h.c(c));
    let output: WlOutput = h.c(c).bind(4);
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.set_window_outputs(id, &[1]);
    h.roundtrip();
    h.c(c).clear_events();

    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let _sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    let grandchild = g.compositor.create_surface(&qh, ());
    let _sub2 = g.subcompositor.get_subsurface(&grandchild, &child, &qh, ());
    w.surface.commit();
    h.roundtrip();
    let entered: Vec<ObjectId> = h
        .c(c)
        .events::<wl_surface::Event>()
        .iter()
        .filter(
            |(_, e)| matches!(e, wl_surface::Event::Enter { output: o } if o.id() == output.id()),
        )
        .map(|(surface, _)| surface.clone())
        .collect();
    assert_eq!(entered, vec![child.id(), grandchild.id()]);
}

#[test]
fn a_destroyed_subsurface_leaves_and_a_new_one_enters_again() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let g = globals(h.c(c));
    let output: WlOutput = h.c(c).bind(4);
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.set_window_outputs(id, &[1]);
    let qh = h.c(c).qh.clone();
    let child = g.compositor.create_surface(&qh, ());
    let sub = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    w.surface.commit();
    h.roundtrip();
    h.c(c).clear_events();

    sub.destroy();
    let _again = g.subcompositor.get_subsurface(&child, &w.surface, &qh, ());
    w.surface.commit();
    h.roundtrip();
    assert_eq!(
        crossings(&mut h, c),
        vec![(false, output.id()), (true, output.id())]
    );
}

#[test]
fn windows_keep_their_outputs_while_paused() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    h.server.add_output(2, monitor());
    let c = h.connect();
    let g = globals(h.c(c));
    let outputs: Vec<WlOutput> = h.c(c).bind_all(4);
    let (_w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.set_window_outputs(id, &[1]);
    h.roundtrip();
    h.c(c).clear_events();

    h.server.set_paused(true);
    h.server.set_window_outputs(id, &[2]);
    h.roundtrip();
    assert!(
        crossings(&mut h, c).is_empty(),
        "no leave or enter while paused"
    );

    h.server.set_paused(false);
    h.server.set_window_outputs(id, &[2]);
    h.roundtrip();
    assert_eq!(
        crossings(&mut h, c),
        vec![(false, outputs[0].id()), (true, outputs[1].id())]
    );
}

#[test]
fn an_output_bound_later_is_entered_by_the_windows_on_it() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let g = globals(h.c(c));
    let (_w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.set_window_outputs(id, &[1]);
    let output: WlOutput = h.c(c).bind(4);
    h.roundtrip();
    assert_eq!(crossings(&mut h, c), vec![(true, output.id())]);
}

#[test]
fn a_re_flow_sends_the_new_position_then_done() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    let c = h.connect();
    let _output: WlOutput = h.c(c).bind(4);
    h.roundtrip();
    h.c(c).clear_events();
    let mut moved = laptop();
    moved.position = (1920, 0);
    h.server.add_output(1, moved);
    h.roundtrip();
    assert_eq!(
        count(&mut h, c, |e| matches!(
            e,
            wl_output::Event::Geometry { x: 1920, y: 0, .. }
        )),
        1
    );
    assert!(ends_with_done(&mut h, c));
}

#[test]
fn a_frame_callback_on_two_outputs_completes_on_the_first_presentation() {
    let mut h = Harness::new();
    h.server.add_output(1, laptop());
    h.server.add_output(2, monitor());
    let c = h.connect();
    let g = globals(h.c(c));
    let (w, id) = open_window(&mut h, c, &g, 64, 64, RED);
    h.server.set_window_outputs(id, &[1, 2]);
    let qh = h.c(c).qh.clone();
    let _cb: WlCallback = w.surface.frame(&qh, ());
    w.surface.commit();
    h.roundtrip();
    // Output 2 flips first; output 1's later flip has nothing left to complete.
    h.server.presented(10, &[id]);
    h.server.presented(12, &[id]);
    h.roundtrip();
    let done: Vec<u32> = h
        .c(c)
        .events::<wl_callback::Event>()
        .iter()
        .map(|(_, e)| match e {
            wl_callback::Event::Done { callback_data } => *callback_data,
            _ => 0,
        })
        .collect();
    assert_eq!(done, vec![10]);
}
