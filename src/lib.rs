//! RubickWay: a Wayland compositor with an endless canvas where every monitor is a viewer.

pub mod backend;
pub mod canvas;
pub mod config;
pub mod distant;
pub mod ffi;
pub mod input;
pub mod render;
pub mod server;
pub mod shell;
pub mod standalone;
pub mod state;
pub mod viewer;
pub mod watchdog;

use std::panic::{self, AssertUnwindSafe};
use std::process::ExitCode;
use std::sync::Mutex;

use crate::state::AppError;

/// The message and location of the latest panic, recorded by the hook of
/// `run_reporting_panics`.
static PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Runs `f` (the application) and turns its end into the exit code, reporting failures on one
/// stderr line as the last line written (FR-013, FR-017, research S20):
/// `rubickway: <error>` for an error, `rubickway: internal error: <message> at <file>:<line>`
/// for a panic. A panic unwinds out of `f` first, so the backend is dropped and the console
/// given back before the line is written.
pub fn run_reporting_panics(f: impl FnOnce() -> Result<(), AppError>) -> ExitCode {
    panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".to_owned());
        let at = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        if let Ok(mut slot) = PANIC.lock() {
            *slot = Some(format!("{message}{at}"));
        }
    }));
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(err)) => {
            eprintln!("rubickway: {err}");
            ExitCode::FAILURE
        }
        Err(_) => {
            let what = PANIC
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .unwrap_or_else(|| "a panic".to_owned());
            eprintln!("rubickway: internal error: {what}");
            ExitCode::FAILURE
        }
    }
}
