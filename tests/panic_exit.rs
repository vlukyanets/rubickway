//! A panic ends RubickWay with one line and status 1, in a child process (FR-017, S20).
#![expect(
    clippy::expect_used,
    reason = "the helper that starts the child process fails the test when it cannot"
)]

use std::process::{Command, ExitCode};

/// Set only by the tests below for the child they start: which case the child runs.
const CHILD: &str = "RUBICKWAY_PANIC_TEST";

/// In the child: `run_reporting_panics` over a function that panics, or one that fails.
#[test]
fn reporting_child() {
    let code: ExitCode = match std::env::var(CHILD).as_deref() {
        Ok("panic") => rubickway::run_reporting_panics(|| panic!("the canvas fell over")),
        Ok("error") => rubickway::run_reporting_panics(|| {
            Err(rubickway::state::AppError::Loop("it broke".into()))
        }),
        _ => return,
    };
    // Leave with the code the binary would return, bypassing the test harness.
    let status = if code == ExitCode::SUCCESS { 0 } else { 1 };
    std::process::exit(status);
}

fn run_child(case: &str) -> (Option<i32>, String) {
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "reporting_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, case)
        .output()
        .expect("child runs");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_panic_is_reported_on_one_last_line_with_status_1() {
    let (code, stderr) = run_child("panic");
    assert_eq!(code, Some(1), "{stderr}");
    let last = stderr.lines().last().unwrap_or_default();
    assert!(
        last.starts_with("rubickway: internal error: the canvas fell over at tests/panic_exit.rs:"),
        "{stderr}"
    );
    assert_eq!(
        stderr.matches("the canvas fell over").count(),
        1,
        "the default panic message is not printed: {stderr}"
    );
}

#[test]
fn an_error_is_reported_on_one_last_line_with_status_1() {
    let (code, stderr) = run_child("error");
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(
        stderr.lines().last(),
        Some("rubickway: event loop: it broke"),
        "{stderr}"
    );
}
