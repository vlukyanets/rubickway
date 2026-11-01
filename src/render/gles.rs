//! The OpenGL ES 3 renderer: draws the grid and window surfaces for a viewer, on screen or
//! offscreen.
#![allow(unsafe_code)]

use std::collections::HashMap;

use glow::HasContext;

use super::egl::{Egl, EglError};
use super::grid::{self, GridParams};

const GRID_VERTEX_SHADER: &str = include_str!("shaders/grid.vert");
const GRID_FRAGMENT_SHADER: &str = include_str!("shaders/grid.frag");
const WINDOW_VERTEX_SHADER: &str = include_str!("shaders/window.vert");
const WINDOW_FRAGMENT_SHADER: &str = include_str!("shaders/window.frag");

/// Errors from rendering.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Egl(#[from] EglError),
    #[error("GL: {0}")]
    Gl(String),
}

/// A buffer transform (`wl_output.transform`), applied when sampling a window texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    /// Whether the transform swaps width and height.
    pub fn swaps_axes(self) -> bool {
        matches!(
            self,
            Transform::Rotate90
                | Transform::Rotate270
                | Transform::Flipped90
                | Transform::Flipped270
        )
    }

    fn shader_value(self) -> i32 {
        self as i32
    }
}

/// Pixels to upload: little-endian ARGB8888 (or XRGB8888 when `opaque`), premultiplied.
#[derive(Debug, Clone, Copy)]
pub struct ImageRef<'a> {
    pub bytes: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per row, at least `width * 4`.
    pub stride: u32,
    pub opaque: bool,
}

/// One surface to draw: the texture uploaded under `key`, drawn at `dst` (x, y, width, height
/// in physical pixels, origin top-left), showing `src` (normalised crop of the surface).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub key: u64,
    pub dst: [f32; 4],
    pub src: [f32; 4],
    pub transform: Transform,
}

struct GridUniforms {
    height: Option<glow::UniformLocation>,
    minor: Option<glow::UniformLocation>,
    offset: Option<glow::UniformLocation>,
    fade: Option<glow::UniformLocation>,
    axis: Option<glow::UniformLocation>,
    axis_on: Option<glow::UniformLocation>,
    background: Option<glow::UniformLocation>,
    alpha: Option<glow::UniformLocation>,
}

struct WindowUniforms {
    viewport: Option<glow::UniformLocation>,
    dst: Option<glow::UniformLocation>,
    src: Option<glow::UniformLocation>,
    transform: Option<glow::UniformLocation>,
    opaque: Option<glow::UniformLocation>,
}

struct Texture {
    texture: glow::Texture,
    opaque: bool,
}

/// Draws the grid with one full-screen triangle and windows as textured quads. Needs its GL
/// context to be current.
pub struct GlesRenderer {
    gl: glow::Context,
    grid_program: glow::Program,
    window_program: glow::Program,
    vao: glow::VertexArray,
    grid: GridUniforms,
    window: WindowUniforms,
    textures: HashMap<u64, Texture>,
    max_buffer_side: u32,
    max_texture_side: u32,
}

/// Compiles and links a program.
///
/// # Safety
/// The context behind `gl` must be current on this thread.
unsafe fn link(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
) -> Result<glow::Program, RenderError> {
    // SAFETY: the caller guarantees a current context; all objects are created and deleted
    // here from that context.
    unsafe {
        let program = gl.create_program().map_err(RenderError::Gl)?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(kind).map_err(RenderError::Gl)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
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
        Ok(program)
    }
}

impl GlesRenderer {
    /// Compiles the programs. The GL context behind `gl` must be current.
    pub fn new(gl: glow::Context) -> Result<GlesRenderer, RenderError> {
        // SAFETY: the caller made the context behind `gl` current on this thread; every object
        // used below is created in this block from that same context.
        unsafe {
            let grid_program = link(&gl, GRID_VERTEX_SHADER, GRID_FRAGMENT_SHADER)?;
            let window_program = match link(&gl, WINDOW_VERTEX_SHADER, WINDOW_FRAGMENT_SHADER) {
                Ok(p) => p,
                Err(err) => {
                    gl.delete_program(grid_program);
                    return Err(err);
                }
            };
            let at = |program, name: &str| gl.get_uniform_location(program, name);
            let grid = GridUniforms {
                height: at(grid_program, "u_height"),
                minor: at(grid_program, "u_minor"),
                offset: at(grid_program, "u_offset"),
                fade: at(grid_program, "u_fade"),
                axis: at(grid_program, "u_axis"),
                axis_on: at(grid_program, "u_axis_on"),
                background: at(grid_program, "u_background"),
                alpha: at(grid_program, "u_alpha"),
            };
            let window = WindowUniforms {
                viewport: at(window_program, "u_viewport"),
                dst: at(window_program, "u_dst"),
                src: at(window_program, "u_src"),
                transform: at(window_program, "u_transform"),
                opaque: at(window_program, "u_opaque"),
            };
            let vao = gl.create_vertex_array().map_err(RenderError::Gl)?;
            // The largest framebuffer this GPU can draw: renderbuffer and viewport limits.
            let mut viewport = [0i32; 2];
            gl.get_parameter_i32_slice(glow::MAX_VIEWPORT_DIMS, &mut viewport);
            let renderbuffer = gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE);
            let max = renderbuffer.min(viewport[0]).min(viewport[1]);
            let max_buffer_side = u32::try_from(max).unwrap_or(0).max(1);
            let max_texture = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE);
            let max_texture_side = u32::try_from(max_texture).unwrap_or(0).max(1);
            Ok(GlesRenderer {
                gl,
                grid_program,
                window_program,
                vao,
                grid,
                window,
                textures: HashMap::new(),
                max_buffer_side,
                max_texture_side,
            })
        }
    }

    /// The largest buffer side (in pixels) this GPU can render to.
    pub fn max_buffer_side(&self) -> u32 {
        self.max_buffer_side
    }

    /// The largest texture side (in pixels) this GPU can sample: the window size limit.
    pub fn max_texture_side(&self) -> u32 {
        self.max_texture_side
    }

    /// Uploads `image` as the texture for `key`, replacing an earlier one (research R5).
    pub fn upload(&mut self, key: u64, image: ImageRef<'_>) -> Result<(), RenderError> {
        let (Ok(width), Ok(height)) = (i32::try_from(image.width), i32::try_from(image.height))
        else {
            return Err(RenderError::Gl("image too large".to_string()));
        };
        let row = image.width as usize * 4;
        let stride = image.stride as usize;
        let needed = stride
            .checked_mul(image.height as usize)
            .ok_or_else(|| RenderError::Gl("image too large".to_string()))?;
        if image.width == 0 || image.height == 0 || stride < row || image.bytes.len() < needed {
            return Err(RenderError::Gl("image does not match its size".to_string()));
        }
        // Rows must be tightly packed unless the stride is a whole number of pixels.
        let packed;
        let (pixels, row_length) = if stride.is_multiple_of(4) {
            (&image.bytes[..needed], (stride / 4) as i32)
        } else {
            packed = image
                .bytes
                .chunks(stride)
                .take(image.height as usize)
                .flat_map(|r| &r[..row])
                .copied()
                .collect::<Vec<u8>>();
            (&packed[..], 0)
        };
        let gl = &self.gl;
        // SAFETY: the context is current (constructor contract). `pixels` holds `height` rows
        // of `row_length` (or `width`) pixels, checked above, which is what glTexImage2D reads
        // with this unpack row length and the default alignment of 4.
        unsafe {
            let texture = match self.textures.get(&key) {
                Some(t) => t.texture,
                None => gl.create_texture().map_err(RenderError::Gl)?,
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, row_length);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            gl.generate_mipmap(glow::TEXTURE_2D);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            for wrap in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, wrap, glow::CLAMP_TO_EDGE as i32);
            }
            self.textures.insert(
                key,
                Texture {
                    texture,
                    opaque: image.opaque,
                },
            );
        }
        Ok(())
    }

    /// Frees the texture for `key`.
    pub fn remove(&mut self, key: u64) {
        if let Some(t) = self.textures.remove(&key) {
            // SAFETY: the texture was created from this renderer's current context.
            unsafe { self.gl.delete_texture(t.texture) };
        }
    }

    /// Draws the grid into the currently bound framebuffer of `width` x `height` pixels.
    pub fn draw(&self, params: &GridParams, width: u32, height: u32) {
        let gl = &self.gl;
        let u = &self.grid;
        let [axis_x, axis_y] = params.axis_px;
        // SAFETY: the context is current (constructor contract); program, VAO and uniform
        // locations were created from it in `new`.
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.disable(glow::BLEND);
            gl.use_program(Some(self.grid_program));
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

    /// Draws window surfaces over what is in the framebuffer, in order (later on top).
    /// Quads without an uploaded texture are skipped.
    pub fn draw_windows(&self, quads: &[Quad], width: u32, height: u32) {
        let gl = &self.gl;
        let u = &self.window;
        // SAFETY: the context is current (constructor contract); program, VAO, uniform
        // locations and textures were created from it.
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.window_program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_2_f32(u.viewport.as_ref(), width as f32, height as f32);
            for q in quads {
                let Some(t) = self.textures.get(&q.key) else {
                    continue;
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(t.texture));
                gl.uniform_4_f32(u.dst.as_ref(), q.dst[0], q.dst[1], q.dst[2], q.dst[3]);
                gl.uniform_4_f32(u.src.as_ref(), q.src[0], q.src[1], q.src[2], q.src[3]);
                gl.uniform_1_i32(u.transform.as_ref(), q.transform.shader_value());
                gl.uniform_1_f32(u.opaque.as_ref(), f32::from(u8::from(t.opaque)));
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            }
            gl.disable(glow::BLEND);
        }
    }

    /// Renders into a new framebuffer object and reads it back as RGBA, top row first.
    fn render_to_pixels(
        &self,
        params: &GridParams,
        quads: &[Quad],
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
                self.draw_windows(quads, width, height);
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
            for t in self.textures.values() {
                self.gl.delete_texture(t.texture);
            }
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_program(self.grid_program);
            self.gl.delete_program(self.window_program);
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
    offscreen_scene(params, &[], &[], width, height)
}

/// Renders the grid and windows offscreen on the software device: each image is uploaded
/// under its key, then the quads are drawn in order. Returns RGBA pixels, top row first.
pub fn offscreen_scene(
    params: &GridParams,
    images: &[(u64, ImageRef<'_>)],
    quads: &[Quad],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let egl = Egl::software_device()?;
    egl.make_current(None)?;
    let mut renderer = GlesRenderer::new(egl.load_gl())?;
    for (key, image) in images {
        renderer.upload(*key, *image)?;
    }
    let pixels = renderer.render_to_pixels(params, quads, width, height);
    drop(renderer);
    pixels
}
