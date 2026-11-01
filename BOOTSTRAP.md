# BOOTSTRAP.md

One-time instructions for an AI agent that turn this template into a real project with
a Spec-Driven Development workflow (GitHub Spec Kit).
Open an agent in the new project folder and say: **"Follow BOOTSTRAP.md"**.
This file deletes itself at the end.

---

You are initialising a new project from a template. Work through the steps in order and
wait for my confirmation wherever a step says so.

## 1. Record the starting point

If this isn't a git repository yet, run `git init -b master`. Ask me to confirm, then
commit the untouched template as `Added project template` - so at the end I can review
exactly what bootstrap changed with `git diff`.

## 2. Check Spec Kit

The template ships Spec Kit files (`.specify/`, `.claude/skills/speckit-*`); the version
is in `.specify/init-options.json` (`speckit_version`).

- Run `specify version`. If the CLI is missing, tell me to install it with
  `uv tool install specify-cli` and wait.
- Run `specify self check`. If a newer release than the template's exists, ask me whether
  to upgrade; if yes: `specify self upgrade`, then `specify integration upgrade claude`.
- If I use an agent other than Claude Code, run `specify integration install <key>` and
  `specify integration use <key>` for it.

## 3. Understand the project

If the folder already contains code, read it first and only ask about what you can't infer.
Then ask me **one question at a time** (offer options where useful) until you know:

- name and one-line description; who it's for and what problem it solves
- language, framework, database - with versions (propose current stable ones if I'm unsure)
- tooling: package manager, formatter, linter, typechecker, test runner
- what kind of app it is and how it runs (CLI, web server, library, ...)
- anything an agent couldn't guess: areas to never touch, unusual conventions, deploy constraints
- license: MIT, Apache-2.0, another open-source license, or none (private / all rights reserved)

Summarise the answers and wait for my confirmation before writing anything.

## 4. Write the constitution

Propose 4-7 principles for `.specify/memory/constitution.md`, derived from step 3 -
e.g. test-first, simplicity/YAGNI, specific quality or security bars, stack constraints.
Each principle must be specific and checkable; no generic advice.
If I choose test-first, word it so `speckit-tasks` acts on it - by default Spec Kit only
generates test tasks when explicitly requested - e.g. "Every `tasks.md` includes test
tasks for each user story, written and failing before the implementation tasks."
Let me edit the list,
then run the `speckit-constitution` skill with the agreed principles
(version `1.0.0`, ratified today). In *Governance*, name `AGENTS.md` as the runtime
guidance file. The constitution holds principles only - don't repeat commands or rules
that live in `AGENTS.md`.

## 5. Choose Spec Kit extensions

Each feature gets its branch from its worktree (`worktree-<name>`) and a timestamped
spec folder from Spec Kit, so don't install the `git` extension - it would add a second
branch per feature.

Ask me whether to install the optional extensions - `bug` (assess -> fix -> validate bug
reports) and `assess` (evaluate an idea before specifying it) - with
`specify extension add <name>`.

## 6. Scaffold - only if there's no code yet

Create the minimal skeleton with the chosen tooling's own generator where one exists
(e.g. `uv init`, `pnpm create vite`, `cargo new`): dependency manifest, formatter / linter /
test config, one trivial module and one passing test. No features - those come through
`speckit-specify`.

## 7. Fill in AGENTS.md

- Replace every `{{...}}` placeholder. Commands must be exact, with flags.
- **Run every command** and fix things until each works; quick check and full check must pass.
- Fill commented sections only with facts specific to this project; otherwise delete the
  whole section. Remove all HTML hint comments.
- No generic advice, no directory listing - only what an agent can't learn from the code.
  Principles belong in the constitution, not here.

## 8. Update the other files

- `README.md` - replace the template README with the project's own: name, description,
  prerequisites, setup, run, test, and a short "Development workflow" note pointing to
  Spec Kit. Keep it short.
- `.github/workflows/ci.yml.disabled` - rename to `ci.yml`, add toolchain setup,
  dependency install and the full check; remove the header and TODO comments.
- `LICENSE` - create it for the license chosen in step 3, filling in year and name
  (`git config user.name`) only where the license text has them; skip if none.
  Whatever the choice, keep `THIRD_PARTY_NOTICES.md`: the Spec Kit files stay under
  their MIT license, and the README's license section must say so, e.g.
  "Licensed under <license>" - or "No license; all rights reserved" if none - followed by
  "except `.specify/` and `.claude/skills/speckit-*` (MIT, GitHub Spec Kit) - see
  `THIRD_PARTY_NOTICES.md`."
- `.gitignore` - add stack-specific entries.
- `.env.example` - the environment variables the app reads, if any.
- `.worktreeinclude` - add any other gitignored files the app needs to run locally,
  so they're copied into every new worktree.
- `.claude/settings.json` - suggest `allow` rules for the project's check commands
  (e.g. `Bash(uv run pytest:*)`), plus `Bash(.specify/scripts/bash/*)` for Spec Kit's
  helper scripts if it isn't there yet; I'll add them myself.
- `.editorconfig` - adjust indentation if the stack's convention differs.

## 9. Set up Dependabot - configured by me

Create `.github/dependabot.yml`, but **ask me for each setting** instead of choosing it.
Propose a default with each question so I can just confirm:

- **Ecosystems** - the project's package manager (`pip`, `uv`, `npm`, `cargo`, `gomod`, ...)
  plus `github-actions` for CI. One `updates` entry each.
- **Schedule** - `interval` (`daily` / `weekly` / `monthly`), and day/time if weekly.
- **Cooldown** - how many days a new release must exist before Dependabot proposes it:
  `default-days`, and optionally `semver-major-days` / `semver-minor-days` /
  `semver-patch-days` (not supported for `github-actions`), plus `exclude` for packages
  that should update immediately. Mention that the default is 3 days, and that cooldown
  doesn't delay security updates.
- **Grouping** - e.g. one PR for all minor + patch updates (`groups` with `update-types`),
  majors separately.
- **PR limit** - `open-pull-requests-limit` (default 5).
- **Ignore rules** - dependencies or update types to skip (e.g. majors of a pinned framework).

Show me the final YAML before writing it.

## 10. Finish

1. Delete this file (`BOOTSTRAP.md`).
2. Run the full check and show the output.
3. Report what you set up and anything you couldn't verify; I'll review it with `git diff`.
4. Propose the commit `Added project skeleton` and commit only after I confirm.
5. Tell me the next step: `/speckit-specify <first feature>`.
