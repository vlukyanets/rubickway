# Contract: command line and process behaviour

## Invocation

```text
rubickway
```

No arguments or options in this feature. Unknown arguments: print `usage: rubickway` to
stderr and exit with status 2.

## Environment

| Variable | Meaning |
| --- | --- |
| `WAYLAND_DISPLAY` | host Wayland socket (standard libwayland lookup, incl. `WAYLAND_SOCKET`) |
| `RUST_LOG` | log filter for stderr output; default `rubickway=info` |

## Exit status

| Status | When |
| --- | --- |
| 0 | window closed by the host, Mod+Shift+Q, SIGINT or SIGTERM |
| 1 | startup failure (FR-013) or host disconnect / protocol error (FR-014) |
| 2 | unknown command-line arguments |

## Error messages (stderr, one line, then exit 1)

| Situation | Message starts with |
| --- | --- |
| no host session (connect fails) | `rubickway: cannot connect to a Wayland session` |
| required global missing | `rubickway: the Wayland session does not provide <global>` |
| EGL / GLES setup fails | `rubickway: cannot initialise OpenGL ES: <reason>` |
| host connection lost | `rubickway: lost the connection to the Wayland session: <reason>` |
| the window's EGL surface cannot be created | `rubickway: cannot create the viewer window: <reason>` |
| a frame cannot be drawn or presented | `rubickway: rendering failed: <reason>` |
| the event loop cannot be set up | `rubickway: event loop: <reason>` |

## Window

- One `xdg_toplevel`, title `RubickWay`, app id `rubickway`.
- Initial size: the host's choice, or 1280x800 logical pixels when the host leaves it to us.
- Server-side decorations requested when the host supports `zxdg_decoration_manager_v1`.
