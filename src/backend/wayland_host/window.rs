//! The viewer window in the host: surface, xdg toplevel, scale, viewport and frame callbacks.

use wayland_client::protocol::{wl_callback, wl_compositor, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_egl::WlEglSurface;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
    self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::{
    self, ZxdgToplevelDecorationV1,
};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use super::{Globals, HostError, HostState};
use crate::backend::BackendEvent;
use crate::canvas::Size;

pub(crate) struct Window {
    surface: wl_surface::WlSurface,
    _xdg_surface: XdgSurface,
    _toplevel: XdgToplevel,
    _decoration: Option<ZxdgToplevelDecorationV1>,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    /// Logical size from the latest `xdg_toplevel.configure`, sent on `xdg_surface.configure`.
    pending_size: Size,
    egl: Option<WlEglSurface>,
    /// Buffer and logical size last applied to the EGL window and surface.
    applied: Option<((u32, u32), Size)>,
}

impl Window {
    pub fn new(
        compositor: &wl_compositor::WlCompositor,
        wm_base: &XdgWmBase,
        globals: &Globals,
        qh: &QueueHandle<HostState>,
    ) -> Window {
        let surface = compositor.create_surface(qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, qh, ());
        let toplevel = xdg_surface.get_toplevel(qh, ());
        toplevel.set_title("RubickWay".to_string());
        toplevel.set_app_id("rubickway".to_string());

        let decoration = globals.decoration.as_ref().map(|manager| {
            let decoration = manager.get_toplevel_decoration(&toplevel, qh, ());
            decoration.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
            decoration
        });
        let viewport = globals
            .viewporter
            .as_ref()
            .map(|viewporter| viewporter.get_viewport(&surface, qh, ()));
        let fractional = globals
            .fractional_scale
            .as_ref()
            .map(|manager| manager.get_fractional_scale(&surface, qh, ()));

        // The first commit, without a buffer, asks the host for the initial configure.
        surface.commit();

        Window {
            surface,
            _xdg_surface: xdg_surface,
            _toplevel: toplevel,
            _decoration: decoration,
            viewport,
            fractional,
            pending_size: Size::default(),
            egl: None,
            applied: None,
        }
    }

    pub fn egl_window(
        &mut self,
        buffer: (u32, u32),
        logical: Size,
    ) -> Result<&WlEglSurface, HostError> {
        // The viewer keeps buffer sides within `i32`; saturate rather than wrap regardless.
        let side = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        let (w, h) = (side(buffer.0), side(buffer.1));
        if self.egl.is_none() {
            let egl = WlEglSurface::new(self.surface.id(), w, h)
                .map_err(|e| HostError::Window(e.to_string()))?;
            self.egl = Some(egl);
        }
        if self.applied != Some((buffer, logical)) {
            if let Some(egl) = &self.egl {
                egl.resize(w, h, 0, 0);
            }
            match &self.viewport {
                // The buffer is drawn at `logical` size whatever its pixel size. Float-to-int
                // `as` saturates, so huge logical sizes cannot wrap.
                Some(viewport) => viewport.set_destination(
                    (logical.w.round() as i32).max(1),
                    (logical.h.round() as i32).max(1),
                ),
                // Integer scale only (no viewporter): buffer = logical * n.
                None => {
                    let n = (f64::from(buffer.0) / logical.w.max(1.0)).round().max(1.0);
                    self.surface.set_buffer_scale(n as i32);
                }
            }
            self.applied = Some((buffer, logical));
        }
        self.egl
            .as_ref()
            .ok_or_else(|| HostError::Window("no EGL window".to_string()))
    }

    pub fn request_frame(&self, qh: &QueueHandle<HostState>) {
        self.surface.frame(qh, ());
    }

    pub fn drop_egl_window(&mut self) {
        self.egl = None;
        self.applied = None;
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &wl_surface::WlSurface,
        event: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Integer scale is only used when the host has no fractional scale.
        if let wl_surface::Event::PreferredBufferScale { factor } = event
            && state.window.fractional.is_none()
            && factor > 0
        {
            state.push(BackendEvent::Scale(f64::from(factor)));
        }
    }
}

impl Dispatch<XdgSurface, ()> for HostState {
    fn event(
        state: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);
            let size = state.window.pending_size;
            state.push(BackendEvent::Configure { size });
        }
    }
}

impl Dispatch<XdgToplevel, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                state.window.pending_size = Size::new(f64::from(width), f64::from(height));
            }
            xdg_toplevel::Event::Close => state.push(BackendEvent::CloseRequested),
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            // The scale is sent in 120ths.
            state.push(BackendEvent::Scale(f64::from(scale) / 120.0));
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for HostState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.push(BackendEvent::FrameDone);
        }
    }
}

impl Dispatch<ZxdgToplevelDecorationV1, ()> for HostState {
    fn event(
        _: &mut Self,
        _: &ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure {
            mode: WEnum::Value(mode),
        } = event
        {
            tracing::debug!("host decoration mode: {mode:?}");
        }
    }
}

delegate_noop!(HostState: ignore WpViewport);
