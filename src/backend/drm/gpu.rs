//! Which graphics card to use (research S4): the one the firmware used at boot, else the first
//! with display connectors. Only `seat0` cards are considered.

use std::path::{Path, PathBuf};

use super::DrmError;

/// A card found by udev.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    node: PathBuf,
    boot_vga: bool,
    has_connectors: bool,
}

/// The device node of the card to use, e.g. `/dev/dri/card2`.
pub(crate) fn primary_gpu() -> Result<PathBuf, DrmError> {
    let candidates = scan().map_err(|e| DrmError::Gpu {
        path: "/dev/dri".into(),
        reason: e.to_string(),
    })?;
    pick(&candidates).ok_or(DrmError::NoGpu)
}

fn scan() -> std::io::Result<Vec<Candidate>> {
    let mut e = udev::Enumerator::new()?;
    e.match_subsystem("drm")?;
    let mut found = Vec::new();
    for dev in e.scan_devices()? {
        let name = dev.sysname().to_string_lossy().into_owned();
        // `card2`, not the connector entries `card2-eDP-1` or render nodes.
        let Some(number) = name.strip_prefix("card") else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let seat = dev
            .property_value("ID_SEAT")
            .and_then(|s| s.to_str())
            .unwrap_or("seat0");
        let Some(node) = dev.devnode() else {
            continue;
        };
        if seat != "seat0" {
            continue;
        }
        let boot_vga = dev
            .parent_with_subsystem("pci")
            .ok()
            .flatten()
            .and_then(|p| p.attribute_value("boot_vga").map(|v| v == "1"))
            .unwrap_or(false);
        found.push(Candidate {
            node: node.to_path_buf(),
            boot_vga,
            has_connectors: has_connectors(dev.syspath(), &name),
        });
    }
    Ok(found)
}

/// Whether `/sys/.../cardN` has connector entries `cardN-*`.
fn has_connectors(syspath: &Path, name: &str) -> bool {
    let prefix = format!("{name}-");
    std::fs::read_dir(syspath).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(&prefix))
    })
}

/// The boot GPU if it has connectors, else the first card with connectors, else the boot GPU.
fn pick(candidates: &[Candidate]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|c| c.boot_vga && c.has_connectors)
        .or_else(|| candidates.iter().find(|c| c.has_connectors))
        .or_else(|| candidates.iter().find(|c| c.boot_vga))
        .map(|c| c.node.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(node: &str, boot_vga: bool, has_connectors: bool) -> Candidate {
        Candidate {
            node: PathBuf::from(node),
            boot_vga,
            has_connectors,
        }
    }

    #[test]
    fn the_boot_gpu_with_connectors_wins() {
        // The author's laptop: NVIDIA card1 without connectors, Intel card2 booted with them.
        let found = [
            c("/dev/dri/card1", false, false),
            c("/dev/dri/card2", true, true),
        ];
        assert_eq!(pick(&found), Some(PathBuf::from("/dev/dri/card2")));
    }

    #[test]
    fn without_boot_vga_the_first_card_with_connectors() {
        let found = [
            c("/dev/dri/card0", false, false),
            c("/dev/dri/card1", false, true),
        ];
        assert_eq!(pick(&found), Some(PathBuf::from("/dev/dri/card1")));
        assert_eq!(pick(&[]), None);
    }
}
