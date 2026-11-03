# Quickstart: Standalone Session on the Hardware

How to run RubickWay on the author's laptop and the manual checks for behaviour that exists
only on real hardware (Principle III). Automated tests cover everything else: `cargo test`.

## Prerequisites

- Logged in on a text console (`tty1`), no desktop running on the machine.
- Packages (Arch): `sudo pacman -S libinput seatd kitty ttf-dejavu` (`mesa`, `libxkbcommon`,
  `systemd` are already installed). The seatd daemon need not run: libseat talks to logind.
  kitty needs a scalable monospace font from fontconfig; a console-only machine often has only
  bitmap fonts (Terminus), with which kitty exits at once ("Line height too small: 0").
- Optional, for recovery during development: SSH access from another device, and an SSH
  session open before Q10.
- Build once: `cargo build --release` (or `cargo run --release` below).
- No `LIBGL_ALWAYS_SOFTWARE` is needed: without GPU buffers (dmabuf) Mesa's EGL in kitty falls
  back to its software renderer by itself (confirmed on the laptop, 2026-11-02). Do not export
  it for RubickWay, whose own Mesa would then render in software too.

Keep the log: `cargo run --release 2> ~/rubickway.log`, read it after exit
(`contracts/cli.md`).

## Automated checks (any machine, headless)

```sh
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo test --release --locked
```

## Manual checks

Each step: what to do, then what must happen. `Mod` is Super unless the configuration says
otherwise.

| # | Steps | Expected |
| --- | --- | --- |
| Q1 | No `~/.config/rubickway/config.kdl`. On tty1: `cargo run --release` | within 3 s the grid fills the built-in display at 1920x1080, origin centred, pointer in the centre (SC-001); the log names the display and the socket |
| Q2 | Move a finger on the touchpad; tap; two-finger scroll; Mod + two-finger scroll; pinch; Mod+0 | pointer follows and stops at the screen edges; the canvas pans, zooms around the pointer and resets, as nested did with Alt |
| Q3 | Press Mod+Shift+Q once, wait 3 s, press it once more | nothing happens either time |
| Q4 | Press Mod+Shift+Q twice quickly | within 2 s the console is back with its earlier content, `ls` types and runs, `echo $?` prints 0 (SC-002); repeat 10 times |
| Q5 | Start again; Mod+Return twice | two kittys open centred, the second focused; typing goes to the focused one |
| Q6 | With the kittys: title-bar drag, border resize, Mod+drag (moves the window), Mod+Shift+drag (pans, no focus change), Mod+right-drag, Mod+Tab, double click the title bar, kitty's fullscreen toggle, Mod+Q | everything as in window-management's quickstart (SC-006); pointer shapes change for pan, move and resize; the pointer stays visible over a fullscreen kitty |
| Q7 | `config.kdl`: `input { mod-key "Alt"; }` and `binds { Mod+T { spawn "kitty"; } }`; start | Alt is Mod; Alt+T opens kitty; Alt+Return reaches kitty; Alt + left drag moves a kitty |
| Q8 | `config.kdl` with a syntax error, then with `mod-key "Hyper"` and `binds { Mod+Q { spawn "kitty"; } }` | RubickWay starts each time with defaults for the bad settings; the log has one warning per problem with the line (SC-010) |
| Q9 | Two kittys open; Ctrl+Alt+F2, log in, run `ls`; Ctrl+Alt+F1 | tty2 works; back on tty1 within 2 s the canvas, view and both kittys are as before and respond; repeat 20 times (SC-003). Ctrl+Alt+F1 while on RubickWay's console does nothing |
| Q10 | Start a kitty move with Mod + left drag, press Ctrl+Alt+F2 mid-drag, come back. With one hand: in kitty run `sudo -v`, then `sleep 4; sudo chvt 2`, and start the drag (Mod or the title bar) before the 4 s pass | the drag ended where it was; no key is stuck in kitty |
| Q11 | From tty2: `pkill -TERM rubickway`, then start again and `pkill -HUP rubickway`, then `pkill -KILL rubickway` | each time tty1 shows a working console again (after Ctrl+Alt+F1 at the latest); no reboot (SC-004) |
| Q12 | Over SSH: `cargo run --release` | one line `rubickway: cannot open the seat: ...`, exit status 1, the laptop screen does not change (SC-007) |
| Q13 | On tty2 while tty1 is shown (`sleep 5; cargo run --release` on tty2, switch to tty1) | one line `rubickway: this console is not active ...`, exit 1, tty1 untouched |
| Q14 | Connect the HDMI monitor before starting; start | both displays show the canvas side by side at 100%, the monitor to the right, as one wide view; the log names both (SC-008) |
| Q15 | Move the pointer across the shared edge; zoom on one display; Mod+0 on each | the pointer crosses without a jump; only the display under the pointer zooms or resets |
| Q16 | Drag a kitty from the laptop display to the monitor, at different zooms | the window moves to the canvas region the monitor shows and appears at its zoom |
| Q17 | Unplug the monitor with a kitty on it, then plug it back in; repeat 10 times | after unplugging: RubickWay runs, the kitty is still on the canvas (Mod+Tab brings it to the laptop display), the pointer is on the laptop display; after plugging: the monitor shows the canvas again within 2 s (SC-008) |
| Q18 | Kitty fullscreen on the monitor, unplug it | fullscreen ends; the kitty has its previous position and size |
| Q19 | Plug a USB mouse in and out while running | it works within 1 s; unplugging changes nothing else |
| Q20 | Suspend (`systemctl suspend`), wake up | displays show the canvas again, touchpad and keyboard work (FR-029) |
| Q21 | With `RUST_LOG=rubickway=debug`, pan, zoom and drag kittys continuously for 10 s; then leave it idle and run `top` from tty2 or SSH | no visible tearing, and no frame-time gap in the debug log longer than two refresh intervals (SC-005); idle CPU under 1%. Note the pointer-move CPU use as the baseline for the cursor-plane feature |
| Q21b | Leave the pointer still over kitty for 3 s, then move it | the pointer disappears when kitty hides it (kitty's default `mouse_hide_wait 3.0` hides it after 3 s without motion, not on typing; `mouse_hide_wait -1` in `kitty.conf` hides it on typing instead) and comes back on motion (FR-008); during Mod+Shift+drag it is shown |
| Q22 | From a terminal inside a Wayland desktop (any other machine with one, or skip): `rubickway` | one line `rubickway: another program controls the display ...`, exit 1, no window, the desktop unaffected (US6) |
| Q23 | Debug build (`cargo run 2> ~/rubickway.log`), a kitty open; from tty2 or SSH: `pkill -USR1 rubickway`, switch back to tty1 | the screen stops changing; within 10 s (the default timeout) plus 1 s the console on tty1 is usable again, the log ends with `rubickway: the event loop did not respond for 10 seconds, exiting`, exit status 1 (FR-030, SC-004). Repeat with `watchdog-timeout-sec 3`: about 3 s. In a release build SIGUSR1 is not handled and ends the process at once, as `kill -9` |

| # | Steps | Expected |
| --- | --- | --- |
| Q24 | `config.kdl`, the left-button gestures swapped from the defaults: `binds { Mod+MouseLeft { pan-canvas; } Mod+Shift+MouseLeft { move-window; } Mod+MouseMiddle { resize-window; } }`; start, open two kittys | Super + left drag pans the canvas without changing focus; Super+Shift + left drag on a kitty moves it and focuses it; Super + middle drag (wheel click on a mouse) resizes; Super + right click reaches kitty as a plain click (kitty shows nothing for it); Super+Return still opens kitty. Without the file: the defaults of Q6 |

The watchdog is also checked automatically: its decision by unit tests, and the real thread and
exit by a subprocess test (exit status 1 within the timeout plus 1 s).
