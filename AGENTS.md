# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, Cursor, Copilot, Gemini, ...).
This is the single source of truth: tool-specific files (e.g. `CLAUDE.md`) import it and
add only tool-specific extras.

> Only write what an agent can't learn by reading the repo, or would waste time
> rediscovering every session. **Delete any section you have nothing specific for** -
> generic or empty sections cost context on every task and change nothing.
> Rules with zero exceptions belong in permission deny rules or CI, not here.

## Project

**RubickWay** - a Wayland compositor with an endless canvas where every monitor is a viewer.

- Stack: Rust 1.99, edition 2024 (pinned in `rust-toolchain.toml`), cargo. No database.
  Dependencies are added per feature; which crates are allowed is in the constitution
  (Principle I).

## Commands

Use exactly these - don't invent equivalents.

| Task                                | Command |
| ----------------------------------- | ------- |
| Install deps                        | `cargo fetch` |
| Run the app                         | `cargo run` |
| Format                              | `cargo fmt` |
| Run one test                        | `cargo test <module::test_name> -- --exact` |
| **Quick check** - while iterating   | `cargo clippy --all-targets -- -D warnings && cargo test --lib` |
| **Full check** - before finishing   | `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked` |

A task isn't finished until the full check passes.

## Workflow - Spec-Driven Development (Spec Kit)

The steps below are Spec Kit skills; invocation depends on the agent (Claude Code:
`/speckit-plan`).

- **Features and non-trivial changes** go through the full cycle, stopping after each
  step for my review:
  `specify` -> `clarify` -> `plan` -> `checklist` -> `tasks` -> `analyze` -> `implement` ->
  `converge`. Repeat `implement` + `converge` until converge reports `Converged`.
  `clarify`, `checklist` and `analyze` may be skipped for small, unambiguous features.
- **Small fixes** (typo, config tweak, obvious one-line bug) skip the cycle: just make the change.
- **One feature per worktree** - develop each feature in its own git worktree under
  `.claude/worktrees/<name>/` (gitignored), not in the main checkout. A worktree is a
  fresh checkout: run *Install deps* there first.
- **Feature folders are timestamped** - `specs/<YYYYMMDD-HHMMSS>-<name>/`, with `<name>`
  the same as the worktree name (Spec Kit's `timestamp` numbering in
  `.specify/init-options.json`), so parallel worktrees never collide.
- **Constitution** - `.specify/memory/constitution.md` holds the project's principles;
  every plan is checked against it. Read it before planning. Change it only via
  `speckit-constitution`, and only when I ask.
- **Feature artifacts** live in `specs/<timestamp>-<name>/` (`spec.md`, `plan.md`, `research.md`,
  `data-model.md`, `contracts/`, `quickstart.md`, `tasks.md`). If implementation has to
  deviate, update the spec/plan first - don't let code and specs drift apart.
- **Bug reports** go through the `bug` extension: `speckit-bug-assess` -> `speckit-bug-fix`
  -> `speckit-bug-test`, with reports in `.specify/bugs/<slug>/`.
- **Ideas not ready for `specify`** go through the `assess` extension: `speckit-assess-intake`
  -> `research` -> `define` -> `shape` -> `decide`, in `.specify/assessments/<slug>/`. A `go`
  verdict hands off to `speckit-specify`.

## Text style

Write code, comments, docs, specs, commit messages and PR descriptions in English, even
when I talk to you in another language. Use plain ASCII punctuation: `-` instead of a long dash (`—`, `–`), `->` instead of an arrow (`→`),
`...` instead of a single-character ellipsis (`…`). Exception: text where the character
itself is the point (user-facing copy that needs it, tests for Unicode handling).

## Git

- Commit subject: past-tense verb + what, capitalised, no trailing period, <= 72 chars.
  - `Added <component>` - new feature, module or file
  - `Fixed <short description of the bug>`
  - `Updated <thing>` - changed behaviour, dependencies or docs
  - `Refactored <component>` - no behaviour change
  - `Removed <thing>`
  - e.g. `Added CSV export to reports`, `Fixed crash on empty input in parser`
- Optional body: *why*, not what.
- One logical change per commit, only when the full check passes.
- Don't commit or push unless asked.

### AI attribution

- When an agent wrote or substantially rewrote code, tests or docs in a commit, add a
  trailer crediting it: the tool's standard `Co-Authored-By:` line if it has one,
  otherwise `Assisted-by: <Tool> (<model>)`. Trivial autocomplete needs none.
- The human is always the commit author and is accountable for the change.
- Name the real tool and model. Never remove, forge, or invent attribution.
- No session links: no `Claude-Session:` (or similar) trailers and no agent session URLs
  in commits or PR descriptions.
- Keep trailers when squashing commits.
- PR descriptions say which AI tools were used, for what, and how the output was verified.

## Boundaries

- Ask first - unless the approved `plan.md` / `tasks.md` already covers it: adding
  dependencies, changing public APIs, deleting files, touching CI, auth,
  or anything security-sensitive.
- Never -> do this instead:
  - Read, print or commit secrets (`.env`, keys) -> use the variable names in `.env.example`.
  - Skip, disable or weaken a failing test -> fix the code, or ask if the test is wrong.
  - Run destructive commands (`rm -rf`, `git reset --hard`) -> ask me.
  - Edit files through shell commands (heredoc `cat > file`, `sed -i`, `tee`, scripts that
    write files) -> use the agent's file-editing tool, so every change is a reviewable
    diff. Running shell or Python for anything else (tests, checks, search) is fine.
  - Edit Spec Kit's managed files (`.specify/templates/`, `.specify/scripts/`,
    `.claude/skills/speckit-*`) -> they're overwritten on upgrade; customise via a
    Spec Kit preset, or ask me.
  - Loosen a lint in `Cargo.toml` or `clippy.toml` -> keep the lint; use a local
    `#[expect(..., reason = "...")]` or ask me.

## Gotchas

- **Lint exceptions**: `unwrap`/`expect`/`panic!` are allowed in `#[test]` functions via
  `clippy.toml`, but not in helper functions of integration tests - those crates declare a
  crate-level `#![expect(..., reason = "...")]`. Elsewhere use
  `#[expect(clippy::unwrap_used, reason = "...")]`, never `#[allow(clippy::...)]`. A module
  that needs `unsafe` opts in with `#![allow(unsafe_code)]`, and only FFI (`src/ffi/`),
  backend, render and plugin loader modules may (constitution, Principle V).
- **Rendering tests** (`tests/render_grid.rs`, `tests/render_windows.rs`) read back llvmpipe
  output and assert pixel facts computed from the inputs (`renders_on_llvmpipe` guards the
  renderer). After an intended visual change, update the expected values - colours come from
  the `render` constants - and never commit reference images.
- **Protocol tests** run the server in-process with `wayland-client` test clients
  (`tests/protocol/harness.rs`); every new global needs its version in the feature's
  `contracts/protocols.md`.
