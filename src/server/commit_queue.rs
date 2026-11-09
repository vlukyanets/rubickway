//! A surface's commits waiting for their images (research G7). Pure: it knows nothing of
//! surfaces, buffers or the event loop.
//!
//! One queue per commit root: the nearest desynchronised subsurface or the toplevel root. A
//! transaction applies when its own waits are done and everything before it has applied, so a
//! later image that finishes first never overtakes an earlier one (FR-008). With nothing to wait
//! for and an empty queue it applies at once, so shared-memory timing does not change.

use std::collections::VecDeque;

/// Something a transaction waits for: an import, or an image that is not finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WaitId(pub u64);

struct Entry<T> {
    item: T,
    waits: Vec<WaitId>,
}

/// The queue of one commit root. It has no length limit (research G10).
pub(crate) struct CommitQueue<T> {
    entries: VecDeque<Entry<T>>,
}

impl<T> Default for CommitQueue<T> {
    fn default() -> Self {
        CommitQueue {
            entries: VecDeque::new(),
        }
    }
}

impl<T> CommitQueue<T> {
    /// Adds `item` last; returns what is ready to apply now, in order.
    pub fn push(&mut self, item: T, waits: Vec<WaitId>) -> Vec<T> {
        self.entries.push_back(Entry { item, waits });
        self.take_ready()
    }

    /// Everything still waited for.
    pub fn waits(&self) -> impl Iterator<Item = WaitId> + '_ {
        self.entries.iter().flat_map(|e| e.waits.iter().copied())
    }

    /// `wait` finished; returns what is ready to apply now, in order. An unknown wait is ignored.
    pub fn done(&mut self, wait: WaitId) -> Vec<T> {
        for entry in &mut self.entries {
            entry.waits.retain(|w| *w != wait);
        }
        self.take_ready()
    }

    fn take_ready(&mut self) -> Vec<T> {
        let mut ready = Vec::new();
        while let Some(front) = self.entries.front()
            && front.waits.is_empty()
        {
            if let Some(entry) = self.entries.pop_front() {
                ready.push(entry.item);
            }
        }
        ready
    }

    /// Lets the caller edit or drop waiting items (a destroyed surface's parts).
    pub fn retain_mut(&mut self, mut keep: impl FnMut(&mut T) -> bool) {
        self.entries.retain_mut(|e| keep(&mut e.item));
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const A: WaitId = WaitId(1);
    const B: WaitId = WaitId(2);

    #[test]
    fn it_tells_what_is_still_waited_for() {
        let mut q = CommitQueue::default();
        q.push("a", vec![A]);
        q.push("b", vec![B]);
        assert_eq!(q.waits().collect::<Vec<_>>(), [A, B]);
        q.done(B);
        assert_eq!(q.waits().collect::<Vec<_>>(), [A]);
    }

    #[test]
    fn nothing_to_wait_for_on_an_empty_queue_applies_at_once() {
        let mut q = CommitQueue::default();
        assert_eq!(q.push("a", vec![]), vec!["a"]);
        assert!(q.is_empty());
    }

    #[test]
    fn a_waiting_transaction_applies_when_its_wait_is_done() {
        let mut q = CommitQueue::default();
        assert!(q.push("a", vec![A]).is_empty());
        assert!(!q.is_empty());
        assert_eq!(q.done(A), vec!["a"]);
        assert!(q.is_empty());
    }

    #[test]
    fn every_wait_of_a_transaction_has_to_finish() {
        let mut q = CommitQueue::default();
        assert!(q.push("a", vec![A, B]).is_empty());
        assert!(q.done(B).is_empty());
        assert_eq!(q.done(A), vec!["a"]);
    }

    #[test]
    fn a_later_transaction_never_overtakes_an_earlier_one() {
        let mut q = CommitQueue::default();
        assert!(q.push("a", vec![A]).is_empty());
        assert!(q.push("b", vec![B]).is_empty());
        assert!(
            q.push("c", vec![]).is_empty(),
            "nothing to wait for, but behind"
        );
        assert!(q.done(B).is_empty(), "b finished first");
        assert_eq!(q.done(A), vec!["a", "b", "c"]);
    }

    #[test]
    fn an_unknown_wait_is_ignored() {
        let mut q = CommitQueue::default();
        assert!(q.push("a", vec![A]).is_empty());
        assert!(q.done(B).is_empty());
        assert!(!q.is_empty());
    }

    #[test]
    fn dropping_a_waiting_item_lets_the_rest_go_when_their_waits_finish() {
        let mut q = CommitQueue::default();
        assert!(q.push("a", vec![A]).is_empty());
        assert!(q.push("b", vec![B]).is_empty());
        q.retain_mut(|item| *item != "a");
        assert_eq!(q.done(B), vec!["b"]);
        assert!(q.is_empty());
    }

    proptest! {
        /// Whatever order the waits finish in, transactions come out in the order they went in,
        /// each only after its own waits and all earlier ones.
        #[test]
        fn order_is_kept_for_any_finishing_order(
            finish in Just((0u64..12).collect::<Vec<_>>()).prop_shuffle(),
            with_wait in proptest::collection::vec(any::<bool>(), 12),
        ) {
            let mut q = CommitQueue::default();
            let mut applied = Vec::new();
            for (i, wait) in with_wait.iter().enumerate() {
                let waits = if *wait { vec![WaitId(i as u64)] } else { vec![] };
                applied.extend(q.push(i, waits));
            }
            let mut finished = Vec::new();
            for f in finish {
                finished.push(f as usize);
                applied.extend(q.done(WaitId(f)));
                for (position, item) in applied.iter().enumerate() {
                    prop_assert_eq!(position, *item, "applied out of order");
                    prop_assert!(!with_wait[*item] || finished.contains(item));
                }
            }
            prop_assert!(q.is_empty());
            prop_assert_eq!(applied.len(), 12);
        }
    }
}
