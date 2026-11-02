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
- System libraries: libwayland-client, libwayland-egl, libEGL with Mesa, libGLESv2,
  libxkbcommon and xkeyboard-config. On Arch: `wayland`, `mesa`, `libglvnd`,
  `libxkbcommon`, `xkeyboard-config`. On Debian/Ubuntu: `libwayland-dev`,
  `libxkbcommon-dev`, `libegl-dev`, `libgles-dev`, `libegl-mesa0`, `xkb-data`.
- A Wayland session, to run the nested compositor.

## Setup, run, test

```sh
cargo fetch    # install dependencies
cargo run      # run
cargo test     # test
```

The full check, the same as CI:

```sh
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked
```

## Running applications on RubickWay

On startup RubickWay prints the socket applications connect to:

```text
INFO rubickway: applications can connect with WAYLAND_DISPLAY=wayland-2
```

Start an application from a host terminal with that name. Only applications that draw into
shared memory are supported for now; GPU applications such as kitty run with Mesa's software
renderer:

```sh
WAYLAND_DISPLAY=wayland-2 LIBGL_ALWAYS_SOFTWARE=1 kitty
```

## Controls

Mod is Alt while RubickWay runs nested. Everything else goes to the application under the
pointer or with keyboard focus.

| Input | Effect |
| --- | --- |
| Alt + left-drag (anywhere), left-drag on the background | pan the canvas |
| Alt + scroll, pinch, wheel over the background | zoom around the pointer |
| touchpad scroll over the background | pan the canvas |
| Alt + Shift + left-drag on a window, or drag its title bar | move the window |
| Alt + right-drag on a window, or drag its border | resize it from the nearest edge or corner |
| Alt+Tab / Alt+Shift+Tab | focus the next / previous window and bring it into view |
| Alt+Q | ask the focused window to close |
| Alt+0 | back to the canvas origin at 100% |
| Alt+Shift+Q | quit RubickWay |

Applications' own maximize and fullscreen controls work: maximize moves the view to the window
at 100% and fills the viewer; fullscreen shows only that window, and panning, zooming or
Alt+Tab leaves it.

## Development workflow

Features are built with [GitHub Spec Kit](https://github.com/github/spec-kit):
spec -> plan -> tasks -> implementation, each checked against the principles in
`.specify/memory/constitution.md`. Each feature is developed in its own git worktree
under `.claude/worktrees/<name>/`, with its spec in `specs/<timestamp>-<name>/`.
Agent instructions and commands are in `AGENTS.md`.

## License

Licensed under the MIT License (see `LICENSE`), except `.specify/` and
`.claude/skills/speckit-*` (MIT, GitHub Spec Kit) - see `THIRD_PARTY_NOTICES.md`.
