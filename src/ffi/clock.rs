//! The CPU time of the calling thread, to tell work from waiting in the frame timing lines.
#![allow(unsafe_code)]

use std::time::Duration;

/// How long this thread has really run: time it spent waiting does not count.
pub fn thread_cpu_time() -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a valid `timespec` for the call to write; this clock always exists on Linux.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut time) };
    if result != 0 {
        return Duration::ZERO;
    }
    Duration::new(
        u64::try_from(time.tv_sec).unwrap_or(0),
        u32::try_from(time.tv_nsec).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_counts_and_sleeping_does_not() {
        let start = thread_cpu_time();
        std::thread::sleep(Duration::from_millis(50));
        let slept = thread_cpu_time() - start;
        assert!(slept < Duration::from_millis(25), "slept {slept:?}");

        let start = thread_cpu_time();
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_millis(20) {
            std::hint::black_box(0);
        }
        assert!(thread_cpu_time() > start, "the clock moved");
    }
}
