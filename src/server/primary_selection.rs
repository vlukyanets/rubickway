//! `zwp_primary_selection_device_manager_v1` version 1: the primary selection, select and paste
//! with the middle button (clipboard FR-006, contracts/protocols.md). The same rules as the
//! clipboard, on the second slot of `selection.rs`; offered unless switched off (research C10).

use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_device_manager_v1::{
    self, ZwpPrimarySelectionDeviceManagerV1,
};
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_device_v1::{
    self, ZwpPrimarySelectionDeviceV1,
};
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_offer_v1::{
    self, ZwpPrimarySelectionOfferV1,
};
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_source_v1::{
    self, ZwpPrimarySelectionSourceV1,
};
use wayland_server::backend::{ClientId, GlobalId};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::selection::{self, Device, OfferData, Source, SourceCell, Which};
use super::{ServerState, seat};

pub(crate) fn create_global(dh: &DisplayHandle) -> GlobalId {
    dh.create_global::<ServerState, ZwpPrimarySelectionDeviceManagerV1, ()>(1, ())
}

impl GlobalDispatch<ZwpPrimarySelectionDeviceManagerV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpPrimarySelectionDeviceManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpPrimarySelectionDeviceManagerV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpPrimarySelectionDeviceManagerV1,
        request: zwp_primary_selection_device_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_primary_selection_device_manager_v1::Request::CreateSource { id } => {
                data_init.init(id, SourceCell::default());
            }
            zwp_primary_selection_device_manager_v1::Request::GetDevice { id, .. } => {
                let device = data_init.init(id, ());
                selection::device_created(state, Device::Primary(device));
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpPrimarySelectionSourceV1, SourceCell> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwpPrimarySelectionSourceV1,
        request: zwp_primary_selection_source_v1::Request,
        data: &SourceCell,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwp_primary_selection_source_v1::Request::Offer { mime_type } = request {
            selection::lock(data).add_type(mime_type);
        }
    }

    fn destroyed(
        state: &mut Self,
        _: ClientId,
        resource: &ZwpPrimarySelectionSourceV1,
        _: &SourceCell,
    ) {
        selection::source_destroyed(state, &resource.id());
    }
}

impl Dispatch<ZwpPrimarySelectionDeviceV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &ZwpPrimarySelectionDeviceV1,
        request: zwp_primary_selection_device_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let zwp_primary_selection_device_v1::Request::SetSelection { source, serial } = request
        else {
            return;
        };
        let source = source.map(Source::Primary);
        // A source used again is no error (the protocol defines none, and Firefox sets the same
        // source again; research C8). A request not in response to the user's input is
        // ignored, the source told (research C4).
        if let Some(s) = &source {
            s.mark_used();
        }
        if !seat::selection_serial_ok(state, &client.id(), serial) {
            if let Some(s) = &source {
                s.cancelled();
            }
            selection::log_ignored(state, client);
            return;
        }
        selection::set(state, Which::Primary, source);
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &ZwpPrimarySelectionDeviceV1, _: &()) {
        selection::device_destroyed(state, &resource.id());
    }
}

impl Dispatch<ZwpPrimarySelectionOfferV1, OfferData> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpPrimarySelectionOfferV1,
        request: zwp_primary_selection_offer_v1::Request,
        data: &OfferData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwp_primary_selection_offer_v1::Request::Receive { mime_type, fd } = request {
            selection::receive(state, *data, mime_type, fd);
        }
    }
}
