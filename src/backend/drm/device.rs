//! The graphics card: connectors, CRTCs, planes, atomic commits and page flips (research S5).
//!
//! The decisions (which mode, which CRTC, connector names) are pure functions over plain
//! values at the top of this file, unit-tested with made-up resources; the rest talks to the
//! kernel and is checked through quickstart.md.

/// A mode as the decisions need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModeChoice {
    pub preferred: bool,
}

/// The mode to use: the one flagged preferred, else the first (S5).
pub(crate) fn choose_mode(modes: &[ModeChoice]) -> Option<usize> {
    modes
        .iter()
        .position(|m| m.preferred)
        .or((!modes.is_empty()).then_some(0))
}

/// A connected connector and the CRTCs its encoders can drive (bit `i` = CRTC index `i`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConnectorChoice {
    pub id: u32,
    pub possible_crtcs: u32,
}

/// The result of [`assign_crtcs`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Assignment {
    /// `(connector id, CRTC index)` in the order the connectors were given.
    pub assigned: Vec<(u32, usize)>,
    /// Connectors left without a CRTC.
    pub skipped: Vec<u32>,
}

/// Gives each connector, in order, the first free CRTC its encoders allow; a CRTC is never
/// used twice, and CRTCs in `taken` (already driving an output) are not used (S5).
pub(crate) fn assign_crtcs(
    connectors: &[ConnectorChoice],
    crtc_count: usize,
    taken: &[usize],
) -> Assignment {
    let mut used: Vec<usize> = taken.to_vec();
    let mut result = Assignment::default();
    for c in connectors {
        let free = (0..crtc_count.min(32))
            .find(|i| c.possible_crtcs & (1u32 << i) != 0 && !used.contains(i));
        match free {
            Some(i) => {
                used.push(i);
                result.assigned.push((c.id, i));
            }
            None => result.skipped.push(c.id),
        }
    }
    result
}

/// The connector name as the kernel writes it: `eDP-1`, `HDMI-A-1`, `DP-2`.
pub(crate) fn connector_name(interface: &str, index: u32) -> String {
    format!("{interface}-{index}")
}

/// Built-in panels come first in the row (FR-018).
pub(crate) fn is_internal(interface: &str) -> bool {
    matches!(interface, "eDP" | "LVDS" | "DSI")
}

// --- The kernel side ------------------------------------------------------------------------

use std::collections::HashMap;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use drm::control::{
    AtomicCommitFlags, Device as ControlDevice, Mode, ResourceHandle, atomic::AtomicModeReq,
    connector, crtc, framebuffer, plane, property,
};

use super::edid;
use crate::backend::{Mode as OutputMode, OutputInfo};

/// The card's fd, as the `drm` and `gbm` crates want it.
pub(crate) struct Card(pub OwnedFd);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

/// The `DRM_PLANE_TYPE_PRIMARY` value of a plane's `type` property.
const PLANE_TYPE_PRIMARY: u64 = 1;

/// Property handles an output's commits use (S5).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Props {
    conn_crtc_id: property::Handle,
    mode_id: property::Handle,
    active: property::Handle,
    fb_id: property::Handle,
    plane_crtc_id: property::Handle,
    src: [property::Handle; 4],
    dst: [property::Handle; 4],
}

/// A connected display ready to drive: its connector, CRTC, primary plane and mode.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub connector: connector::Handle,
    pub crtc: crtc::Handle,
    pub plane: plane::Handle,
    pub mode: Mode,
    pub info: OutputInfo,
    pub props: Props,
}

/// Enables the capabilities atomic modesetting needs (S5).
pub(crate) fn enable_atomic<D: ControlDevice>(dev: &D) -> io::Result<()> {
    dev.set_client_capability(drm::ClientCapability::UniversalPlanes, true)?;
    dev.set_client_capability(drm::ClientCapability::Atomic, true)
}

/// Property handles of one object, by name.
fn property_handles<D: ControlDevice, H: ResourceHandle>(
    dev: &D,
    handle: H,
) -> io::Result<HashMap<String, (property::Handle, property::RawValue)>> {
    let set = dev.get_properties(handle)?;
    let (handles, values) = set.as_props_and_values();
    let mut map = HashMap::new();
    for (h, v) in handles.iter().zip(values) {
        if let Ok(info) = dev.get_property(*h) {
            map.insert(info.name().to_string_lossy().into_owned(), (*h, *v));
        }
    }
    Ok(map)
}

fn prop(
    map: &HashMap<String, (property::Handle, property::RawValue)>,
    name: &str,
) -> io::Result<property::Handle> {
    map.get(name)
        .map(|(h, _)| *h)
        .ok_or_else(|| io::Error::other(format!("no `{name}` property")))
}

/// Reads the connected displays and gives each a CRTC and primary plane, skipping CRTCs in
/// `taken` (S5, S13). Connectors are in the card's order (FR-018).
pub(crate) fn scan<D: ControlDevice>(dev: &D, taken: &[crtc::Handle]) -> io::Result<Vec<Found>> {
    let res = dev.resource_handles()?;
    let crtcs = res.crtcs();
    let mut connected = Vec::new();
    let mut choices = Vec::new();
    for &handle in res.connectors() {
        let Ok(info) = dev.get_connector(handle, true) else {
            continue;
        };
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let mut mask = 0u32;
        for &enc in info.encoders() {
            if let Ok(e) = dev.get_encoder(enc) {
                for c in res.filter_crtcs(e.possible_crtcs()) {
                    if let Some(i) = crtcs.iter().position(|x| *x == c) {
                        mask |= 1u32 << i.min(31);
                    }
                }
            }
        }
        choices.push(ConnectorChoice {
            id: u32::from(handle),
            possible_crtcs: mask,
        });
        connected.push(info);
    }
    let taken_idx: Vec<usize> = taken
        .iter()
        .filter_map(|t| crtcs.iter().position(|c| c == t))
        .collect();
    let assignment = assign_crtcs(&choices, crtcs.len(), &taken_idx);
    for id in &assignment.skipped {
        tracing::warn!("display connector {id} has no free CRTC; it stays dark");
    }
    let planes = dev.plane_handles()?;
    let mut used_planes: Vec<plane::Handle> = Vec::new();
    let mut found = Vec::new();
    for (conn_id, crtc_idx) in assignment.assigned {
        let Some(info) = connected.iter().find(|c| u32::from(c.handle()) == conn_id) else {
            continue;
        };
        let Some(&crtc) = crtcs.get(crtc_idx) else {
            continue;
        };
        let Some(plane) = primary_plane(dev, &res, &planes, crtc, &used_planes) else {
            tracing::warn!("no primary plane for CRTC {crtc:?}; display stays dark");
            continue;
        };
        used_planes.push(plane);
        let choices: Vec<ModeChoice> = info
            .modes()
            .iter()
            .map(|m| ModeChoice {
                preferred: m
                    .mode_type()
                    .contains(drm::control::ModeTypeFlags::PREFERRED),
            })
            .collect();
        let Some(mode) = choose_mode(&choices)
            .and_then(|i| info.modes().get(i))
            .copied()
        else {
            continue;
        };
        let conn_props = property_handles(dev, info.handle())?;
        let crtc_props = property_handles(dev, crtc)?;
        let plane_props = property_handles(dev, plane)?;
        let props = Props {
            conn_crtc_id: prop(&conn_props, "CRTC_ID")?,
            mode_id: prop(&crtc_props, "MODE_ID")?,
            active: prop(&crtc_props, "ACTIVE")?,
            fb_id: prop(&plane_props, "FB_ID")?,
            plane_crtc_id: prop(&plane_props, "CRTC_ID")?,
            src: [
                prop(&plane_props, "SRC_X")?,
                prop(&plane_props, "SRC_Y")?,
                prop(&plane_props, "SRC_W")?,
                prop(&plane_props, "SRC_H")?,
            ],
            dst: [
                prop(&plane_props, "CRTC_X")?,
                prop(&plane_props, "CRTC_Y")?,
                prop(&plane_props, "CRTC_W")?,
                prop(&plane_props, "CRTC_H")?,
            ],
        };
        let edid = conn_props
            .get("EDID")
            .filter(|(_, blob)| *blob != 0)
            .and_then(|(_, blob)| dev.get_property_blob(*blob).ok())
            .map(|bytes| edid::parse(&bytes))
            .unwrap_or_else(|| edid::parse(&[]));
        let interface = info.interface().as_str();
        let (w, h) = mode.size();
        found.push(Found {
            connector: info.handle(),
            crtc,
            plane,
            mode,
            info: OutputInfo {
                name: connector_name(interface, info.interface_id()),
                make: edid.make,
                model: edid.model,
                physical_mm: info.size().unwrap_or((0, 0)),
                mode: OutputMode {
                    width: u32::from(w),
                    height: u32::from(h),
                    refresh_mhz: refresh_mhz(&mode),
                },
                internal: is_internal(interface),
            },
            props,
        });
    }
    Ok(found)
}

/// The connectors that are connected now (as last probed by the kernel; a hotplug event has
/// just probed them), as raw ids.
pub(crate) fn connected<D: ControlDevice>(dev: &D) -> io::Result<Vec<u32>> {
    let res = dev.resource_handles()?;
    Ok(res
        .connectors()
        .iter()
        .filter(|&&handle| {
            dev.get_connector(handle, false).is_ok_and(|info| {
                info.state() == connector::State::Connected && !info.modes().is_empty()
            })
        })
        .map(|&handle| u32::from(handle))
        .collect())
}

/// The mode's refresh rate in millihertz, from its timings when they are known.
fn refresh_mhz(mode: &Mode) -> u32 {
    let (htotal, vtotal) = (u64::from(mode.hsync().2), u64::from(mode.vsync().2));
    let total = htotal.saturating_mul(vtotal);
    if total == 0 {
        return mode.vrefresh().saturating_mul(1000);
    }
    let mhz = u64::from(mode.clock()).saturating_mul(1_000_000) / total;
    u32::try_from(mhz).unwrap_or(u32::MAX)
}

/// The primary plane that can drive `crtc` and is not used yet.
fn primary_plane<D: ControlDevice>(
    dev: &D,
    res: &drm::control::ResourceHandles,
    planes: &[plane::Handle],
    crtc: crtc::Handle,
    used: &[plane::Handle],
) -> Option<plane::Handle> {
    planes.iter().copied().find(|&p| {
        !used.contains(&p)
            && dev
                .get_plane(p)
                .is_ok_and(|info| res.filter_crtcs(info.possible_crtcs()).contains(&crtc))
            && property_handles(dev, p)
                .ok()
                .and_then(|m| m.get("type").map(|(_, v)| *v))
                == Some(PLANE_TYPE_PRIMARY)
    })
}

/// Shows framebuffer `fb` on the output; with `modeset`, sets the mode too. Always
/// non-blocking with a page-flip event (S5).
pub(crate) fn commit<D: ControlDevice>(
    dev: &D,
    out: &Found,
    fb: framebuffer::Handle,
    modeset: bool,
) -> io::Result<()> {
    let mut req = AtomicModeReq::new();
    let mut flags = AtomicCommitFlags::NONBLOCK | AtomicCommitFlags::PAGE_FLIP_EVENT;
    if modeset {
        flags |= AtomicCommitFlags::ALLOW_MODESET;
        let blob = dev.create_property_blob(&out.mode)?;
        let p = &out.props;
        req.add_property(
            out.connector,
            p.conn_crtc_id,
            property::Value::CRTC(Some(out.crtc)),
        );
        req.add_property(out.crtc, p.mode_id, blob);
        req.add_property(out.crtc, p.active, property::Value::Boolean(true));
        req.add_property(
            out.plane,
            p.plane_crtc_id,
            property::Value::CRTC(Some(out.crtc)),
        );
        let (w, h) = out.mode.size();
        let (w, h) = (u64::from(w), u64::from(h));
        // Source rectangle in 16.16 fixed point, destination in pixels.
        for (handle, value) in p.src.iter().zip([0, 0, w << 16, h << 16]) {
            req.add_property(out.plane, *handle, property::Value::UnsignedRange(value));
        }
        for (handle, value) in p.dst.iter().zip([0, 0, w, h]) {
            req.add_property(out.plane, *handle, property::Value::UnsignedRange(value));
        }
    }
    req.add_property(
        out.plane,
        out.props.fb_id,
        property::Value::Framebuffer(Some(fb)),
    );
    dev.atomic_commit(flags, req)
}

/// Turns an output off (disconnect, exit). Blocking only when `block` (the final commit on
/// exit, after the loop ended).
pub(crate) fn disable<D: ControlDevice>(dev: &D, out: &Found, block: bool) -> io::Result<()> {
    let mut req = AtomicModeReq::new();
    let p = &out.props;
    req.add_property(out.connector, p.conn_crtc_id, property::Value::CRTC(None));
    req.add_property(out.crtc, p.active, property::Value::Boolean(false));
    req.add_property(out.crtc, p.mode_id, property::Value::Unknown(0));
    req.add_property(out.plane, p.fb_id, property::Value::Framebuffer(None));
    req.add_property(out.plane, p.plane_crtc_id, property::Value::CRTC(None));
    let mut flags = AtomicCommitFlags::ALLOW_MODESET;
    if !block {
        flags |= AtomicCommitFlags::NONBLOCK;
    }
    dev.atomic_commit(flags, req)
}

/// The CRTCs whose page flips completed since the last read.
pub(crate) fn flipped<D: ControlDevice>(dev: &D) -> io::Result<Vec<crtc::Handle>> {
    Ok(dev
        .receive_events()?
        .filter_map(|e| match e {
            drm::control::Event::PageFlip(flip) => Some(flip.crtc),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(preferred: bool) -> ModeChoice {
        ModeChoice { preferred }
    }

    #[test]
    fn the_preferred_mode_wins_else_the_first() {
        assert_eq!(
            choose_mode(&[mode(false), mode(true), mode(false)]),
            Some(1)
        );
        assert_eq!(choose_mode(&[mode(false), mode(false)]), Some(0));
        assert_eq!(choose_mode(&[]), None);
    }

    #[test]
    fn crtcs_are_assigned_in_order_and_never_twice() {
        let connectors = [
            ConnectorChoice {
                id: 10,
                possible_crtcs: 0b011,
            },
            ConnectorChoice {
                id: 11,
                possible_crtcs: 0b011,
            },
            ConnectorChoice {
                id: 12,
                possible_crtcs: 0b011,
            },
        ];
        let a = assign_crtcs(&connectors, 3, &[]);
        assert_eq!(a.assigned, vec![(10, 0), (11, 1)]);
        assert_eq!(a.skipped, vec![12]);
    }

    #[test]
    fn taken_crtcs_and_encoder_limits_are_respected() {
        let connectors = [ConnectorChoice {
            id: 20,
            possible_crtcs: 0b110,
        }];
        assert_eq!(assign_crtcs(&connectors, 3, &[1]).assigned, vec![(20, 2)]);
        assert_eq!(assign_crtcs(&connectors, 3, &[1, 2]).skipped, vec![20]);
        // A mask bit beyond the CRTCs that exist is not a CRTC.
        assert_eq!(assign_crtcs(&connectors, 1, &[]).skipped, vec![20]);
    }

    #[test]
    fn connector_names_and_internal_panels() {
        assert_eq!(connector_name("eDP", 1), "eDP-1");
        assert_eq!(connector_name("HDMI-A", 1), "HDMI-A-1");
        assert_eq!(connector_name("DP", 2), "DP-2");
        assert!(is_internal("eDP"));
        assert!(is_internal("LVDS"));
        assert!(is_internal("DSI"));
        assert!(!is_internal("HDMI-A"));
        assert!(!is_internal("DP"));
    }
}
