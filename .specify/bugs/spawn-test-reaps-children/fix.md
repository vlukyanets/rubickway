# Bug Fix: the zombie check in the spawn tests reaps other tests' children

- **Slug**: spawn-test-reaps-children
- **Fixed**: 2026-11-19
- **Assessment**: ./assessment.md
- **Status**: applied

## Summary

`no_finished_child_stays_behind` now checks only the child it started, with `waitpid(pid)`
instead of `waitpid(-1)`, so it can no longer take the children of the spawn tests running in
parallel threads of the same process. Production code is unchanged.

## Changes

| File | Change | Notes |
|------|--------|-------|
| `src/ffi/spawn.rs` | modified test | `no_finished_child_stays_behind` only; `run` and the other tests untouched |

## Diff Highlights

```rust
let file = std::env::temp_dir().join(format!("rubickway-spawn-child-{}", std::process::id()));
let script = format!("echo $$ > '{}'", file.display());
assert_eq!(run_and_collect(&["sh", "-c", &script], Vec::new()), Vec::new());
let pid: libc::pid_t = std::fs::read_to_string(&file).unwrap().trim().parse().unwrap();
// ...
let reaped = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
assert_eq!(reaped, -1, "a finished child was left: {pid}");
assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
```

The child is `sh` itself (started directly by `spawn`), so `$$` is the pid `run` waits for. The
file name carries the test process's pid, so test runs in parallel processes do not share it.

## Tests Added or Updated

- `src/ffi/spawn.rs::tests::no_finished_child_stays_behind` - after `spawn` reports, its own
  child is already reaped (`waitpid` gives -1 with ECHILD); a zombie left behind comes back as its
  pid and fails the test.

## Local Verification

- The rewritten test passes: `cargo test --lib ffi::spawn::tests::no_finished_child_stays_behind`.
- It still catches the bug it guards: with `run` temporarily changed to drop its child without
  waiting, it failed with "a finished child was left: 336658"; `run` was then restored (the diff
  touches the test only).
- Full check from AGENTS.md: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D
  warnings`, `cargo test --locked` and `cargo test --release --locked` pass, 728 tests in each
  profile.
- Repeated runs: `cargo test --lib -q` 300 times in a row -> 0 failures (before the fix: 1 in
  150 on master the same day, so about 2 expected over 300 runs).

## Deviations from Assessment

None.

## Follow-ups

- None.
