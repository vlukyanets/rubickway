//! Keyboards, touchpads and mice through libinput (research S8).
//!
//! [`translate`] is the pure part: libinput's values, already extracted, to `InputEvent`s.

use crate::canvas::Vector;
use crate::input::{Button, InputEvent, ScrollSource};

/// A libinput event reduced to the values RubickWay uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RawEvent {
    /// Accelerated relative motion.
    Motion {
        dx: f64,
        dy: f64,
    },
    /// `seat_count`: how many devices on the seat hold this button after the event.
    Button {
        code: u32,
        pressed: bool,
        seat_count: u32,
    },
    /// A wheel: `v120` in 1/120 notches, `delta` in logical pixels.
    Wheel {
        v120: Vector,
        delta: Vector,
    },
    /// Fingers on a touchpad.
    Finger {
        delta: Vector,
    },
    /// A continuous device such as a trackpoint.
    Continuous {
        delta: Vector,
    },
    PinchBegin,
    PinchUpdate {
        scale: f64,
    },
    PinchEnd {
        cancelled: bool,
    },
    /// `seat_count`: how many devices on the seat hold this key after the event.
    Key {
        code: u32,
        pressed: bool,
        seat_count: u32,
        time_ms: u32,
    },
}

/// A translated event: pointer and gesture events are complete; a key still needs its
/// binding symbol from the keyboard state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Translated {
    Input(InputEvent),
    Key {
        code: u32,
        pressed: bool,
        time_ms: u32,
    },
}

/// Translates one event; `None` for a duplicate press or release from a second device on the
/// seat (only the first press and the last release count).
pub(crate) fn translate(event: RawEvent) -> Option<Translated> {
    let input = match event {
        RawEvent::Motion { dx, dy } => InputEvent::PointerMoved {
            delta: Vector::new(dx, dy),
        },
        RawEvent::Button {
            code,
            pressed,
            seat_count,
        } => {
            if !first_or_last(pressed, seat_count) {
                return None;
            }
            InputEvent::PointerButton {
                button: Button::from_code(code),
                code,
                pressed,
            }
        }
        RawEvent::Wheel { v120, delta } => InputEvent::Scroll {
            source: ScrollSource::Wheel,
            delta,
            value120: Some(v120),
        },
        RawEvent::Finger { delta } => InputEvent::Scroll {
            source: ScrollSource::Finger,
            delta,
            value120: None,
        },
        RawEvent::Continuous { delta } => InputEvent::Scroll {
            source: ScrollSource::Continuous,
            delta,
            value120: None,
        },
        RawEvent::PinchBegin => InputEvent::PinchBegin,
        RawEvent::PinchUpdate { scale } => InputEvent::PinchUpdate { scale },
        RawEvent::PinchEnd { cancelled } => InputEvent::PinchEnd { cancelled },
        RawEvent::Key {
            code,
            pressed,
            seat_count,
            time_ms,
        } => {
            if !first_or_last(pressed, seat_count) {
                return None;
            }
            return Some(Translated::Key {
                code,
                pressed,
                time_ms,
            });
        }
    };
    Some(Translated::Input(input))
}

// --- The libinput context -------------------------------------------------------------------

use std::io;
use std::os::fd::{AsFd, OwnedFd};

use input::event::gesture::{
    GestureEndEvent, GestureEvent, GesturePinchEvent, GesturePinchEventTrait,
};
use input::event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait};
use input::event::pointer::{Axis, ButtonState, PointerEvent, PointerScrollEvent};
use input::event::{DeviceEvent, Event, EventTrait};

use super::session::{InputDevices, SharedSession};
use super::{DrmError, ReadFailures};

/// libinput on `seat0`, opening devices through the session.
pub(crate) struct InputContext {
    libinput: input::Libinput,
    failures: ReadFailures,
}

impl InputContext {
    pub fn new(session: SharedSession) -> Result<InputContext, DrmError> {
        let mut libinput = input::Libinput::new_with_udev(InputDevices(session));
        libinput
            .udev_assign_seat("seat0")
            .map_err(|()| DrmError::Input("cannot use seat0".into()))?;
        Ok(InputContext {
            libinput,
            failures: ReadFailures::default(),
        })
    }

    /// A duplicate of libinput's fd, for the event loop.
    pub fn fd(&self) -> io::Result<OwnedFd> {
        self.libinput.as_fd().try_clone_to_owned()
    }

    /// Stops reading devices (session paused).
    pub fn suspend(&mut self) {
        self.libinput.suspend();
    }

    /// Reopens the devices (session back).
    pub fn resume(&mut self) {
        if self.libinput.resume().is_err() {
            tracing::warn!("libinput could not resume its devices");
        }
    }

    /// Reads pending events. New touchpads get tap-to-click (FR-006).
    /// Reads pending events; fails once reading has failed `READ_FAILURES` times in a row
    /// (FR-012).
    pub fn dispatch(&mut self) -> Result<Vec<RawEvent>, DrmError> {
        let failed = self.libinput.dispatch().err();
        if let Some(err) = &failed {
            tracing::warn!("reading input devices failed: {err}");
        }
        if self.failures.note(failed.is_some())
            && let Some(err) = failed
        {
            return Err(DrmError::Input(format!(
                "reading them keeps failing: {err}"
            )));
        }
        let mut out = Vec::new();
        for event in &mut self.libinput {
            if let Some(raw) = reduce(event) {
                out.push(raw);
            }
        }
        Ok(out)
    }
}

/// Reduces a libinput event to the values RubickWay uses (and configures new devices).
fn reduce(event: Event) -> Option<RawEvent> {
    let scroll = |e: &dyn PointerScrollEvent| {
        let value = |axis| {
            if e.has_axis(axis) {
                e.scroll_value(axis)
            } else {
                0.0
            }
        };
        Vector::new(value(Axis::Horizontal), value(Axis::Vertical))
    };
    match event {
        Event::Device(DeviceEvent::Added(added)) => {
            let mut device = added.device();
            let fingers = device.config_tap_finger_count();
            if fingers > 0 {
                let _ = device.config_tap_set_enabled(true);
                // libinput's tap map: 1 finger left, 2 right, 3 middle - if the pad sees 3.
                tracing::info!(
                    "touchpad {}: tap with up to {fingers} fingers",
                    device.name()
                );
            }
            None
        }
        Event::Pointer(PointerEvent::Motion(e)) => Some(RawEvent::Motion {
            dx: e.dx(),
            dy: e.dy(),
        }),
        Event::Pointer(PointerEvent::Button(e)) => Some(RawEvent::Button {
            code: e.button(),
            pressed: e.button_state() == ButtonState::Pressed,
            seat_count: e.seat_button_count(),
        }),
        Event::Pointer(PointerEvent::ScrollWheel(e)) => {
            let v120 = |axis| {
                if e.has_axis(axis) {
                    e.scroll_value_v120(axis)
                } else {
                    0.0
                }
            };
            Some(RawEvent::Wheel {
                v120: Vector::new(v120(Axis::Horizontal), v120(Axis::Vertical)),
                delta: scroll(&e),
            })
        }
        Event::Pointer(PointerEvent::ScrollFinger(e)) => {
            Some(RawEvent::Finger { delta: scroll(&e) })
        }
        Event::Pointer(PointerEvent::ScrollContinuous(e)) => {
            Some(RawEvent::Continuous { delta: scroll(&e) })
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::Begin(_))) => {
            Some(RawEvent::PinchBegin)
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::Update(e))) => {
            Some(RawEvent::PinchUpdate { scale: e.scale() })
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::End(e))) => {
            Some(RawEvent::PinchEnd {
                cancelled: e.cancelled(),
            })
        }
        Event::Keyboard(KeyboardEvent::Key(e)) => Some(RawEvent::Key {
            code: e.key(),
            pressed: e.key_state() == KeyState::Pressed,
            seat_count: e.seat_key_count(),
            time_ms: e.time(),
        }),
        _ => None,
    }
}

/// A press counts when it is the first on the seat, a release when it is the last.
fn first_or_last(pressed: bool, seat_count: u32) -> bool {
    if pressed {
        seat_count == 1
    } else {
        seat_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_is_relative() {
        assert_eq!(
            translate(RawEvent::Motion { dx: 3.5, dy: -2.0 }),
            Some(Translated::Input(InputEvent::PointerMoved {
                delta: Vector::new(3.5, -2.0)
            }))
        );
    }

    #[test]
    fn buttons_from_a_second_device_are_dropped() {
        let press = |seat_count| RawEvent::Button {
            code: 0x110,
            pressed: true,
            seat_count,
        };
        assert_eq!(
            translate(press(1)),
            Some(Translated::Input(InputEvent::PointerButton {
                button: Button::Left,
                code: 0x110,
                pressed: true
            }))
        );
        assert_eq!(translate(press(2)), None);
        let release = |seat_count| RawEvent::Button {
            code: 0x111,
            pressed: false,
            seat_count,
        };
        assert_eq!(translate(release(1)), None);
        assert!(matches!(
            translate(release(0)),
            Some(Translated::Input(InputEvent::PointerButton {
                button: Button::Right,
                ..
            }))
        ));
    }

    #[test]
    fn scrolling_keeps_its_source() {
        let v = Vector::new(0.0, 120.0);
        assert_eq!(
            translate(RawEvent::Wheel {
                v120: v,
                delta: Vector::new(0.0, 15.0)
            }),
            Some(Translated::Input(InputEvent::Scroll {
                source: ScrollSource::Wheel,
                delta: Vector::new(0.0, 15.0),
                value120: Some(v)
            }))
        );
        assert_eq!(
            translate(RawEvent::Finger {
                delta: Vector::new(1.0, 2.0)
            }),
            Some(Translated::Input(InputEvent::Scroll {
                source: ScrollSource::Finger,
                delta: Vector::new(1.0, 2.0),
                value120: None
            }))
        );
        assert_eq!(
            translate(RawEvent::Continuous {
                delta: Vector::new(0.0, -4.0)
            }),
            Some(Translated::Input(InputEvent::Scroll {
                source: ScrollSource::Continuous,
                delta: Vector::new(0.0, -4.0),
                value120: None
            }))
        );
    }

    #[test]
    fn pinches_pass_through() {
        assert_eq!(
            translate(RawEvent::PinchBegin),
            Some(Translated::Input(InputEvent::PinchBegin))
        );
        assert_eq!(
            translate(RawEvent::PinchUpdate { scale: 1.5 }),
            Some(Translated::Input(InputEvent::PinchUpdate { scale: 1.5 }))
        );
        assert_eq!(
            translate(RawEvent::PinchEnd { cancelled: true }),
            Some(Translated::Input(InputEvent::PinchEnd { cancelled: true }))
        );
    }

    #[test]
    fn keys_keep_their_time_and_drop_duplicates() {
        let key = |pressed, seat_count| RawEvent::Key {
            code: 16,
            pressed,
            seat_count,
            time_ms: 1234,
        };
        assert_eq!(
            translate(key(true, 1)),
            Some(Translated::Key {
                code: 16,
                pressed: true,
                time_ms: 1234
            })
        );
        assert_eq!(translate(key(true, 2)), None);
        assert_eq!(translate(key(false, 1)), None);
        assert!(translate(key(false, 0)).is_some());
    }
}
