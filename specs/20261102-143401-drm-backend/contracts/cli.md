# Contract: command line and process behaviour

Replaces `specs/20261101-040740-nested-canvas/contracts/cli.md` and the changes in
`specs/20261101-175316-client-windows/contracts/cli.md`; what is not listed here is gone with
nested mode.

## Invocation

```text
rubickway
```

No arguments or options (FR-027). Any argument: `usage: rubickway` on stderr, exit status 2.
Start it from a local text console where you are logged in, the console being shown.

## Environment

| Variable | Meaning |
| --- | --- |
| `XDG_CONFIG_HOME` | configuration directory; `~/.config` when unset, empty or not absolute (`contracts/config.md`) |
| `HOME` | the working directory of programs started by spawn bindings |
| `XDG_RUNTIME_DIR` | where the socket for applications goes (unchanged) |
| `RUST_LOG` | log filter for stderr; default `rubickway=info` |

`WAYLAND_DISPLAY` is no longer read, nor are the `XKB_DEFAULT_*` variables: the keymap is US
English (FR-007). Only when the seat cannot be opened, `XDG_SESSION_TYPE` (`wayland` or `x11`)
and a non-empty `WAYLAND_DISPLAY` tell a desktop already controlling this session from a
missing seat, because libseat reports every failure as `ENOSYS`.

## Startup output

```text
INFO rubickway: applications can connect with WAYLAND_DISPLAY=<name>
```

as before (socket rules unchanged). Then one line per display:
`INFO rubickway: display <name> (<make> <model>) <w>x<h>@<hz> Hz`; a display connected later
logs the same line, and one disconnected logs `INFO rubickway: display <name> disconnected`.
Configuration warnings:
`WARN rubickway: <file>:<line>: <reason>, using the default`, or `<file>: <reason>, using the
default` when no line applies (an unreadable file).

## Exit status

| Status | When |
| --- | --- |
| 0 | Mod+Shift+Q pressed twice within 2 s, SIGINT, SIGTERM, SIGHUP |
| 1 | a startup failure, a fatal error while running, the watchdog |
| 2 | any command-line argument |

Debug builds only (`cfg(debug_assertions)`): SIGUSR1 makes the event loop spin forever, to
check the watchdog on real hardware (quickstart Q23). Release builds do not handle SIGUSR1, so
its default action applies: the process ends at once, without cleanup, as with `kill -9`.

## Error messages (stderr, one line, then exit 1)

| Situation | Message starts with |
| --- | --- |
| no login session or seat (over SSH, no logind) | `rubickway: cannot open the seat: <reason>; start RubickWay from a local text console` |
| the console is not the one shown (no access within 2 s) | `rubickway: this console is not active; start RubickWay from the console being shown` |
| a desktop already controls this session | `rubickway: another program controls the display; start RubickWay from a text console, not inside a desktop` |
| no usable graphics device | `rubickway: no graphics device with displays found` |
| the graphics device cannot be used (open, atomic modesetting, GBM, EGL) | `rubickway: cannot use the graphics device <path>: <reason>` |
| input cannot be set up, or reading it failed 10 times in a row | `rubickway: cannot read input devices: <reason>` |
| reading the login session failed 10 times in a row | `rubickway: lost the login session: <reason>` |
| no free socket name, or `XDG_RUNTIME_DIR` unset | `rubickway: cannot open a socket for applications: <reason>` (unchanged) |
| the graphics device disappears, or every display failed a commit and its modeset retry | `rubickway: lost the graphics device: <reason>` |
| a frame cannot be drawn | `rubickway: rendering failed: <reason>` (unchanged) |
| the event loop cannot be set up or fails | `rubickway: event loop: <reason>` (unchanged) |
| the watchdog | `rubickway: the event loop did not respond for <n> seconds, exiting` |
| an internal panic | `rubickway: internal error: <message> at <file>:<line>` (after the console was given back) |

The error line is always the last line RubickWay writes.

In every case the text console is shown again afterwards (FR-012); startup failures before the
display was taken leave the screen untouched.

## Log lines (info and warn)

Kept from client-windows (`application <id> connected`, window opened/closed, ...). Added:
`display <name> (<make> <model>) <w>x<h>@<hz> Hz` for each display found, `display <name>
disconnected`, `switched to console <n>`, `paused` / `resumed`, `touchpad <name>: tap with up
to <n> fingers`, `starting <command>`, `cannot start <command>: <reason>` (warn), `display
<name> stays dark: the modeset retry failed too (<reason>); it is tried again when a display is
plugged in or out, or after a console switch` (warn).

## Processes RubickWay starts

A spawn binding's command (`contracts/config.md`): run directly (no shell), in the user's home
directory (`$HOME`), with RubickWay's environment plus `WAYLAND_DISPLAY=<name>`, in its own
session (`setsid`), stdin `/dev/null` (stdout and stderr are RubickWay's), all signals
unblocked. RubickWay reaps it. Logged: `cannot start <command>:
<reason>` (warn) when it fails to start, `<command> exited with status <n>` or `<command> was
killed by signal <n>` (info) when it ends unsuccessfully; a successful exit is not logged. When
RubickWay exits, such programs keep running until their connection closes.
