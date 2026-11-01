# Personal project template (Spec-Driven Development)

A stack-agnostic starting point for projects built with AI coding agents using
[GitHub Spec Kit](https://github.com/github/spec-kit): every feature goes from a written
spec -> plan -> tasks -> implementation, checked against the project's constitution.

## What's inside

```
AGENTS.md                     <- operational instructions for all agents: commands, traps, git
CLAUDE.md                     <- imports AGENTS.md + Claude Code-specific extras
BOOTSTRAP.md                  <- one-time setup prompt for an agent (deletes itself)
THIRD_PARTY_NOTICES.md        <- Spec Kit's MIT license, kept in every project
.specify/                     <- Spec Kit (managed by the Specify CLI)
  memory/constitution.md      <- project principles - filled in during bootstrap
  templates/  scripts/        <- spec/plan/tasks templates and helper scripts
.claude/
  skills/speckit-*/           <- Spec Kit skills: /speckit-specify, /speckit-plan, ...
  settings.json               <- shared permissions: allow safe cmds, ask on risky, deny secrets
  settings.local.example.json <- personal settings to copy (real file is gitignored)
.github/
  workflows/ci.yml.disabled   <- CI skeleton; bootstrap renames it to ci.yml and configures it
  pull_request_template.md    <- links the spec; "AI assistance" section
.worktreeinclude              <- gitignored files copied into each worktree (.env, CLAUDE.local.md)
.editorconfig  .gitattributes  .gitignore  .env.example
```

Feature work appears later under `specs/<timestamp>-<name>/` (e.g. `specs/20261101-143022-csv-export/`),
developed in worktrees under `.claude/worktrees/<name>/` (gitignored).

## Prerequisites

| Tool | Needed for | Required? |
| --- | --- | --- |
| [Claude Code](https://code.claude.com) (or another [supported agent](https://github.github.io/spec-kit/integrations/index.html)) | running the `/speckit-*` commands | yes |
| bash, git | Spec Kit helper scripts; version control | yes |
| [uv](https://docs.astral.sh/uv/) | installing the Specify CLI | yes |
| Specify CLI - `uv tool install specify-cli` | bootstrap step 2, upgrading Spec Kit, adding extensions | yes |
| jq, python3 | faster JSON handling in the helper scripts (they fall back without them) | optional |
| `gh` | letting the agent open PRs and read reviews | optional |

The day-to-day `/speckit-*` commands don't call the CLI - the skills and scripts are
already in the repo. Stack tools (formatter, test runner, ...) are chosen per project
during bootstrap.

## Starting a new project

1. Copy the template into a new folder without its git history:
   ```sh
   rsync -a --exclude .git ~/Projects/personal-project-template/ ~/Projects/my-app/
   ```
2. Open an AI agent in the new folder and say:

   > **Follow BOOTSTRAP.md**

   The agent will:
   - `git init` and commit the untouched template, so its changes are reviewable;
   - check the Spec Kit version and offer an upgrade;
   - interview you about the project: purpose, stack with versions, tooling;
   - write the constitution with you;
   - ask about the optional `bug` and `assess` extensions;
   - scaffold a minimal skeleton if the folder is empty;
   - fill in `AGENTS.md`, running every command to prove it works;
   - write a project `README.md` and `LICENSE`, and configure CI, `.gitignore`, `.env.example`;
   - set up Dependabot, asking you for the schedule, cooldown, grouping and limits;
   - delete `BOOTSTRAP.md`, run the full check, and propose the first commit.
3. Review the changes with `git diff` and cut anything generic - instructions written or edited by the
   developer work better than generated ones.
4. Optional: `cp .claude/settings.local.example.json .claude/settings.local.json` and
   adjust for your own taste.

## Development workflow

| Step | Claude Code | Output | When |
| --- | --- | --- | --- |
| Principles | `/speckit-constitution` | `.specify/memory/constitution.md` | once, then rare amendments |
| Specify | `/speckit-specify <what & why>` | `specs/<timestamp>-<name>/spec.md` | every feature |
| Clarify | `/speckit-clarify` | answers recorded in `spec.md` | optional: when the spec is ambiguous |
| Plan | `/speckit-plan <tech choices>` | `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md` | every feature |
| Checklist | `/speckit-checklist` | `checklists/*.md` | optional: requirement quality gate |
| Tasks | `/speckit-tasks` | `tasks.md` | every feature |
| Analyze | `/speckit-analyze` | consistency report | optional: before implementing |
| Implement | `/speckit-implement` | code + ticked tasks | every feature |
| Converge | `/speckit-converge` | remaining work appended to `tasks.md` | repeat with implement until `Converged` |

Review each artifact before moving to the next step. Small fixes skip the cycle.

**Parallel features: one worktree each.** Start a feature with
`claude --worktree csv-export`: Claude Code creates `.claude/worktrees/csv-export/` on
branch `worktree-csv-export`, copies the files listed in `.worktreeinclude`, and you run
the whole cycle there; its spec lands in `specs/20261101-143022-csv-export/`. Feature
folders are timestamped, so two features in progress at once never collide. Each worktree
has its own `specs/` and its own current feature. Worktrees branch from the main
checkout's current `HEAD` (`worktree.baseRef: head` in `.claude/settings.json`), so
unpushed local commits are included. Keep the worktree until its PR is merged; on exit
Claude asks whether to keep or remove it. After the merge, clean up:

```sh
git worktree remove .claude/worktrees/csv-export
git branch -d worktree-csv-export
```

**Constitution vs AGENTS.md:** the constitution says *what must hold* (principles every
plan is checked against); `AGENTS.md` says *how to operate* (commands, traps, git rules).
Neither repeats the other.

## Keeping Spec Kit up to date

In this template and in every project made from it:

```sh
specify self upgrade                 # update the CLI
specify integration upgrade claude   # refresh skills, templates and scripts
specify extension update             # update installed extensions
```

`specs/` and `constitution.md` are never overwritten. Managed files you've edited locally
block the upgrade until you resolve them - customise through Spec Kit presets instead of
editing `.specify/templates/` directly.

## How the pieces fit

- **`AGENTS.md` is the single source of truth for agents.** Most agents (Codex, Cursor,
  Copilot, Zed, ...) read it natively; `CLAUDE.md` imports it with `@AGENTS.md`. Others need config:
  - Gemini CLI - `.gemini/settings.json`: `{ "context": { "fileName": "AGENTS.md" } }`
  - Aider - `.aider.conf.yml`: `read: AGENTS.md`

  Spec Kit skills are installed for Claude Code; add another agent with
  `specify integration install <key>`.
- **Write only what the agent can't see.** Commands, traps in the code, canonical examples,
  boundaries. No architecture tour, no generic advice.
- **Guidelines in Markdown, guarantees in config.** Instruction files are guidelines the
  model may judge don't apply. The few rules with zero exceptions are enforced instead:
  secrets via `deny` rules in `.claude/settings.json`, passing checks via CI.
- **Gotchas list.** When an agent gets something wrong twice, add a line to *Gotchas* in
  `AGENTS.md`. Remove lines that no longer change behaviour; a stale rule is worse than none.
- **AI attribution.** AI-written work is disclosed with a `Co-Authored-By:` /
  `Assisted-by:` trailer on commits and an *AI assistance* section in PRs. The human
  stays the author and is accountable. Session links (`Claude-Session:` trailers) are
  left out: `attribution.sessionUrl` is `false` in `.claude/settings.json`.
- **Shared vs personal settings.** `settings.json` is committed and applies to everyone:
  project rules, guardrails. `settings.local.json` is gitignored and holds personal
  choices: default permission mode, extra allowed commands, env vars. Claude Code also
  saves your "don't ask again" approvals there. Local settings override shared ones,
  except that a `deny` rule in either file always wins.

## License

No license; all rights reserved - except `.specify/` and `.claude/skills/speckit-*`,
which come from [GitHub Spec Kit](https://github.com/github/spec-kit) under the MIT
License (see `THIRD_PARTY_NOTICES.md`). Projects made from this template choose their own
license during bootstrap; the Spec Kit notice stays in every one of them.
