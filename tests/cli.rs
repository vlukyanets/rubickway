//! The command line stays as it is (FR-027, contracts/cli.md "Invocation"): any argument is a
//! usage error that never touches the seat or the display.

use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn any_argument_is_a_usage_error() {
    for args in [&["foo"][..], &["--help"][..], &["a", "b"][..]] {
        let started = Instant::now();
        let out = Command::new(env!("CARGO_BIN_EXE_rubickway"))
            .args(args)
            .output()
            .expect("runs");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{args:?} took too long"
        );
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), "usage: rubickway\n");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}
