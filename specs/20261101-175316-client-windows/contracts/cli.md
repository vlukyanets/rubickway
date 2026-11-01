# Contract: command line and process behaviour (changes)

Everything in `specs/20261101-040740-nested-canvas/contracts/cli.md` still holds. Changes:

## Startup output

After the viewer window is created, one line on stderr (info level, shown with the default
`RUST_LOG`):

```text
INFO rubickway: applications can connect with WAYLAND_DISPLAY=<name>
```

`<name>` is the chosen socket, e.g. `wayland-2`. Applications started with that environment
variable connect to RubickWay instead of the host.

## Socket

- Location: `$XDG_RUNTIME_DIR/<name>`, plus the lock file `<name>.lock`.
- Name: the first free `wayland-1` .. `wayland-32`.
- Removed on every exit path that runs destructors (window close, quit shortcut, SIGINT,
  SIGTERM, startup and host errors).

## Error messages (stderr, one line, exit 1) - added

| Situation | Message starts with |
| --- | --- |
| no free socket name, or `XDG_RUNTIME_DIR` unset | `rubickway: cannot open a socket for applications: <reason>` |

## Exit status

Unchanged. Connected applications do not change the exit status; they lose their connection
when RubickWay exits.

## Log lines about applications (info)

- `application <id> connected`.
- `application <id> disconnected: protocol error <code> on <interface>@<object>: <message>`
  when RubickWay disconnected it for a protocol error (edge cases 1, 2 and 7).
- `application <id> disconnected: connection closed or messages unread` otherwise:
  wayland-backend reports an application that exits and one disconnected for unread messages
  (edge case 4) the same way, so the line names both (research R8).
- `window <id> opened: <app_id> "<title>"` / `window <id> closed`.
