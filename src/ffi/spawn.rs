//! Starting programs: a thread per command, signals unblocked and a new session in the child
//! (research S18; reference: niri `src/utils/spawning.rs`).
//!
//! The event loop's signal source blocks SIGINT, SIGTERM and SIGHUP, and a child inherits the
//! mask, so the child unblocks everything before `exec`. `setsid` detaches it from the text
//! console RubickWay was started on. The thread waits for the child, so no finished child
//! stays behind, and reports failures back.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

/// How a started program ended, when that is worth logging (`contracts/cli.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnResult {
    /// It could not be started.
    Failed { command: String, error: String },
    /// It ended unsuccessfully: "exited with status 3", "was killed by signal 9".
    Ended { command: String, how: String },
}

/// What to start and how.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Program and arguments, run without a shell; must not be empty.
    pub command: Vec<String>,
    /// Added to RubickWay's own environment (`WAYLAND_DISPLAY`).
    pub env: Vec<(OsString, OsString)>,
    /// The working directory (the user's home), or RubickWay's if `None`.
    pub dir: Option<PathBuf>,
}

/// Starts `request` on its own thread; `report` gets failures and unsuccessful exits (a
/// successful exit is not reported). Never blocks the caller (Principle VII).
pub fn spawn(request: SpawnRequest, report: impl FnOnce(SpawnResult) + Send + 'static) {
    let name = request.command.join(" ");
    let started = std::thread::Builder::new()
        .name("spawn".into())
        .spawn(move || run(request, report));
    if let Err(err) = started {
        tracing::warn!("cannot start {name}: no thread ({err})");
    }
}

fn run(request: SpawnRequest, report: impl FnOnce(SpawnResult)) {
    let name = request.command.join(" ");
    let Some((program, args)) = request.command.split_first() else {
        report(SpawnResult::Failed {
            command: name,
            error: "empty command".into(),
        });
        return;
    };
    let mut cmd = Command::new(program);
    cmd.args(args).envs(request.env).stdin(Stdio::null());
    if let Some(dir) = request.dir {
        cmd.current_dir(dir);
    }
    // SAFETY: the closure runs in the child between fork and exec and calls only
    // async-signal-safe functions (sigemptyset, pthread_sigmask, setsid) on a local sigset; it
    // allocates nothing and touches no locks.
    unsafe {
        cmd.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Err(err) => report(SpawnResult::Failed {
            command: name,
            error: err.to_string(),
        }),
        Ok(mut child) => match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => report(SpawnResult::Ended {
                command: name,
                how: describe(status),
            }),
            Err(err) => report(SpawnResult::Failed {
                command: name,
                error: format!("waiting for it failed: {err}"),
            }),
        },
    }
}

fn describe(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with status {code}"),
        (None, Some(signal)) => format!("was killed by signal {signal}"),
        (None, None) => "ended".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn run_and_collect(command: &[&str], env: Vec<(OsString, OsString)>) -> Vec<SpawnResult> {
        let (tx, rx) = mpsc::channel();
        spawn(
            SpawnRequest {
                command: command.iter().map(|s| s.to_string()).collect(),
                env,
                dir: Some(PathBuf::from("/")),
            },
            move |r| {
                let _ = tx.send(r);
            },
        );
        // The sender is dropped when the thread ends: collect until then.
        let mut out = Vec::new();
        while let Ok(r) = rx.recv_timeout(Duration::from_secs(10)) {
            out.push(r);
        }
        out
    }

    #[test]
    fn an_unsuccessful_exit_is_reported() {
        assert_eq!(
            run_and_collect(&["sh", "-c", "exit 3"], Vec::new()),
            vec![SpawnResult::Ended {
                command: "sh -c exit 3".into(),
                how: "exited with status 3".into()
            }]
        );
    }

    #[test]
    fn a_missing_program_is_reported() {
        let out = run_and_collect(&["/nonexistent/rubickway-test"], Vec::new());
        assert!(
            matches!(out.as_slice(), [SpawnResult::Failed { .. }]),
            "{out:?}"
        );
    }

    #[test]
    fn a_successful_exit_is_not_reported() {
        assert!(run_and_collect(&["true"], Vec::new()).is_empty());
    }

    #[test]
    fn the_child_gets_the_environment_directory_and_its_own_session() {
        // Exits 0 only if WAYLAND_DISPLAY is set, the directory is /, and it leads its own
        // session (its session id is its pid).
        let script = "test \"$WAYLAND_DISPLAY\" = wl-test || exit 1; \
                      test \"$(pwd)\" = / || exit 2; \
                      test \"$(ps -o sid= -p $$ | tr -d ' ')\" = \"$$\" || exit 3";
        let env = vec![(OsString::from("WAYLAND_DISPLAY"), OsString::from("wl-test"))];
        assert_eq!(run_and_collect(&["sh", "-c", script], env), Vec::new());
    }

    #[test]
    fn no_finished_child_stays_behind() {
        run_and_collect(&["true"], Vec::new());
        // SAFETY: waitpid with WNOHANG on any child only reads status; a zombie of ours would
        // be reaped here and show up as a pid > 0.
        let reaped = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        assert!(reaped <= 0, "a finished child was left: {reaped}");
    }
}
