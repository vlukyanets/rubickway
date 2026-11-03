//! Display hotplug through a udev monitor (research S13): a change event of the card asks the
//! backend to re-read its connectors and compare them with the displays it drives.

use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

/// The driven connectors that are no longer connected, in the order they are driven. Every
/// rescan compares with the current state, so fast plug and unplug sequences end right
/// (spec Edge Cases).
pub(crate) fn gone(driven: &[u32], connected: &[u32]) -> Vec<u32> {
    driven
        .iter()
        .copied()
        .filter(|c| !connected.contains(c))
        .collect()
}

/// The udev monitor on the `drm` subsystem, reporting changes of one card.
pub(crate) struct Hotplug {
    socket: udev::MonitorSocket,
    /// The card's name in sysfs, e.g. `card2`.
    card: OsString,
}

impl Hotplug {
    pub fn new(card_path: &Path) -> io::Result<Hotplug> {
        let socket = udev::MonitorBuilder::new()?
            .match_subsystem("drm")?
            .listen()?;
        let card = card_path.file_name().unwrap_or_default().to_os_string();
        Ok(Hotplug { socket, card })
    }

    /// A duplicate of the monitor's fd, for the event loop.
    pub fn fd(&self) -> io::Result<OwnedFd> {
        self.socket.as_fd().try_clone_to_owned()
    }

    /// Reads every pending event; whether one was a change of the card.
    pub fn changed(&mut self) -> bool {
        let mut changed = false;
        for event in self.socket.iter() {
            if event.event_type() == udev::EventType::Change && event.sysname() == self.card {
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_driven_connectors_that_went_away_are_gone() {
        assert_eq!(gone(&[40, 50], &[40, 50, 60]), Vec::<u32>::new());
        assert_eq!(gone(&[40, 50], &[40]), vec![50]);
        assert_eq!(gone(&[40, 50], &[]), vec![40, 50]);
        assert_eq!(gone(&[], &[40]), Vec::<u32>::new());
    }

    #[test]
    fn unplug_and_replug_between_rescans_leaves_it_driven() {
        // The monitor was unplugged and plugged back before the rescan: still connected.
        assert!(gone(&[50], &[50]).is_empty());
    }
}
