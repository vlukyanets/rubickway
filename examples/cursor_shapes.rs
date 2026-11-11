//! Test program for cursor-shapes quickstart Q6a: a window of 36 cells, one per shape of the cursor
//! shape protocol, in protocol order from the top-left, row by row. Over each cell it names that
//! shape and prints the shape's name, so every shape can be looked at.
//!
//! Run from a terminal inside RubickWay: `cargo run --release --example cursor_shapes`.

use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsFd;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

const CELL: u32 = 64;
const COLUMNS: u32 = 6;
const SIDE: u32 = CELL * COLUMNS;

/// The shapes' names, in protocol order (value 1 first).
const NAMES: [&str; 36] = [
    "default",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "e-resize",
    "n-resize",
    "ne-resize",
    "nw-resize",
    "s-resize",
    "se-resize",
    "sw-resize",
    "w-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "col-resize",
    "row-resize",
    "all-scroll",
    "zoom-in",
    "zoom-out",
    "dnd-ask",
    "all-resize",
];

struct App {
    surface: WlSurface,
    buffer: WlBuffer,
    manager: WpCursorShapeManagerV1,
    pointer: Option<WlPointer>,
    device: Option<WpCursorShapeDeviceV1>,
    /// The serial of the latest `wl_pointer.enter`.
    serial: u32,
    /// The protocol value of the shape named last.
    shown: Option<u32>,
    running: bool,
}

impl App {
    /// Names the shape of the cell under window point (x, y), if it is not named already.
    fn point_at(&mut self, x: f64, y: f64) {
        let column = (x.max(0.0) as u32 / CELL).min(COLUMNS - 1);
        let row = (y.max(0.0) as u32 / CELL).min(COLUMNS - 1);
        let value = row * COLUMNS + column + 1;
        if self.shown == Some(value) {
            return;
        }
        let (Some(device), Ok(shape)) = (&self.device, Shape::try_from(value)) else {
            return;
        };
        device.set_shape(self.serial, shape);
        self.shown = Some(value);
        let name = NAMES.get(value as usize - 1).copied().unwrap_or("?");
        println!("{value:2} {name}");
    }
}

/// The window's picture: a checkerboard of cells, each with a dark border.
fn picture() -> Vec<u8> {
    let mut bytes = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (cx, cy) = (x % CELL, y % CELL);
            let border = cx < 2 || cy < 2 || cx >= CELL - 2 || cy >= CELL - 2;
            let shade: u8 = if border {
                0x30
            } else if (x / CELL + y / CELL).is_multiple_of(2) {
                0xd8
            } else {
                0xb0
            };
            bytes.extend_from_slice(&[shade, shade, shade, 0xff]);
        }
    }
    bytes
}

fn main() -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 1..=6, ())?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let wm_base: XdgWmBase = globals.bind(&qh, 1..=6, ())?;
    let _seat: WlSeat = globals.bind(&qh, 1..=8, ())?;
    let manager: WpCursorShapeManagerV1 = globals.bind(&qh, 1..=2, ())?;

    // The picture in a file the server maps; the name goes at once. Opened for reading too: the
    // server maps it read-only, which a write-only descriptor refuses.
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let path =
        std::path::Path::new(&dir).join(format!("rubickway-cursor-shapes-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    let bytes = picture();
    file.write_all(&bytes)?;
    let size = i32::try_from(bytes.len())?;
    let side = i32::try_from(SIDE)?;
    let pool = shm.create_pool(file.as_fd(), size, &qh, ());
    let buffer = pool.create_buffer(0, side, side, side * 4, wl_shm::Format::Xrgb8888, &qh, ());

    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("cursor shapes".into());
    surface.commit();

    let mut app = App {
        surface,
        buffer,
        manager,
        pointer: None,
        device: None,
        serial: 0,
        shown: None,
        running: true,
    };
    println!("move the pointer over the cells; each names one shape:");
    while app.running {
        queue.blocking_dispatch(&mut app)?;
    }
    Ok(())
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        xdg: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            app.surface.attach(Some(&app.buffer), 0, 0);
            app.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
            app.surface.commit();
        }
    }
}

impl Dispatch<XdgToplevel, ()> for App {
    fn event(
        app: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            app.running = false;
        }
    }
}

impl Dispatch<WlSeat, ()> for App {
    fn event(
        app: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Pointer) && app.pointer.is_none() {
            let pointer = seat.get_pointer(qh, ());
            app.device = Some(app.manager.get_pointer(&pointer, qh, ()));
            app.pointer = Some(pointer);
        }
    }
}

impl Dispatch<WlPointer, ()> for App {
    fn event(
        app: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial,
                surface_x,
                surface_y,
                ..
            } => {
                // Every enter starts from the arrow: name the shape again.
                app.serial = serial;
                app.shown = None;
                app.point_at(surface_x, surface_y);
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => app.point_at(surface_x, surface_y),
            wl_pointer::Event::Leave { .. } => app.shown = None,
            _ => {}
        }
    }
}

delegate_noop!(App: ignore WlCompositor);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlShmPool);
delegate_noop!(App: ignore WlBuffer);
delegate_noop!(App: ignore WpCursorShapeManagerV1);
delegate_noop!(App: ignore WpCursorShapeDeviceV1);
