//! The OpenGL ES 3 renderer: draws the grid and window surfaces for a viewer, on screen or
//! offscreen.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::c_void;

use glow::HasContext;
use khronos_egl as egl;

use super::egl::{Egl, EglError, EglImage};
use super::grid::{self, GridParams};
use crate::distant::copy_size;

const GRID_VERTEX_SHADER: &str = include_str!("shaders/grid.vert");
const GRID_FRAGMENT_SHADER: &str = include_str!("shaders/grid.frag");
const WINDOW_VERTEX_SHADER: &str = include_str!("shaders/window.vert");
const WINDOW_FRAGMENT_SHADER: &str = include_str!("shaders/window.frag");
const WINDOW_EXTERNAL_FRAGMENT_SHADER: &str = include_str!("shaders/window_external.frag");
/// `GL_TEXTURE_EXTERNAL_OES`.
const TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

/// Linear RGB behind a fullscreen window: black (spec FR-014).
pub const FULLSCREEN_BACKGROUND: [f32; 3] = [0.0, 0.0, 0.0];

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
    y_invert: Option<glow::UniformLocation>,
    opaque: Option<glow::UniformLocation>,
}

/// Draws GPU images, sampled as external textures (research G4).
struct ExternalProgram {
    program: glow::Program,
    window: WindowUniforms,
    swizzle: Option<glow::UniformLocation>,
    bind_image: ImageTargetTexture,
}

/// `glEGLImageTargetTexture2DOES`.
type ImageTargetTexture = unsafe extern "system" fn(target: u32, image: *mut c_void);

/// A GPU image bound to an external texture.
struct ImportedImage {
    texture: glow::Texture,
    width: u32,
    height: u32,
    /// Its `wl_buffer` is gone: it goes as soon as no surface shows it.
    unwanted: bool,
    _image: EglImage,
}

/// A copy of a GPU image with mipmaps, for drawing it below its pixel size (research G4).
struct MipCopy {
    texture: glow::Texture,
    width: u32,
    height: u32,
}

/// The texture of a copy no surface uses now. Creating one of a large window's size takes the event
/// loop hundreds of milliseconds (research G4), so the next image of that size uses it again.
struct SpareCopy {
    copy: MipCopy,
    /// Calls of `prepare_frame` it has stayed unused through.
    idle_frames: u32,
}

/// How many calls of `prepare_frame` a spare texture survives unused. More than one, as each display
/// calls it for each of its frames and the window may be on any of them; few, as it holds memory.
const SPARE_COPY_FRAMES: u32 = 8;

/// What the whole image would measure as `q` draws it, along the image's own axes.
fn drawn_size(q: &Quad) -> (f32, f32) {
    let (w, h) = (q.dst[2] / q.src[2], q.dst[3] / q.src[3]);
    if q.transform.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    }
}

/// Whether `q` draws an image of `size` below its pixel size.
fn drawn_below(q: &Quad, size: (u32, u32)) -> bool {
    let (w, h) = drawn_size(q);
    w < size.0 as f32 || h < size.1 as f32
}

/// What a surface (or cursor image) is drawn from.
enum Source {
    /// A texture its pixels were copied into.
    Copied {
        texture: glow::Texture,
        opaque: bool,
    },
    /// A GPU image, with a copy made once it was drawn zoomed out.
    Image {
        image: u64,
        y_invert: bool,
        mip: Option<MipCopy>,
    },
}

/// Draws the grid with one full-screen triangle and windows as textured quads. Needs its GL
/// context to be current.
pub struct GlesRenderer {
    gl: glow::Context,
    grid_program: glow::Program,
    window_program: glow::Program,
    /// `None`: this GPU cannot sample external textures, so no GPU images are offered (FR-014).
    external: Option<ExternalProgram>,
    vao: glow::VertexArray,
    grid: GridUniforms,
    window: WindowUniforms,
    sources: HashMap<u64, Source>,
    images: HashMap<u64, ImportedImage>,
    spare_copies: Vec<SpareCopy>,
    /// How many textures for mipmap copies were created.
    copy_textures_made: u64,
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
            let window_uniforms = |program| WindowUniforms {
                viewport: at(program, "u_viewport"),
                dst: at(program, "u_dst"),
                src: at(program, "u_src"),
                transform: at(program, "u_transform"),
                y_invert: at(program, "u_y_invert"),
                opaque: at(program, "u_opaque"),
            };
            let window = window_uniforms(window_program);
            // A GPU without these only misses GPU images (FR-014): no error here.
            let external = egl::API
                .get_proc_address("glEGLImageTargetTexture2DOES")
                .and_then(|bind| {
                    link(&gl, WINDOW_VERTEX_SHADER, WINDOW_EXTERNAL_FRAGMENT_SHADER)
                        .inspect_err(|err| tracing::debug!("no external textures: {err}"))
                        .ok()
                        .map(|program| ExternalProgram {
                            program,
                            window: window_uniforms(program),
                            swizzle: at(program, "u_swizzle"),
                            // SAFETY: the name comes from GL_OES_EGL_image, whose specification
                            // defines exactly this signature.
                            bind_image: std::mem::transmute::<
                                extern "system" fn(),
                                ImageTargetTexture,
                            >(bind),
                        })
                });
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
                external,
                vao,
                grid,
                window,
                sources: HashMap::new(),
                images: HashMap::new(),
                spare_copies: Vec::new(),
                copy_textures_made: 0,
                max_buffer_side,
                max_texture_side,
            })
        }
    }

    /// The largest buffer side (in pixels) this GPU can render to.
    pub fn max_buffer_side(&self) -> u32 {
        self.max_buffer_side
    }

    /// The GPU's name, as the log calls it.
    pub fn name(&self) -> String {
        // SAFETY: the context is current (constructor contract); a plain query.
        unsafe { self.gl.get_parameter_string(glow::RENDERER) }
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
            let texture = match self.sources.get(&key) {
                Some(Source::Copied { texture, .. }) => *texture,
                Some(Source::Image { .. }) | None => {
                    gl.create_texture().map_err(RenderError::Gl)?
                }
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
            let old = self.sources.insert(
                key,
                Source::Copied {
                    texture,
                    opaque: image.opaque,
                },
            );
            if let Some(old @ Source::Image { .. }) = old {
                self.drop_source(old);
            }
        }
        Ok(())
    }

    /// Frees what `key` is drawn from.
    pub fn remove(&mut self, key: u64) {
        if let Some(source) = self.sources.remove(&key) {
            self.drop_source(source);
        }
    }

    fn drop_source(&mut self, source: Source) {
        match source {
            Source::Copied { texture, .. } => {
                // SAFETY: the texture was created from this renderer's current context.
                unsafe { self.gl.delete_texture(texture) };
            }
            Source::Image { image, mip, .. } => {
                if let Some(copy) = mip {
                    self.spare_copies.push(SpareCopy {
                        copy,
                        idle_frames: 0,
                    });
                }
                self.drop_image_if_unwanted(image);
            }
        }
    }

    /// Whether GPU images can be drawn at all (FR-014).
    pub fn can_sample_images(&self) -> bool {
        self.external.is_some()
    }

    /// Binds an imported image to a texture, under `key`. `width` and `height` are its pixel size.
    pub fn add_image(
        &mut self,
        key: u64,
        image: EglImage,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        let Some(external) = &self.external else {
            return Err(RenderError::Gl("no external textures".to_string()));
        };
        let gl = &self.gl;
        // SAFETY: the context is current (constructor contract); `bind_image` gets a live EGLImage
        // of the same display, kept alive with the texture in `ImportedImage`.
        unsafe {
            let texture = gl.create_texture().map_err(RenderError::Gl)?;
            gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(texture));
            (external.bind_image)(TEXTURE_EXTERNAL_OES, image.as_ptr());
            for (name, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(TEXTURE_EXTERNAL_OES, name, value as i32);
            }
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                gl.delete_texture(texture);
                return Err(RenderError::Gl(format!(
                    "binding the image failed: {error:#x}"
                )));
            }
            self.images.insert(
                key,
                ImportedImage {
                    texture,
                    width,
                    height,
                    unwanted: false,
                    _image: image,
                },
            );
        }
        Ok(())
    }

    /// Makes image `key` from RGBA pixels, row 0 first, through an ordinary texture: how rendering
    /// tests get external textures with no dma-buf and no device (research G11). Without
    /// `has_alpha` it is a format with no alpha channel, and the given alpha values are dropped.
    pub fn add_image_from_pixels(
        &mut self,
        egl: &Egl,
        key: u64,
        rgba: &[u8],
        size: (u32, u32),
        has_alpha: bool,
    ) -> Result<(), RenderError> {
        let (width, height) = size;
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(RenderError::Gl("image does not match its size".to_string()));
        }
        let rgb: Vec<u8>;
        let (internal, format, pixels) = if has_alpha {
            (glow::RGBA8, glow::RGBA, rgba)
        } else {
            let (pixels, _) = rgba.as_chunks::<4>();
            rgb = pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
            (glow::RGB8, glow::RGB, &rgb[..])
        };
        let gl = &self.gl;
        // SAFETY: the context is current (constructor contract); `pixels` holds exactly the
        // width x height pixels glTexImage2D reads, checked above, with rows not padded given
        // an alignment of 1; the EGLImage keeps the pixels after the texture is deleted.
        let image = unsafe {
            let texture = gl.create_texture().map_err(RenderError::Gl)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal as i32,
                width as i32,
                height as i32,
                0,
                format,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            let image = egl.image_from_texture(texture.0.get());
            gl.delete_texture(texture);
            image?
        };
        self.add_image(key, image, width, height)
    }

    /// The `wl_buffer` of image `key` is gone. A surface showing it keeps it until replaced.
    pub fn remove_image(&mut self, key: u64) {
        if let Some(image) = self.images.get_mut(&key) {
            image.unwanted = true;
        }
        self.drop_image_if_unwanted(key);
    }

    fn drop_image_if_unwanted(&mut self, key: u64) {
        let shown = self
            .sources
            .values()
            .any(|s| matches!(s, Source::Image { image, .. } if *image == key));
        if shown || !self.images.get(&key).is_some_and(|i| i.unwanted) {
            return;
        }
        if let Some(image) = self.images.remove(&key) {
            // SAFETY: the texture was created from this renderer's current context.
            unsafe { self.gl.delete_texture(image.texture) };
        }
    }

    /// Surface `key` shows image `image` from now on.
    pub fn show_image(&mut self, key: u64, image: u64, y_invert: bool) {
        let old = self.sources.insert(
            key,
            Source::Image {
                image,
                y_invert,
                mip: None,
            },
        );
        if let Some(old) = old {
            self.drop_source(old);
        }
    }

    /// The size of surface `key`'s smoothing copy.
    pub fn mip_copy_size(&self, key: u64) -> Option<(u32, u32)> {
        match self.sources.get(&key) {
            Some(Source::Image { mip: Some(mip), .. }) => Some((mip.width, mip.height)),
            _ => None,
        }
    }

    /// How many mipmap copies exist now.
    pub fn mip_copies(&self) -> usize {
        self.sources
            .values()
            .filter(|s| matches!(s, Source::Image { mip: Some(_), .. }))
            .count()
    }

    /// How many textures for mipmap copies were created since the renderer started.
    pub fn copy_textures_made(&self) -> u64 {
        self.copy_textures_made
    }

    /// How many textures wait to be used again for a mipmap copy.
    pub fn spare_copy_textures(&self) -> usize {
        self.spare_copies.len()
    }

    /// Makes what `quads` need before drawing starts: a mipmap copy of each image drawn below its
    /// pixel size, once per image (research G4). `detail` is the copy's size as a fraction of the
    /// drawn size, for distant windows (FR-017); `None` is the image's size. A copy only ever grows
    /// for one image, as two displays at different zooms would otherwise make one every frame. A copy
    /// uses a spare texture of its size if there is one, and spares nobody took are freed after
    /// [`SPARE_COPY_FRAMES`] calls. Leaves the default framebuffer bound.
    pub fn prepare_frame(
        &mut self,
        quads: &[Quad],
        detail: Option<f32>,
    ) -> Result<(), RenderError> {
        for q in quads {
            let Some(Source::Image { image, mip, .. }) = self.sources.get(&q.key) else {
                continue;
            };
            let Some(imported) = self.images.get(image) else {
                continue;
            };
            let size = (imported.width, imported.height);
            if !drawn_below(q, size) {
                continue;
            }
            let (width, height) = copy_size(size, drawn_size(q), detail);
            if mip
                .as_ref()
                .is_some_and(|m| m.width >= width && m.height >= height)
            {
                continue;
            }
            let spare = self
                .spare_copies
                .iter()
                .position(|s| s.copy.width == width && s.copy.height == height)
                .map(|at| self.spare_copies.swap_remove(at).copy.texture);
            if spare.is_none() {
                self.copy_textures_made += 1;
            }
            let texture = self.make_mip_copy(imported, spare, width, height)?;
            if let Some(Source::Image { mip, .. }) = self.sources.get_mut(&q.key) {
                let old = mip.replace(MipCopy {
                    texture,
                    width,
                    height,
                });
                if let Some(copy) = old {
                    self.spare_copies.push(SpareCopy {
                        copy,
                        idle_frames: 0,
                    });
                }
            }
        }
        let gl = &self.gl;
        self.spare_copies.retain_mut(|spare| {
            spare.idle_frames += 1;
            let keep = spare.idle_frames <= SPARE_COPY_FRAMES;
            if !keep {
                // SAFETY: the texture was created from this renderer's current context, and GL
                // keeps it for commands already sent.
                unsafe { gl.delete_texture(spare.copy.texture) };
            }
            keep
        });
        Ok(())
    }

    /// Whether drawing `q` from `mip` of image `image` loses nothing: a copy smaller than the image
    /// serves only below the image's pixel size.
    fn copy_serves(&self, q: &Quad, image: u64, mip: &MipCopy) -> bool {
        let Some(imported) = self.images.get(&image) else {
            return true;
        };
        let size = (imported.width, imported.height);
        (mip.width >= size.0 && mip.height >= size.1) || drawn_below(q, size)
    }

    /// Draws `image` into `spare`, or a new texture, of the given size, generates its mipmaps and
    /// returns it. Channels are stored as `window.frag` expects, in the image's own row order.
    fn make_mip_copy(
        &self,
        image: &ImportedImage,
        spare: Option<glow::Texture>,
        width: u32,
        height: u32,
    ) -> Result<glow::Texture, RenderError> {
        let Some(external) = &self.external else {
            return Err(RenderError::Gl("no external textures".to_string()));
        };
        let gl = &self.gl;
        let (w, h) = (width as i32, height as i32);
        let started = std::time::Instant::now();
        // SAFETY: the context is current (constructor contract); the texture and framebuffer come
        // from it, no pixels are read from client memory, and the default framebuffer is bound again
        // before returning. Commands already sent that read a spare texture run before these write
        // to it, as GL keeps their order.
        unsafe {
            let texture = match spare {
                Some(texture) => {
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    texture
                }
                None => {
                    let texture = gl.create_texture().map_err(RenderError::Gl)?;
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    // All levels at once: generating mipmaps into a texture without them makes the
                    // driver allocate it again, every time.
                    let levels = (u32::BITS - width.max(height).leading_zeros()) as i32;
                    gl.tex_storage_2d(glow::TEXTURE_2D, levels, glow::RGBA8, w, h);
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
                    texture
                }
            };
            let allocated = started.elapsed();
            let fbo = gl.create_framebuffer().map_err(RenderError::Gl)?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.viewport(0, 0, w, h);
            gl.disable(glow::BLEND);
            gl.use_program(Some(external.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(image.texture));
            let u = &external.window;
            gl.uniform_2_f32(u.viewport.as_ref(), w as f32, h as f32);
            gl.uniform_4_f32(u.dst.as_ref(), 0.0, 0.0, w as f32, h as f32);
            // The vertex shader's y points down and a framebuffer's up: sampling v from 1 to 0
            // keeps row 0 of the image at v = 0 of the copy.
            gl.uniform_4_f32(u.src.as_ref(), 0.0, 1.0, 1.0, -1.0);
            gl.uniform_1_i32(u.transform.as_ref(), Transform::Normal.shader_value());
            gl.uniform_1_f32(u.y_invert.as_ref(), 0.0);
            gl.uniform_1_f32(external.swizzle.as_ref(), 1.0);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(fbo);
            let copied = started.elapsed();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.generate_mipmap(glow::TEXTURE_2D);
            if started.elapsed().as_millis() >= 5 {
                tracing::debug!(
                    "copy {width}x{height}: allocating {} ms, drawing {} ms, mipmaps {} ms",
                    allocated.as_millis(),
                    copied.saturating_sub(allocated).as_millis(),
                    started.elapsed().saturating_sub(copied).as_millis()
                );
            }
            Ok(texture)
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

    /// Fills the currently bound framebuffer of `width` x `height` pixels with `rgb` (the
    /// background of a fullscreen window, research W8).
    pub fn clear(&self, rgb: [f32; 3], width: u32, height: u32) {
        let gl = &self.gl;
        let [r, g, b] = rgb;
        // SAFETY: the context is current (constructor contract); clearing touches only the
        // bound framebuffer.
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(r, g, b, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Draws window surfaces over what is in the framebuffer, in order (later on top).
    /// Quads with nothing to draw from yet are skipped.
    pub fn draw_windows(&self, quads: &[Quad], width: u32, height: u32) {
        let gl = &self.gl;
        let u = &self.window;
        // SAFETY: the context is current (constructor contract); programs, VAO, uniform
        // locations and textures were created from it.
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            let (w, h) = (width as f32, height as f32);
            for q in quads {
                let Some(source) = self.sources.get(&q.key) else {
                    continue;
                };
                // Copied pixels and mipmap copies go through the plain program, images directly
                // through the external one.
                let (uniforms, y_invert) = match source {
                    Source::Copied { texture, opaque } => {
                        gl.use_program(Some(self.window_program));
                        gl.bind_texture(glow::TEXTURE_2D, Some(*texture));
                        gl.uniform_1_f32(u.opaque.as_ref(), f32::from(u8::from(*opaque)));
                        (u, false)
                    }
                    Source::Image {
                        image,
                        y_invert,
                        mip: Some(mip),
                    } if self.copy_serves(q, *image, mip) => {
                        gl.use_program(Some(self.window_program));
                        gl.bind_texture(glow::TEXTURE_2D, Some(mip.texture));
                        gl.uniform_1_f32(u.opaque.as_ref(), 0.0);
                        (u, *y_invert)
                    }
                    Source::Image {
                        image, y_invert, ..
                    } => {
                        let (Some(external), Some(imported)) =
                            (&self.external, self.images.get(image))
                        else {
                            continue;
                        };
                        gl.use_program(Some(external.program));
                        gl.bind_texture(TEXTURE_EXTERNAL_OES, Some(imported.texture));
                        gl.uniform_1_f32(external.swizzle.as_ref(), 0.0);
                        (&external.window, *y_invert)
                    }
                };
                gl.uniform_2_f32(uniforms.viewport.as_ref(), w, h);
                gl.uniform_4_f32(
                    uniforms.dst.as_ref(),
                    q.dst[0],
                    q.dst[1],
                    q.dst[2],
                    q.dst[3],
                );
                gl.uniform_4_f32(
                    uniforms.src.as_ref(),
                    q.src[0],
                    q.src[1],
                    q.src[2],
                    q.src[3],
                );
                gl.uniform_1_i32(uniforms.transform.as_ref(), q.transform.shader_value());
                gl.uniform_1_f32(uniforms.y_invert.as_ref(), f32::from(u8::from(y_invert)));
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            }
            gl.disable(glow::BLEND);
        }
    }

    /// Renders into a new framebuffer object and reads it back as RGBA, top row first.
    /// `grid`: the grid to draw behind the windows; `None`: the fullscreen background.
    fn render_to_pixels(
        &self,
        grid: Option<&GridParams>,
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
                match grid {
                    Some(params) => self.draw(params, width, height),
                    None => self.clear(FULLSCREEN_BACKGROUND, width, height),
                }
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
            for source in self.sources.values() {
                match source {
                    Source::Copied { texture, .. } => self.gl.delete_texture(*texture),
                    Source::Image { mip: Some(mip), .. } => self.gl.delete_texture(mip.texture),
                    Source::Image { mip: None, .. } => {}
                }
            }
            for spare in &self.spare_copies {
                self.gl.delete_texture(spare.copy.texture);
            }
            for image in self.images.values() {
                self.gl.delete_texture(image.texture);
            }
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_program(self.grid_program);
            self.gl.delete_program(self.window_program);
            if let Some(external) = &self.external {
                self.gl.delete_program(external.program);
            }
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
    let pixels = renderer.render_to_pixels(Some(params), quads, width, height);
    drop(renderer);
    pixels
}

/// Like [`offscreen_scene`], with `setup` preparing the renderer: how rendering tests give it GPU
/// images (research G11). `check` looks at the renderer after drawing.
pub fn offscreen_with_images(
    setup: impl FnOnce(&Egl, &mut GlesRenderer) -> Result<(), RenderError>,
    params: &GridParams,
    quads: &[Quad],
    width: u32,
    height: u32,
    check: impl FnOnce(&mut GlesRenderer),
) -> Result<Vec<u8>, RenderError> {
    offscreen_with_detail(None, setup, params, quads, width, height, check)
}

/// Like [`offscreen_with_images`], drawing distant windows with `detail` (FR-017).
pub fn offscreen_with_detail(
    detail: Option<f32>,
    setup: impl FnOnce(&Egl, &mut GlesRenderer) -> Result<(), RenderError>,
    params: &GridParams,
    quads: &[Quad],
    width: u32,
    height: u32,
    check: impl FnOnce(&mut GlesRenderer),
) -> Result<Vec<u8>, RenderError> {
    let egl = Egl::software_device()?;
    egl.make_current(None)?;
    let mut renderer = GlesRenderer::new(egl.load_gl())?;
    setup(&egl, &mut renderer)?;
    renderer.prepare_frame(quads, detail)?;
    let pixels = renderer.render_to_pixels(Some(params), quads, width, height);
    check(&mut renderer);
    drop(renderer);
    pixels
}

/// Like [`offscreen_scene`], drawn the way a fullscreen window is: on the plain
/// [`FULLSCREEN_BACKGROUND`] instead of the grid (FR-014).
pub fn offscreen_fullscreen_scene(
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
    let pixels = renderer.render_to_pixels(None, quads, width, height);
    drop(renderer);
    pixels
}
