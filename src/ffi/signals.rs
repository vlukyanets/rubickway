//! Blocking the signals the event loop handles, for every thread (research S20).
//!
//! A signal sent to the process goes to any thread that does not block it. The event loop's
//! signal source blocks its signals only in the thread that creates it, so a thread started
//! earlier (the watchdog, Mesa's, a spawn thread) would take the default action and end the
//! process at once. Blocking them first, in the main thread, lets every later thread inherit
//! the mask.
#![allow(unsafe_code)]

use std::io;
use std::mem::MaybeUninit;

/// Blocks `signals` in the calling thread; threads it starts afterwards inherit the mask.
pub fn block(signals: &[libc::c_int]) -> io::Result<()> {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: `sigemptyset` initialises the set it is given; `set` is a local, so the pointer
    // is valid and unaliased.
    if unsafe { libc::sigemptyset(set.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for &signal in signals {
        // SAFETY: the set was initialised by `sigemptyset` above; `sigaddset` only writes it.
        if unsafe { libc::sigaddset(set.as_mut_ptr(), signal) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: the set is initialised; `pthread_sigmask` reads it and changes only this
    // thread's mask; a null old-set pointer is allowed.
    let r = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, set.as_ptr(), std::ptr::null_mut()) };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r))
    }
}

/// Whether `signal` is blocked in the calling thread.
pub fn is_blocked(signal: libc::c_int) -> bool {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: with a null new set, `pthread_sigmask` only writes the current mask into `set`.
    let r = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), set.as_mut_ptr()) };
    if r != 0 {
        return false;
    }
    // SAFETY: `pthread_sigmask` succeeded, so `set` is initialised.
    unsafe { libc::sigismember(set.as_ptr(), signal) == 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_started_after_blocking_inherit_the_mask() {
        // Its own thread, so the test harness's threads keep their mask.
        std::thread::spawn(|| {
            assert!(!is_blocked(libc::SIGUSR2));
            block(&[libc::SIGUSR2]).unwrap();
            assert!(is_blocked(libc::SIGUSR2));
            let child =
                std::thread::spawn(|| (is_blocked(libc::SIGUSR2), is_blocked(libc::SIGUSR1)));
            assert_eq!(child.join().unwrap(), (true, false));
        })
        .join()
        .unwrap();
    }
}
