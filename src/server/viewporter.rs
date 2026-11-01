//! `wp_viewporter` (crop and scale a surface) and `wp_fractional_scale_manager_v1` (the
//! preferred fractional scale), needed for sharp windows at fractional scales (FR-017).

use wayland_protocols::wp::fractional_scale::v1::server::wp_fractional_scale_manager_v1::{
    self, WpFractionalScaleManagerV1,
};
use wayland_protocols::wp::fractional_scale::v1::server::wp_fractional_scale_v1::{
    self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::server::wp_viewport::{self, WpViewport};
use wayland_protocols::wp::viewporter::server::wp_viewporter::{self, WpViewporter};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::ServerState;
use crate::canvas::{Point, Rect, Size};

pub(crate) fn create_globals(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WpViewporter, ()>(1, ());
    dh.create_global::<ServerState, WpFractionalScaleManagerV1, ()>(1, ());
}

impl GlobalDispatch<WpViewporter, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpViewporter>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpViewporter, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WpViewporter,
        request: wp_viewporter::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let wp_viewporter::Request::GetViewport { id, surface } = request else {
            return;
        };
        let viewport = data_init.init(id, surface.id());
        let Some(s) = state.surfaces.get_mut(&surface.id()) else {
            return;
        };
        if s.viewport.as_ref().is_some_and(|v| v.is_alive()) {
            resource.post_error(
                wp_viewporter::Error::ViewportExists,
                "the surface already has a viewport",
            );
            return;
        }
        s.viewport = Some(viewport);
    }
}

impl Dispatch<WpViewport, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WpViewport,
        request: wp_viewport::Request,
        surface: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(s) = state.surfaces.get_mut(surface) else {
            if !matches!(request, wp_viewport::Request::Destroy) {
                resource.post_error(wp_viewport::Error::NoSurface, "the surface is gone");
            }
            return;
        };
        match request {
            wp_viewport::Request::SetSource {
                x,
                y,
                width,
                height,
            } => {
                if x == -1.0 && y == -1.0 && width == -1.0 && height == -1.0 {
                    s.pending.viewport_src = None;
                } else if x >= 0.0 && y >= 0.0 && width > 0.0 && height > 0.0 {
                    s.pending.viewport_src =
                        Some(Rect::new(Point::new(x, y), Size::new(width, height)));
                } else {
                    resource.post_error(wp_viewport::Error::BadValue, "invalid source rectangle");
                }
            }
            wp_viewport::Request::SetDestination { width, height } => {
                if width == -1 && height == -1 {
                    s.pending.viewport_dst = None;
                } else if width > 0 && height > 0 {
                    s.pending.viewport_dst = Some((width, height));
                } else {
                    resource.post_error(wp_viewport::Error::BadValue, "invalid destination size");
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &WpViewport, surface: &ObjectId) {
        if let Some(s) = state.surfaces.get_mut(surface) {
            s.pending.viewport_src = None;
            s.pending.viewport_dst = None;
            s.viewport = None;
        }
    }
}

impl GlobalDispatch<WpFractionalScaleManagerV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpFractionalScaleManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WpFractionalScaleManagerV1,
        request: wp_fractional_scale_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let wp_fractional_scale_manager_v1::Request::GetFractionalScale { id, surface } = request
        else {
            return;
        };
        let fractional = data_init.init(id, surface.id());
        let (_, preferred) = state.scales();
        let Some(s) = state.surfaces.get_mut(&surface.id()) else {
            return;
        };
        if s.fractional.as_ref().is_some_and(|f| f.is_alive()) {
            resource.post_error(
                wp_fractional_scale_manager_v1::Error::FractionalScaleExists,
                "the surface already has a fractional scale object",
            );
            return;
        }
        fractional.preferred_scale(preferred);
        s.fractional = Some(fractional);
    }
}

impl Dispatch<WpFractionalScaleV1, ObjectId> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WpFractionalScaleV1,
        _: wp_fractional_scale_v1::Request,
        _: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &WpFractionalScaleV1, surface: &ObjectId) {
        if let Some(s) = state.surfaces.get_mut(surface) {
            s.fractional = None;
        }
    }
}
