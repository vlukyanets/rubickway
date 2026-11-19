//! Clipboard tools and managers (wl-copy, wl-paste, cliphist, wl-clip-persist; clipboard FR-007):
//! `ext_data_control_manager_v1` version 1 and the older `zwlr_data_control_manager_v1` version 2,
//! with the same behaviour (spec Clarifications, contracts/protocols.md). Every application may
//! use them (spec Clarifications). The two protocols differ only in their names, so one macro
//! writes the handlers for both.

use wayland_server::backend::ClientId;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::ServerState;
use super::selection::{self, Device, OfferData, Source, SourceCell, Which};

/// Writes the global and the four interfaces of one data-control protocol.
macro_rules! data_control {
    (
        $proto:path, $version:expr, $variant:ident,
        $manager:ident, $manager_mod:ident,
        $device:ident, $device_mod:ident,
        $source:ident, $source_mod:ident,
        $offer:ident, $offer_mod:ident $(,)?
    ) => {
        use $proto::{
            $device_mod::{self, $device},
            $manager_mod::{self, $manager},
            $offer_mod::{self, $offer},
            $source_mod::{self, $source},
        };

        pub(crate) fn create_global(dh: &DisplayHandle) {
            dh.create_global::<ServerState, $manager, ()>($version, ());
        }

        impl GlobalDispatch<$manager, ()> for ServerState {
            fn bind(
                _: &mut Self,
                _: &DisplayHandle,
                _: &Client,
                resource: New<$manager>,
                _: &(),
                data_init: &mut DataInit<'_, Self>,
            ) {
                data_init.init(resource, ());
            }
        }

        impl Dispatch<$manager, ()> for ServerState {
            fn request(
                state: &mut Self,
                _: &Client,
                _: &$manager,
                request: $manager_mod::Request,
                _: &(),
                _: &DisplayHandle,
                data_init: &mut DataInit<'_, Self>,
            ) {
                match request {
                    $manager_mod::Request::CreateDataSource { id } => {
                        data_init.init(id, SourceCell::default());
                    }
                    $manager_mod::Request::GetDataDevice { id, .. } => {
                        let device = data_init.init(id, ());
                        selection::device_created(state, Device::$variant(device));
                    }
                    _ => {}
                }
            }
        }

        impl Dispatch<$source, SourceCell> for ServerState {
            fn request(
                _: &mut Self,
                _: &Client,
                resource: &$source,
                request: $source_mod::Request,
                data: &SourceCell,
                _: &DisplayHandle,
                _: &mut DataInit<'_, Self>,
            ) {
                if let $source_mod::Request::Offer { mime_type } = request {
                    let mut data = selection::lock(data);
                    if data.is_used() {
                        resource.post_error(
                            $source_mod::Error::InvalidOffer,
                            "offer after the source was used",
                        );
                        return;
                    }
                    data.add_type(mime_type);
                }
            }

            fn destroyed(state: &mut Self, _: ClientId, resource: &$source, _: &SourceCell) {
                selection::source_destroyed(state, &resource.id());
            }
        }

        impl Dispatch<$device, ()> for ServerState {
            fn request(
                state: &mut Self,
                _: &Client,
                resource: &$device,
                request: $device_mod::Request,
                _: &(),
                _: &DisplayHandle,
                _: &mut DataInit<'_, Self>,
            ) {
                // No serial and no focus needed: these are the user's tools (FR-007).
                let (which, source) = match request {
                    $device_mod::Request::SetSelection { source } => (Which::Clipboard, source),
                    $device_mod::Request::SetPrimarySelection { source } => {
                        (Which::Primary, source)
                    }
                    _ => return,
                };
                let source = source.map(Source::$variant);
                if let Some(s) = &source
                    && !s.mark_used()
                {
                    resource.post_error(
                        $device_mod::Error::UsedSource,
                        "the data source was already used",
                    );
                    return;
                }
                if which == Which::Primary && !state.selection.primary_enabled {
                    // Switched off: ignored, as the protocol says for a compositor without a
                    // primary selection; the source is told so the tool does not wait (C10).
                    if let Some(s) = &source {
                        s.cancelled();
                    }
                    return;
                }
                selection::set(state, which, source);
            }

            fn destroyed(state: &mut Self, _: ClientId, resource: &$device, _: &()) {
                selection::device_destroyed(state, &resource.id());
            }
        }

        impl Dispatch<$offer, OfferData> for ServerState {
            fn request(
                state: &mut Self,
                _: &Client,
                _: &$offer,
                request: $offer_mod::Request,
                data: &OfferData,
                _: &DisplayHandle,
                _: &mut DataInit<'_, Self>,
            ) {
                if let $offer_mod::Request::Receive { mime_type, fd } = request {
                    selection::receive(state, *data, mime_type, fd);
                }
            }
        }
    };
}

/// `ext_data_control_manager_v1` (staging, `wayland-protocols`).
pub(crate) mod ext {
    use super::*;

    data_control!(
        wayland_protocols::ext::data_control::v1::server,
        1,
        ExtControl,
        ExtDataControlManagerV1,
        ext_data_control_manager_v1,
        ExtDataControlDeviceV1,
        ext_data_control_device_v1,
        ExtDataControlSourceV1,
        ext_data_control_source_v1,
        ExtDataControlOfferV1,
        ext_data_control_offer_v1,
    );
}

/// `zwlr_data_control_manager_v1` (`wayland-protocols-wlr`, clipboard research C1).
pub(crate) mod wlr {
    use super::*;

    data_control!(
        wayland_protocols_wlr::data_control::v1::server,
        2,
        WlrControl,
        ZwlrDataControlManagerV1,
        zwlr_data_control_manager_v1,
        ZwlrDataControlDeviceV1,
        zwlr_data_control_device_v1,
        ZwlrDataControlSourceV1,
        zwlr_data_control_source_v1,
        ZwlrDataControlOfferV1,
        zwlr_data_control_offer_v1,
    );
}
