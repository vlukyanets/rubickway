//! `wl_data_device_manager` stub (research R10): some applications (foot) refuse to start
//! without it. Devices and sources exist, but nothing is ever offered or transferred, and
//! drags are cancelled at once.

use wayland_server::protocol::wl_data_device::{self, WlDataDevice};
use wayland_server::protocol::wl_data_device_manager::{self, WlDataDeviceManager};
use wayland_server::protocol::wl_data_source::{self, WlDataSource};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New};

use super::ServerState;

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
        _: &mut Self,
        _: &Client,
        _: &WlDataDeviceManager,
        request: wl_data_device_manager::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_data_device_manager::Request::CreateDataSource { id } => {
                data_init.init(id, ());
            }
            wl_data_device_manager::Request::GetDataDevice { id, .. } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataSource, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlDataSource,
        _: wl_data_source::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // Offers and actions are accepted; nothing is transferred.
    }
}

impl Dispatch<WlDataDevice, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlDataDevice,
        request: wl_data_device::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wl_data_device::Request::StartDrag {
            source: Some(source),
            ..
        } = request
        {
            source.cancelled();
        }
        // set_selection is accepted and ignored.
    }
}
