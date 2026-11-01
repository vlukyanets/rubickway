# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, Cursor, Copilot, Gemini, ...).
This is the single source of truth: tool-specific files (e.g. `CLAUDE.md`) import it and
add only tool-specific extras.

> Only write what an agent can't learn by reading the repo, or would waste time
> rediscovering every session. **Delete any section you have nothing specific for** -
> generic or empty sections cost context on every task and change nothing.
> Rules with zero exceptions belong in permission deny rules or CI, not here.

## Project

**{{PROJECT_NAME}}** - {{ONE_LINE_DESCRIPTION}}

- Stack: {{STACK}} <!-- with versions: "Python 3.13, FastAPI 0.115, PostgreSQL 17" - not "Python API" -->

## Commands

Use exactly these - don't invent equivalents.

| Task                                | Command |
| ----------------------------------- | ------- |
| Install deps                        | `{{SETUP_CMD}}` <!-- e.g. uv sync or pnpm install or go mod download --> |
| Run the app                         | `{{RUN_CMD}}` |
| Format                              | `{{FMT_CMD}}` <!-- e.g. uv run ruff format . or pnpm prettier -w . --> |
| Run one test                        | `{{SINGLE_TEST_CMD}}` <!-- e.g. uv run pytest tests/test_x.py::test_y -v --> |
| **Quick check** - while iterating   | `{{QUICK_CHECK_CMD}}` <!-- seconds, not minutes: typecheck + unit tests, e.g. uv run mypy src && uv run pytest -x -q tests/unit --> |
| **Full check** - before finishing   | `{{FULL_CHECK_CMD}}` <!-- format check + lint + typecheck + all tests; the same command runs in CI --> |

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

## Where the code misleads

<!-- Traps an agent can't spot by reading the code. e.g.
- `src/legacy/` is still live but deprecated - don't copy its patterns into new code.
- `src/utils/old_db.py` looks unused but is loaded dynamically by the job runner. -->

## Canonical examples

<!-- The best existing file to copy for each common kind of change. One real example
     beats paragraphs of description. e.g.
- New endpoint -> copy `src/api/users.py`, tests in `tests/api/test_users.py`
- New background job -> copy `src/jobs/cleanup.py` -->

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
  dependencies, changing public APIs or DB schema, deleting files, touching CI, auth,
  or anything security-sensitive.
- Never -> do this instead:
  - Read, print or commit secrets (`.env`, keys) -> use the variable names in `.env.example`.
  - Skip, disable or weaken a failing test -> fix the code, or ask if the test is wrong.
  - Run destructive commands (`rm -rf`, `git reset --hard`, DB drops) -> ask me.
  - Edit Spec Kit's managed files (`.specify/templates/`, `.specify/scripts/`,
    `.claude/skills/speckit-*`) -> they're overwritten on upgrade; customise via a
    Spec Kit preset, or ask me.
  <!-- e.g. - Edit generated code in `src/gen/` -> edit `schema.graphql` and run `make gen`. -->

## Gotchas

<!-- Add a line whenever an agent gets something wrong twice. This is how the project
     "learns". Format: - **Thing**: what to do instead. -->
