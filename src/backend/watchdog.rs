//! The watchdog thread: ends a frozen RubickWay so the session manager gives the console back
//! (research S19, FR-030).
#![allow(unsafe_code)]

use std::time::Duration;

use crate::watchdog::{Heartbeat, expired};

/// How often the thread looks at the heartbeat.
const CHECK: Duration = Duration::from_millis(250);

/// Starts the watchdog: when `heartbeat` is older than `timeout`, one line goes to stderr and
/// the process ends at once with status 1, running no destructors (the frozen loop may hold
/// anything). The session manager then restores the console, as after a kill.
pub fn start(heartbeat: Heartbeat, timeout: Duration) {
    heartbeat.beat();
    let started = std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            loop {
                std::thread::sleep(CHECK);
                if expired(heartbeat.last_ms(), heartbeat.now_ms(), timeout) {
                    eprintln!(
                        "rubickway: the event loop did not respond for {} seconds, exiting",
                        timeout.as_secs()
                    );
                    // SAFETY: `_exit` ends the process immediately; it touches no Rust state,
                    // which is what is wanted when another thread is stuck holding it.
                    unsafe { libc::_exit(1) };
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!("cannot start the watchdog: {err}");
    }
}
