# Bug Verification: the zombie check in the spawn tests reaps other tests' children

- **Slug**: spawn-test-reaps-children
- **Tested**: 2026-11-19
- **Assessment**: ./assessment.md
- **Fix**: ./fix.md
- **Result**: verified

## Summary

The race no longer shows: 500 more runs of the lib tests (300 with the default threads, 200 with
64 test threads for more overlap) had no failure, where before the fix about 1 run in 150 failed.
The rewritten test still catches a child left unwaited, and the full check passes.

## Checks Performed

| Check | Command / Action | Result | Notes |
|-------|------------------|--------|-------|
| Reproduction (post-fix) | `cargo test --lib -q`, 300 runs | pass | 0 failures; before the fix 1 in 150 (assessment) |
| Reproduction under load | `cargo test --lib -q -- --test-threads=64`, 200 runs | pass | 0 failures; more spawn tests overlap |
| Updated test | `cargo test --lib ffi::spawn` | pass | all 6 spawn tests |
| Test still guards the bug | `run` changed to drop its child unwaited (during the fix, then restored) | pass | failed with "a finished child was left: 336658" (fix.md) |
| Regression suite | `cargo test --locked`, `cargo test --release --locked` | pass | 728 tests in each profile |
| Lint / format | `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` | pass | |

## Output Excerpts

```text
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 234 filtered out
default threads: runs 300, failed 0
64 threads: runs 200, failed 0
debug 728 failed 0
release 728 failed 0
```

## Residual Risks

- The bug is a timing race with no deterministic reproduction: the evidence is statistical. With
  a failure rate of about 1 in 150 before the fix, 800 clean runs in all (300 during the fix, 500
  here) make a remaining race at the old rate very unlikely.
- The 64-thread variant was not run before the fix, so its baseline failure rate is unknown; it
  adds overlap, not a measured comparison.
- A future test that reaps children process-wide (`waitpid(-1)`) would bring the race back; the
  comment on the rewritten test says why it checks one pid.

## Recommendation

Close the bug - verified by the repeated runs and the full check. Commit the test change with
the bug folder and open a PR.
