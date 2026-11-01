//! Rendering: grid parameters (pure) and the EGL/GLES renderer.

pub mod egl;
mod gles;
mod grid;

pub use gles::{
    GlesRenderer, ImageRef, Quad, RenderError, Transform, offscreen, offscreen_renderer_name,
    offscreen_scene,
};
pub use grid::{AXIS_ALPHA, BACKGROUND, GridParams, MAJOR_ALPHA, MINOR_ALPHA};
