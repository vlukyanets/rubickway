# RubickWay

A Wayland compositor with an endless canvas where every monitor is a viewer: a camera
looking at a region of one shared 2D canvas.

It is its own implementation in Rust, built on the `wayland-server` / `wayland-protocols`
crates and system libraries rather than on Smithay or wlroots. It runs directly on the
hardware (DRM/KMS, libinput, libseat), started from a text console.

Status: runs on the hardware with several monitors; applications open windows on the canvas
and are moved, resized, maximized and made fullscreen there. Applications draw into shared
memory only (no GPU buffers yet).

## Prerequisites

- [rustup](https://rustup.rs/) - the toolchain (Rust 1.99) is pinned in
  `rust-toolchain.toml` and installed automatically.
- Linux with a graphics card that supports atomic modesetting, and a login session from
  logind (systemd) or seatd.
- System libraries: libinput, libseat, libgbm, libudev, libEGL with Mesa, libGLESv2,
  libxkbcommon and xkeyboard-config. On Arch: `libinput`, `seatd` (for libseat), `mesa`,
  `libglvnd`, `libxkbcommon`, `xkeyboard-config`. On Debian/Ubuntu: `libinput-dev`,
  `libseat-dev`, `libgbm-dev`, `libudev-dev`, `libxkbcommon-dev`, `libegl-dev`,
  `libgles-dev`, `libegl-mesa0`, `xkb-data`.
- [kitty](https://sw.kovidgoyal.net/kitty/), the terminal Mod+Return starts, and a scalable
  monospace font for it, such as `ttf-dejavu` (kitty does not start with bitmap fonts only).

## Setup, run, test

```sh
cargo fetch              # install dependencies
cargo run --release      # run, from a text console (see below)
cargo test               # test
```

The full check, the same as CI:

```sh
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked
```

## Starting RubickWay

Log in on a text console (for example tty1, reached with Ctrl+Alt+F1 from a desktop) and run
`cargo run --release`, or the built `target/release/rubickway`. It takes no arguments. Every
connected display shows the canvas, side by side: the built-in panel first, then external
monitors in the order they were connected. Monitors can be plugged in and out while it runs.

It does not start over SSH, from a console that is not the one shown, or inside a desktop;
it then prints one line saying why and exits with status 1. The log goes to stderr;
`RUST_LOG=rubickway=debug` shows more.

To quit, press Mod+Shift+Q twice within 2 seconds. The console comes back as it was. If
RubickWay stops responding, its watchdog ends it after 10 seconds and the console comes back
too.

## Running applications on RubickWay

Mod+Return starts kitty. Other programs can be bound to keys in the configuration file, or
started from a terminal on the canvas: they inherit `WAYLAND_DISPLAY`. On startup RubickWay
prints the socket name:

```text
INFO rubickway: applications can connect with WAYLAND_DISPLAY=wayland-1
```

GPU applications such as kitty work too: with no GPU-buffer protocol offered, Mesa draws them
with its software renderer by itself.

## Configuration

`~/.config/rubickway/config.kdl` (or `$XDG_CONFIG_HOME/rubickway/config.kdl`), in
[KDL 2.0](https://kdl.dev). It is read once at startup; every setting is optional, and a bad
one is reported in the log and replaced by its default. All defaults spelled out:

```kdl
input {
    // "Super", "Alt" or "Ctrl".
    mod-key "Super"
}

binds {
    // Spawn bindings: each key combination runs a program. Any number; with none
    // configured, this one is the default.
    Mod+Return { spawn "kitty"; }

    // Pointer bindings: modifiers plus MouseLeft, MouseRight or MouseMiddle, and one of
    // move-window, resize-window, pan-canvas. With none configured, these are the defaults.
    Mod+MouseLeft        { move-window; }
    Mod+Shift+MouseLeft  { pan-canvas; }
    Mod+MouseRight       { resize-window; }
    Mod+Shift+MouseRight { resize-window; }
}

// Seconds without a response from the event loop before RubickWay ends itself.
watchdog-timeout-sec 10
```

Spawn and pointer defaults are replaced separately: binding only pointer gestures keeps
Mod+Return, and binding only programs keeps the gestures. A pointer binding needs at least one
modifier. The keyboard is US English for now; other layouts come later.

For example, three programs on their own keys, and the canvas panned with Mod + left drag,
windows moved with Mod+Shift + left drag and resized with Mod + middle drag:

```kdl
binds {
    Mod+Return { spawn "kitty"; }
    Mod+B      { spawn "firefox"; }
    Mod+E      { spawn "kitty" "-e" "yazi"; }

    Mod+MouseLeft       { pan-canvas; }
    Mod+Shift+MouseLeft { move-window; }
    Mod+MouseMiddle     { resize-window; }
}
```

The full reference is in `specs/20261102-143401-drm-backend/contracts/config.md`.

## Controls

Mod is Super unless the configuration says otherwise. Pan, zoom and Mod+0 act on the monitor
under the pointer. The Mod drags below are the default pointer bindings and can be changed in
the configuration. Everything else goes to the application under the pointer or with keyboard
focus.

| Input | Effect |
| --- | --- |
| Mod + left-drag on a window, or drag its title bar | focus and move the window, also to another monitor |
| Mod + Shift + left-drag (anywhere), Mod + left-drag or left-drag on the background | pan the canvas (no focus change) |
| Mod + scroll, pinch, wheel over the background | zoom around the pointer |
| touchpad scroll over the background | pan the canvas |
| Mod + right-drag on a window, or drag its border | resize it from the nearest edge or corner |
| Mod+Tab / Mod+Shift+Tab | focus the next / previous window and bring it into view |
| Mod+Q | ask the focused window to close |
| Mod+0 | back to this monitor's home view at 100% |
| Mod+Return | start kitty (the default spawn binding) |
| Mod+Shift+Q, twice within 2 s | quit RubickWay |
| Ctrl+Alt+F1 .. F12 | switch to that text console; switching back resumes RubickWay |

Applications' own maximize and fullscreen controls work on the monitor that shows the window:
maximize moves that monitor's view to the window at 100% and fills it; fullscreen shows only
that window there, and panning, zooming or Mod+Tab on that monitor leaves it.

## Development workflow

Features are built with [GitHub Spec Kit](https://github.com/github/spec-kit):
spec -> plan -> tasks -> implementation, each checked against the principles in
`.specify/memory/constitution.md`. Each feature is developed in its own git worktree
under `.claude/worktrees/<name>/`, with its spec in `specs/<timestamp>-<name>/`.
Agent instructions and commands are in `AGENTS.md`.

## License

Licensed under the MIT License (see `LICENSE`), except `.specify/` and
`.claude/skills/speckit-*` (MIT, GitHub Spec Kit) - see `THIRD_PARTY_NOTICES.md`.
