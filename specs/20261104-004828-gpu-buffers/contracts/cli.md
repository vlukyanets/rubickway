# Contract: log lines

Adds to `specs/20261102-143401-drm-backend/contracts/cli.md`. The command line (`rubickway`, no
arguments) and the exit statuses do not change. Each fallback leaves one line in plain words.

## At startup (info)

```text
composing on <GPU> (<render node>), offering applications <n> image formats
```

`<GPU>` is the GL renderer string, for example `Mesa Intel(R) UHD Graphics 620 (KBL GT2)`.

With `distant-windows` configured (FR-017), each part only when set:

```text
windows at <n>% zoom or below are distant: asked for at most <n> frames a second, smoothed with a copy at <n>% of the size they are drawn at
```

## Fallbacks (warn)

```text
<GPU> cannot import GPU images (<reason>): applications will draw in software
explicit synchronisation is off: <GPU> does not support it
```

## Applications (warn, at most once per application for the same cause)

```text
application <id>: <GPU> cannot read its image (<format> <modifier>); its window keeps its last picture
application <id>: an image failed to import (<reason>)
```

Protocol errors use the existing `application <id> disconnected: protocol error ...` line.

## Running out of file descriptors (warn, once each time it starts and ends)

```text
out of file descriptors: applications may stall and new ones cannot connect; application <id> holds the most GPU images (<n>)
out of file descriptors: applications may stall and new ones cannot connect; no application holds GPU images
file descriptors are available again
```

Found when accepting a new application, or getting a file to wait for an image on, fails with `EMFILE`
or `ENFILE`; an EGL import cannot tell, as it reports only an EGL error. It ends when a pass of the event
loop can open a descriptor again. While it lasts, the existing `accepting an application failed` and
`cannot wait for an image to be finished` lines are not repeated, so the log is not flooded.

## Debug

```text
window <id>: first frame presented
```

For SC-001: compared with the `application <id> connected` line.

```text
an image is not finished yet: its commit waits
```

For SC-003: shows that a slow program's commits really waited.

```text
display <name>: canvas zoom <n>%, windows <distant|near>
```

Each time a display is drawn at a new zoom, for reading frame times against it (FR-017).

```text
frame work <n> ms, <n> ms of it on the CPU: copies <n> ms, drawing <n> ms, showing <n> ms
showing a frame: swap <n> ms, then commit <n> ms
copy <width>x<height>: allocating <n> ms, drawing <n> ms, mipmaps <n> ms
```

When that work on the event loop takes 5 ms or more, for Principle VII: it says where the loop stood,
and whether it was working or waiting.
