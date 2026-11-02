//! Protocol tests: the real server, in-process, with `wayland-client` test clients.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test harness helpers fail loudly; clippy.toml only exempts #[test] functions"
)]

mod harness;
mod input;
mod misbehaving;
mod surfaces;
mod wm;

use harness::Harness;

#[test]
fn a_client_connects_and_lists_globals() {
    let mut h = Harness::new();
    let c = h.connect();
    assert!(!h.c(c).is_disconnected());
}
