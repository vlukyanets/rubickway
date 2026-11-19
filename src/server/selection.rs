//! The clipboard and the primary selection (clipboard research C2-C7, C11): two slots, their
//! owners, the devices to tell, and the offers made to them. The protocol modules
//! (`data_device.rs`, `primary_selection.rs`, `data_control.rs`) turn requests into calls here.
//! RubickWay never reads, stores or waits for the content: a paste hands the descriptor to the
//! owner (FR-003, FR-008).

use std::collections::HashMap;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wayland_protocols::ext::data_control::v1::server::ext_data_control_device_v1::ExtDataControlDeviceV1;
use wayland_protocols::ext::data_control::v1::server::ext_data_control_offer_v1::ExtDataControlOfferV1;
use wayland_protocols::ext::data_control::v1::server::ext_data_control_source_v1::ExtDataControlSourceV1;
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1;
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1;
use wayland_protocols::wp::primary_selection::zv1::server::zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1;
use wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_device_v1::ZwlrDataControlDeviceV1;
use wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_offer_v1::ZwlrDataControlOfferV1;
use wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_source_v1::ZwlrDataControlSourceV1;
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_data_device::WlDataDevice;
use wayland_server::protocol::wl_data_offer::WlDataOffer;
use wayland_server::protocol::wl_data_source::WlDataSource;
use wayland_server::{Client, Resource};

use super::ServerState;

/// The most distinct formats one source offers (FR-011).
pub(crate) const MAX_TYPES: usize = 256;

/// One of the two slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    Clipboard,
    Primary,
}

/// What a source offers (clipboard data-model "SourceData"); the user data of every source.
#[derive(Debug, Default)]
pub(crate) struct SourceData {
    /// Distinct, in the order offered, at most [`MAX_TYPES`].
    mime_types: Vec<String>,
    /// Put into a selection or used for a drag: it cannot be used again.
    used: bool,
}

impl SourceData {
    /// Adds a format, unless it is there already, the source is full, or it was used.
    pub(crate) fn add_type(&mut self, mime: String) {
        if !self.used && self.mime_types.len() < MAX_TYPES && !self.mime_types.contains(&mime) {
            self.mime_types.push(mime);
        }
    }

    pub(crate) fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    pub(crate) fn is_used(&self) -> bool {
        self.used
    }

    /// Marks the source used; `false` if it was already (the protocols' `used_source`).
    pub(crate) fn mark_used(&mut self) -> bool {
        !std::mem::replace(&mut self.used, true)
    }
}

/// The user data of every source.
pub(crate) type SourceCell = Mutex<SourceData>;

/// Locks a source's data; a poisoned lock still holds valid data.
pub(crate) fn lock(cell: &SourceCell) -> std::sync::MutexGuard<'_, SourceData> {
    cell.lock().unwrap_or_else(|e| e.into_inner())
}

/// A source of any of the four protocols.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Source {
    Data(WlDataSource),
    Primary(ZwpPrimarySelectionSourceV1),
    ExtControl(ExtDataControlSourceV1),
    WlrControl(ZwlrDataControlSourceV1),
}

impl Source {
    pub(crate) fn id(&self) -> ObjectId {
        match self {
            Source::Data(s) => s.id(),
            Source::Primary(s) => s.id(),
            Source::ExtControl(s) => s.id(),
            Source::WlrControl(s) => s.id(),
        }
    }

    fn client(&self) -> Option<Client> {
        match self {
            Source::Data(s) => s.client(),
            Source::Primary(s) => s.client(),
            Source::ExtControl(s) => s.client(),
            Source::WlrControl(s) => s.client(),
        }
    }

    /// A clipboard tool's source (no serial, no window).
    fn is_control(&self) -> bool {
        matches!(self, Source::ExtControl(_) | Source::WlrControl(_))
    }

    fn cell(&self) -> Option<&SourceCell> {
        match self {
            Source::Data(s) => s.data::<SourceCell>(),
            Source::Primary(s) => s.data::<SourceCell>(),
            Source::ExtControl(s) => s.data::<SourceCell>(),
            Source::WlrControl(s) => s.data::<SourceCell>(),
        }
    }

    /// The formats it offers.
    pub(crate) fn mime_types(&self) -> Vec<String> {
        self.cell()
            .map(|c| lock(c).mime_types().to_vec())
            .unwrap_or_default()
    }

    /// Marks it used; `false` if it was already.
    pub(crate) fn mark_used(&self) -> bool {
        self.cell().is_none_or(|c| lock(c).mark_used())
    }

    pub(crate) fn cancelled(&self) {
        match self {
            Source::Data(s) if s.is_alive() => s.cancelled(),
            Source::Primary(s) if s.is_alive() => s.cancelled(),
            Source::ExtControl(s) if s.is_alive() => s.cancelled(),
            Source::WlrControl(s) if s.is_alive() => s.cancelled(),
            _ => {}
        }
    }

    /// Asks the owner to write `mime` into `fd`.
    pub(crate) fn send(&self, mime: String, fd: impl AsFd) {
        let fd = fd.as_fd();
        match self {
            Source::Data(s) => s.send(mime, fd),
            Source::Primary(s) => s.send(mime, fd),
            Source::ExtControl(s) => s.send(mime, fd),
            Source::WlrControl(s) => s.send(mime, fd),
        }
    }
}

/// A device of any of the four protocols.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Device {
    /// Told the clipboard while its client has the keyboard focus.
    Data(WlDataDevice),
    /// Told the primary selection while its client has the keyboard focus.
    Primary(ZwpPrimarySelectionDeviceV1),
    /// Told both slots, always.
    ExtControl(ExtDataControlDeviceV1),
    /// Told both slots, always (the primary selection from version 2).
    WlrControl(ZwlrDataControlDeviceV1),
}

impl Device {
    pub(crate) fn id(&self) -> ObjectId {
        match self {
            Device::Data(d) => d.id(),
            Device::Primary(d) => d.id(),
            Device::ExtControl(d) => d.id(),
            Device::WlrControl(d) => d.id(),
        }
    }

    pub(crate) fn client(&self) -> Option<ClientId> {
        let client = match self {
            Device::Data(d) => d.client(),
            Device::Primary(d) => d.client(),
            Device::ExtControl(d) => d.client(),
            Device::WlrControl(d) => d.client(),
        };
        client.map(|c| c.id())
    }

    /// A clipboard-tool device: told of every change, focus or not.
    pub(crate) fn is_control(&self) -> bool {
        matches!(self, Device::ExtControl(_) | Device::WlrControl(_))
    }
}

/// One slot: empty or owned.
#[derive(Debug, Default)]
pub(crate) struct Slot {
    pub(crate) owner: Option<Source>,
    /// Bumped on every change; offers made for an older one are stale (research C6).
    pub(crate) generation: u64,
}

impl Slot {
    pub(crate) fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

/// The user data of every offer: which slot, and the content it was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OfferData {
    pub(crate) which: Which,
    pub(crate) generation: u64,
}

/// The clipboard state (clipboard data-model "SelectionState").
#[derive(Debug)]
pub(crate) struct SelectionState {
    clipboard: Slot,
    primary: Slot,
    /// The primary selection is offered (research C10).
    pub(crate) primary_enabled: bool,
    /// Every live device of every client.
    pub(crate) devices: Vec<Device>,
    /// The focused client last told, with the generations it was told (research C3).
    pub(crate) announced: Option<(ClientId, u64, u64)>,
    /// The generations clipboard-tool devices were last told.
    pub(crate) control_announced: (u64, u64),
    /// The generations last written to the log (research C12).
    logged: (u64, u64),
    /// When each application's ignored request was last logged: at most once a minute.
    ignored_logged: HashMap<ClientId, Instant>,
}

impl Default for SelectionState {
    fn default() -> Self {
        SelectionState {
            clipboard: Slot::default(),
            primary: Slot::default(),
            primary_enabled: true,
            devices: Vec::new(),
            announced: None,
            control_announced: (0, 0),
            logged: (0, 0),
            ignored_logged: HashMap::new(),
        }
    }
}

impl SelectionState {
    pub(crate) fn slot(&self, which: Which) -> &Slot {
        match which {
            Which::Clipboard => &self.clipboard,
            Which::Primary => &self.primary,
        }
    }

    pub(crate) fn slot_mut(&mut self, which: Which) -> &mut Slot {
        match which {
            Which::Clipboard => &mut self.clipboard,
            Which::Primary => &mut self.primary,
        }
    }

    /// The user data for an offer of `which`'s current content.
    pub(crate) fn offer_data(&self, which: Which) -> OfferData {
        OfferData {
            which,
            generation: self.slot(which).generation,
        }
    }

    /// Whether `offer` was made for the current content of its slot.
    pub(crate) fn is_current(&self, offer: OfferData) -> bool {
        self.slot(offer.which).generation == offer.generation
    }
}

/// Puts `source` (or nothing) into `which`: the previous owner is told it no longer owns it, and
/// the change is announced at the end of the batch (research C3, C7).
pub(crate) fn set(state: &mut ServerState, which: Which, source: Option<Source>) {
    let slot = state.selection.slot_mut(which);
    let new_id = source.as_ref().map(Source::id);
    if new_id.is_some() && slot.owner.as_ref().map(Source::id) == new_id {
        // The owner set again (Firefox does): nothing changes, nothing is announced.
        return;
    }
    let old = std::mem::replace(&mut slot.owner, source);
    slot.bump();
    if let Some(old) = old
        && Some(old.id()) != new_id
    {
        old.cancelled();
    }
}

/// A new device of any protocol: it is told the current content at once if it would be told
/// of changes now (research C3).
pub(crate) fn device_created(state: &mut ServerState, device: Device) {
    let tell = device.is_control() || device.client() == super::seat::keyboard_client(state);
    if tell {
        for which in device_slots(state, &device) {
            tell_device(state, &device, which);
        }
    }
    state.selection.devices.push(device);
}

/// The slots `device` is told about.
fn device_slots(state: &ServerState, device: &Device) -> Vec<Which> {
    let primary = state.selection.primary_enabled;
    match device {
        Device::Data(_) => vec![Which::Clipboard],
        Device::Primary(_) if primary => vec![Which::Primary],
        Device::Primary(_) => Vec::new(),
        Device::ExtControl(_) if primary => vec![Which::Clipboard, Which::Primary],
        Device::WlrControl(d) if primary && d.version() >= 2 => {
            vec![Which::Clipboard, Which::Primary]
        }
        Device::ExtControl(_) | Device::WlrControl(_) => vec![Which::Clipboard],
    }
}

/// Tells `device` the current content of `which`: a new offer with its types, or empty.
fn tell_device(state: &ServerState, device: &Device, which: Which) {
    let owner = state.selection.slot(which).owner.clone();
    let types = owner.as_ref().map(Source::mime_types);
    let data = state.selection.offer_data(which);
    let dh = &state.dh;
    match device {
        Device::Data(d) => {
            let offer = types.and_then(|types| {
                let offer = d
                    .client()?
                    .create_resource::<WlDataOffer, OfferData, ServerState>(dh, d.version(), data)
                    .ok()?;
                d.data_offer(&offer);
                for t in types {
                    offer.offer(t);
                }
                Some(offer)
            });
            d.selection(offer.as_ref());
        }
        Device::Primary(d) => {
            let offer = types.and_then(|types| {
                let offer = d
                    .client()?
                    .create_resource::<ZwpPrimarySelectionOfferV1, OfferData, ServerState>(
                        dh,
                        d.version(),
                        data,
                    )
                    .ok()?;
                d.data_offer(&offer);
                for t in types {
                    offer.offer(t);
                }
                Some(offer)
            });
            d.selection(offer.as_ref());
        }
        Device::ExtControl(d) => {
            let offer = types.and_then(|types| {
                let offer = d
                    .client()?
                    .create_resource::<ExtDataControlOfferV1, OfferData, ServerState>(
                        dh,
                        d.version(),
                        data,
                    )
                    .ok()?;
                d.data_offer(&offer);
                for t in types {
                    offer.offer(t);
                }
                Some(offer)
            });
            match which {
                Which::Clipboard => d.selection(offer.as_ref()),
                Which::Primary => d.primary_selection(offer.as_ref()),
            }
        }
        Device::WlrControl(d) => {
            let offer = types.and_then(|types| {
                let offer = d
                    .client()?
                    .create_resource::<ZwlrDataControlOfferV1, OfferData, ServerState>(
                        dh,
                        d.version(),
                        data,
                    )
                    .ok()?;
                d.data_offer(&offer);
                for t in types {
                    offer.offer(t);
                }
                Some(offer)
            });
            match which {
                Which::Clipboard => d.selection(offer.as_ref()),
                Which::Primary => d.primary_selection(offer.as_ref()),
            }
        }
    }
}

/// The generations of both slots.
fn generations(state: &ServerState) -> (u64, u64) {
    (
        state.selection.clipboard.generation,
        state.selection.primary.generation,
    )
}

/// Tells `client`'s clipboard and primary devices the slots in `slots`.
fn tell_client(state: &ServerState, client: &ClientId, slots: &[Which]) {
    for device in &state.selection.devices {
        if device.is_control() || device.client().as_ref() != Some(client) {
            continue;
        }
        for which in device_slots(state, device) {
            if slots.contains(&which) {
                tell_device(state, device, which);
            }
        }
    }
}

/// The keyboard moved to `client` (or to nothing): it is told both slots now, before its
/// `wl_keyboard.enter` (research C3).
pub(crate) fn focus_changed(state: &mut ServerState, client: Option<ClientId>) {
    let Some(client) = client else {
        state.selection.announced = None;
        return;
    };
    let (clip, primary) = generations(state);
    if state.selection.announced == Some((client.clone(), clip, primary)) {
        return;
    }
    tell_client(state, &client, &[Which::Clipboard, Which::Primary]);
    state.selection.announced = Some((client, clip, primary));
}

/// A device was destroyed or released.
pub(crate) fn device_destroyed(state: &mut ServerState, id: &ObjectId) {
    state.selection.devices.retain(|d| d.id() != *id);
}

/// A source was destroyed (by its client or because the client is gone): a slot it owned
/// becomes empty (research C7).
pub(crate) fn source_destroyed(state: &mut ServerState, id: &ObjectId) {
    for which in [Which::Clipboard, Which::Primary] {
        let slot = state.selection.slot_mut(which);
        if slot.owner.as_ref().is_some_and(|o| o.id() == *id) {
            slot.owner = None;
            slot.bump();
        }
    }
}

/// A paste of `mime` from `offer` into `fd`: handed to the owner if the offer is current and the
/// owner offered that type. RubickWay's copy of `fd` is closed when this returns, whatever
/// happens, so a pasting application never waits on a stale offer (research C5).
pub(crate) fn receive(state: &mut ServerState, offer: OfferData, mime: String, fd: OwnedFd) {
    if !state.selection.is_current(offer) {
        return;
    }
    if let Some(owner) = &state.selection.slot(offer.which).owner
        && owner.mime_types().contains(&mime)
    {
        owner.send(mime, &fd);
    }
}

/// The log line for a slot's new state (research C12): the slot, the owner and how many formats
/// it offers, never their names or the content - type names can hint at secrets.
fn change_line(which: Which, owner: Option<(&str, &[String], bool)>) -> String {
    let slot = match which {
        Which::Clipboard => "clipboard",
        Which::Primary => "primary selection",
    };
    match owner {
        None => format!("{slot} emptied"),
        Some((application, types, tool)) => {
            let tool = if tool { ", a clipboard tool" } else { "" };
            let count = types.len();
            let formats = if count == 1 { "format" } else { "formats" };
            format!("{slot} set by {application}{tool}: {count} {formats}")
        }
    }
}

/// `application N (pid, name)`, as in the connection log line.
fn describe_application(state: &ServerState, client: &Client) -> String {
    format!(
        "application {}{}",
        super::dmabuf::client_number(client),
        super::describe_process(client, &state.dh)
    )
}

/// Logs the slots that changed since the last call, once each (research C12).
fn log_changes(state: &mut ServerState, generations: (u64, u64)) {
    let (logged_clip, logged_primary) = state.selection.logged;
    state.selection.logged = generations;
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    for (which, changed) in [
        (Which::Clipboard, logged_clip != generations.0),
        (Which::Primary, logged_primary != generations.1),
    ] {
        if !changed {
            continue;
        }
        let line = match &state.selection.slot(which).owner {
            None => change_line(which, None),
            Some(owner) => {
                let application = owner
                    .client()
                    .map(|c| describe_application(state, &c))
                    .unwrap_or_else(|| "a gone application".to_string());
                change_line(
                    which,
                    Some((&application, &owner.mime_types(), owner.is_control())),
                )
            }
        };
        tracing::debug!("{line}");
    }
}

/// A request to set a selection was ignored (not in response to the user's input): logged at most
/// once a minute per application (research C12).
pub(crate) fn log_ignored(state: &mut ServerState, client: &Client) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let now = Instant::now();
    let minute = Duration::from_secs(60);
    let logged = &mut state.selection.ignored_logged;
    logged.retain(|_, at| now.duration_since(*at) < minute);
    if logged.contains_key(&client.id()) {
        return;
    }
    logged.insert(client.id(), now);
    let application = describe_application(state, client);
    tracing::debug!("ignored a clipboard request from {application}: not in response to its input");
}

/// Tells devices what changed since the last call (research C3): the focused application the
/// slots it has not seen, clipboard tools every changed slot. Called once per batch, so a flood
/// of changes is told once, with the newest content (research C11).
pub(crate) fn announce(state: &mut ServerState) {
    let (clip, primary) = generations(state);
    log_changes(state, (clip, primary));
    match super::seat::keyboard_client(state) {
        Some(client) => {
            let slots: Vec<Which> = match &state.selection.announced {
                Some((c, seen_clip, seen_primary)) if *c == client => [
                    (Which::Clipboard, *seen_clip != clip),
                    (Which::Primary, *seen_primary != primary),
                ]
                .into_iter()
                .filter_map(|(which, changed)| changed.then_some(which))
                .collect(),
                _ => vec![Which::Clipboard, Which::Primary],
            };
            if !slots.is_empty() {
                tell_client(state, &client, &slots);
            }
            state.selection.announced = Some((client, clip, primary));
        }
        None => state.selection.announced = None,
    }
    let (seen_clip, seen_primary) = state.selection.control_announced;
    let changed: Vec<Which> = [
        (Which::Clipboard, seen_clip != clip),
        (Which::Primary, seen_primary != primary),
    ]
    .into_iter()
    .filter_map(|(which, changed)| changed.then_some(which))
    .collect();
    if !changed.is_empty() {
        for device in state.selection.devices.iter().filter(|d| d.is_control()) {
            for which in device_slots(state, device) {
                if changed.contains(&which) {
                    tell_device(state, device, which);
                }
            }
        }
        state.selection.control_announced = (clip, primary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_kept_once_in_order() {
        let mut data = SourceData::default();
        for mime in ["text/plain", "text/html", "text/plain", "image/png"] {
            data.add_type(mime.to_string());
        }
        assert_eq!(data.mime_types(), ["text/plain", "text/html", "image/png"]);
    }

    #[test]
    fn at_most_256_distinct_types_are_kept() {
        let mut data = SourceData::default();
        for i in 0..300 {
            data.add_type(format!("type/{i}"));
        }
        assert_eq!(data.mime_types().len(), 256);
        assert_eq!(data.mime_types()[255], "type/255");
        // A repeat of a kept type changes nothing either.
        data.add_type("type/0".to_string());
        assert_eq!(data.mime_types().len(), 256);
    }

    #[test]
    fn a_used_source_takes_no_more_types_and_is_used_once() {
        let mut data = SourceData::default();
        data.add_type("text/plain".to_string());
        assert!(data.mark_used(), "the first use");
        assert!(!data.mark_used(), "a second use is refused");
        data.add_type("text/html".to_string());
        assert_eq!(data.mime_types(), ["text/plain"]);
    }

    #[test]
    fn a_log_line_names_the_slot_the_owner_and_the_count_but_no_type() {
        let types = vec![
            "text/plain".to_string(),
            "x-kde-passwordManagerHint".to_string(),
        ];
        let line = change_line(
            Which::Clipboard,
            Some(("application 3 (pid 4321, firefox)", &types, false)),
        );
        assert_eq!(
            line,
            "clipboard set by application 3 (pid 4321, firefox): 2 formats"
        );
        assert!(!line.contains("text/plain") && !line.contains("password"));
        let one = vec!["text/plain".to_string()];
        assert_eq!(
            change_line(
                Which::Primary,
                Some(("application 9 (pid 5012, wl-copy)", &one, true))
            ),
            "primary selection set by application 9 (pid 5012, wl-copy), a clipboard tool: 1 format"
        );
        assert_eq!(change_line(Which::Clipboard, None), "clipboard emptied");
    }

    #[test]
    fn every_change_of_a_slot_makes_older_offers_stale() {
        let mut state = SelectionState::default();
        let offer = state.offer_data(Which::Clipboard);
        assert!(state.is_current(offer));
        state.slot_mut(Which::Clipboard).bump();
        assert!(!state.is_current(offer));
        let newer = state.offer_data(Which::Clipboard);
        assert!(state.is_current(newer));
        // The other slot is independent.
        let primary = state.offer_data(Which::Primary);
        state.slot_mut(Which::Clipboard).bump();
        assert!(state.is_current(primary));
    }
}
