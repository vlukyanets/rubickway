//! The clipboard, the primary selection and clipboard tools (clipboard contracts/protocols.md):
//! offers to the focused application, transfers through real pipes, who may set a selection,
//! replacement, and the misbehaving cases tied to them.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::sync::mpsc;
use std::time::Duration;

use rubickway::canvas::Point;
use rubickway::server::Damage;
use rubickway::shell::{ClientInput, IRect};
use wayland_client::Proxy;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_keyboard;
use wayland_client::protocol::wl_pointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols::wp::primary_selection::zv1::client::zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1;
use wayland_protocols::wp::primary_selection::zv1::client::zwp_primary_selection_device_v1 as pd;
use wayland_protocols::wp::primary_selection::zv1::client::zwp_primary_selection_offer_v1 as po;
use wayland_protocols::wp::primary_selection::zv1::client::zwp_primary_selection_source_v1 as ps;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1 as ed;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1 as eo;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_source_v1 as es;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1 as wd;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1 as wo;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_source_v1 as ws;

use crate::harness::{Client, Harness};
use crate::popups::{PopupClient, popup_client, shown_popup};

const KEY_C: u32 = 46;

// --- helpers --------------------------------------------------------------------------------

/// A client with a window, a keyboard, a pointer and a clipboard device.
pub struct Clip {
    pub pc: PopupClient,
    pub manager: WlDataDeviceManager,
    pub device: WlDataDevice,
}

pub fn clip_client(h: &mut Harness) -> Clip {
    let pc = popup_client(h);
    let manager: WlDataDeviceManager = h.c(pc.index).bind(3);
    let qh = h.c(pc.index).qh.clone();
    let device = manager.get_data_device(&pc.seat, &qh, ());
    h.roundtrip();
    Clip {
        pc,
        manager,
        device,
    }
}

/// Gives `c`'s window the keyboard focus.
pub fn focus(h: &mut Harness, c: &Clip) {
    h.server
        .deliver(&[ClientInput::KeyboardFocus(Some(c.pc.id))], 1);
    h.roundtrip();
}

/// A key press and release delivered to the focused `c`; returns the press's serial.
pub fn key_serial(h: &mut Harness, c: &Clip) -> u32 {
    for pressed in [true, false] {
        h.server.deliver(
            &[ClientInput::Key {
                window: c.pc.id,
                keycode: KEY_C,
                pressed,
            }],
            2,
        );
        h.roundtrip();
    }
    let serials: Vec<u32> = h
        .c(c.pc.index)
        .events::<wl_keyboard::Event>()
        .iter()
        .filter_map(|(_, e)| match e {
            wl_keyboard::Event::Key { serial, .. } => Some(*serial),
            _ => None,
        })
        .collect();
    serials[serials.len() - 2]
}

/// A source offering `types`.
pub fn source(h: &mut Harness, c: &Clip, types: &[&str]) -> WlDataSource {
    let qh = h.c(c.pc.index).qh.clone();
    let source = c.manager.create_data_source(&qh, ());
    for t in types {
        source.offer(t.to_string());
    }
    source
}

/// `c` copies `types` with `serial`.
pub fn copy(h: &mut Harness, c: &Clip, types: &[&str], serial: u32) -> WlDataSource {
    let s = source(h, c, types);
    c.device.set_selection(Some(&s), serial);
    h.roundtrip();
    s
}

/// The focused `c` presses a key and copies `types` with that key's serial.
pub fn copy_now(h: &mut Harness, c: &Clip, types: &[&str]) -> WlDataSource {
    let serial = key_serial(h, c);
    copy(h, c, types, serial)
}

/// The latest `selection` `c`'s device got, with its offer's types; `None` inside: empty.
pub fn last_selection(
    client: &Client,
    device: &WlDataDevice,
) -> Option<Option<(WlDataOffer, Vec<String>)>> {
    let selection = client
        .events::<wl_data_device::Event>()
        .into_iter()
        .rev()
        .find_map(|(id, e)| match e {
            wl_data_device::Event::Selection { id: offer } if id == device.id() => {
                Some(offer.clone())
            }
            _ => None,
        })?;
    Some(selection.map(|offer| {
        let types = client
            .events::<wl_data_offer::Event>()
            .into_iter()
            .filter(|(id, _)| *id == offer.id())
            .filter_map(|(_, e)| match e {
                wl_data_offer::Event::Offer { mime_type } => Some(mime_type.clone()),
                _ => None,
            })
            .collect();
        (offer, types)
    }))
}

fn selections(client: &Client, device: &WlDataDevice) -> usize {
    client
        .events::<wl_data_device::Event>()
        .iter()
        .filter(|(id, e)| {
            *id == device.id() && matches!(e, wl_data_device::Event::Selection { .. })
        })
        .count()
}

fn cancelled(client: &Client, source: &WlDataSource) -> bool {
    client
        .events::<wl_data_source::Event>()
        .iter()
        .any(|(id, e)| *id == source.id() && matches!(e, wl_data_source::Event::Cancelled))
}

/// Writes `text` into every `send` the client got, closes those descriptors and forgets the
/// recorded events (which hold them). Returns the types asked for.
pub fn answer_sends(h: &mut Harness, ci: usize, text: &[u8]) -> Vec<String> {
    let mut asked = Vec::new();
    for (_, e) in h.c(ci).events::<wl_data_source::Event>() {
        if let wl_data_source::Event::Send { mime_type, fd } = e {
            asked.push(mime_type.clone());
            let mut file = File::from(fd.try_clone().unwrap());
            let _ = file.write_all(text);
        }
    }
    h.c(ci).clear_events();
    asked
}

/// Reads `reader` to its end on a thread; `None` if it does not end within 5 s.
pub fn read_all(mut reader: std::io::PipeReader) -> Option<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = reader.read_to_end(&mut out);
        let _ = tx.send(out);
    });
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// `to` pastes `mime` from `offer`; the owner `from` answers with `text`. Returns what `to`
/// read.
fn paste(
    h: &mut Harness,
    to: &Clip,
    offer: &WlDataOffer,
    mime: &str,
    from: usize,
    text: &[u8],
) -> Option<Vec<u8>> {
    let (reader, writer) = std::io::pipe().unwrap();
    offer.receive(mime.to_string(), writer.as_fd());
    let _ = to;
    drop(writer);
    h.roundtrip();
    answer_sends(h, from, text);
    h.roundtrip();
    read_all(reader)
}

// --- US1: copy and paste (clipboard tasks T012) ---------------------------------------------

#[test]
fn the_focused_application_is_offered_the_clipboard_before_its_keyboard_enter() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    assert_eq!(
        last_selection(h.c(a.pc.index), &a.device),
        Some(None),
        "an empty clipboard is announced too"
    );
    let serial = key_serial(&mut h, &a);
    copy(&mut h, &a, &["text/plain", "text/html"], serial);
    h.c(b.pc.index).clear_events();
    focus(&mut h, &b);
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(
        types,
        Some(vec!["text/plain".to_string(), "text/html".to_string()])
    );
    let c = h.c(b.pc.index);
    let selection_at = c.state.events.iter().position(|r| {
        matches!(
            r.event.downcast_ref::<wl_data_device::Event>(),
            Some(wl_data_device::Event::Selection { .. })
        )
    });
    let enter_at = c.state.events.iter().position(|r| {
        matches!(
            r.event.downcast_ref::<wl_keyboard::Event>(),
            Some(wl_keyboard::Event::Enter { .. })
        )
    });
    assert!(selection_at.is_some() && enter_at.is_some());
    assert!(
        selection_at < enter_at,
        "selection before wl_keyboard.enter"
    );
}

#[test]
fn a_paste_passes_the_owners_text_to_the_pasting_application() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    copy(&mut h, &a, &["text/plain;charset=utf-8"], serial);
    focus(&mut h, &b);
    let (offer, _) = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap();
    h.c(a.pc.index).clear_events();
    let got = paste(
        &mut h,
        &b,
        &offer,
        "text/plain;charset=utf-8",
        a.pc.index,
        b"hello, canvas",
    );
    assert_eq!(got.as_deref(), Some(&b"hello, canvas"[..]));
}

#[test]
fn an_application_without_the_focus_is_not_offered_anything() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let c = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    copy(&mut h, &a, &["text/plain"], serial);
    assert_eq!(selections(h.c(c.pc.index), &c.device), 0);
}

#[test]
fn a_copy_arriving_after_the_focus_moved_away_is_accepted() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let first = copy_now(&mut h, &a, &["text/plain"]);
    // Ctrl+C, then Mod+Tab at once: the request comes after the focus moved to B.
    let serial = key_serial(&mut h, &a);
    focus(&mut h, &b);
    h.c(b.pc.index).clear_events();
    copy(&mut h, &a, &["text/uri-list"], serial);
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(
        types,
        Some(vec!["text/uri-list".to_string()]),
        "B is told at once"
    );
    assert!(
        cancelled(h.c(a.pc.index), &first),
        "the previous source is cancelled"
    );
}

#[test]
fn a_copy_without_a_recent_input_serial_is_ignored() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let kept = copy_now(&mut h, &a, &["text/plain"]);
    // An old serial: 16 more input events since.
    let old = key_serial(&mut h, &a);
    for _ in 0..8 {
        key_serial(&mut h, &a);
    }
    let foreign = {
        focus(&mut h, &b);
        key_serial(&mut h, &b)
    };
    focus(&mut h, &a);
    for bad in [0xdead_beef, foreign, old] {
        let s = copy(&mut h, &a, &["text/html"], bad);
        assert!(
            cancelled(h.c(a.pc.index), &s),
            "serial {bad}: the source is told"
        );
        assert!(
            !cancelled(h.c(a.pc.index), &kept),
            "serial {bad}: the clipboard is kept"
        );
        assert!(!h.c(a.pc.index).is_disconnected(), "no error");
    }
    focus(&mut h, &b);
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(types, Some(vec!["text/plain".to_string()]));
}

#[test]
fn emptying_destroying_or_quitting_clears_the_clipboard() {
    for how in ["null", "destroy", "quit"] {
        let mut h = Harness::new();
        let b = clip_client(&mut h);
        let a = clip_client(&mut h);
        focus(&mut h, &a);
        let s = copy_now(&mut h, &a, &["text/plain"]);
        let serial = key_serial(&mut h, &a);
        focus(&mut h, &b);
        assert!(
            last_selection(h.c(b.pc.index), &b.device)
                .flatten()
                .is_some(),
            "{how}"
        );
        match how {
            "null" => {
                a.device.set_selection(None, serial);
                h.roundtrip();
                assert!(cancelled(h.c(a.pc.index), &s), "{how}");
            }
            "destroy" => {
                s.destroy();
                h.roundtrip();
            }
            _ => {
                drop(a);
                let client = h.clients.remove(1);
                drop(client);
                h.roundtrip();
            }
        }
        assert_eq!(
            last_selection(h.c(b.pc.index), &b.device),
            Some(None),
            "{how}"
        );
    }
}

#[test]
fn a_stale_offer_or_an_unknown_type_reads_end_of_file_at_once() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    copy_now(&mut h, &a, &["text/plain"]);
    focus(&mut h, &b);
    let (old, _) = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap();
    // An unknown type on the current offer.
    h.c(a.pc.index).clear_events();
    let got = paste(&mut h, &b, &old, "image/png", a.pc.index, b"x");
    assert_eq!(got, Some(Vec::new()));
    assert!(
        answer_sends(&mut h, a.pc.index, b"").is_empty(),
        "the owner is not asked"
    );
    // New content makes the old offer stale.
    focus(&mut h, &a);
    copy_now(&mut h, &a, &["text/plain"]);
    h.c(a.pc.index).clear_events();
    let got = paste(&mut h, &b, &old, "text/plain", a.pc.index, b"x");
    assert_eq!(got, Some(Vec::new()));
}

#[test]
fn a_device_made_while_focused_gets_the_clipboard_at_once() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    copy_now(&mut h, &a, &["text/plain"]);
    focus(&mut h, &b);
    let qh = h.c(b.pc.index).qh.clone();
    let second = b.manager.get_data_device(&b.pc.seat, &qh, ());
    h.roundtrip();
    let types = last_selection(h.c(b.pc.index), &second)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(types, Some(vec!["text/plain".to_string()]));
}

#[test]
fn the_keyboard_moving_to_a_popup_sends_no_new_offer() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = crate::popups::press(&mut h, &a.pc, Point::new(10.0, 10.0));
    let (_popup, id) = shown_popup(&mut h, &a.pc, IRect::new(10, 20, 30, 40), Some(serial));
    let before = selections(h.c(a.pc.index), &a.device);
    h.server.deliver(
        &[ClientInput::PopupKeyboard {
            window: a.pc.id,
            popup: Some(id),
        }],
        3,
    );
    h.roundtrip();
    assert_eq!(selections(h.c(a.pc.index), &a.device), before);
}

#[test]
fn a_console_switch_keeps_the_clipboard() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let s = copy_now(&mut h, &a, &["text/plain"]);
    // FocusLost: the shell takes the keyboard from every window; FocusGained gives it back.
    h.server.deliver(&[ClientInput::KeyboardFocus(None)], 4);
    h.roundtrip();
    focus(&mut h, &b);
    let (offer, _) = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap();
    assert!(!cancelled(h.c(a.pc.index), &s));
    h.c(a.pc.index).clear_events();
    let got = paste(&mut h, &b, &offer, "text/plain", a.pc.index, b"kept");
    assert_eq!(got.as_deref(), Some(&b"kept"[..]));
}

#[test]
fn a_flood_of_copies_is_announced_once_with_the_newest() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    focus(&mut h, &b);
    h.c(b.pc.index).clear_events();
    let qh = h.c(a.pc.index).qh.clone();
    let mut sources = Vec::new();
    for i in 0..10_000 {
        let s = a.manager.create_data_source(&qh, ());
        s.offer(format!("type/{i}"));
        a.device.set_selection(Some(&s), serial);
        sources.push(s);
    }
    h.roundtrip();
    // At most once per dispatch (research C3): 10 000 requests arrive over a few reads of the
    // socket, so B is told a few times, not 10 000.
    let told = selections(h.c(b.pc.index), &b.device);
    assert!((1..=8).contains(&told), "told {told} times");
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(types, Some(vec!["type/9999".to_string()]));
    let ca = h.c(a.pc.index);
    let cancelled_count = sources.iter().filter(|s| cancelled(ca, s)).count();
    assert_eq!(cancelled_count, 9_999);
    assert!(!h.c(a.pc.index).is_disconnected());
}

#[test]
fn a_source_keeps_at_most_256_distinct_types() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    let names: Vec<String> = (0..300).map(|i| format!("type/{}", i % 200)).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    copy(&mut h, &a, &refs, serial);
    focus(&mut h, &b);
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap()
        .1;
    assert_eq!(types.len(), 200, "repeats once");
    focus(&mut h, &a);
    let names: Vec<String> = (0..300).map(|i| format!("type/{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    copy_now(&mut h, &a, &refs);
    focus(&mut h, &b);
    let types = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap()
        .1;
    assert_eq!(types.len(), 256);
    assert_eq!(types[255], "type/255");
}

/// Firefox puts the same source into the clipboard again (found on the laptop, 2026-11-19: a
/// `used_source` error disconnected it). It keeps working, and the content stays.
#[test]
fn an_application_setting_the_same_source_again_keeps_working() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    let s = source(&mut h, &a, &["text/plain"]);
    a.device.set_selection(Some(&s), serial);
    a.device.set_selection(Some(&s), serial);
    h.roundtrip();
    a.device.set_selection(Some(&s), key_serial(&mut h, &a));
    h.roundtrip();
    assert!(
        !h.c(a.pc.index).is_disconnected(),
        "{:?}",
        h.c(a.pc.index).error()
    );
    assert!(
        !cancelled(h.c(a.pc.index), &s),
        "it still owns the clipboard"
    );
    focus(&mut h, &b);
    let (offer, _) = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap();
    h.c(a.pc.index).clear_events();
    let got = paste(&mut h, &b, &offer, "text/plain", a.pc.index, b"same");
    assert_eq!(got.as_deref(), Some(&b"same"[..]));
}

#[test]
fn copying_and_pasting_redraws_nothing() {
    let mut h = Harness::new();
    let a = clip_client(&mut h);
    let b = clip_client(&mut h);
    focus(&mut h, &a);
    let serial = key_serial(&mut h, &a);
    focus(&mut h, &b);
    h.server.take_damage();
    copy(&mut h, &a, &["text/plain"], serial);
    let (offer, _) = last_selection(h.c(b.pc.index), &b.device)
        .flatten()
        .unwrap();
    h.c(a.pc.index).clear_events();
    paste(&mut h, &b, &offer, "text/plain", a.pc.index, b"x");
    assert_eq!(
        h.server.take_damage(),
        Damage {
            all: false,
            windows: vec![],
        }
    );
}

// --- US2: clipboard tools (clipboard tasks T018) --------------------------------------------

/// Which data-control protocol a tool speaks: ext, or wlr at a version.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Ext,
    Wlr(u32),
}

pub const KINDS: [Kind; 2] = [Kind::Ext, Kind::Wlr(2)];

pub enum ToolDevice {
    Ext(ed::ExtDataControlDeviceV1),
    Wlr(wd::ZwlrDataControlDeviceV1),
}

pub enum ToolSource {
    Ext(es::ExtDataControlSourceV1),
    Wlr(ws::ZwlrDataControlSourceV1),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOffer {
    Ext(eo::ExtDataControlOfferV1),
    Wlr(wo::ZwlrDataControlOfferV1),
}

/// A clipboard tool: a client with no window, only a data-control device.
pub struct Tool {
    pub index: usize,
    ext: Option<ExtDataControlManagerV1>,
    wlr: Option<ZwlrDataControlManagerV1>,
    pub device: ToolDevice,
}

pub fn tool_client(h: &mut Harness, kind: Kind) -> Tool {
    let index = h.connect();
    let seat: WlSeat = h.c(index).bind(8);
    let qh = h.c(index).qh.clone();
    let tool = match kind {
        Kind::Ext => {
            let m: ExtDataControlManagerV1 = h.c(index).bind(1);
            let device = ToolDevice::Ext(m.get_data_device(&seat, &qh, ()));
            Tool {
                index,
                ext: Some(m),
                wlr: None,
                device,
            }
        }
        Kind::Wlr(version) => {
            let m: ZwlrDataControlManagerV1 = h.c(index).bind(version);
            let device = ToolDevice::Wlr(m.get_data_device(&seat, &qh, ()));
            Tool {
                index,
                ext: None,
                wlr: Some(m),
                device,
            }
        }
    };
    h.roundtrip();
    tool
}

impl Tool {
    pub fn source(&self, h: &mut Harness, types: &[&str]) -> ToolSource {
        let qh = h.c(self.index).qh.clone();
        match (&self.ext, &self.wlr) {
            (Some(m), _) => {
                let s = m.create_data_source(&qh, ());
                for t in types {
                    s.offer(t.to_string());
                }
                ToolSource::Ext(s)
            }
            (_, Some(m)) => {
                let s = m.create_data_source(&qh, ());
                for t in types {
                    s.offer(t.to_string());
                }
                ToolSource::Wlr(s)
            }
            _ => unreachable!(),
        }
    }

    /// Sets the clipboard (or with `primary` the primary selection) to `source`.
    pub fn set(&self, h: &mut Harness, primary: bool, source: Option<&ToolSource>) {
        match &self.device {
            ToolDevice::Ext(d) => {
                let s = source.map(|s| match s {
                    ToolSource::Ext(s) => s,
                    ToolSource::Wlr(_) => unreachable!(),
                });
                if primary {
                    d.set_primary_selection(s);
                } else {
                    d.set_selection(s);
                }
            }
            ToolDevice::Wlr(d) => {
                let s = source.map(|s| match s {
                    ToolSource::Wlr(s) => s,
                    ToolSource::Ext(_) => unreachable!(),
                });
                if primary {
                    d.set_primary_selection(s);
                } else {
                    d.set_selection(s);
                }
            }
        }
        h.roundtrip();
    }

    /// Every `selection` (or `primary_selection`) the tool got, oldest first, with the offer's
    /// types; `None`: empty.
    pub fn told(&self, h: &mut Harness, primary: bool) -> Vec<Option<(ToolOffer, Vec<String>)>> {
        let c = h.c(self.index);
        let mut out = Vec::new();
        for r in &c.state.events {
            let offer = if let Some(e) = r.event.downcast_ref::<ed::Event>() {
                match e {
                    ed::Event::Selection { id } if !primary => Some(id.clone().map(ToolOffer::Ext)),
                    ed::Event::PrimarySelection { id } if primary => {
                        Some(id.clone().map(ToolOffer::Ext))
                    }
                    _ => None,
                }
            } else if let Some(e) = r.event.downcast_ref::<wd::Event>() {
                match e {
                    wd::Event::Selection { id } if !primary => Some(id.clone().map(ToolOffer::Wlr)),
                    wd::Event::PrimarySelection { id } if primary => {
                        Some(id.clone().map(ToolOffer::Wlr))
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(offer) = offer {
                out.push(offer.map(|o| {
                    let types = tool_offer_types(c, &o);
                    (o, types)
                }));
            }
        }
        out
    }

    pub fn last(&self, h: &mut Harness, primary: bool) -> Option<Option<(ToolOffer, Vec<String>)>> {
        self.told(h, primary).pop()
    }

    pub fn cancelled(&self, h: &mut Harness, source: &ToolSource) -> bool {
        let c = h.c(self.index);
        match source {
            ToolSource::Ext(s) => c
                .events::<es::Event>()
                .iter()
                .any(|(id, e)| *id == s.id() && matches!(e, es::Event::Cancelled)),
            ToolSource::Wlr(s) => c
                .events::<ws::Event>()
                .iter()
                .any(|(id, e)| *id == s.id() && matches!(e, ws::Event::Cancelled)),
        }
    }
}

fn tool_offer_types(c: &Client, offer: &ToolOffer) -> Vec<String> {
    match offer {
        ToolOffer::Ext(o) => c
            .events::<eo::Event>()
            .into_iter()
            .filter(|(id, _)| *id == o.id())
            .map(|(_, e)| match e {
                eo::Event::Offer { mime_type } => mime_type.clone(),
                _ => String::new(),
            })
            .collect(),
        ToolOffer::Wlr(o) => c
            .events::<wo::Event>()
            .into_iter()
            .filter(|(id, _)| *id == o.id())
            .map(|(_, e)| match e {
                wo::Event::Offer { mime_type } => mime_type.clone(),
                _ => String::new(),
            })
            .collect(),
    }
}

/// Writes `text` into every `send` the tool got, closes those descriptors and forgets the
/// recorded events.
pub fn answer_tool_sends(h: &mut Harness, ci: usize, text: &[u8]) {
    let c = h.c(ci);
    let mut fds = Vec::new();
    for (_, e) in c.events::<es::Event>() {
        if let es::Event::Send { fd, .. } = e {
            fds.push(fd.try_clone().unwrap());
        }
    }
    for (_, e) in c.events::<ws::Event>() {
        if let ws::Event::Send { fd, .. } = e {
            fds.push(fd.try_clone().unwrap());
        }
    }
    for fd in fds {
        let _ = File::from(fd).write_all(text);
    }
    h.c(ci).clear_events();
}

/// The tool pastes `mime` from `offer`; client `owner` answers with `text`.
fn tool_paste(
    h: &mut Harness,
    offer: &ToolOffer,
    mime: &str,
    owner: usize,
    text: &[u8],
) -> Option<Vec<u8>> {
    let (reader, writer) = std::io::pipe().unwrap();
    match offer {
        ToolOffer::Ext(o) => o.receive(mime.to_string(), writer.as_fd()),
        ToolOffer::Wlr(o) => o.receive(mime.to_string(), writer.as_fd()),
    }
    drop(writer);
    h.roundtrip();
    answer_sends(h, owner, text);
    h.roundtrip();
    read_all(reader)
}

/// The application `to` pastes `mime` from `offer`, which a tool `tool` owns.
fn paste_from_tool(
    h: &mut Harness,
    offer: &WlDataOffer,
    mime: &str,
    tool: usize,
    text: &[u8],
) -> Option<Vec<u8>> {
    let (reader, writer) = std::io::pipe().unwrap();
    offer.receive(mime.to_string(), writer.as_fd());
    drop(writer);
    h.roundtrip();
    answer_tool_sends(h, tool, text);
    h.roundtrip();
    read_all(reader)
}

fn tool_error(h: &mut Harness, ci: usize) -> Option<(String, u32)> {
    h.c(ci)
        .error()
        .map(|e| (e.object_interface.clone(), e.code))
}

#[test]
fn a_new_tool_device_gets_the_current_content_at_once() {
    for kind in KINDS {
        let mut h = Harness::new();
        let a = clip_client(&mut h);
        focus(&mut h, &a);
        copy_now(&mut h, &a, &["text/plain", "text/html"]);
        let tool = tool_client(&mut h, kind);
        let types = tool.last(&mut h, false).flatten().map(|(_, t)| t);
        assert_eq!(
            types,
            Some(vec!["text/plain".to_string(), "text/html".to_string()]),
            "{kind:?}"
        );
        assert_eq!(
            tool.last(&mut h, true),
            Some(None),
            "{kind:?}: the primary selection, empty"
        );
    }
}

#[test]
fn a_tool_sets_the_clipboard_without_focus_and_applications_paste_it() {
    for kind in KINDS {
        let mut h = Harness::new();
        let a = clip_client(&mut h);
        let b = clip_client(&mut h);
        focus(&mut h, &a);
        let previous = copy_now(&mut h, &a, &["text/plain"]);
        focus(&mut h, &b);
        let tool = tool_client(&mut h, kind);
        let s = tool.source(&mut h, &["text/plain;charset=utf-8"]);
        tool.set(&mut h, false, Some(&s));
        assert!(cancelled(h.c(a.pc.index), &previous), "{kind:?}");
        let (offer, types) = last_selection(h.c(b.pc.index), &b.device)
            .flatten()
            .unwrap();
        assert_eq!(
            types,
            vec!["text/plain;charset=utf-8".to_string()],
            "{kind:?}"
        );
        let got = paste_from_tool(
            &mut h,
            &offer,
            "text/plain;charset=utf-8",
            tool.index,
            b"hi",
        );
        assert_eq!(got.as_deref(), Some(&b"hi"[..]), "{kind:?}");
    }
}

#[test]
fn a_tool_reads_an_applications_clipboard_without_focus() {
    for kind in KINDS {
        let mut h = Harness::new();
        let a = clip_client(&mut h);
        let tool = tool_client(&mut h, kind);
        focus(&mut h, &a);
        copy_now(&mut h, &a, &["text/plain"]);
        let (offer, _) = tool.last(&mut h, false).flatten().unwrap();
        h.c(a.pc.index).clear_events();
        let got = tool_paste(&mut h, &offer, "text/plain", a.pc.index, b"from firefox");
        assert_eq!(got.as_deref(), Some(&b"from firefox"[..]), "{kind:?}");
    }
}

#[test]
fn every_change_is_told_to_tools_at_most_once_per_dispatch() {
    for kind in KINDS {
        let mut h = Harness::new();
        let a = clip_client(&mut h);
        let tool = tool_client(&mut h, kind);
        let before = tool.told(&mut h, false).len();
        focus(&mut h, &a);
        copy_now(&mut h, &a, &["text/plain"]);
        copy_now(&mut h, &a, &["text/html"]);
        assert_eq!(tool.told(&mut h, false).len(), before + 2, "{kind:?}");
        let serial = key_serial(&mut h, &a);
        let qh = h.c(a.pc.index).qh.clone();
        for i in 0..10_000 {
            let s = a.manager.create_data_source(&qh, ());
            s.offer(format!("type/{i}"));
            a.device.set_selection(Some(&s), serial);
        }
        h.roundtrip();
        let told = tool.told(&mut h, false);
        assert!(
            told.len() - (before + 2) <= 8,
            "{kind:?}: told {} times",
            told.len()
        );
        let last = told.last().cloned().flatten().map(|(_, t)| t);
        assert_eq!(last, Some(vec!["type/9999".to_string()]), "{kind:?}");
    }
}

#[test]
fn a_tool_keeps_the_clipboard_after_its_application_quits() {
    for kind in KINDS {
        let mut h = Harness::new();
        let b = clip_client(&mut h);
        let tool = tool_client(&mut h, kind);
        let a = clip_client(&mut h);
        focus(&mut h, &a);
        copy_now(&mut h, &a, &["text/plain"]);
        let (offer, types) = tool.last(&mut h, false).flatten().unwrap();
        h.c(a.pc.index).clear_events();
        let text = tool_paste(&mut h, &offer, "text/plain", a.pc.index, b"keep me").unwrap();
        // The application quits: the clipboard is empty, and the tool is told.
        drop(a);
        let client = h.clients.remove(2);
        drop(client);
        h.roundtrip();
        assert_eq!(tool.last(&mut h, false), Some(None), "{kind:?}");
        // The tool offers the content again; the focused application pastes it.
        let refs: Vec<&str> = types.iter().map(String::as_str).collect();
        let s = tool.source(&mut h, &refs);
        tool.set(&mut h, false, Some(&s));
        focus(&mut h, &b);
        let (offer, _) = last_selection(h.c(b.pc.index), &b.device)
            .flatten()
            .unwrap();
        let got = paste_from_tool(&mut h, &offer, "text/plain", tool.index, &text);
        assert_eq!(got.as_deref(), Some(&b"keep me"[..]), "{kind:?}");
    }
}

#[test]
fn a_tool_using_a_source_twice_or_offering_after_use_is_an_error() {
    for kind in KINDS {
        for twice in [true, false] {
            let mut h = Harness::new();
            let tool = tool_client(&mut h, kind);
            let s = tool.source(&mut h, &["text/plain"]);
            tool.set(&mut h, false, Some(&s));
            if twice {
                tool.set(&mut h, true, Some(&s));
            } else {
                match &s {
                    ToolSource::Ext(s) => s.offer("text/html".into()),
                    ToolSource::Wlr(s) => s.offer("text/html".into()),
                }
                h.roundtrip();
            }
            let expected = match (kind, twice) {
                (Kind::Ext, true) => ("ext_data_control_device_v1", ed::Error::UsedSource as u32),
                (Kind::Ext, false) => {
                    ("ext_data_control_source_v1", es::Error::InvalidOffer as u32)
                }
                (Kind::Wlr(_), true) => {
                    ("zwlr_data_control_device_v1", wd::Error::UsedSource as u32)
                }
                (Kind::Wlr(_), false) => (
                    "zwlr_data_control_source_v1",
                    ws::Error::InvalidOffer as u32,
                ),
            };
            assert_eq!(
                tool_error(&mut h, tool.index),
                Some((expected.0.to_string(), expected.1)),
                "{kind:?}, twice: {twice}"
            );
        }
    }
}

#[test]
fn a_tool_quitting_with_the_clipboard_empties_it() {
    for kind in KINDS {
        let mut h = Harness::new();
        let b = clip_client(&mut h);
        focus(&mut h, &b);
        let tool = tool_client(&mut h, kind);
        let s = tool.source(&mut h, &["text/plain"]);
        tool.set(&mut h, false, Some(&s));
        assert!(
            last_selection(h.c(b.pc.index), &b.device)
                .flatten()
                .is_some()
        );
        drop(s);
        drop(tool);
        let client = h.clients.remove(1);
        drop(client);
        h.roundtrip();
        assert_eq!(
            last_selection(h.c(b.pc.index), &b.device),
            Some(None),
            "{kind:?}"
        );
    }
}

#[test]
fn an_old_wlr_tool_gets_no_primary_selection() {
    let mut h = Harness::new();
    let tool = tool_client(&mut h, Kind::Wlr(1));
    assert_eq!(tool.last(&mut h, false), Some(None));
    assert!(tool.told(&mut h, true).is_empty());
}

// --- US3: the primary selection (clipboard tasks T021) --------------------------------------

/// A clipboard client that also has a primary selection device.
pub struct Prim {
    pub clip: Clip,
    pub manager: ZwpPrimarySelectionDeviceManagerV1,
    pub device: pd::ZwpPrimarySelectionDeviceV1,
}

pub fn prim_client(h: &mut Harness) -> Prim {
    let clip = clip_client(h);
    let manager: ZwpPrimarySelectionDeviceManagerV1 = h.c(clip.pc.index).bind(1);
    let qh = h.c(clip.pc.index).qh.clone();
    let device = manager.get_device(&clip.pc.seat, &qh, ());
    h.roundtrip();
    Prim {
        clip,
        manager,
        device,
    }
}

/// A left button press and release on `p`'s window (text selected with the mouse); returns
/// the release's serial.
pub fn release_serial(h: &mut Harness, p: &Prim) -> u32 {
    crate::popups::press(h, &p.clip.pc, Point::new(10.0, 10.0));
    crate::popups::release(h, &p.clip.pc);
    let serials: Vec<u32> = h
        .c(p.clip.pc.index)
        .events::<wl_pointer::Event>()
        .iter()
        .filter_map(|(_, e)| match e {
            wl_pointer::Event::Button { serial, .. } => Some(*serial),
            _ => None,
        })
        .collect();
    serials[serials.len() - 1]
}

/// `p` selects text offering `types`, with `serial`.
pub fn select(
    h: &mut Harness,
    p: &Prim,
    types: &[&str],
    serial: u32,
) -> ps::ZwpPrimarySelectionSourceV1 {
    let qh = h.c(p.clip.pc.index).qh.clone();
    let s = p.manager.create_source(&qh, ());
    for t in types {
        s.offer(t.to_string());
    }
    p.device.set_selection(Some(&s), serial);
    h.roundtrip();
    s
}

/// The latest primary `selection` `p`'s device got, with its offer's types.
pub fn last_primary(
    client: &Client,
    device: &pd::ZwpPrimarySelectionDeviceV1,
) -> Option<Option<(po::ZwpPrimarySelectionOfferV1, Vec<String>)>> {
    let selection = client
        .events::<pd::Event>()
        .into_iter()
        .rev()
        .find_map(|(id, e)| match e {
            pd::Event::Selection { id: offer } if id == device.id() => Some(offer.clone()),
            _ => None,
        })?;
    Some(selection.map(|offer| {
        let types = client
            .events::<po::Event>()
            .into_iter()
            .filter(|(id, _)| *id == offer.id())
            .map(|(_, e)| match e {
                po::Event::Offer { mime_type } => mime_type.clone(),
                _ => String::new(),
            })
            .collect();
        (offer, types)
    }))
}

fn primary_cancelled(client: &Client, source: &ps::ZwpPrimarySelectionSourceV1) -> bool {
    client
        .events::<ps::Event>()
        .iter()
        .any(|(id, e)| *id == source.id() && matches!(e, ps::Event::Cancelled))
}

/// Writes `text` into every primary `send` the client got and forgets the recorded events.
fn answer_primary_sends(h: &mut Harness, ci: usize, text: &[u8]) {
    let fds: Vec<_> = h
        .c(ci)
        .events::<ps::Event>()
        .into_iter()
        .filter_map(|(_, e)| match e {
            ps::Event::Send { fd, .. } => Some(fd.try_clone().unwrap()),
            _ => None,
        })
        .collect();
    for fd in fds {
        let _ = File::from(fd).write_all(text);
    }
    h.c(ci).clear_events();
}

#[test]
fn selected_text_pastes_with_the_middle_button_in_another_application() {
    let mut h = Harness::new();
    let a = prim_client(&mut h);
    let b = prim_client(&mut h);
    focus(&mut h, &a.clip);
    let serial = release_serial(&mut h, &a);
    select(&mut h, &a, &["text/plain"], serial);
    h.c(b.clip.pc.index).clear_events();
    focus(&mut h, &b.clip);
    let (offer, types) = last_primary(h.c(b.clip.pc.index), &b.device)
        .flatten()
        .unwrap();
    assert_eq!(types, vec!["text/plain".to_string()]);
    let c = h.c(b.clip.pc.index);
    let selection_at = c.state.events.iter().position(|r| {
        matches!(
            r.event.downcast_ref::<pd::Event>(),
            Some(pd::Event::Selection { .. })
        )
    });
    let enter_at = c.state.events.iter().position(|r| {
        matches!(
            r.event.downcast_ref::<wl_keyboard::Event>(),
            Some(wl_keyboard::Event::Enter { .. })
        )
    });
    assert!(selection_at < enter_at, "before wl_keyboard.enter");
    let (reader, writer) = std::io::pipe().unwrap();
    offer.receive("text/plain".to_string(), writer.as_fd());
    drop(writer);
    h.roundtrip();
    answer_primary_sends(&mut h, a.clip.pc.index, b"selected");
    h.roundtrip();
    assert_eq!(read_all(reader).as_deref(), Some(&b"selected"[..]));
}

#[test]
fn the_clipboard_and_the_primary_selection_are_independent() {
    let mut h = Harness::new();
    let a = prim_client(&mut h);
    let b = prim_client(&mut h);
    focus(&mut h, &a.clip);
    copy_now(&mut h, &a.clip, &["text/copied"]);
    let serial = release_serial(&mut h, &a);
    select(&mut h, &a, &["text/selected"], serial);
    focus(&mut h, &b.clip);
    let clip = last_selection(h.c(b.clip.pc.index), &b.clip.device)
        .flatten()
        .unwrap()
        .1;
    let primary = last_primary(h.c(b.clip.pc.index), &b.device)
        .flatten()
        .unwrap()
        .1;
    assert_eq!(clip, vec!["text/copied".to_string()]);
    assert_eq!(primary, vec!["text/selected".to_string()]);
    // Changing one tells only that one.
    let clip_told = selections(h.c(b.clip.pc.index), &b.clip.device);
    focus(&mut h, &a.clip);
    let serial = release_serial(&mut h, &a);
    focus(&mut h, &b.clip);
    let clip_told_after_focus = selections(h.c(b.clip.pc.index), &b.clip.device);
    select(&mut h, &a, &["text/other"], serial);
    assert_eq!(
        selections(h.c(b.clip.pc.index), &b.clip.device),
        clip_told_after_focus,
        "the clipboard is not told again"
    );
    assert!(clip_told_after_focus > clip_told);
}

#[test]
fn the_primary_selection_follows_the_clipboards_rules() {
    let mut h = Harness::new();
    let a = prim_client(&mut h);
    let b = prim_client(&mut h);
    focus(&mut h, &a.clip);
    let serial = release_serial(&mut h, &a);
    let first = select(&mut h, &a, &["text/plain"], serial);
    // A serial never sent: ignored and cancelled.
    let bad = select(&mut h, &a, &["text/html"], 0xdead_beef);
    assert!(primary_cancelled(h.c(a.clip.pc.index), &bad));
    assert!(!primary_cancelled(h.c(a.clip.pc.index), &first));
    // Set again (Firefox does this): no error, it stays the owner.
    a.device.set_selection(Some(&first), serial);
    h.roundtrip();
    assert!(!h.c(a.clip.pc.index).is_disconnected());
    assert!(!primary_cancelled(h.c(a.clip.pc.index), &first));
    // Replaced: the previous source is cancelled.
    let second = select(&mut h, &a, &["text/uri-list"], serial);
    assert!(primary_cancelled(h.c(a.clip.pc.index), &first));
    focus(&mut h, &b.clip);
    let types = last_primary(h.c(b.clip.pc.index), &b.device)
        .flatten()
        .map(|(_, t)| t);
    assert_eq!(types, Some(vec!["text/uri-list".to_string()]));
    // The owner quits: empty.
    drop(second);
    drop(a);
    let client = h.clients.remove(0);
    drop(client);
    h.roundtrip();
    assert_eq!(last_primary(h.c(0), &b.device), Some(None));
}

#[test]
fn a_tool_sees_and_sets_the_primary_selection() {
    for kind in KINDS {
        let mut h = Harness::new();
        let a = prim_client(&mut h);
        let tool = tool_client(&mut h, kind);
        focus(&mut h, &a.clip);
        let serial = release_serial(&mut h, &a);
        select(&mut h, &a, &["text/plain"], serial);
        let types = tool.last(&mut h, true).flatten().map(|(_, t)| t);
        assert_eq!(types, Some(vec!["text/plain".to_string()]), "{kind:?}");
        let s = tool.source(&mut h, &["text/from-tool"]);
        tool.set(&mut h, true, Some(&s));
        let types = last_primary(h.c(a.clip.pc.index), &a.device)
            .flatten()
            .map(|(_, t)| t);
        assert_eq!(types, Some(vec!["text/from-tool".to_string()]), "{kind:?}");
    }
}

#[test]
fn with_the_primary_selection_off_tools_see_none_and_cannot_set_it() {
    for kind in KINDS {
        let mut h = Harness::new();
        h.server.set_primary_selection(false);
        let tool = tool_client(&mut h, kind);
        assert!(tool.told(&mut h, true).is_empty(), "{kind:?}");
        let s = tool.source(&mut h, &["text/plain"]);
        tool.set(&mut h, true, Some(&s));
        assert!(tool.cancelled(&mut h, &s), "{kind:?}");
        assert!(tool.told(&mut h, true).is_empty(), "{kind:?}");
        assert!(!h.c(tool.index).is_disconnected(), "{kind:?}");
    }
}
