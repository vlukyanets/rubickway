# Bug Assessment: the zombie check in the spawn tests reaps other tests' children

- **Slug**: spawn-test-reaps-children
- **Created**: 2026-11-19
- **Source**: pasted text (seen by the agent while running the full check for the popups feature)
- **Verdict**: valid
- **Severity**: low

## Report (verbatim or summarized)

> Flaky unit test ffi::spawn::tests::no_finished_child_stays_behind (src/ffi/spawn.rs): fails
> about 1 in 100 runs of `cargo test --lib`. It calls waitpid(-1, WNOHANG) to check that no
> finished child was left as a zombie, but tests run in parallel threads of one process, so it
> can reap (or see) a child of another spawn test running at the same time; the other test then
> fails too (e.g. a_missing_program_is_reported, with a panic in the spawn thread at std's unix
> process code). Seen on master before the popups feature; src/ffi was not changed by it.
> Observed failures: `no_finished_child_stays_behind` panicked at src/ffi/spawn.rs:233 ("a
> finished child was left"), `a_missing_program_is_reported` panicked at src/ffi/spawn.rs:179,
> and thread 'spawn' panicked at library/std/src/sys/process/unix/unix.rs:149.

## Symptom

Now and then `cargo test --lib` fails with two spawn tests at once: `no_finished_child_stays_behind`
("a finished child was left: <pid>") and one other spawn test whose child disappeared. Expected:
the spawn tests pass on every run; RubickWay itself is not affected.

## Reproduction

1. On master (27d3dc0), run `cargo test --lib -q` in a loop.
2. Reproduced on 2026-11-19: 1 failing run in 150. The failing pair was
   `no_finished_child_stays_behind` (spawn.rs:233) and
   `a_program_gets_the_original_file_limit_back` (spawn.rs:221). Earlier the same day (popups
   worktree): `no_finished_child_stays_behind` and `a_missing_program_is_reported` (spawn.rs:179),
   with a panic in a `spawn` thread inside std.
3. Running only `cargo test --lib ffi::spawn` did not fail in 200 runs: the race needs the load of
   the whole lib test binary, which stretches the window between a child's exit and its waiting
   thread's `wait`.

## Suspected Code Paths

- `src/ffi/spawn.rs:228-234` (`no_finished_child_stays_behind`) - `waitpid(-1, ..., WNOHANG)`
  reaps any finished child of the test process, not only the one this test started. The six spawn
  tests run as threads of the same process, so the reaped child can be another test's.
- `src/ffi/spawn.rs:112-128` (`run`) - the victim side: its thread then finds its own child gone.
  `child.wait()` fails with ECHILD and reports `Failed { "waiting for it failed" }` (the file-limit
  test sees one result where it expected none, :221). Or, when the program could not be executed,
  std's `Command::spawn` itself waits for the failed child and panics (unix.rs:149), the report is
  never sent, and `a_missing_program_is_reported` sees no result (:179).
- Production code is not involved: nothing outside this test calls `waitpid(-1)`, and every
  `spawn` thread waits only for its own child.

## Root Cause Hypothesis

The check "no zombie is left" is written against the whole process (`waitpid(-1)`) while the
test harness runs the other spawn tests in parallel threads of that process. When another test's
child has exited but its thread has not reached `wait` yet, the check reaps it: the check fails
(the child was not this test's), and the other test loses its child. Confidence: high - both
observed failure pairs match, and the victim varies between runs as the race predicts.

## Proposed Remediation

**Preferred**: check only the child this test started. Run a program that leaves its own pid
behind (for example `sh -c 'echo $$ > <temp file>'`, where `$$` is the pid of the child `spawn`
started), read it after `run_and_collect` returns, and call `waitpid(pid, WNOHANG)` for that pid
alone. The thread has already waited, so the expected result is -1 with `ECHILD` (already
reaped); a pid back means a zombie was left. No other test's child can be touched.

**Alternatives**:
- Move the check into its own integration test file (`tests/`), a process with no other
  children, and keep `waitpid(-1)`. Simple to reason about, but it needs `spawn` reachable from
  outside the crate and adds a test binary for one test.
- Serialize all spawn tests with a shared mutex. Fragile: every future spawn test must remember
  it, and it hides the real requirement.

**Files likely to change**:
- `src/ffi/spawn.rs` (tests module only)

**Tests to add or update**:
- `no_finished_child_stays_behind`: rewritten as above. It must still fail if `run` stops waiting
  for its child (check by temporarily removing the `wait`).
- No new test for the victims: they are correct once nothing else reaps their children.

## Risks & Considerations

- Pid reuse between the child's exit and the check: negligible (milliseconds, and the pid is only
  reused after it has been reaped, which is what the check expects).
- The temp file must be unique per run (the tests may run in parallel processes too, as in the
  devcontainer and on the host at the same time).
- Verification needs many runs: at about 1 in 150, a few hundred clean runs of `cargo test --lib`
  are the evidence that the flake is gone.

## Open Questions

- None.
