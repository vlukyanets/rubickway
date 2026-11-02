//! The host seat: keyboard, pointer, pinch gestures and cursor shape -> `InputEvent`s.

use std::fs::File;
use std::os::unix::fs::FileExt;

use wayland_client::protocol::{wl_keyboard, wl_pointer, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    self, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{
    self, ZwpPointerGesturePinchV1,
};

use super::{Globals, HostState};
use crate::backend::BackendEvent;
use crate::canvas::{Point, Vector};
use crate::input::keyboard::Keyboard;
use crate::input::{Button, InputEvent, RawModifiers, ScrollSource};

/// Linux input event codes for pointer buttons.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// The pointer cursor to show over the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Grabbing,
    Move,
    ResizeN,
    ResizeNE,
    ResizeE,
    ResizeSE,
    ResizeS,
    ResizeSW,
    ResizeW,
    ResizeNW,
}

/// Axis events collected until `wl_pointer.frame`.
#[derive(Default)]
struct PendingScroll {
    source: Option<ScrollSource>,
    delta: Vector,
    value120: Option<Vector>,
    any: bool,
}

pub(crate) struct Seat {
    _seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    xkb: Option<Keyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pinch: Option<ZwpPointerGesturePinchV1>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    /// Serial of the latest pointer enter, needed to set the cursor.
    enter_serial: Option<u32>,
    cursor: Cursor,
    scroll: PendingScroll,
}

impl Seat {
    pub fn new(seat: Option<wl_seat::WlSeat>) -> Seat {
        Seat {
            _seat: seat,
            keyboard: None,
            xkb: None,
            pointer: None,
            pinch: None,
            cursor_device: None,
            enter_serial: None,
            cursor: Cursor::Default,
            scroll: PendingScroll::default(),
        }
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
        self.apply_cursor();
    }

    fn apply_cursor(&self) {
        if let (Some(device), Some(serial)) = (&self.cursor_device, self.enter_serial) {
            use wp_cursor_shape_device_v1::Shape;
            let shape = match self.cursor {
                Cursor::Default => Shape::Default,
                Cursor::Grabbing => Shape::Grabbing,
                Cursor::Move => Shape::Move,
                Cursor::ResizeN => Shape::NResize,
                Cursor::ResizeNE => Shape::NeResize,
                Cursor::ResizeE => Shape::EResize,
                Cursor::ResizeSE => Shape::SeResize,
                Cursor::ResizeS => Shape::SResize,
                Cursor::ResizeSW => Shape::SwResize,
                Cursor::ResizeW => Shape::WResize,
                Cursor::ResizeNW => Shape::NwResize,
            };
            device.set_shape(serial, shape);
        }
    }

    fn add_pointer(
        &mut self,
        seat: &wl_seat::WlSeat,
        globals: &Globals,
        qh: &QueueHandle<HostState>,
    ) {
        let pointer = seat.get_pointer(qh, ());
        self.pinch = globals
            .pointer_gestures
            .as_ref()
            .map(|gestures| gestures.get_pinch_gesture(&pointer, qh, ()));
        self.cursor_device = globals
            .cursor_shape
            .as_ref()
            .map(|manager| manager.get_pointer(&pointer, qh, ()));
        self.pointer = Some(pointer);
    }

    fn remove_pointer(&mut self) {
        if let Some(pinch) = self.pinch.take() {
            pinch.destroy();
        }
        if let Some(device) = self.cursor_device.take() {
            device.destroy();
        }
        if let Some(pointer) = self.pointer.take() {
            pointer.release();
        }
        self.enter_serial = None;
    }
}

/// Reads a keymap fd into a string without moving the shared file offset (research R7).
///
/// `size` comes from the host's message, so it is checked against the real length of the file
/// before anything is allocated: a size the file cannot back is an invalid keymap.
fn read_keymap(file: &File, size: u32) -> Option<String> {
    let file_len = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(err) => {
            tracing::warn!("cannot inspect the keymap fd: {err}");
            return None;
        }
    };
    if u64::from(size) > file_len {
        tracing::warn!("keymap claims {size} bytes but its file has {file_len}; ignoring it");
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    if let Err(err) = file.read_exact_at(&mut buf, 0) {
        tracing::warn!("cannot read the keymap: {err}");
        return None;
    }
    // The keymap is NUL-terminated.
    if let Some(end) = buf.iter().position(|&b| b == 0) {
        buf.truncate(end);
    }
    String::from_utf8(buf)
        .map_err(|err| tracing::warn!("keymap is not UTF-8: {err}"))
        .ok()
}

fn input(state: &mut HostState, event: InputEvent) {
    state.push(BackendEvent::Input(event));
}

impl Dispatch<wl_seat::WlSeat, ()> for HostState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        else {
            return;
        };
        let has_keyboard = caps.contains(wl_seat::Capability::Keyboard);
        match (&state.seat.keyboard, has_keyboard) {
            (None, true) => state.seat.keyboard = Some(seat.get_keyboard(qh, ())),
            (Some(keyboard), false) => {
                keyboard.release();
                state.seat.keyboard = None;
                state.seat.xkb = None;
            }
            _ => {}
        }
        let has_pointer = caps.contains(wl_seat::Capability::Pointer);
        match (state.seat.pointer.is_some(), has_pointer) {
            (false, true) => state.seat.add_pointer(seat, &state.globals, qh),
            (true, false) => state.seat.remove_pointer(),
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
                    tracing::warn!("unsupported keymap format {format:?}");
                    return;
                }
                let file = File::from(fd);
                let Some(text) = read_keymap(&file, size) else {
                    state.seat.xkb = None;
                    return;
                };
                state.seat.xkb = Keyboard::from_keymap_string(text.clone());
                match state.seat.xkb {
                    // Applications get the same keymap (research R11).
                    Some(_) => state.push(BackendEvent::Keymap(text)),
                    None => tracing::warn!("the host keymap does not compile"),
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let Some(xkb) = &state.seat.xkb else { return };
                let pressed = key_state == WEnum::Value(wl_keyboard::KeyState::Pressed);
                let binding_sym = xkb.binding_sym(key);
                input(
                    state,
                    InputEvent::Key {
                        keycode: key,
                        binding_sym,
                        pressed,
                    },
                );
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                state.push(BackendEvent::RepeatInfo { rate, delay });
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                let Some(xkb) = &mut state.seat.xkb else {
                    return;
                };
                let mods = xkb.update_mask(mods_depressed, mods_latched, mods_locked, group);
                let raw = RawModifiers {
                    depressed: mods_depressed,
                    latched: mods_latched,
                    locked: mods_locked,
                    group,
                };
                input(state, InputEvent::Modifiers { mods, raw });
            }
            wl_keyboard::Event::Leave { .. } => input(state, InputEvent::FocusLost),
            // Modifiers follow in their own event.
            wl_keyboard::Event::Enter { .. } => input(state, InputEvent::FocusGained),
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial,
                surface_x,
                surface_y,
                ..
            } => {
                state.seat.enter_serial = Some(serial);
                state.seat.apply_cursor();
                let pos = Point::new(surface_x, surface_y);
                input(state, InputEvent::PointerEnter { pos });
            }
            wl_pointer::Event::Leave { .. } => {
                state.seat.enter_serial = None;
                input(state, InputEvent::PointerLeave);
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                let pos = Point::new(surface_x, surface_y);
                input(state, InputEvent::PointerMotion { pos });
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let kind = match button {
                    BTN_LEFT => Button::Left,
                    BTN_RIGHT => Button::Right,
                    BTN_MIDDLE => Button::Middle,
                    other => Button::Other(other),
                };
                let pressed = button_state == WEnum::Value(wl_pointer::ButtonState::Pressed);
                input(
                    state,
                    InputEvent::PointerButton {
                        button: kind,
                        code: button,
                        pressed,
                    },
                );
            }
            wl_pointer::Event::AxisSource {
                axis_source: WEnum::Value(source),
            } => {
                state.seat.scroll.source = Some(match source {
                    wl_pointer::AxisSource::Finger => ScrollSource::Finger,
                    wl_pointer::AxisSource::Continuous => ScrollSource::Continuous,
                    _ => ScrollSource::Wheel,
                });
            }
            wl_pointer::Event::Axis {
                axis: WEnum::Value(axis),
                value,
                ..
            } => {
                let scroll = &mut state.seat.scroll;
                scroll.any = true;
                match axis {
                    wl_pointer::Axis::VerticalScroll => scroll.delta.y += value,
                    wl_pointer::Axis::HorizontalScroll => scroll.delta.x += value,
                    _ => {}
                }
            }
            wl_pointer::Event::AxisDiscrete {
                axis: WEnum::Value(axis),
                discrete,
            } => add_value120(&mut state.seat.scroll, axis, f64::from(discrete) * 120.0),
            wl_pointer::Event::AxisValue120 {
                axis: WEnum::Value(axis),
                value120,
            } => add_value120(&mut state.seat.scroll, axis, f64::from(value120)),
            wl_pointer::Event::Frame => {
                let scroll = std::mem::take(&mut state.seat.scroll);
                if scroll.any || scroll.value120.is_some() {
                    let source = scroll.source.unwrap_or(if scroll.value120.is_some() {
                        ScrollSource::Wheel
                    } else {
                        ScrollSource::Continuous
                    });
                    input(
                        state,
                        InputEvent::Scroll {
                            source,
                            delta: scroll.delta,
                            value120: scroll.value120,
                        },
                    );
                }
            }
            // AxisStop (fingers lifted) needs no handling: there is no kinetic scrolling.
            _ => {}
        }
    }
}

fn add_value120(scroll: &mut PendingScroll, axis: wl_pointer::Axis, value: f64) {
    let v = scroll.value120.get_or_insert(Vector::default());
    match axis {
        wl_pointer::Axis::VerticalScroll => v.y += value,
        wl_pointer::Axis::HorizontalScroll => v.x += value,
        _ => {}
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { .. } => {
                input(state, InputEvent::PinchBegin)
            }
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
                input(state, InputEvent::PinchUpdate { scale });
            }
            zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => input(
                state,
                InputEvent::PinchEnd {
                    cancelled: cancelled != 0,
                },
            ),
            _ => {}
        }
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for HostState {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: wp_cursor_shape_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The cursor shape device has no events.
    }
}
