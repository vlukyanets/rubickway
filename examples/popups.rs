//! Test program for popups quickstart Q11: a window whose right-click opens a grabbing menu at the
//! pointer; keys then open more popups with chosen placement rules, so the edge cases can be
//! repeated by hand. Every configure, reposition and dismissal is printed.
//!
//! Run from a terminal inside RubickWay: `cargo run --release --example popups`. Keys (the
//! keyboard goes to the topmost grabbing popup, and to the window otherwise):
//!
//! - right-click: a grabbing menu at the pointer (flip, slide and resize allowed);
//! - `s`: a grabbing submenu beside the topmost popup (flip on x, slide and resize on y);
//! - `n`: a grabbing 300x300 menu at the pointer that allows no adjustment;
//! - `t`: a grabbing menu 2000 pixels tall (slide and resize allowed);
//! - `h`: a tooltip without a grab under the pointer;
//! - `r`: a reactive popup without a grab at the window's right edge;
//! - `w`: the window becomes wider or narrower (reactive popups are placed again);
//! - `p`: the topmost popup asks to move 40 pixels to the right (reposition);
//! - Escape: the topmost popup closes; `q`: quit.

use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsFd;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::xdg::shell::client::xdg_popup::{self, XdgPopup};
use wayland_protocols::xdg::shell::client::xdg_positioner::{
    Anchor, ConstraintAdjustment, Gravity, XdgPositioner,
};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

/// Bytes per colour region of the pool; a popup's buffer must fit in one.
const REGION: usize = 2 << 20;
/// Region colours (xrgb): window, menus, tooltips, reactive popups.
const COLOURS: [u32; 4] = [0x00c0_c0c0, 0x0040_70b0, 0x00e0_d070, 0x0060_a060];
const BTN_RIGHT: u32 = 0x111;
const KEY_ESC: u32 = 1;
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_P: u32 = 25;
const KEY_S: u32 = 31;
const KEY_H: u32 = 35;
const KEY_N: u32 = 49;

/// What a popup is, for its colour and its name in the output.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Menu,
    Tooltip,
    Reactive,
}

struct Pop {
    surface: WlSurface,
    xdg: XdgSurface,
    popup: XdgPopup,
    kind: Kind,
    grab: bool,
    size: (i32, i32),
}

struct App {
    compositor: WlCompositor,
    wm_base: XdgWmBase,
    pool: WlShmPool,
    seat: WlSeat,
    window: WlSurface,
    window_xdg: XdgSurface,
    window_size: (i32, i32),
    pointer: Option<WlPointer>,
    keyboard: Option<WlKeyboard>,
    /// The pointer over the window, in window coordinates.
    at: (i32, i32),
    popups: Vec<Pop>,
    next_token: u32,
    running: bool,
}

/// A buffer of `w x h` from the colour region of `region`, clamped to fit.
fn buffer(pool: &WlShmPool, region: usize, w: i32, h: i32, qh: &QueueHandle<App>) -> WlBuffer {
    let w = w.clamp(1, 1024);
    let max_h = i32::try_from(REGION / (w as usize * 4)).unwrap_or(1);
    let h = h.clamp(1, max_h);
    let offset = i32::try_from(region * REGION).unwrap_or(0);
    pool.create_buffer(offset, w, h, w * 4, wl_shm::Format::Xrgb8888, qh, ())
}

impl App {
    fn positioner(
        &self,
        qh: &QueueHandle<Self>,
        size: (i32, i32),
        rect: (i32, i32, i32, i32),
        anchor: Anchor,
        gravity: Gravity,
        adjust: ConstraintAdjustment,
    ) -> XdgPositioner {
        let p = self.wm_base.create_positioner(qh, ());
        p.set_size(size.0, size.1);
        p.set_anchor_rect(rect.0, rect.1, rect.2, rect.3);
        p.set_anchor(anchor);
        p.set_gravity(gravity);
        p.set_constraint_adjustment(adjust);
        p
    }

    /// Opens a popup on `parent` (the window if `None`); with `grab`, a grab with `serial`.
    fn open(
        &mut self,
        qh: &QueueHandle<Self>,
        positioner: &XdgPositioner,
        parent: Option<usize>,
        kind: Kind,
        grab: Option<u32>,
    ) {
        let surface = self.compositor.create_surface(qh, ());
        let xdg = self.wm_base.get_xdg_surface(&surface, qh, ());
        let parent_xdg = match parent {
            Some(i) => &self.popups[i].xdg,
            None => &self.window_xdg,
        };
        let popup = xdg.get_popup(Some(parent_xdg), positioner, qh, ());
        if let Some(serial) = grab {
            popup.grab(&self.seat, serial);
        }
        surface.commit();
        positioner.destroy();
        println!("open {kind:?} (popup {})", popup.id().protocol_id());
        self.popups.push(Pop {
            surface,
            xdg,
            popup,
            kind,
            grab: grab.is_some(),
            size: (0, 0),
        });
    }

    /// Destroys the topmost popup.
    fn close_top(&mut self) {
        if let Some(p) = self.popups.pop() {
            p.popup.destroy();
            p.xdg.destroy();
            p.surface.destroy();
        }
    }

    fn menu_at_pointer(
        &mut self,
        qh: &QueueHandle<Self>,
        serial: u32,
        size: (i32, i32),
        adjust: ConstraintAdjustment,
    ) {
        let pos = self.positioner(
            qh,
            size,
            (self.at.0, self.at.1, 1, 1),
            Anchor::BottomRight,
            Gravity::BottomRight,
            adjust,
        );
        self.open(qh, &pos, None, Kind::Menu, Some(serial));
    }

    fn key(&mut self, qh: &QueueHandle<Self>, key: u32, serial: u32) {
        let all = ConstraintAdjustment::all();
        match key {
            KEY_S => {
                let Some(i) = self.popups.iter().rposition(|p| p.grab) else {
                    println!("no grabbing popup to open a submenu on");
                    return;
                };
                let w = self.popups[i].size.0.max(1);
                let pos = self.positioner(
                    qh,
                    (180, 160),
                    (0, 20, w, 20),
                    Anchor::TopRight,
                    Gravity::BottomRight,
                    ConstraintAdjustment::FlipX
                        | ConstraintAdjustment::SlideY
                        | ConstraintAdjustment::ResizeY,
                );
                self.open(qh, &pos, Some(i), Kind::Menu, Some(serial));
            }
            KEY_N => self.menu_at_pointer(qh, serial, (300, 300), ConstraintAdjustment::empty()),
            KEY_T => self.menu_at_pointer(
                qh,
                serial,
                (160, 2000),
                ConstraintAdjustment::SlideX
                    | ConstraintAdjustment::SlideY
                    | ConstraintAdjustment::ResizeY,
            ),
            KEY_H => {
                let pos = self.positioner(
                    qh,
                    (120, 30),
                    (self.at.0, self.at.1 + 20, 1, 1),
                    Anchor::BottomRight,
                    Gravity::BottomRight,
                    all,
                );
                self.open(qh, &pos, None, Kind::Tooltip, None);
            }
            KEY_R => {
                let pos = self.positioner(
                    qh,
                    (100, 60),
                    (self.window_size.0 - 1, 40, 1, 1),
                    Anchor::Right,
                    Gravity::Right,
                    all,
                );
                pos.set_reactive();
                self.open(qh, &pos, None, Kind::Reactive, None);
            }
            KEY_W => {
                self.window_size.0 = if self.window_size.0 == 400 { 600 } else { 400 };
                let b = buffer(&self.pool, 0, self.window_size.0, self.window_size.1, qh);
                self.window.attach(Some(&b), 0, 0);
                self.window.damage_buffer(0, 0, i32::MAX, i32::MAX);
                self.window.commit();
                println!("window width {}", self.window_size.0);
            }
            KEY_P => {
                let Some(top) = self.popups.last() else {
                    return;
                };
                let (w, h) = top.size;
                let pos = self.positioner(
                    qh,
                    (w.max(1), h.max(1)),
                    (40, 0, 1, 1),
                    Anchor::TopLeft,
                    Gravity::BottomRight,
                    all,
                );
                self.next_token += 1;
                if top.popup.version() >= 3 {
                    top.popup.reposition(&pos, self.next_token);
                    println!("reposition, token {}", self.next_token);
                }
                pos.destroy();
            }
            KEY_ESC => self.close_top(),
            KEY_Q => self.running = false,
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 1..=6, ())?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let wm_base: XdgWmBase = globals.bind(&qh, 3..=6, ())?;
    let seat: WlSeat = globals.bind(&qh, 1..=8, ())?;

    // One file of colour regions, mapped by the server; the name goes at once. Opened for
    // reading too: the server maps it read-only, which a write-only descriptor refuses.
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let path = std::path::Path::new(&dir).join(format!("rubickway-popups-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    for colour in COLOURS {
        let region: Vec<u8> = (0..REGION / 4).flat_map(|_| colour.to_le_bytes()).collect();
        file.write_all(&region)?;
    }
    let pool = shm.create_pool(
        file.as_fd(),
        i32::try_from(REGION * COLOURS.len())?,
        &qh,
        (),
    );

    let window = compositor.create_surface(&qh, ());
    let window_xdg = wm_base.get_xdg_surface(&window, &qh, ());
    let toplevel = window_xdg.get_toplevel(&qh, ());
    toplevel.set_title("popups".into());
    window.commit();

    let mut app = App {
        compositor,
        wm_base,
        pool,
        seat,
        window,
        window_xdg,
        window_size: (400, 300),
        pointer: None,
        keyboard: None,
        at: (0, 0),
        popups: Vec::new(),
        next_token: 0,
        running: true,
    };
    println!("right-click the window for a menu; then s, n, t, h, r, w, p, Escape, q");
    while app.running {
        queue.blocking_dispatch(&mut app)?;
    }
    Ok(())
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
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
        qh: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        xdg.ack_configure(serial);
        if *xdg == app.window_xdg {
            let b = buffer(&app.pool, 0, app.window_size.0, app.window_size.1, qh);
            app.window.attach(Some(&b), 0, 0);
            app.window.damage_buffer(0, 0, i32::MAX, i32::MAX);
            app.window.commit();
            return;
        }
        if let Some(p) = app.popups.iter().find(|p| p.xdg == *xdg) {
            let region = match p.kind {
                Kind::Menu => 1,
                Kind::Tooltip => 2,
                Kind::Reactive => 3,
            };
            let b = buffer(&app.pool, region, p.size.0, p.size.1, qh);
            p.surface.attach(Some(&b), 0, 0);
            p.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
            p.surface.commit();
        }
    }
}

impl Dispatch<XdgPopup, ()> for App {
    fn event(
        app: &mut Self,
        popup: &XdgPopup,
        event: xdg_popup::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let n = popup.id().protocol_id();
        match event {
            xdg_popup::Event::Configure {
                x,
                y,
                width,
                height,
            } => {
                println!("popup {n}: at ({x}, {y}) relative to its parent, {width}x{height}");
                if let Some(p) = app.popups.iter_mut().find(|p| p.popup == *popup) {
                    p.size = (width, height);
                }
            }
            xdg_popup::Event::Repositioned { token } => {
                println!("popup {n}: repositioned, token {token}");
            }
            xdg_popup::Event::PopupDone => {
                println!("popup {n}: dismissed");
                // Dismissals come topmost first; destroy from the top down to this one.
                if let Some(i) = app.popups.iter().position(|p| p.popup == *popup) {
                    while app.popups.len() > i {
                        app.close_top();
                    }
                }
            }
            _ => {}
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
            app.pointer = Some(seat.get_pointer(qh, ()));
        }
        if capabilities.contains(wl_seat::Capability::Keyboard) && app.keyboard.is_none() {
            app.keyboard = Some(seat.get_keyboard(qh, ()));
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
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } if surface == app.window => app.at = (surface_x as i32, surface_y as i32),
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } if app.popups.is_empty() => app.at = (surface_x as i32, surface_y as i32),
            wl_pointer::Event::Button {
                serial,
                button: BTN_RIGHT,
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => app.menu_at_pointer(qh, serial, (160, 200), ConstraintAdjustment::all()),
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            serial,
            key,
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            app.key(qh, key, serial);
        }
    }
}

delegate_noop!(App: ignore WlCompositor);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlShmPool);
delegate_noop!(App: ignore WlBuffer);
delegate_noop!(App: ignore XdgPositioner);
