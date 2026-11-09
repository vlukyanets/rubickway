//! What a client-supplied file is, before anything else touches it (research G3).
//!
//! `lseek` on a client's file could block on a FUSE daemon, so the file is first recognised by
//! its name in `/proc/self/fd`, an in-memory lookup.
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// Whether `fd` is a dma-buf: the kernel names those `/dmabuf:...`.
pub fn is_dma_buf(fd: BorrowedFd<'_>) -> bool {
    std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
        .is_ok_and(|target| target.to_string_lossy().starts_with("/dmabuf:"))
}

/// The size of a dma-buf, without moving its offset for anybody else: the description is its own
/// after the client's descriptor came through `SCM_RIGHTS`, but it is put back regardless.
pub fn size_of(fd: BorrowedFd<'_>) -> Option<u64> {
    if !is_dma_buf(fd) {
        return None;
    }
    // SAFETY: `lseek` on a valid descriptor, which was just recognised as a dma-buf, so it
    // cannot block; no memory is involved.
    let end = unsafe { libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_END) };
    // SAFETY: as above.
    unsafe { libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_SET) };
    u64::try_from(end).ok()
}

/// Whether `fd` is readable right now, never waiting. For a dma-buf: its writers finished (research
/// G6). A poll that fails says no: the caller then waits on the file as usual.
pub fn is_readable(fd: BorrowedFd<'_>) -> bool {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid `pollfd`, as the count says, on a valid descriptor; a timeout of 0 never
    // blocks.
    unsafe { libc::poll(&raw mut poll, 1, 0) > 0 }
}

/// A new eventfd, closed on exec and never blocking.
pub fn eventfd() -> std::io::Result<OwnedFd> {
    // SAFETY: no pointers are involved; on success the result is a new descriptor owned by nobody
    // else.
    unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::shm_map::sealed_memfd;
    use std::io::Write;
    use std::os::fd::AsFd;

    #[test]
    fn readable_says_what_a_file_has_to_read_without_waiting() {
        let (read, mut write) = std::io::pipe().unwrap();
        assert!(!is_readable(read.as_fd()));
        write.write_all(b"x").unwrap();
        assert!(is_readable(read.as_fd()));
    }

    #[test]
    fn a_new_eventfd_has_nothing_to_read() {
        let fd = eventfd().unwrap();
        assert!(!is_readable(fd.as_fd()));
    }

    #[test]
    fn a_memfd_is_not_a_dma_buf() {
        let (fd, _) = sealed_memfd("x").unwrap();
        assert!(!is_dma_buf(fd.as_fd()));
        assert_eq!(size_of(fd.as_fd()), None);
    }

    #[test]
    fn an_ordinary_file_is_not_a_dma_buf() {
        let file = std::fs::File::open("/proc/self/exe").unwrap();
        assert!(!is_dma_buf(file.as_fd()));
        assert_eq!(size_of(file.as_fd()), None);
    }

    #[test]
    fn a_descriptor_that_cannot_be_looked_up_is_not_a_dma_buf() {
        // A pipe has its own name in /proc as well.
        let (read, _write) = std::io::pipe().unwrap();
        assert!(!is_dma_buf(read.as_fd()));
    }
}
