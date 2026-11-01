# CLAUDE.md

@AGENTS.md

## Claude Code specifics

Everything above comes from `AGENTS.md` (shared by all agents). Only add Claude-specific
notes here - if a rule applies to every agent, put it in `AGENTS.md` instead.

- **Spec Kit skills** are in `.claude/skills/speckit-*`: `/speckit-specify`,
  `/speckit-clarify`, `/speckit-plan`, `/speckit-checklist`, `/speckit-tasks`,
  `/speckit-analyze`, `/speckit-implement`, `/speckit-converge`, `/speckit-constitution`.
- **Worktrees**: start each feature with `claude --worktree <name>`, named after the
  feature (or ask to work in a worktree). It creates `.claude/worktrees/<name>/` on
  branch `worktree-<name>`, based on the main checkout's current `HEAD`
  (`worktree.baseRef: head`), so unpushed local commits are included - keep the main
  checkout on `master`, or new worktrees inherit whatever branch it's on; `.worktreeinclude`
  copies `.env` and `CLAUDE.local.md` in. On exit, keep the worktree until its PR is
  merged; then remove it with `git worktree remove .claude/worktrees/<name>` and
  `git branch -d worktree-<name>`.
- **Plan mode** for changes outside the Spec Kit cycle that are still more than a small fix.
- **Learning loop**: when I correct you on something that will recur, propose a one-line
  addition to the *Gotchas* section of `AGENTS.md`.
- **Attribution**: keep Claude Code's default `Co-Authored-By` trailer on commits and the
  PR footer (it tracks the current model name), but never add a `Claude-Session:` trailer
  or session link - `attribution.sessionUrl` is `false` in `.claude/settings.json`. To
  customise further, use the `attribution` setting rather than editing messages by hand.
- Personal, uncommitted preferences go in `CLAUDE.local.md` and
  `.claude/settings.local.json` (both gitignored; see `.claude/settings.local.example.json`).
