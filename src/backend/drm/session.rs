//! The login session through libseat: device access and console switching (research S3).
//!
//! RubickWay never touches the console itself: the session manager switches it to graphics
//! mode when it hands over the devices, and back to text when RubickWay releases them or dies.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libseat::{Seat, SeatEvent};

use super::{DrmError, ReadFailures};

/// How long startup waits for the session to grant the devices (FR-002).
const FIRST_ENABLE: Duration = Duration::from_secs(2);

/// Why the seat could not be opened (contracts/cli.md). libseat tries every backend and then
/// reports `ENOSYS` whatever went wrong, so a desktop already controlling this session is
/// recognised by the environment RubickWay was started in (`in_desktop`), or by `EBUSY` if a
/// backend passes it on; anything else means no usable seat.
fn open_error(errno: i32, reason: &dyn std::fmt::Display, in_desktop: bool) -> DrmError {
    if in_desktop || errno == libc::EBUSY {
        DrmError::Busy
    } else {
        DrmError::NoSeat(reason.to_string())
    }
}

/// Whether RubickWay was started inside a graphical session: logind's session type says so
/// (`XDG_SESSION_TYPE` `wayland` or `x11`; over SSH or on a console it is `tty`), or a
/// Wayland compositor's socket is set. Read only to explain a seat that cannot be opened.
fn in_desktop(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    matches!(session_type, Some("wayland" | "x11"))
        || wayland_display.is_some_and(|d| !d.is_empty())
}

fn started_in_desktop() -> bool {
    let var = |name| std::env::var(name).ok();
    in_desktop(
        var("XDG_SESSION_TYPE").as_deref(),
        var("WAYLAND_DISPLAY").as_deref(),
    )
}

/// What the session did since the last dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionEvent {
    /// RubickWay has the devices (again).
    Enabled,
    /// RubickWay must stop using the devices; acknowledge with [`Session::acknowledge_disable`].
    Disabled,
}

/// The seat and the devices opened through it.
pub(crate) struct Session {
    seat: Seat,
    events: Rc<RefCell<Vec<SeatEvent>>>,
    active: bool,
    /// Devices by the raw fd of the duplicate handed out; libseat closes the original.
    devices: HashMap<RawFd, libseat::Device>,
    failures: ReadFailures,
}

impl Session {
    /// Opens the seat and waits up to 2 seconds for the devices (startup checks, FR-002).
    pub fn open() -> Result<Session, DrmError> {
        let events: Rc<RefCell<Vec<SeatEvent>>> = Rc::default();
        let queue = Rc::clone(&events);
        let seat = Seat::open(move |_, event| queue.borrow_mut().push(event))
            .map_err(|err| open_error(err.0, &err, started_in_desktop()))?;
        let mut session = Session {
            seat,
            events,
            active: false,
            devices: HashMap::new(),
            failures: ReadFailures::default(),
        };
        let deadline = Instant::now() + FIRST_ENABLE;
        while !session.active {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(DrmError::NotActive);
            }
            let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX).max(1);
            session
                .seat
                .dispatch(ms)
                .map_err(|err| DrmError::NoSeat(err.to_string()))?;
            for event in session.take() {
                if event == SessionEvent::Disabled {
                    session.acknowledge_disable();
                }
            }
        }
        Ok(session)
    }

    /// The seat's fd, for the event loop.
    pub fn fd(&mut self) -> io::Result<BorrowedFd<'_>> {
        self.seat.get_fd().map_err(io::Error::from)
    }

    /// Reads what the session did; call when its fd is readable. Fails once reading has
    /// failed `READ_FAILURES` times in a row (FR-012).
    pub fn dispatch(&mut self) -> Result<Vec<SessionEvent>, String> {
        let failed = match self.seat.dispatch(0) {
            Ok(_) => None,
            Err(err) => {
                tracing::warn!("reading the session failed: {err}");
                Some(err)
            }
        };
        if self.failures.note(failed.is_some())
            && let Some(err) = failed
        {
            return Err(format!("reading it keeps failing: {err}"));
        }
        Ok(self.take())
    }

    fn take(&mut self) -> Vec<SessionEvent> {
        let events: Vec<SeatEvent> = std::mem::take(&mut *self.events.borrow_mut());
        events
            .into_iter()
            .map(|e| match e {
                SeatEvent::Enable => {
                    self.active = true;
                    SessionEvent::Enabled
                }
                SeatEvent::Disable => {
                    self.active = false;
                    SessionEvent::Disabled
                }
            })
            .collect()
    }

    /// Tells the session RubickWay stopped using the devices (after a `Disabled`).
    pub fn acknowledge_disable(&mut self) {
        if let Err(err) = self.seat.disable() {
            tracing::warn!("acknowledging the session pause failed: {err}");
        }
    }

    /// Opens a device (card or input device). The returned fd is a duplicate: the session
    /// revoking access affects it too, as both share one open file description.
    pub fn open_device(&mut self, path: &Path) -> io::Result<OwnedFd> {
        let device = self.seat.open_device(&path).map_err(io::Error::from)?;
        let fd = match device.as_fd().try_clone_to_owned() {
            Ok(fd) => fd,
            Err(err) => {
                let _ = self.seat.close_device(device);
                return Err(err);
            }
        };
        self.devices.insert(fd.as_raw_fd(), device);
        Ok(fd)
    }

    /// Closes a device opened with [`Session::open_device`].
    pub fn close_device(&mut self, fd: OwnedFd) {
        if let Some(device) = self.devices.remove(&fd.as_raw_fd())
            && let Err(err) = self.seat.close_device(device)
        {
            tracing::warn!("closing a device failed: {err}");
        }
        drop(fd);
    }

    /// Switches to text console `vt` (FR-011).
    pub fn switch_vt(&mut self, vt: i32) {
        match self.seat.switch_session(vt) {
            Ok(()) => tracing::info!("switched to console {vt}"),
            Err(err) => tracing::warn!("switching to console {vt} failed: {err}"),
        }
    }
}

/// The session shared with libinput's device opener.
pub(crate) type SharedSession = Rc<RefCell<Session>>;

/// Opens and closes libinput's devices through the session.
pub(crate) struct InputDevices(pub SharedSession);

impl input::LibinputInterface for InputDevices {
    fn open_restricted(&mut self, path: &Path, _flags: i32) -> Result<OwnedFd, i32> {
        self.0
            .borrow_mut()
            .open_device(path)
            .map_err(|err| -err.raw_os_error().unwrap_or(libc::EIO))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        self.0.borrow_mut().close_device(fd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_session_says_another_program_controls_the_display() {
        let err = open_error(libc::EBUSY, &"Device or resource busy", false);
        assert_eq!(
            format!("rubickway: {err}"),
            "rubickway: another program controls the display; start RubickWay from a text \
             console, not inside a desktop"
        );
    }

    #[test]
    fn libseats_enosys_inside_a_desktop_says_another_program_controls_the_display() {
        let err = open_error(libc::ENOSYS, &"Function not implemented", true);
        assert!(matches!(err, DrmError::Busy), "{err}");
    }

    #[test]
    fn a_desktop_is_recognised_by_the_session_type_or_a_wayland_socket() {
        assert!(in_desktop(Some("wayland"), None));
        assert!(in_desktop(Some("x11"), None));
        assert!(in_desktop(Some("tty"), Some("wayland-0")));
        assert!(!in_desktop(Some("tty"), None), "a console or SSH");
        assert!(!in_desktop(None, Some("")));
        assert!(!in_desktop(None, None));
    }

    #[test]
    fn other_failures_say_there_is_no_seat() {
        let err = open_error(libc::ENOSYS, &"Function not implemented", false);
        assert_eq!(
            format!("rubickway: {err}"),
            "rubickway: cannot open the seat: Function not implemented; start RubickWay from a \
             local text console"
        );
        let err = open_error(libc::ENOENT, &"No such file or directory", false);
        assert_eq!(
            format!("rubickway: {err}"),
            "rubickway: cannot open the seat: No such file or directory; start RubickWay from a \
             local text console"
        );
    }
}
