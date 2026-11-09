//! Shared memory from applications, read safely; sealed memfds for keymaps (research R4).
//!
//! An application can truncate the file behind a pool at any time, and the next read of the
//! mapping raises SIGBUS. Every read therefore goes through [`ShmMap::with_access`]: it records
//! the mapping in a per-thread slot, and the process SIGBUS handler, when the fault lies in
//! that mapping, maps anonymous zero pages over it and returns, so the read finishes on zeros
//! and reports [`AccessError::Fault`]. Faults anywhere else get the previous handler back.
//! References: libwayland `wayland-shm.c`, Smithay `wayland/shm/pool.rs`.
#![allow(unsafe_code)]

use std::ffi::{c_int, c_void};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering, compiler_fence};
use std::sync::{Once, OnceLock};

/// Why a read of pool memory failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    /// The requested range is not inside the mapping.
    OutOfBounds,
    /// The file behind the mapping shrank while it was read (SIGBUS); the bytes read were
    /// zeros.
    Fault,
}

thread_local! {
    /// Start address of the mapping this thread is reading. Constant initialisers keep access
    /// free of allocation and locks, so the signal handler may read it.
    static ACCESS_START: AtomicUsize = const { AtomicUsize::new(0) };
    /// Length of the mapping this thread is reading; 0 = none.
    static ACCESS_LEN: AtomicUsize = const { AtomicUsize::new(0) };
    /// Set by the signal handler when it replaced this thread's mapping.
    static FAULTED: AtomicBool = const { AtomicBool::new(false) };
}

/// The SIGBUS disposition before ours, restored for faults that are not ours.
static PREVIOUS: OnceLock<libc::sigaction> = OnceLock::new();

/// A read-only shared mapping of an application's pool file.
pub struct ShmMap {
    ptr: *mut c_void,
    len: usize,
    _fd: OwnedFd,
}

// SAFETY: the mapping is read-only memory owned by this value; concurrent reads from several
// threads are fine, and the fault handler only ever replaces the mapping with zero pages of the
// same size, so other readers keep reading valid memory.
unsafe impl Send for ShmMap {}
// SAFETY: as for `Send`; `&ShmMap` only allows reads.
unsafe impl Sync for ShmMap {}

impl ShmMap {
    /// Maps `len` bytes of `fd`.
    pub fn new(fd: OwnedFd, len: usize) -> io::Result<ShmMap> {
        if len == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty pool"));
        }
        // SAFETY: a new read-only shared mapping chosen by the kernel; fd and len are checked
        // by the kernel, and a failure is reported as MAP_FAILED.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(ShmMap { ptr, len, _fd: fd })
    }

    /// The mapped length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always false: an empty pool cannot be mapped.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Extends the mapping to `new_len` bytes (smaller values are ignored; the protocol only
    /// allows growing).
    pub fn grow(&mut self, new_len: usize) -> io::Result<()> {
        if new_len <= self.len {
            return Ok(());
        }
        // SAFETY: ptr/len describe our own live mapping; with MREMAP_MAYMOVE the kernel returns
        // the (possibly moved) new mapping and the old address becomes invalid, so we replace
        // it. `&mut self` guarantees no read is in progress.
        let ptr = unsafe { libc::mremap(self.ptr, self.len, new_len, libc::MREMAP_MAYMOVE) };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        self.ptr = ptr;
        self.len = new_len;
        Ok(())
    }

    /// Calls `f` with `len` bytes at `offset`. If the file shrank meanwhile, `f` sees zeros and
    /// the result is [`AccessError::Fault`].
    pub fn with_access<T>(
        &self,
        offset: usize,
        len: usize,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, AccessError> {
        let end = offset.checked_add(len).ok_or(AccessError::OutOfBounds)?;
        if end > self.len {
            return Err(AccessError::OutOfBounds);
        }
        install_handler();
        // The signal handler runs on this thread, invisibly to the optimiser: atomics plus
        // compiler fences keep these stores, the reads in `f` and the `FAULTED` check in program
        // order (without them a release build folds the check to "no fault").
        ACCESS_START.with(|s| s.store(self.ptr as usize, Ordering::Relaxed));
        ACCESS_LEN.with(|l| l.store(self.len, Ordering::Relaxed));
        FAULTED.with(|f| f.store(false, Ordering::Relaxed));
        compiler_fence(Ordering::SeqCst);
        // SAFETY: offset..end lies inside the mapping (checked above), which stays mapped for
        // the whole borrow (`&self` prevents `grow` and drop). If the file behind it shrinks,
        // the read raises SIGBUS and the handler maps zero pages over exactly this mapping, so
        // the memory stays readable.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>().add(offset), len) };
        let out = f(bytes);
        compiler_fence(Ordering::SeqCst);
        ACCESS_LEN.with(|l| l.store(0, Ordering::Relaxed));
        if FAULTED.with(|f| f.swap(false, Ordering::Relaxed)) {
            Err(AccessError::Fault)
        } else {
            Ok(out)
        }
    }
}

impl Drop for ShmMap {
    fn drop(&mut self) {
        // SAFETY: ptr/len describe our own mapping, unmapped exactly once here.
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

fn install_handler() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: an all-zero sigaction is a valid empty value; we set a handler with the
        // SA_SIGINFO signature and an empty mask, and keep the previous disposition.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_sigbus as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGBUS, &action, &mut previous) == 0 {
                let _ = PREVIOUS.set(previous);
            }
        }
    });
}

extern "C" fn on_sigbus(_signal: c_int, info: *mut libc::siginfo_t, _context: *mut c_void) {
    // SAFETY: for SA_SIGINFO handlers the kernel passes a valid siginfo_t.
    let addr = unsafe { (*info).si_addr() } as usize;
    let len = ACCESS_LEN
        .try_with(|l| l.load(Ordering::Relaxed))
        .unwrap_or(0);
    let start = ACCESS_START
        .try_with(|s| s.load(Ordering::Relaxed))
        .unwrap_or(0);
    if len != 0 && addr >= start && addr - start < len {
        // SAFETY: start..start+len is the mapping this thread is reading. Replacing it with
        // anonymous zero pages at the same address keeps the read valid; mmap is
        // async-signal-safe.
        let zeros = unsafe {
            libc::mmap(
                start as *mut c_void,
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE | libc::MAP_FIXED | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if zeros != libc::MAP_FAILED {
            let _ = FAULTED.try_with(|f| f.store(true, Ordering::Relaxed));
            return;
        }
    }
    // Not ours: restore the previous disposition. Returning repeats the faulting access, which
    // then gets the previous (normally default) treatment.
    match PREVIOUS.get() {
        // SAFETY: restores a disposition the kernel gave us earlier.
        Some(previous) => unsafe {
            libc::sigaction(libc::SIGBUS, previous, ptr::null_mut());
        },
        // SAFETY: resetting SIGBUS to its default disposition.
        None => unsafe {
            libc::signal(libc::SIGBUS, libc::SIG_DFL);
        },
    }
}

/// A memfd holding `text` plus a terminating NUL, sealed against shrinking, growing and
/// writing, and its size in bytes - the form `wl_keyboard.keymap` expects.
pub fn sealed_memfd(text: &str) -> io::Result<(OwnedFd, u32)> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);
    sealed_memfd_bytes(&bytes)
}

/// A read-only memfd holding `bytes` and its size: safe to share with any number of clients.
pub fn sealed_memfd_bytes(bytes: &[u8]) -> io::Result<(OwnedFd, u32)> {
    // SAFETY: memfd_create with a static NUL-terminated name; it returns a new fd or -1.
    let raw = unsafe {
        libc::memfd_create(
            c"rubickway-shared".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw is a fresh fd that nothing else owns.
    let mut file = File::from(unsafe { OwnedFd::from_raw_fd(raw) });
    file.write_all(bytes)?;
    let size = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file too large"))?;
    let seals = libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    // SAFETY: fcntl on our own fd with an integer argument.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((file.into(), size))
}

/// The seals of a memfd.
#[cfg(test)]
fn seals(file: &File) -> io::Result<c_int> {
    // SAFETY: fcntl on a valid fd, no argument.
    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
    if seals < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(seals)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::OwnedFd;

    use super::*;

    /// An unlinked temporary file of `len` bytes, filled with `byte`.
    fn pool_file(len: usize, byte: u8) -> File {
        let path = std::env::temp_dir().join(format!(
            "rubickway-shm-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        file.write_all(&vec![byte; len]).unwrap();
        file
    }

    fn map(file: &File, len: usize) -> ShmMap {
        let fd: OwnedFd = file.try_clone().unwrap().into();
        ShmMap::new(fd, len).unwrap()
    }

    #[test]
    fn reads_the_pool_bytes() {
        let file = pool_file(4096, 7);
        let map = map(&file, 4096);
        let sum = map
            .with_access(100, 200, |bytes| {
                assert_eq!(bytes.len(), 200);
                bytes.iter().map(|&b| u32::from(b)).sum::<u32>()
            })
            .unwrap();
        assert_eq!(sum, 7 * 200);
    }

    #[test]
    fn access_outside_the_mapping_is_refused() {
        let file = pool_file(4096, 1);
        let map = map(&file, 4096);
        assert_eq!(
            map.with_access(4000, 200, |_| ()),
            Err(AccessError::OutOfBounds)
        );
        assert_eq!(
            map.with_access(usize::MAX, 2, |_| ()),
            Err(AccessError::OutOfBounds)
        );
    }

    #[test]
    fn a_truncated_file_faults_instead_of_crashing() {
        let file = pool_file(8192, 9);
        let map = map(&file, 8192);
        file.set_len(0).unwrap();
        let result = map.with_access(0, 8192, |bytes| bytes.iter().all(|&b| b == 0));
        assert_eq!(result, Err(AccessError::Fault));
    }

    #[test]
    fn faults_are_per_thread() {
        let good = pool_file(4096, 3);
        let bad = pool_file(4096, 3);
        let good_map = map(&good, 4096);
        let bad_map = map(&bad, 4096);
        bad.set_len(0).unwrap();
        std::thread::scope(|s| {
            let a = s.spawn(|| {
                (0..200)
                    .map(|_| good_map.with_access(0, 4096, |b| b[4095]))
                    .collect::<Vec<_>>()
            });
            let b = s.spawn(|| bad_map.with_access(0, 4096, |b| b[0]));
            assert!(a.join().unwrap().iter().all(|r| *r == Ok(3)));
            assert_eq!(b.join().unwrap(), Err(AccessError::Fault));
        });
    }

    #[test]
    fn growing_keeps_the_bytes() {
        let file = pool_file(4096, 5);
        let mut map = map(&file, 4096);
        file.set_len(16384).unwrap();
        map.grow(16384).unwrap();
        assert_eq!(map.len(), 16384);
        assert_eq!(map.with_access(0, 1, |b| b[0]), Ok(5));
        assert_eq!(map.with_access(16000, 1, |b| b[0]), Ok(0));
    }

    #[test]
    fn sealed_memfd_holds_the_text_and_is_sealed() {
        let (fd, size) = sealed_memfd("xkb_keymap {};").unwrap();
        assert_eq!(size, 15, "text plus the terminating NUL");
        let file = File::from(fd);
        let mut text = vec![0u8; 15];
        use std::os::unix::fs::FileExt;
        file.read_exact_at(&mut text, 0).unwrap();
        assert_eq!(text, b"xkb_keymap {};\0");
        let seals = seals(&file).unwrap();
        for seal in [libc::F_SEAL_SHRINK, libc::F_SEAL_GROW, libc::F_SEAL_WRITE] {
            assert_ne!(seals & seal, 0);
        }
    }
}
