# RubickWay

A Wayland compositor with an endless canvas where every monitor is a viewer: a camera
looking at a region of one shared 2D canvas.

It is its own implementation in Rust, built on the `wayland-server` / `wayland-protocols`
crates and system libraries rather than on Smithay or wlroots. It runs nested inside an
existing Wayland session first; a standalone DRM/KMS backend comes later.

Status: early skeleton - no compositor features yet.

## Prerequisites

- [rustup](https://rustup.rs/) - the toolchain (Rust 1.99) is pinned in
  `rust-toolchain.toml` and installed automatically.
- A Wayland session, to run the nested compositor (once it exists).

## Setup, run, test

```sh
cargo fetch    # install dependencies
cargo run      # run
cargo test     # test
```

The full check, the same as CI:

```sh
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

## Development workflow

Features are built with [GitHub Spec Kit](https://github.com/github/spec-kit):
spec -> plan -> tasks -> implementation, each checked against the principles in
`.specify/memory/constitution.md`. Each feature is developed in its own git worktree
under `.claude/worktrees/<name>/`, with its spec in `specs/<timestamp>-<name>/`.
Agent instructions and commands are in `AGENTS.md`.

## License

Licensed under the MIT License (see `LICENSE`), except `.specify/` and
`.claude/skills/speckit-*` (MIT, GitHub Spec Kit) - see `THIRD_PARTY_NOTICES.md`.
