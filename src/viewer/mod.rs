//! A viewer: one window onto the canvas, with its camera and input handling.
//!
//! The viewer is pure state: it turns `InputEvent`s into camera changes and effects, and knows
//! nothing about Wayland or GL.

mod gesture;

use crate::canvas::{Camera, Point, Size, Vector};
use crate::input::bindings::{self, Action};
use crate::input::{Button, InputEvent, Modifiers, ScrollSource};
use crate::render::GridParams;

pub use gesture::Gesture;

/// Logical size used when the host leaves the choice to us.
pub const DEFAULT_SIZE: Size = Size {
    w: 1280.0,
    h: 800.0,
};

/// Zoom factor per wheel notch (10%).
const ZOOM_STEP: f64 = 1.1;
/// One wheel notch in `value120` units.
const NOTCH: f64 = 120.0;
/// Touchpad / trackpoint scroll distance (logical px) for one zoom step with Mod.
const SCROLL_PER_STEP: f64 = 50.0;
/// Horizontal pan per horizontal wheel notch, in logical px.
const WHEEL_PAN: f64 = 50.0;

/// What the application must do after an event. Redraws are signalled through
/// [`Viewer::take_dirty`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effect {
    /// The user asked to quit.
    pub quit: bool,
}

/// One viewer's state.
#[derive(Debug, Clone)]
pub struct Viewer {
    size: Size,
    scale: f64,
    /// Largest buffer side the renderer can draw, in physical pixels.
    max_buffer_side: u32,
    camera: Camera,
    gesture: Gesture,
    modifiers: Modifiers,
    pointer: Option<Point>,
    dirty: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Buffer sides must fit the `int` used by the protocol and EGL.
const MAX_INT_SIDE: u32 = i32::MAX as u32;

impl Viewer {
    /// A viewer of the default size showing the canvas origin at 100%; it needs a first frame.
    pub fn new() -> Viewer {
        Viewer {
            size: DEFAULT_SIZE,
            scale: 1.0,
            max_buffer_side: MAX_INT_SIDE,
            camera: Camera::default(),
            gesture: Gesture::Idle,
            modifiers: Modifiers::NONE,
            pointer: None,
            dirty: true,
        }
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Replaces the camera (zoom clamped to the limits); marks the viewer dirty if it changed.
    pub fn set_camera(&mut self, camera: Camera) {
        let zoom = if camera.zoom.is_finite() {
            camera
                .zoom
                .clamp(crate::canvas::ZOOM_MIN, crate::canvas::ZOOM_MAX)
        } else {
            self.camera.zoom
        };
        let camera = Camera { zoom, ..camera };
        if camera != self.camera {
            self.camera = camera;
            self.dirty = true;
        }
    }

    /// Logical size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Buffer scale (physical pixels per logical pixel).
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Sets the largest buffer side the renderer can draw (the GPU's framebuffer limit).
    /// Larger windows get a uniformly smaller buffer that the host scales up.
    pub fn set_max_buffer_side(&mut self, side: u32) {
        let side = side.clamp(1, MAX_INT_SIDE);
        if side != self.max_buffer_side {
            self.max_buffer_side = side;
            self.dirty = true;
        }
    }

    /// The scale actually drawn at: the host's scale, reduced uniformly when the buffer would
    /// exceed the largest side the renderer can draw.
    pub fn render_scale(&self) -> f64 {
        let max = f64::from(self.max_buffer_side);
        let longest = self.size.w.max(self.size.h);
        if longest * self.scale > max {
            max / longest
        } else {
            self.scale
        }
    }

    /// Buffer size in physical pixels: logical size times the render scale, rounded, at least
    /// 1x1 and at most the largest drawable side.
    pub fn buffer_size(&self) -> (u32, u32) {
        let scale = self.render_scale();
        let max = f64::from(self.max_buffer_side);
        // Float-to-int `as` saturates; the clamp keeps it within the drawable limit anyway.
        let px = |v: f64| (v * scale).round().clamp(1.0, max) as u32;
        (px(self.size.w), px(self.size.h))
    }

    /// Grid parameters for the current camera, size and render scale.
    pub fn grid_params(&self) -> GridParams {
        GridParams::new(&self.camera, self.size, self.render_scale())
    }

    /// The last pointer position in logical pixels, if the pointer is over the viewer.
    pub fn pointer(&self) -> Option<Point> {
        self.pointer
    }

    /// Whether a pan drag is in progress (for the cursor shape).
    pub fn is_panning(&self) -> bool {
        self.gesture.is_panning()
    }

    /// Returns whether a new frame is needed, and clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Applies a size from the host. Zero components fall back to the default size; the
    /// camera centre and zoom are kept, so the canvas point at the centre stays there.
    pub fn configure(&mut self, size: Size) {
        let pick = |v: f64, default: f64| if v > 0.0 && v.is_finite() { v } else { default };
        let size = Size::new(pick(size.w, DEFAULT_SIZE.w), pick(size.h, DEFAULT_SIZE.h));
        if size != self.size {
            self.size = size;
            self.dirty = true;
        }
    }

    /// Applies the host's preferred scale; non-finite or non-positive values are ignored.
    pub fn set_scale(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0.0 && scale != self.scale {
            self.scale = scale;
            self.dirty = true;
        }
    }

    /// Handles one input event.
    pub fn handle(&mut self, event: &InputEvent) -> Effect {
        let before = self.camera;
        let effect = self.dispatch(event);
        if self.camera != before {
            self.dirty = true;
        }
        effect
    }

    fn dispatch(&mut self, event: &InputEvent) -> Effect {
        match *event {
            InputEvent::Modifiers { mods, .. } => self.modifiers = mods,
            InputEvent::Key {
                binding_sym,
                pressed,
                ..
            } => {
                if let Some(action) = bindings::lookup(self.modifiers, binding_sym, pressed) {
                    return self.run(action);
                }
            }
            InputEvent::PointerEnter { pos } | InputEvent::PointerMotion { pos } => {
                self.pointer = Some(pos);
                if let Gesture::Panning { anchor } = self.gesture {
                    self.camera.pan_anchor(anchor, pos, self.size);
                }
            }
            InputEvent::PointerLeave => {
                // During a drag the host keeps the implicit grab, so a leave here means the
                // host broke it: the drag ends.
                self.pointer = None;
                if self.gesture.is_panning() {
                    self.gesture = Gesture::Idle;
                }
            }
            InputEvent::PointerButton {
                button: Button::Left,
                pressed,
                ..
            } => self.left_button(pressed),
            InputEvent::PointerButton { .. } => {}
            InputEvent::Scroll {
                source,
                delta,
                value120,
            } => self.scroll(source, delta, value120),
            InputEvent::PinchBegin => {
                if self.gesture == Gesture::Idle {
                    self.gesture = Gesture::Pinching {
                        zoom_at_begin: self.camera.zoom,
                    };
                }
            }
            InputEvent::PinchUpdate { scale } => {
                if let Gesture::Pinching { zoom_at_begin } = self.gesture
                    && scale.is_finite()
                    && scale > 0.0
                {
                    let p = self.focus_point();
                    self.camera.set_zoom_at(zoom_at_begin * scale, p, self.size);
                }
            }
            InputEvent::PinchEnd { .. } => {
                if matches!(self.gesture, Gesture::Pinching { .. }) {
                    self.gesture = Gesture::Idle;
                }
            }
            InputEvent::FocusLost => {
                self.modifiers = Modifiers::NONE;
                self.gesture = Gesture::Idle;
            }
            // Modifiers follow in their own event.
            InputEvent::FocusGained => {}
        }
        Effect::default()
    }

    /// Everything is background in this feature, so a plain left press pans, and so does
    /// Mod + left press (which will keep panning over windows later).
    fn left_button(&mut self, pressed: bool) {
        match (pressed, self.gesture, self.pointer) {
            (true, Gesture::Idle, Some(pos)) => {
                let anchor = self.camera.to_canvas(pos, self.size);
                self.gesture = Gesture::Panning { anchor };
            }
            (false, Gesture::Panning { .. }, _) => self.gesture = Gesture::Idle,
            _ => {}
        }
    }

    fn scroll(&mut self, source: ScrollSource, delta: Vector, value120: Option<Vector>) {
        if self.gesture.is_panning() {
            return;
        }
        let p = self.focus_point();
        match source {
            ScrollSource::Wheel => {
                // Notches; a wheel without value120 counts SCROLL_PER_STEP px as one notch.
                let notches = value120
                    .map(|v| v / NOTCH)
                    .unwrap_or(delta / SCROLL_PER_STEP);
                if notches.y != 0.0 {
                    // Wheel up (negative) zooms in.
                    self.camera
                        .zoom_at(ZOOM_STEP.powf(-notches.y), p, self.size);
                }
                if notches.x != 0.0 {
                    self.camera.pan_by(Vector::new(-notches.x * WHEEL_PAN, 0.0));
                }
            }
            ScrollSource::Finger | ScrollSource::Continuous => {
                if bindings::mod_held(self.modifiers) {
                    let steps = -delta.y / SCROLL_PER_STEP;
                    if steps != 0.0 {
                        self.camera.zoom_at(ZOOM_STEP.powf(steps), p, self.size);
                    }
                } else {
                    // Like a scrolled document: scrolling down shows what is below.
                    self.camera.pan_by(-delta);
                }
            }
        }
    }

    /// The point zoom is centred on: the pointer, or the viewer centre without one.
    fn focus_point(&self) -> Point {
        self.pointer.unwrap_or_else(|| self.size.center())
    }

    fn run(&mut self, action: Action) -> Effect {
        match action {
            Action::Quit => Effect { quit: true },
            Action::ResetView => {
                // The camera comparison in `handle` marks the viewer dirty if this moved it.
                self.camera.reset();
                Effect::default()
            }
        }
    }
}
