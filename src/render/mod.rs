//! Rendering: grid parameters (pure) and the EGL/GLES renderer.

pub mod egl;
mod gles;
mod grid;

pub use gles::{GlesRenderer, RenderError, offscreen, offscreen_renderer_name};
pub use grid::{AXIS_ALPHA, BACKGROUND, GridParams, MAJOR_ALPHA, MINOR_ALPHA};
