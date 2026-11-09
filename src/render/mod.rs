//! Rendering: grid parameters (pure) and the EGL/GLES renderer.

pub mod cursor;
pub mod egl;
pub mod formats;
mod gles;
mod grid;
mod image;

pub use gles::{
    FULLSCREEN_BACKGROUND, GlesRenderer, ImageRef, Quad, RenderError, Transform, offscreen,
    offscreen_fullscreen_scene, offscreen_renderer_name, offscreen_scene, offscreen_with_detail,
    offscreen_with_images,
};
pub use grid::{AXIS_ALPHA, BACKGROUND, GridParams, MAJOR_ALPHA, MINOR_ALPHA};
pub use image::{ImageDesc, PlaneDesc};
