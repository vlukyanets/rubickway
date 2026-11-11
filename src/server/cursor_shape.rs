//! `wp_cursor_shape_manager_v1` version 2: applications name their pointer shape
//! (cursor-shapes research C1-C4, contracts/protocols.md). A shape is a cursor request like
//! `wl_pointer.set_cursor`, counted by [`seat::request_cursor`].

use wayland_protocols::wp::cursor_shape::v1::server::wp_cursor_shape_device_v1::{
    self, Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::server::wp_cursor_shape_manager_v1::{
    self, WpCursorShapeManagerV1,
};
use wayland_server::protocol::wl_pointer::WlPointer;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use super::ServerState;
use super::dmabuf::client_number;
use super::seat;
use crate::shell::{AppCursor, CursorName};

/// The version implemented and tested (Principle VI).
const VERSION: u32 = 2;

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WpCursorShapeManagerV1, ()>(VERSION, ());
}

/// The shell's name for a protocol shape.
pub(crate) fn shape_name(shape: Shape) -> CursorName {
    match shape {
        Shape::Default => CursorName::Default,
        Shape::ContextMenu => CursorName::ContextMenu,
        Shape::Help => CursorName::Help,
        Shape::Pointer => CursorName::Pointer,
        Shape::Progress => CursorName::Progress,
        Shape::Wait => CursorName::Wait,
        Shape::Cell => CursorName::Cell,
        Shape::Crosshair => CursorName::Crosshair,
        Shape::Text => CursorName::Text,
        Shape::VerticalText => CursorName::VerticalText,
        Shape::Alias => CursorName::Alias,
        Shape::Copy => CursorName::Copy,
        Shape::Move => CursorName::Move,
        Shape::NoDrop => CursorName::NoDrop,
        Shape::NotAllowed => CursorName::NotAllowed,
        Shape::Grab => CursorName::Grab,
        Shape::Grabbing => CursorName::Grabbing,
        Shape::EResize => CursorName::EResize,
        Shape::NResize => CursorName::NResize,
        Shape::NeResize => CursorName::NeResize,
        Shape::NwResize => CursorName::NwResize,
        Shape::SResize => CursorName::SResize,
        Shape::SeResize => CursorName::SeResize,
        Shape::SwResize => CursorName::SwResize,
        Shape::WResize => CursorName::WResize,
        Shape::EwResize => CursorName::EwResize,
        Shape::NsResize => CursorName::NsResize,
        Shape::NeswResize => CursorName::NeswResize,
        Shape::NwseResize => CursorName::NwseResize,
        Shape::ColResize => CursorName::ColResize,
        Shape::RowResize => CursorName::RowResize,
        Shape::AllScroll => CursorName::AllScroll,
        Shape::ZoomIn => CursorName::ZoomIn,
        Shape::ZoomOut => CursorName::ZoomOut,
        Shape::DndAsk => CursorName::DndAsk,
        Shape::AllResize => CursorName::AllResize,
        _ => CursorName::Default,
    }
}

/// Whether `shape` exists at a device's `version` (the last two are `since="2"`).
fn known_at(shape: Shape, version: u32) -> bool {
    version >= 2 || !matches!(shape, Shape::DndAsk | Shape::AllResize)
}

impl GlobalDispatch<WpCursorShapeManagerV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpCursorShapeManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WpCursorShapeManagerV1,
        request: wp_cursor_shape_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_cursor_shape_manager_v1::Request::GetPointer {
                cursor_shape_device,
                pointer,
            } => {
                data_init.init(cursor_shape_device, Some(pointer));
            }
            // No client can have a tablet tool (RubickWay offers no tablets): an inert device.
            wp_cursor_shape_manager_v1::Request::GetTabletToolV2 {
                cursor_shape_device,
                ..
            } => {
                data_init.init(cursor_shape_device, None);
            }
            _ => {}
        }
    }
}

/// A device keeps the pointer it was made for; `None` for a tablet tool's (never made).
impl Dispatch<WpCursorShapeDeviceV1, Option<WlPointer>> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        device: &WpCursorShapeDeviceV1,
        request: wp_cursor_shape_device_v1::Request,
        pointer: &Option<WlPointer>,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let wp_cursor_shape_device_v1::Request::SetShape { serial, shape } = request else {
            // `destroy`: the shape stays as it is (the protocol says so).
            return;
        };
        let shape = match shape {
            WEnum::Value(shape) if known_at(shape, device.version()) => shape,
            WEnum::Value(_) | WEnum::Unknown(_) => {
                device.post_error(
                    wp_cursor_shape_device_v1::Error::InvalidShape,
                    "the shape value is not in the enum of this version",
                );
                return;
            }
        };
        if !pointer.as_ref().is_some_and(Resource::is_alive) {
            return;
        }
        let name = shape_name(shape);
        if seat::request_cursor(state, &client.id(), serial, AppCursor::Shape(name)) {
            tracing::debug!(
                "application {} asks for the {} pointer",
                client_number(client),
                name.css_name()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_protocol_shape_has_its_name_in_order() {
        for (i, name) in CursorName::ALL.iter().enumerate() {
            let value = u32::try_from(i + 1).unwrap_or(0);
            let shape = Shape::try_from(value).map_err(|()| value);
            assert_eq!(shape.map(shape_name), Ok(*name), "value {value}");
        }
    }

    #[test]
    fn the_version_2_shapes_need_version_2() {
        assert!(known_at(Shape::ZoomOut, 1));
        assert!(!known_at(Shape::DndAsk, 1));
        assert!(!known_at(Shape::AllResize, 1));
        assert!(known_at(Shape::AllResize, 2));
    }
}
