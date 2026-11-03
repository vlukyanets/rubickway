//! A viewer: one window onto the canvas, with its camera and input handling.
//!
//! The viewer is pure state: it turns `InputEvent`s into camera changes and effects, and knows
//! nothing about Wayland or GL.

mod gesture;

use crate::canvas::{Camera, Point, Size, Vector};
use crate::input::bindings::Bindings;
use crate::input::{InputEvent, Modifiers, ScrollSource};
use crate::render::GridParams;

pub use gesture::Gesture;

/// Zoom factor per wheel notch (10%).
const ZOOM_STEP: f64 = 1.1;
/// One wheel notch in `value120` units.
const NOTCH: f64 = 120.0;
/// Touchpad / trackpoint scroll distance (logical px) for one zoom step with Mod.
const SCROLL_PER_STEP: f64 = 50.0;
/// Horizontal pan per horizontal wheel notch, in logical px.
const WHEEL_PAN: f64 = 50.0;

/// One viewer's state.
#[derive(Debug, Clone)]
pub struct Viewer {
    size: Size,
    scale: f64,
    camera: Camera,
    gesture: Gesture,
    modifiers: Modifiers,
    pointer: Option<Point>,
    dirty: bool,
    bindings: Bindings,
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Buffer sides must fit the `int` used by the protocol and EGL.
const MAX_INT_SIDE: u32 = i32::MAX as u32;

impl Viewer {
    /// A viewer showing the canvas origin at 100%, 1 x 1 until its display's size is set with
    /// [`Viewer::configure`]; it needs a first frame.
    pub fn new() -> Viewer {
        Viewer {
            size: Size::new(1.0, 1.0),
            scale: 1.0,
            camera: Camera::default(),
            gesture: Gesture::Idle,
            modifiers: Modifiers::NONE,
            pointer: None,
            dirty: true,
            bindings: Bindings::default(),
        }
    }

    /// The shortcuts and Mod this viewer reacts to.
    pub fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
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

    /// Buffer size in physical pixels: logical size times the scale, rounded, at least 1x1 and
    /// at most what the protocol's and EGL's `int` can hold. A display's mode is far below that.
    pub fn buffer_size(&self) -> (u32, u32) {
        let max = f64::from(MAX_INT_SIDE);
        // Float-to-int `as` saturates; the clamp keeps it within `int` anyway.
        let px = |v: f64| (v * self.scale).round().clamp(1.0, max) as u32;
        (px(self.size.w), px(self.size.h))
    }

    /// Grid parameters for the current camera, size and scale.
    pub fn grid_params(&self) -> GridParams {
        GridParams::new(&self.camera, self.size, self.scale)
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

    /// Applies the display's logical size; a non-positive or non-finite component keeps the
    /// current one. The camera centre and zoom are kept, so the canvas point at the centre
    /// stays there.
    pub fn configure(&mut self, size: Size) {
        let pick = |v: f64, current: f64| if v > 0.0 && v.is_finite() { v } else { current };
        let size = Size::new(pick(size.w, self.size.w), pick(size.h, self.size.h));
        if size != self.size {
            self.size = size;
            self.dirty = true;
        }
    }

    /// Applies a display scale; non-finite or non-positive values are ignored.
    pub fn set_scale(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0.0 && scale != self.scale {
            self.scale = scale;
            self.dirty = true;
        }
    }

    /// Handles one input event; redraws are signalled through [`Viewer::take_dirty`].
    pub fn handle(&mut self, event: &InputEvent) {
        let before = self.camera;
        self.dispatch(event);
        if self.camera != before {
            self.dirty = true;
        }
    }

    fn dispatch(&mut self, event: &InputEvent) {
        match *event {
            InputEvent::Modifiers { mods, .. } => self.modifiers = mods,
            // Shortcuts and buttons are the shell's: it resets the view (to the home view,
            // FR-020) and starts pans with `begin_pan` (FR-032).
            InputEvent::Key { .. } | InputEvent::PointerButton { .. } => {}
            InputEvent::PointerEnter { pos } | InputEvent::PointerMotion { pos } => {
                self.pointer = Some(pos);
                if let Gesture::Panning { anchor } = self.gesture {
                    self.camera.pan_anchor(anchor, pos, self.size);
                }
            }
            InputEvent::PointerLeave => {
                // A leave during a drag means the grab broke (the console was switched away,
                // research S23): the drag ends.
                self.pointer = None;
                if self.gesture.is_panning() {
                    self.gesture = Gesture::Idle;
                }
            }
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
            // The shell turns relative motion into positions for its viewers (research S9).
            InputEvent::PointerMoved { .. } => {}
        }
    }

    /// Starts a pan drag at the pointer, whichever button the shell decided pans (FR-032).
    /// Nothing without a pointer or during another gesture; returns whether a pan started.
    pub fn begin_pan(&mut self) -> bool {
        match (self.gesture, self.pointer) {
            (Gesture::Idle, Some(pos)) => {
                let anchor = self.camera.to_canvas(pos, self.size);
                self.gesture = Gesture::Panning { anchor };
                true
            }
            _ => false,
        }
    }

    /// Ends a pan drag started with [`Viewer::begin_pan`].
    pub fn end_pan(&mut self) {
        if self.gesture.is_panning() {
            self.gesture = Gesture::Idle;
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
                if self.bindings.mod_held(self.modifiers) {
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
}
