//! One output's GBM surface, EGL surface and scanout framebuffers (research S6).

use std::io;

use drm::control::{Device as ControlDevice, framebuffer};
use gbm::{BufferObject, BufferObjectFlags, Format};
use khronos_egl as egl;

use super::device::Card;
use crate::render::egl::{Egl, EglError};

/// The buffers one output renders into and scans out.
pub(crate) struct OutputSurface {
    // Field order matters for drop: buffers go back to the surface before it is destroyed.
    /// The buffer on screen.
    current: Option<BufferObject<framebuffer::Handle>>,
    /// The buffer committed and waiting for its page flip.
    pending: Option<BufferObject<framebuffer::Handle>>,
    egl_surface: egl::Surface,
    gbm_surface: gbm::Surface<framebuffer::Handle>,
    /// Framebuffers created for this surface's buffers, destroyed with it.
    framebuffers: Vec<framebuffer::Handle>,
}

impl OutputSurface {
    pub fn new(
        gbm: &gbm::Device<Card>,
        egl: &Egl,
        width: u32,
        height: u32,
    ) -> Result<OutputSurface, String> {
        let gbm_surface = gbm
            .create_surface::<framebuffer::Handle>(
                width,
                height,
                Format::Xrgb8888,
                BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
            )
            .map_err(|e| format!("cannot create a scanout surface: {e}"))?;
        let egl_surface = egl
            .create_gbm_surface(&gbm_surface)
            .map_err(|e: EglError| e.to_string())?;
        Ok(OutputSurface {
            current: None,
            pending: None,
            egl_surface,
            gbm_surface,
            framebuffers: Vec::new(),
        })
    }

    /// The EGL surface to make current before drawing.
    pub fn egl_surface(&self) -> egl::Surface {
        self.egl_surface
    }

    /// After `eglSwapBuffers`: the new front buffer and its framebuffer (created on first use).
    pub fn lock_front(&mut self, gbm: &gbm::Device<Card>) -> io::Result<framebuffer::Handle> {
        // SAFETY: called once after each eglSwapBuffers on this surface, as GBM requires; the
        // buffer is released by dropping it, after its page flip (`flipped`).
        let mut bo = unsafe { self.gbm_surface.lock_front_buffer() }
            .map_err(|e| io::Error::other(e.to_string()))?;
        let fb = match bo.userdata() {
            Some(fb) => *fb,
            None => {
                let fb = gbm.add_framebuffer(&bo, 24, 32)?;
                bo.set_userdata(fb);
                self.framebuffers.push(fb);
                fb
            }
        };
        self.pending = Some(bo);
        Ok(fb)
    }

    /// The committed buffer is on screen: the one before goes back to the surface.
    pub fn flipped(&mut self) {
        if let Some(bo) = self.pending.take() {
            self.current = Some(bo);
        }
    }

    /// A commit was refused: the locked buffer goes back without being shown.
    pub fn discard_pending(&mut self) {
        self.pending = None;
    }

    /// Releases everything; EGL surface before the GBM surface, framebuffers last.
    pub fn destroy(mut self, gbm: &gbm::Device<Card>, egl: &Egl) {
        self.pending = None;
        self.current = None;
        egl.destroy_surface(self.egl_surface);
        for fb in self.framebuffers.drain(..) {
            let _ = gbm.destroy_framebuffer(fb);
        }
    }
}
