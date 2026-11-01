# Quickstart: Client Windows on the Canvas

How to prove the feature works. Automated checks first, then manual steps that need a real
host session and a real application (Principle III).

## Prerequisites

- Everything from `specs/20261101-040740-nested-canvas/quickstart.md`.
- kitty (the acceptance application, spec Assumptions). Until GPU buffers exist it runs with
  Mesa's software renderer, which makes it draw into shared memory.

## Automated

Run the *Full check* from `AGENTS.md`. Added coverage, headless:

| Area | Proves |
| --- | --- |
| shell unit + property tests | placement (FR-004), top-left anchor on resize, stacking, focus rules (FR-008, FR-009, FR-012, FR-013, FR-014), hit-testing with input regions at any zoom (FR-010, SC-003) |
| shell routing tests | Mod over windows, grabs (incl. scroll during a grab), background buttons, consumed shortcuts with their releases (FR-011, FR-012) |
| protocol tests (in-process clients) | globals and versions, surface / subsurface / xdg-shell semantics, input delivery with coordinates, focus loss of the viewer, frame callbacks only for intersecting windows (FR-006), new content in the next frame (SC-002), scale changes re-sent (FR-017), clipboard stub |
| misbehaving-client tests | the seven cases in the spec's Edge Cases (FR-015, FR-016, SC-004) |
| pixel assertions | windows at their rectangle at zoom 1 and 2, stacking, alpha, `xrgb`, a buffer transform, part order, thin lines even grey at 25% (FR-005) |

## Manual (host session required)

Start `RUST_LOG=rubickway=debug cargo run`; note the `WAYLAND_DISPLAY=<name>` line. Start
applications from a host terminal with:

```sh
WAYLAND_DISPLAY=<name> LIBGL_ALWAYS_SOFTWARE=1 kitty
```

If kitty fails with an EGL error on the hybrid Intel/NVIDIA machine, add
`__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json` to force Mesa. If
kitty's software path does not work at all, install foot (`pacman -S foot`), use it for the
steps below and record the switch in spec.md Assumptions.

| # | Step | Expected | Spec |
| --- | --- | --- | --- |
| M1 | Start RubickWay | the socket line appears; `ls $XDG_RUNTIME_DIR` shows `<name>` and `<name>.lock` | FR-001 |
| M2 | Start a second RubickWay | it reports a different name | FR-002 |
| M3 | Start kitty | within 1 s its window appears centred in the view, with kitty's own title bar, the prompt drawn, sharp | FR-003, FR-004, FR-017, SC-001 |
| M4 | Type `ls`, Enter | characters appear immediately; the output is shown | FR-011, SC-002 |
| M5 | Pan and zoom (drag background, wheel) | the window moves and scales with the grid; smooth when zoomed in, no shimmer zoomed out | FR-005 |
| M6 | Start a second kitty; type into it; click the first kitty; type | the new window had focus and was on top; after the click the first is on top and receives the typing | FR-008, FR-009 |
| M7 | Select text with the mouse at 50% and 200% zoom | the selection starts exactly under the pointer | FR-010, SC-003 |
| M8 | Scroll inside kitty (wheel and touchpad) after `seq 1000` | kitty scrolls its output; the canvas does not move | FR-010 |
| M9 | Alt + left-drag, Alt + scroll and pinch over an unfocused kitty | the canvas pans/zooms; kitty receives nothing and does not get focus or come to the front | FR-012 |
| M10 | With kitty focused, click and drag on the background, right-click it, then type | the canvas pans; nothing else happens; typing still goes to kitty | FR-013 |
| M11 | Press Alt+0 and Alt+Shift+Q while kitty has focus (start again after quitting) | view resets / RubickWay quits with status 0; kitty shows nothing for those keys | FR-011, FR-019 |
| M12 | Switch the host layout (e.g. `us,ua`) and type in kitty | kitty receives the other layout's characters | Edge cases |
| M13 | Change kitty's font size (Ctrl+Shift+Equal) | the window grows to the right and down; its top-left stays | clarification 2 |
| M14 | `exit` in kitty; start kitty and `kill -9` it from the host | the window disappears each time; focus moves to the remaining window | FR-007, FR-014 |
| M15 | Five kittys started with `-o cursor_blink_interval=0`, idle 30 s; `top` | RubickWay under 1% of one core | SC-005 |
| M16 | Pan and zoom with five windows open | smooth at the display refresh rate | SC-006 |
| M17 | Run `yes` in kitty and switch the host to another workspace | RubickWay draws nothing and its CPU stays idle while hidden | FR-018 |
| M18 | On the 1.25-scale display: compare kitty at 100% zoom with kitty on the host | equally sharp text | FR-017 |
| M19 | Move the RubickWay window to a monitor with another scale (if available) | kitty redraws at the new scale | FR-017 |
| M20 | Close RubickWay while kitty is open | RubickWay exits 0; kitty exits with a connection error; the socket files are gone | FR-019 |

Record results (host name and version, pass/fail per step) in the PR description.
