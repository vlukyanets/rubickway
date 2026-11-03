//! The real watchdog thread and its exit, in a child process (research S19, FR-030).

use std::process::Command;
use std::time::{Duration, Instant};

use rubickway::watchdog::Heartbeat;

/// Set only by `a_stalled_loop_ends_the_process` for the child it starts.
const CHILD: &str = "RUBICKWAY_WATCHDOG_TEST";

/// In the child: a watchdog with a 1-second timeout over a loop that never beats again.
#[test]
fn stalled_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    rubickway::backend::watchdog::start(Heartbeat::new(), Duration::from_secs(1));
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn a_stalled_loop_ends_the_process_within_the_timeout() {
    let started = Instant::now();
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "stalled_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .expect("child runs");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("rubickway: the event loop did not respond for 1 seconds, exiting"),
        "{stderr}"
    );
}
