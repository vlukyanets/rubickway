# Quickstart: Nested Canvas Viewer

How to prove the feature works. Automated checks first, then the manual steps that need a real
host session, GPU and input devices (Principle III).

## Prerequisites

- A Wayland desktop session (developed on niri; also try one other host if available).
- System libraries: libwayland-client, libwayland-egl, libEGL + Mesa, libGLESv2,
  libxkbcommon, xkeyboard-config. On Arch they come with `wayland`, `mesa`, `libglvnd`,
  `libxkbcommon`, `xkeyboard-config`.
- Run *Install deps* from `AGENTS.md`.

## Automated

Run the *Full check* from `AGENTS.md`. It covers, headless:

| Area | Proves |
| --- | --- |
| camera unit + property tests | FR-003, FR-005..FR-007, FR-009, FR-012, FR-016; SC-002, SC-003, SC-007 |
| grid level tests | FR-004 |
| viewer input integration tests | gestures, bindings, focus loss, zoom limits, quit effect |
| keymap tests (`us`, `us,ua`) | shortcuts match on the first layout; no repeat |
| pixel assertions (`tests/render_grid.rs`) | background, axes position and width, y direction, line spacing and tiers at 100%, 10%, 1000% and scale 1.5, no jitter at 1e6 units |

After an intended visual change, update the expected values in `tests/render_grid.rs`
(colours come from the `render` constants).

## Manual (host session required)

Start with `RUST_LOG=rubickway=debug cargo run` from a terminal inside the session.

| # | Step | Expected | Spec |
| --- | --- | --- | --- |
| M1 | Start the program | Within 1 s a window titled RubickWay shows the dark grid; brighter axes cross at the window centre | FR-001..003, SC-001 |
| M2 | Resize the window (host shortcut or mouse) | The axes stay at the window centre; grid spacing on screen does not change | FR-012 |
| M3 | Left-drag on the canvas, move far, also outside the window, release | The grid point grabbed stays under the cursor the whole time; pan continues outside the window | FR-005, SC-002 |
| M4 | Alt + left-drag | Pans the same way (if the host itself uses Alt+drag, note it as a host conflict) | FR-005 |
| M5 | Mouse wheel up/down over different points | Zooms in/out around the cursor; the point under the cursor stays put | FR-006, SC-003 |
| M6 | Keep zooming in, then out | Stops at 1000% / 10%; the view does not drift at the limits; the grid never turns into a solid fill or empty | FR-004, FR-007 |
| M7 | Two-finger scroll on the touchpad | Pans with the fingers | FR-008 |
| M8 | Pinch on the touchpad; Alt + two-finger scroll | Both zoom around the cursor | FR-006, FR-008 |
| M9 | Pan and zoom away, press Alt+0 (also with the second keyboard layout active) | View returns to the startup view | FR-009, SC-007 |
| M10 | Continuous fast panning and zooming | Smooth at the display refresh rate | SC-004 |
| M11 | Leave it idle for 30 s; check `top` | CPU under 1% of one core | FR-015, SC-005 |
| M12 | Switch the host to another workspace; check `top` | No frames drawn while hidden (CPU stays idle) | FR-015 |
| M13 | On a display with fractional scale (e.g. 1.5) | Grid lines are sharp, 1 physical pixel wide | FR-017 |
| M14 | Press Alt+Shift+Q (hold it down once) | Exits within 1 s, `echo $?` prints 0; exits only once | FR-010, SC-006 |
| M15 | Start again, close the window through the host | Exits with status 0 | FR-011 |
| M16 | `WAYLAND_DISPLAY=nonexistent cargo run` | `rubickway: cannot connect to a Wayland session...`, status 1 | FR-013 |
| M17 | Start nested in a nested host (e.g. a nested niri/sway window), then close that host | `rubickway: lost the connection...`, status 1, no hang | FR-014 |
| M18 | Press Ctrl+C in the terminal | Exits with status 0 | contracts/cli.md |
| M19 | Start a drag, then switch workspace with the host shortcut while holding the button | The drag ends; the view stays where it was | Edge cases |

Record results (host name and version, pass/fail per step) in the PR description.
