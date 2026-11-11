//! `wl_seat`, `wl_pointer`, `wl_keyboard`: input for applications (research R11).
//!
//! The shell decides which window gets what ([`ClientInput`]); this module turns that into
//! protocol events for the right surface of that window, with serials and timestamps.

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};

use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_server::protocol::wl_pointer::{self, WlPointer};
use wayland_server::protocol::wl_seat::{self, WlSeat};
use wayland_server::protocol::wl_surface::WlSurface;
use wayland_server::protocol::wl_touch::WlTouch;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::compositor::{self, Role};
use super::dmabuf::client_number;
use super::{ServerState, xdg_shell};
use crate::canvas::Point;
use crate::ffi::shm_map::sealed_memfd;
use crate::input::{RawModifiers, ScrollSource};
use crate::shell::{AppCursor, ClientInput, CursorName, WindowEvent, WindowId};

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlSeat, ()>(8, ());
}

/// Default key repeat (FR-007): 25 keys per second after 600 ms.
const DEFAULT_REPEAT: (i32, i32) = (25, 600);

/// Input objects of all clients, and where input currently goes.
pub(crate) struct SeatState {
    pub pointers: Vec<WlPointer>,
    pub keyboards: Vec<WlKeyboard>,
    /// The surface the pointer is over.
    pointer_surface: Option<WlSurface>,
    /// Buttons held: while any is, the pointer stays on its surface (implicit grab).
    buttons_down: Vec<u32>,
    /// The window and serial of the latest button press delivered (research W2).
    pub last_press: Option<(WindowId, u32)>,
    /// The surface with keyboard focus (a window's root surface).
    keyboard_surface: Option<WlSurface>,
    keyboard_window: Option<WindowId>,
    pressed_keys: Vec<u32>,
    modifiers: RawModifiers,
    /// The keymap in a sealed memfd, and its size.
    keymap: Option<(File, u32)>,
    repeat: (i32, i32),
    /// The window of `pointer_surface`.
    pointer_window: Option<WindowId>,
    /// The serial of the latest `wl_pointer.enter`: only the focused client's requests count,
    /// so this is that client's latest enter serial (cursor-shapes research C2).
    enter_serial: Option<u32>,
    /// What the focused client asked for since that enter.
    focus_cursor: AppCursor,
}

/// The cursor at each enter, until the client asks for something (cursor-shapes FR-003).
const ENTER_CURSOR: AppCursor = AppCursor::Shape(CursorName::Default);

impl Default for SeatState {
    fn default() -> Self {
        SeatState {
            pointers: Vec::new(),
            keyboards: Vec::new(),
            pointer_surface: None,
            buttons_down: Vec::new(),
            last_press: None,
            keyboard_surface: None,
            keyboard_window: None,
            pressed_keys: Vec::new(),
            modifiers: RawModifiers::default(),
            keymap: None,
            repeat: DEFAULT_REPEAT,
            pointer_window: None,
            enter_serial: None,
            focus_cursor: ENTER_CURSOR,
        }
    }
}

/// A cursor request (`wl_pointer.set_cursor` or `wp_cursor_shape_device_v1.set_shape`) from
/// `client`: it counts only while the pointer is over that client's surface and with the serial
/// of the latest enter (cursor-shapes research C2); the shell hears of it only on a change.
/// Returns whether it changed what is shown, for the caller's debug line.
pub(crate) fn request_cursor(
    state: &mut ServerState,
    client: &ClientId,
    serial: u32,
    cursor: AppCursor,
) -> bool {
    let focused = state
        .seat
        .pointer_surface
        .as_ref()
        .and_then(|s| s.client())
        .is_some_and(|c| c.id() == *client);
    if !focused || state.seat.enter_serial != Some(serial) {
        return false;
    }
    let Some(window) = state.seat.pointer_window else {
        return false;
    };
    if state.seat.focus_cursor == cursor {
        return false;
    }
    state.seat.focus_cursor = cursor;
    state
        .window_events
        .push(WindowEvent::CursorChanged { id: window, cursor });
    true
}

fn same_client(resource: &impl Resource, surface: &WlSurface) -> bool {
    match (resource.client(), surface.client()) {
        (Some(a), Some(b)) => a.id() == b.id(),
        _ => false,
    }
}

fn pointers_for<'a>(
    state: &'a ServerState,
    surface: &'a WlSurface,
) -> impl Iterator<Item = &'a WlPointer> {
    state
        .seat
        .pointers
        .iter()
        .filter(move |p| p.is_alive() && same_client(*p, surface))
}

fn keyboards_for<'a>(
    state: &'a ServerState,
    surface: &'a WlSurface,
) -> impl Iterator<Item = &'a WlKeyboard> {
    state
        .seat
        .keyboards
        .iter()
        .filter(move |k| k.is_alive() && same_client(*k, surface))
}

/// The surface of `window` under window point `pos`, with `pos` relative to it.
fn surface_at(state: &ServerState, window: WindowId, pos: Point) -> Option<(WlSurface, Point)> {
    let root = state.windows.get(&window)?;
    let origin = root_origin(state, root);
    let (id, local) = compositor::surface_at(state, root, origin, pos)
        .unwrap_or_else(|| (root.clone(), Point::new(pos.x - origin.x, pos.y - origin.y)));
    let surface = state.surfaces.get(&id)?.resource.clone();
    Some((surface, local))
}

/// Window coordinates of a window's root surface origin.
fn root_origin(state: &ServerState, root: &ObjectId) -> Point {
    let geometry = xdg_shell::geometry_of(state, root);
    Point::new(-geometry.origin.x, -geometry.origin.y)
}

/// `pos` (window coordinates of `window`) relative to `surface`.
fn relative_to(state: &ServerState, window: WindowId, surface: &WlSurface, pos: Point) -> Point {
    let Some(root) = state.windows.get(&window) else {
        return pos;
    };
    let origin = root_origin(state, root);
    let at = compositor::surface_origin(state, root, origin, &surface.id()).unwrap_or(origin);
    Point::new(pos.x - at.x, pos.y - at.y)
}

fn pointer_enter(state: &mut ServerState, window: WindowId, surface: WlSurface, local: Point) {
    let serial = state.next_serial();
    for p in pointers_for(state, &surface) {
        p.enter(serial, &surface, local.x, local.y);
    }
    state.seat.pointer_surface = Some(surface);
    state.seat.pointer_window = Some(window);
    state.seat.enter_serial = Some(serial);
    // The arrow until the client asks for something; the shell forgets its own copy when the
    // pointer leaves a window, so it needs telling only within one window (a subsurface).
    if state.seat.focus_cursor != ENTER_CURSOR {
        state.seat.focus_cursor = ENTER_CURSOR;
        state.window_events.push(WindowEvent::CursorChanged {
            id: window,
            cursor: ENTER_CURSOR,
        });
    }
}

fn pointer_leave(state: &mut ServerState) {
    if let Some(surface) = state.seat.pointer_surface.take() {
        let serial = state.next_serial();
        for p in pointers_for(state, &surface) {
            p.leave(serial, &surface);
        }
    }
    state.seat.buttons_down.clear();
    // The buttons count as released (a broken grab, or a console switch): an older press no
    // longer allows a move or resize (FR-016, research S22).
    state.seat.last_press = None;
}

fn axis_source(source: ScrollSource) -> wl_pointer::AxisSource {
    match source {
        ScrollSource::Wheel => wl_pointer::AxisSource::Wheel,
        ScrollSource::Finger => wl_pointer::AxisSource::Finger,
        ScrollSource::Continuous => wl_pointer::AxisSource::Continuous,
    }
}

/// Delivers shell decisions as protocol events. `time_ms` is the compositor clock.
pub(crate) fn deliver(state: &mut ServerState, inputs: &[ClientInput], time_ms: u32) {
    for input in inputs {
        match input {
            ClientInput::PointerEnter { window, pos } => {
                pointer_leave(state);
                if let Some((surface, local)) = surface_at(state, *window, *pos) {
                    pointer_enter(state, *window, surface, local);
                }
            }
            ClientInput::PointerMotion { window, pos } => {
                let current = state.seat.pointer_surface.clone();
                let grabbed = !state.seat.buttons_down.is_empty();
                let (surface, local) = match (&current, grabbed) {
                    (Some(s), true) => (s.clone(), relative_to(state, *window, s, *pos)),
                    _ => match surface_at(state, *window, *pos) {
                        Some(hit) => hit,
                        None => continue,
                    },
                };
                if current.as_ref() != Some(&surface) {
                    let buttons = std::mem::take(&mut state.seat.buttons_down);
                    pointer_leave(state);
                    state.seat.buttons_down = buttons;
                    pointer_enter(state, *window, surface, local);
                } else {
                    for p in pointers_for(state, &surface) {
                        p.motion(time_ms, local.x, local.y);
                    }
                }
            }
            ClientInput::PointerLeave { .. } => pointer_leave(state),
            ClientInput::PointerButton {
                window,
                code,
                pressed,
            } => {
                let Some(surface) = state.seat.pointer_surface.clone() else {
                    continue;
                };
                if *pressed {
                    state.seat.buttons_down.push(*code);
                } else {
                    state.seat.buttons_down.retain(|b| b != code);
                }
                let serial = state.next_serial();
                if *pressed {
                    state.seat.last_press = Some((*window, serial));
                } else if state.seat.buttons_down.is_empty() {
                    // Only a press still held counts for move and resize (FR-016).
                    state.seat.last_press = None;
                }
                let button_state = if *pressed {
                    wl_pointer::ButtonState::Pressed
                } else {
                    wl_pointer::ButtonState::Released
                };
                for p in pointers_for(state, &surface) {
                    p.button(serial, time_ms, *code, button_state);
                }
            }
            ClientInput::Scroll {
                source,
                delta,
                value120,
                ..
            } => {
                let Some(surface) = state.seat.pointer_surface.clone() else {
                    continue;
                };
                for p in pointers_for(state, &surface) {
                    if p.version() >= 5 {
                        p.axis_source(axis_source(*source));
                    }
                    let axes = [
                        (
                            wl_pointer::Axis::VerticalScroll,
                            delta.y,
                            value120.map(|v| v.y),
                        ),
                        (
                            wl_pointer::Axis::HorizontalScroll,
                            delta.x,
                            value120.map(|v| v.x),
                        ),
                    ];
                    for (axis, value, notches) in axes {
                        if let Some(v120) = notches.filter(|v| *v != 0.0) {
                            if p.version() >= 8 {
                                p.axis_value120(axis, v120.round() as i32);
                            } else if p.version() >= 5 {
                                p.axis_discrete(axis, (v120 / 120.0).round() as i32);
                            }
                        }
                        if value != 0.0 {
                            p.axis(time_ms, axis, value);
                        }
                    }
                }
            }
            ClientInput::PointerFrame { .. } => {
                if let Some(surface) = state.seat.pointer_surface.clone() {
                    for p in pointers_for(state, &surface) {
                        if p.version() >= 5 {
                            p.frame();
                        }
                    }
                }
            }
            ClientInput::KeyboardFocus(window) => keyboard_focus(state, *window),
            ClientInput::Key {
                keycode, pressed, ..
            } => {
                if *pressed {
                    state.seat.pressed_keys.push(*keycode);
                } else {
                    state.seat.pressed_keys.retain(|k| k != keycode);
                }
                let Some(surface) = state.seat.keyboard_surface.clone() else {
                    continue;
                };
                let serial = state.next_serial();
                let key_state = if *pressed {
                    wl_keyboard::KeyState::Pressed
                } else {
                    wl_keyboard::KeyState::Released
                };
                for k in keyboards_for(state, &surface) {
                    k.key(serial, time_ms, *keycode, key_state);
                }
            }
            ClientInput::Modifiers { raw, .. } => {
                state.seat.modifiers = *raw;
                if let Some(surface) = state.seat.keyboard_surface.clone() {
                    send_modifiers(state, &surface);
                }
            }
        }
    }
}

fn send_modifiers(state: &mut ServerState, surface: &WlSurface) {
    let serial = state.next_serial();
    let m = state.seat.modifiers;
    for k in keyboards_for(state, surface) {
        k.modifiers(serial, m.depressed, m.latched, m.locked, m.group);
    }
}

fn pressed_keys_array(state: &ServerState) -> Vec<u8> {
    state
        .seat
        .pressed_keys
        .iter()
        .flat_map(|k| k.to_ne_bytes())
        .collect()
}

fn keyboard_focus(state: &mut ServerState, window: Option<WindowId>) {
    if let Some(surface) = state.seat.keyboard_surface.take() {
        let serial = state.next_serial();
        for k in keyboards_for(state, &surface) {
            k.leave(serial, &surface);
        }
    }
    if let Some(old) = state.seat.keyboard_window.take() {
        xdg_shell::set_activated(state, old, false);
    }
    let Some(window) = window else {
        state.seat.pressed_keys.clear();
        return;
    };
    let Some(root) = state.windows.get(&window).cloned() else {
        return;
    };
    let Some(surface) = state.surfaces.get(&root).map(|s| s.resource.clone()) else {
        return;
    };
    let serial = state.next_serial();
    let keys = pressed_keys_array(state);
    for k in keyboards_for(state, &surface) {
        k.enter(serial, &surface, keys.clone());
    }
    send_modifiers(state, &surface);
    state.seat.keyboard_surface = Some(surface);
    state.seat.keyboard_window = Some(window);
    xdg_shell::set_activated(state, window, true);
}

/// A new keymap: sent to every keyboard and kept for new ones.
pub(crate) fn set_keymap(state: &mut ServerState, text: &str) {
    match sealed_memfd(text) {
        Ok((fd, size)) => {
            state.seat.keymap = Some((File::from(fd), size));
            let keyboards = state.seat.keyboards.clone();
            for k in keyboards.iter().filter(|k| k.is_alive()) {
                send_keymap(state, k);
            }
        }
        Err(err) => tracing::warn!("cannot share the keymap with applications: {err}"),
    }
}

/// New key repeat settings.
pub(crate) fn set_repeat(state: &mut ServerState, rate: i32, delay: i32) {
    state.seat.repeat = (rate.max(0), delay.max(0));
    for k in state.seat.keyboards.iter().filter(|k| k.is_alive()) {
        if k.version() >= 4 {
            k.repeat_info(state.seat.repeat.0, state.seat.repeat.1);
        }
    }
}

fn send_keymap(state: &ServerState, keyboard: &WlKeyboard) {
    let Some((file, size)) = &state.seat.keymap else {
        return;
    };
    // A fresh open file description per client, so no client moves another's file offset.
    match File::open(format!("/proc/self/fd/{}", file.as_raw_fd())) {
        Ok(own) => keyboard.keymap(wl_keyboard::KeymapFormat::XkbV1, own.as_fd(), *size),
        Err(err) => tracing::warn!("cannot reopen the keymap: {err}"),
    }
}

/// Forgets a destroyed surface wherever input points at it.
pub(crate) fn surface_destroyed(state: &mut ServerState, surface: &ObjectId) {
    if state
        .seat
        .pointer_surface
        .as_ref()
        .is_some_and(|s| s.id() == *surface)
    {
        state.seat.pointer_surface = None;
        state.seat.buttons_down.clear();
    }
    if state
        .seat
        .keyboard_surface
        .as_ref()
        .is_some_and(|s| s.id() == *surface)
    {
        state.seat.keyboard_surface = None;
        state.seat.keyboard_window = None;
    }
}

impl GlobalDispatch<WlSeat, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlSeat>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
        if seat.version() >= 2 {
            seat.name("seat0".to_string());
        }
    }
}

impl Dispatch<WlSeat, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        seat: &WlSeat,
        request: wl_seat::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                let pointer = data_init.init(id, ());
                state.seat.pointers.push(pointer);
            }
            wl_seat::Request::GetKeyboard { id } => {
                let keyboard = data_init.init(id, ());
                send_keymap(state, &keyboard);
                if keyboard.version() >= 4 {
                    keyboard.repeat_info(state.seat.repeat.0, state.seat.repeat.1);
                }
                if let Some(surface) = state.seat.keyboard_surface.clone()
                    && same_client(&keyboard, &surface)
                {
                    let serial = state.next_serial();
                    keyboard.enter(serial, &surface, pressed_keys_array(state));
                    let m = state.seat.modifiers;
                    let serial = state.next_serial();
                    keyboard.modifiers(serial, m.depressed, m.latched, m.locked, m.group);
                }
                state.seat.keyboards.push(keyboard);
            }
            wl_seat::Request::GetTouch { id } => {
                data_init.init(id, ());
                seat.post_error(wl_seat::Error::MissingCapability, "no touch device");
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        pointer: &WlPointer,
        request: wl_pointer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // Cursor images are accepted but not shown (the arrow is drawn); a cursor without a
        // surface hides the pointer over the client's windows (FR-008, cursor-shapes
        // contracts/protocols.md "wl_pointer").
        let wl_pointer::Request::SetCursor {
            serial, surface, ..
        } = request
        else {
            return;
        };
        let (cursor, what) = match surface {
            Some(surface) => {
                if !compositor::set_role(state, &surface.id(), Role::Cursor) {
                    pointer.post_error(wl_pointer::Error::Role, "the surface already has a role");
                    return;
                }
                (
                    ENTER_CURSOR,
                    "shows its own pointer image (the arrow is drawn)",
                )
            }
            None => (AppCursor::Hidden, "hides the pointer"),
        };
        if request_cursor(state, &client.id(), serial, cursor) {
            tracing::debug!("application {} {what}", client_number(client));
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, pointer: &WlPointer, _: &()) {
        state.seat.pointers.retain(|p| p.id() != pointer.id());
    }
}

impl Dispatch<WlKeyboard, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlKeyboard,
        _: wl_keyboard::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, keyboard: &WlKeyboard, _: &()) {
        state.seat.keyboards.retain(|k| k.id() != keyboard.id());
    }
}

impl Dispatch<WlTouch, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlTouch,
        _: <WlTouch as Resource>::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
