//! The OpenGL ES 3 renderer: draws the grid for a viewer, on screen or offscreen.
#![allow(unsafe_code)]

use glow::HasContext;

use super::egl::{Egl, EglError};
use super::grid::{self, GridParams};

const VERTEX_SHADER: &str = include_str!("shaders/grid.vert");
const FRAGMENT_SHADER: &str = include_str!("shaders/grid.frag");

/// Errors from rendering.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Egl(#[from] EglError),
    #[error("GL: {0}")]
    Gl(String),
}

struct Uniforms {
    height: Option<glow::UniformLocation>,
    minor: Option<glow::UniformLocation>,
    offset: Option<glow::UniformLocation>,
    fade: Option<glow::UniformLocation>,
    axis: Option<glow::UniformLocation>,
    axis_on: Option<glow::UniformLocation>,
    background: Option<glow::UniformLocation>,
    alpha: Option<glow::UniformLocation>,
}

/// Draws the grid with one full-screen triangle. Needs its GL context to be current.
pub struct GlesRenderer {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    uniforms: Uniforms,
    max_buffer_side: u32,
}

impl GlesRenderer {
    /// Compiles the grid program. The GL context behind `gl` must be current.
    pub fn new(gl: glow::Context) -> Result<GlesRenderer, RenderError> {
        // SAFETY: the caller made the context behind `gl` current on this thread; every object
        // used below is created in this block from that same context.
        unsafe {
            let program = gl.create_program().map_err(RenderError::Gl)?;
            let mut shaders = Vec::new();
            for (kind, source) in [
                (glow::VERTEX_SHADER, VERTEX_SHADER),
                (glow::FRAGMENT_SHADER, FRAGMENT_SHADER),
            ] {
                let shader = gl.create_shader(kind).map_err(RenderError::Gl)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let log = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    return Err(RenderError::Gl(format!("shader compile: {log}")));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(RenderError::Gl(format!("program link: {log}")));
            }
            let at = |name: &str| gl.get_uniform_location(program, name);
            let uniforms = Uniforms {
                height: at("u_height"),
                minor: at("u_minor"),
                offset: at("u_offset"),
                fade: at("u_fade"),
                axis: at("u_axis"),
                axis_on: at("u_axis_on"),
                background: at("u_background"),
                alpha: at("u_alpha"),
            };
            let vao = gl.create_vertex_array().map_err(RenderError::Gl)?;
            // The largest framebuffer this GPU can draw: renderbuffer and viewport limits.
            let mut viewport = [0i32; 2];
            gl.get_parameter_i32_slice(glow::MAX_VIEWPORT_DIMS, &mut viewport);
            let renderbuffer = gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE);
            let max = renderbuffer.min(viewport[0]).min(viewport[1]);
            let max_buffer_side = u32::try_from(max).unwrap_or(0).max(1);
            Ok(GlesRenderer {
                gl,
                program,
                vao,
                uniforms,
                max_buffer_side,
            })
        }
    }

    /// The largest buffer side (in pixels) this GPU can render to.
    pub fn max_buffer_side(&self) -> u32 {
        self.max_buffer_side
    }

    /// Draws the grid into the currently bound framebuffer of `width` x `height` pixels.
    pub fn draw(&self, params: &GridParams, width: u32, height: u32) {
        let gl = &self.gl;
        let u = &self.uniforms;
        let [axis_x, axis_y] = params.axis_px;
        // SAFETY: the context is current (constructor contract); program, VAO and uniform
        // locations were created from it in `new`.
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_1_f32(u.height.as_ref(), height as f32);
            gl.uniform_1_f32(u.minor.as_ref(), params.minor_px);
            gl.uniform_2_f32(u.offset.as_ref(), params.offset_px[0], params.offset_px[1]);
            gl.uniform_1_f32(u.fade.as_ref(), params.minor_alpha);
            gl.uniform_2_f32(
                u.axis.as_ref(),
                axis_x.unwrap_or(0.0),
                axis_y.unwrap_or(0.0),
            );
            gl.uniform_2_f32(
                u.axis_on.as_ref(),
                f32::from(u8::from(axis_x.is_some())),
                f32::from(u8::from(axis_y.is_some())),
            );
            let [r, g, b] = grid::BACKGROUND;
            gl.uniform_3_f32(u.background.as_ref(), r, g, b);
            gl.uniform_3_f32(
                u.alpha.as_ref(),
                grid::MINOR_ALPHA,
                grid::MAJOR_ALPHA,
                grid::AXIS_ALPHA,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }

    /// Renders into a new framebuffer object and reads it back as RGBA, top row first.
    fn render_to_pixels(
        &self,
        params: &GridParams,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RenderError> {
        let gl = &self.gl;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        // SAFETY: the context is current; the framebuffer and renderbuffer are created,
        // used and deleted in this block, and `pixels` is exactly width * height * 4 bytes, the
        // size glReadPixels writes for RGBA / UNSIGNED_BYTE with the default pack alignment.
        unsafe {
            let fbo = gl.create_framebuffer().map_err(RenderError::Gl)?;
            let rbo = gl.create_renderbuffer().map_err(RenderError::Gl)?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rbo));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, width as i32, height as i32);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(rbo),
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            let result = if status == glow::FRAMEBUFFER_COMPLETE {
                self.draw(params, width, height);
                gl.read_pixels(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                Ok(())
            } else {
                Err(RenderError::Gl(format!(
                    "framebuffer incomplete: {status:#x}"
                )))
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(fbo);
            gl.delete_renderbuffer(rbo);
            result?;
        }
        // GL rows are bottom-up; return top row first.
        let row = width as usize * 4;
        let flipped = pixels.chunks(row).rev().flatten().copied().collect();
        Ok(flipped)
    }
}

impl Drop for GlesRenderer {
    fn drop(&mut self) {
        // SAFETY: objects were created from this context, which must still be current when the
        // renderer is dropped (owners drop the renderer before the EGL context).
        unsafe {
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_program(self.program);
        }
    }
}

/// The `GL_RENDERER` string of the offscreen software device (for tests: must be llvmpipe).
pub fn offscreen_renderer_name() -> Result<String, RenderError> {
    let egl = Egl::software_device()?;
    egl.make_current(None)?;
    let gl = egl.load_gl();
    // SAFETY: the context behind `gl` was just made current on this thread.
    Ok(unsafe { gl.get_parameter_string(glow::RENDERER) })
}

/// Renders the grid offscreen on the software device and returns RGBA pixels, top row first.
pub fn offscreen(params: &GridParams, width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let egl = Egl::software_device()?;
    egl.make_current(None)?;
    let renderer = GlesRenderer::new(egl.load_gl())?;
    let pixels = renderer.render_to_pixels(params, width, height);
    drop(renderer);
    pixels
}
