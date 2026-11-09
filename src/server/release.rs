//! When a replaced GPU image can go back to its application (research G9). Pure.
//!
//! An image is read during frames, and a frame may be in flight on any display. So a replaced
//! image goes back once no frame in flight is one that may have read it: every frame numbered up
//! to the one drawn last when it was replaced.

/// Images waiting to go back, with the frame drawn last when each was replaced.
pub(crate) struct ReleaseTracker<T> {
    entries: Vec<(T, u64)>,
}

impl<T> Default for ReleaseTracker<T> {
    fn default() -> Self {
        ReleaseTracker {
            entries: Vec::new(),
        }
    }
}

impl<T> ReleaseTracker<T> {
    /// `buffer` was replaced; `seq` is the newest frame drawn so far.
    pub fn retire(&mut self, buffer: T, seq: u64) {
        self.entries.push((buffer, seq));
    }

    /// What can go back now, given the oldest frame still in flight (`None`: nothing is).
    pub fn release_ready(&mut self, oldest_in_flight: Option<u64>) -> Vec<T> {
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|(_, seq)| oldest_in_flight.is_none_or(|oldest| *seq < oldest));
        self.entries = waiting;
        ready.into_iter().map(|(buffer, _)| buffer).collect()
    }

    /// Takes back entries `keep` says no to: a buffer attached again is in use again.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.entries.retain(|(buffer, _)| keep(buffer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_nothing_in_flight_everything_goes_back() {
        let mut t = ReleaseTracker::default();
        t.retire("a", 5);
        t.retire("b", 9);
        assert_eq!(t.release_ready(None), vec!["a", "b"]);
        assert!(t.release_ready(None).is_empty());
    }

    #[test]
    fn a_frame_that_may_have_read_it_holds_it_back() {
        let mut t = ReleaseTracker::default();
        t.retire("a", 5);
        assert!(t.release_ready(Some(5)).is_empty(), "frame 5 may read it");
        assert!(t.release_ready(Some(3)).is_empty(), "so may an older one");
        assert_eq!(
            t.release_ready(Some(6)),
            vec!["a"],
            "frame 6 was drawn without"
        );
    }

    #[test]
    fn only_those_ready_go() {
        let mut t = ReleaseTracker::default();
        t.retire("a", 2);
        t.retire("b", 7);
        assert_eq!(t.release_ready(Some(4)), vec!["a"]);
        assert_eq!(t.release_ready(Some(8)), vec!["b"]);
    }

    #[test]
    fn a_buffer_attached_again_does_not_go_back() {
        let mut t = ReleaseTracker::default();
        t.retire("a", 2);
        t.retain(|b| *b != "a");
        assert!(t.release_ready(None).is_empty());
    }
}
