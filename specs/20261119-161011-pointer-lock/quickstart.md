# Quickstart: Pointer Lock

Manual checks on the laptop, started from a text console (AGENTS.md, "Hardware code"). The automated
tests cover the rules headless; these checks cover a real mouse or touchpad and real applications
(spec Success Criteria).

## Prerequisites

- RubickWay built from this branch: `cargo build --release`.
- A mouse (a touchpad works too, but turning many times is easier with a mouse).
- Applications:
  - `firefox`, with the MDN pointer lock demo: <https://mdn.github.io/dom-examples/pointer-lock/>;
  - a native first-person game with mouse-look on Wayland, for example Xonotic
    (`SDL_VIDEODRIVER=wayland xonotic-sdl`);
  - `gamescope` (it runs its own XWayland inside, so X11 games work through it);
  - `blender`;
  - for confinement: `weston-confine` if the installed `weston` package has it
    (`pacman -Ql weston | grep confine`), otherwise a game with a windowed "confine mouse" option
    (for example 0 A.D. or OpenRA);
  - `kitty` as a second application.
- Start from a text console: `RUST_LOG=rubickway=debug target/release/rubickway 2> /tmp/rubickway.log`.
- After the checks, quit (Mod+Shift+Q twice) and run
  `grep -E "frame work|lagging behind|libinput error|protocol error|WARN" /tmp/rubickway.log`.

## US1: mouse-look and the release shortcut

| # | Do | Expect |
| --- | --- | --- |
| Q1 | In Firefox, open the pointer lock demo and click the canvas. Move the mouse far in one direction, well past where a screen edge would be. | The cursor disappears and stays; the demo's ball keeps moving the whole time. |
| Q2 | While the demo holds the pointer, press Mod+Escape. Then click the canvas again. | The cursor is back and moves freely, visible also over Firefox; the page did not see an Escape key. Firefox ignores `unlocked`, so the page still thinks it holds the pointer and the ball follows the free cursor oddly until the click or a plain Escape - Firefox's own state (research L16). After the click the demo captures the pointer again (FR-008). |
| Q3 | Start the game, enter a level, turn in one direction for at least 10 full turns. | The view never stops turning (SC-001). |
| Q4 | In the game, press Mod+Escape, move the cursor around, then click inside the game window. | The cursor is free after Mod+Escape, every time on the first press (SC-002); mouse-look resumes after the click. |
| Q5 | In the game with mouse-look, press Mod+Tab to kitty, type something, then Mod+Tab back while the cursor is over the game. | In kitty the cursor is free; back in the game, mouse-look resumes without a click (FR-006). |
| Q6 | Run the same game through gamescope (`gamescope -- <game>`), repeat Q3 and Q4. | The same. Skipped (2026-11-22): gamescope 3.17 does not start in RubickWay with either backend (missing `wp_presentation`, "Failed to create backend"); see spec Assumptions. |
| Q7 | In Blender, drag a numeric field (for example the cube's X location) left and right well past the screen edge; orbit the view with the middle button the same way. | The value keeps changing past the edge; when you let go, the cursor reappears near where the drag started (the release point, FR-007), not at the screen edge. |

## US2: confinement

| # | Do | Expect |
| --- | --- | --- |
| Q8 | Start `weston-confine` (or the game with "confine mouse"), click into it, then flick the mouse hard toward every edge and corner, about 100 times. Repeat at zoom 50%, 100% and 200% (Mod + scroll on the background first, then focus the window again). | The cursor never leaves the area; it slides along the edges (SC-003). |
| Q9 | While confined, press Mod+Escape and move away. | The cursor leaves the window freely. |
| Q9a | Optional, if you use a remote desktop: connect with a Wayland remote desktop client (for example Remmina or `wlfreerdp`) to another machine, click into the session, move and drag. | The remote cursor follows; the local cursor stays inside the session window (or is locked, depending on the client); Mod+Escape frees it. Skipped: say so in the PR; the automated tests cover the protocol. |

## US3: the canvas while the pointer is held

| # | Do | Expect |
| --- | --- | --- |
| Q10 | With mouse-look active in the game, hold Mod and left-drag: pan the canvas a long way, release. | The canvas pans; the view in the game does not turn during the pan; afterwards mouse-look resumes, and with Mod+Escape the cursor appears on the same spot of the game window as before the pan (SC-004). |
| Q11 | Same with Mod + scroll (zoom in and out) and Mod+Shift + left-drag (move the game window). | Zoom and move work; mouse-look resumes after each. |
| Q12 | Put the confined window on the external monitor at 50% zoom; flick toward the side facing the laptop panel. | The cursor stays inside the window as shown on that monitor, and never crosses to the panel (FR-013). Skipped (2026-11-22): the owner's external monitor is on the NVIDIA GPU's HDMI port, which RubickWay does not open; covered by the two-viewer shell tests. |
| Q13 | With mouse-look active, press Mod+0, then Mod+Return, then switch to a text console (Ctrl+Alt+F2) and back. | Each works as usual; after the console switch, mouse-look resumes when the cursor is over the game and it has the focus. |

## Log

| # | Do | Expect |
| --- | --- | --- |
| Q14 | After the checks, look at the debug log: `grep -E "pointer (un)?(locked|confined)" /tmp/rubickway.log`. | One line per start and stop, naming the application, single-use or lasting, and the reason (FR-015); no coordinates. The grep from Prerequisites shows nothing new. |
| Q15 | From the Q14 lines, note which applications used single-use and which lasting constraints. | Recorded in the PR. Single-use and lasting behave differently after Mod+Escape and focus changes (FR-006, FR-008); if every application used one kind, the other is covered only by the automated tests - say so in the PR. |
