//! Running out of file descriptors, told once when it starts and once when it ends (FR-015,
//! research G10).

use std::collections::HashMap;
use std::io;

use super::ServerState;
use super::dmabuf::client_number;
use super::shm::BufferKind;

/// Whether descriptors have run out.
#[derive(Default)]
pub(crate) struct DescriptorWatch {
    exhausted: bool,
}

impl DescriptorWatch {
    /// One more failure; whether it is the first since they ran out.
    fn failed(&mut self) -> bool {
        !std::mem::replace(&mut self.exhausted, true)
    }

    /// One could be opened; whether that ends a shortage.
    fn recovered(&mut self) -> bool {
        std::mem::replace(&mut self.exhausted, false)
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        self.exhausted
    }
}

/// Whether `err` says the process or the system has no descriptor left.
fn is_exhaustion(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::EMFILE | libc::ENFILE))
}

/// Who holds the most, of those named once per thing held, and how many. Ties go to the lowest number,
/// so the line does not depend on the order of a map.
fn most_held(holders: impl IntoIterator<Item = u64>) -> Option<(u64, usize)> {
    let mut counts = HashMap::<u64, usize>::new();
    for holder in holders {
        *counts.entry(holder).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(holder, count)| (count, std::cmp::Reverse(holder)))
}

fn started_line(most: Option<(u64, usize)>) -> String {
    let who = match most {
        Some((application, count)) => {
            format!("application {application} holds the most GPU images ({count})")
        }
        None => "no application holds GPU images".to_string(),
    };
    format!("out of file descriptors: applications may stall and new ones cannot connect; {who}")
}

/// Notes a failure to get a descriptor, logging it if it starts a shortage. Returns whether `err` was
/// such a failure: anything else the caller still has to tell.
pub(crate) fn note_failure(state: &mut ServerState, err: &io::Error) -> bool {
    if !is_exhaustion(err) {
        return false;
    }
    if state.descriptors.failed() {
        let line = started_line(most_images(state));
        tracing::warn!("{line}");
    }
    true
}

/// The application holding the most GPU images, and how many.
pub(crate) fn most_images(state: &ServerState) -> Option<(u64, usize)> {
    most_held(
        state
            .buffers
            .iter()
            .filter(|(_, kind)| matches!(kind, BufferKind::Gpu(_)))
            .filter_map(|(id, _)| state.dh.get_client(id.clone()).ok())
            .map(|client| client_number(&client)),
    )
}

/// During a shortage: logs its end once a descriptor can be opened.
pub(crate) fn check(state: &mut ServerState) {
    if state.descriptors.is_exhausted()
        && crate::ffi::dmabuf::eventfd().is_ok()
        && state.descriptors.recovered()
    {
        tracing::warn!("file descriptors are available again");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortage_starts_and_ends_once() {
        let mut watch = DescriptorWatch::default();
        assert!(!watch.is_exhausted());
        assert!(!watch.recovered(), "nothing to end yet");
        assert!(watch.failed(), "it starts");
        assert!(!watch.failed(), "and is not told again");
        assert!(watch.is_exhausted());
        assert!(watch.recovered(), "it ends");
        assert!(!watch.recovered(), "and is not told again");
        assert!(watch.failed(), "a new one starts");
    }

    #[test]
    fn only_no_descriptor_left_counts_as_a_shortage() {
        for errno in [libc::EMFILE, libc::ENFILE] {
            assert!(is_exhaustion(&io::Error::from_raw_os_error(errno)));
        }
        for errno in [libc::EAGAIN, libc::ENOMEM, libc::EBADF] {
            assert!(!is_exhaustion(&io::Error::from_raw_os_error(errno)));
        }
        assert!(!is_exhaustion(&io::Error::other("no number")));
    }

    #[test]
    fn the_one_holding_the_most_is_named_and_ties_go_to_the_lowest_number() {
        assert_eq!(most_held([]), None);
        assert_eq!(most_held([4, 7, 7, 4, 7]), Some((7, 3)));
        assert_eq!(most_held([7, 4, 4, 7]), Some((4, 2)));
    }

    #[test]
    fn the_lines_are_those_of_the_contract() {
        assert_eq!(
            started_line(Some((7, 300))),
            "out of file descriptors: applications may stall and new ones cannot connect; \
             application 7 holds the most GPU images (300)"
        );
        assert_eq!(
            started_line(None),
            "out of file descriptors: applications may stall and new ones cannot connect; \
             no application holds GPU images"
        );
    }
}
