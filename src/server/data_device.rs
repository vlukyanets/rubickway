//! `wl_data_device_manager` version 3: the clipboard (clipboard FR-001-FR-005,
//! contracts/protocols.md). Requests go to `selection.rs`; drag-and-drop stays cancelled at once
//! (FR-010, research C9).

use wayland_server::backend::ClientId;
use wayland_server::protocol::wl_data_device::{self, WlDataDevice};
use wayland_server::protocol::wl_data_device_manager::{self, WlDataDeviceManager};
use wayland_server::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_server::protocol::wl_data_source::{self, WlDataSource};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::selection::{self, Device, OfferData, Source, SourceCell, Which};
use super::{ServerState, seat};

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlDataDeviceManager, ()>(3, ());
}

impl GlobalDispatch<WlDataDeviceManager, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlDataDeviceManager>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlDataDeviceManager, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WlDataDeviceManager,
        request: wl_data_device_manager::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_data_device_manager::Request::CreateDataSource { id } => {
                data_init.init(id, SourceCell::default());
            }
            wl_data_device_manager::Request::GetDataDevice { id, .. } => {
                let device = data_init.init(id, ());
                selection::device_created(state, Device::Data(device));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataSource, SourceCell> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlDataSource,
        request: wl_data_source::Request,
        data: &SourceCell,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // `set_actions` only matters for drag-and-drop, which is not offered.
        if let wl_data_source::Request::Offer { mime_type } = request {
            selection::lock(data).add_type(mime_type);
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlDataSource, _: &SourceCell) {
        selection::source_destroyed(state, &resource.id());
    }
}

impl Dispatch<WlDataDevice, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &WlDataDevice,
        request: wl_data_device::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_data_device::Request::StartDrag {
                source: Some(source),
                ..
            } => {
                // Drag-and-drop is a later feature: cancelled at once.
                let source = Source::Data(source);
                source.mark_used();
                source.cancelled();
            }
            wl_data_device::Request::SetSelection { source, serial } => {
                // A source used again is no error here: the protocol only says it "may" be,
                // and Firefox sets the same source again (found on the laptop, research C8).
                let source = source.map(Source::Data);
                if let Some(s) = &source {
                    s.mark_used();
                }
                if !seat::selection_serial_ok(state, &client.id(), serial) {
                    // Not in response to the user's input: ignored, the source told so
                    // (FR-004, research C4).
                    if let Some(s) = &source {
                        s.cancelled();
                    }
                    selection::log_ignored(state, client);
                    return;
                }
                selection::set(state, Which::Clipboard, source);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlDataDevice, _: &()) {
        selection::device_destroyed(state, &resource.id());
    }
}

impl Dispatch<WlDataOffer, OfferData> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WlDataOffer,
        request: wl_data_offer::Request,
        data: &OfferData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_data_offer::Request::Receive { mime_type, fd } => {
                selection::receive(state, *data, mime_type, fd);
            }
            // Drag-and-drop requests on a clipboard offer (research C8).
            wl_data_offer::Request::Finish => resource.post_error(
                wl_data_offer::Error::InvalidFinish,
                "finish on an offer that is not from a drag",
            ),
            wl_data_offer::Request::SetActions { .. } => resource.post_error(
                wl_data_offer::Error::InvalidOffer,
                "set_actions on an offer that is not from a drag",
            ),
            _ => {}
        }
    }
}
