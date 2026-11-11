# Quickstart: Cursor Shapes

Manual checks with real applications and real cursor themes on the laptop (Principle III). Automated
tests cover the rest with no device: which requests count, protocol errors, the shell's choice, the
image table, theme reading from directories made in the test, and the log lines.

## Prerequisites

- Logged in on a text console, no desktop running; SSH session open for recovery.
- `cargo build --release`.
- Keep the log: `RUST_LOG=rubickway=debug cargo run --release 2> /tmp/rubickway.log`. The debug
  line `application <id> asks for the <shape> pointer` shows what each application names
  ([contracts/cli.md](contracts/cli.md)).
- No cursor theme installed, as today: `/usr/share/icons/default/index.theme` inherits `Adwaita`, and
  `/usr/share/icons/Adwaita/cursors` does not exist. No `cursor` block in `config.kdl`.
- kitty 0.49 and mpv are installed. GTK 4 and Qt 6 programs are not installed. Their checks (Q10) are
  for when one is at hand.

## US1: the pointer shows what the application asks for (no theme)

| # | Do | Expect |
| --- | --- | --- |
| Q1 | Start RubickWay. Read the log. | One line: `cursor theme "default" has no pointer images (it inherits from Adwaita, not installed): RubickWay draws its own`. No other cursor theme line (SC-003). |
| Q2 | Open kitty. Move the pointer onto each of its four borders and four corners, 10 times each, from inside and from outside. | Left and right borders: left-right arrow. Top and bottom: up-down. Top-left and bottom-right corners: that diagonal; the other two: the other diagonal. 10 out of 10 (SC-001). The debug lines name `w-resize`, `ne-resize` and so on. If there are no `asks for the ... pointer` lines at all, kitty does not use the protocol here: stop and look into it (`WAYLAND_DEBUG=1 kitty 2>&1 \| grep cursor_shape`) before going on, as US1 rests on it. |
| Q3 | Move the pointer over kitty's text, 10 times. | The I-beam, 10 out of 10 (SC-001). |
| Q4 | Move from kitty to the background, and from kitty to a second kitty window. | The arrow over the background. Over the second window, its own shape as soon as it names one (FR-003). |
| Q5 | Place kitty across both monitors (HDMI connected). Move the pointer from one monitor to the other over its text and over its border. | The shape stays the same across the edge. |
| Q6 | In kitty, run `echo https://example.org` and hover over the URL. | The pointing hand, if kitty names `pointer` there (debug line); otherwise record which shape it names. |
| Q6a | From kitty, run the test program: `cargo run --release --example cursor_shapes`. It opens a window of 36 cells, one per shape in protocol order; over each cell it names that shape and prints the shape's name. Move the pointer over every cell. | Every cell shows an image (SC-002). The 19 FR-006 images can be told apart by eye at 24 px (FR-006). Single-border, column and row resizes show their axis' double arrow; alias, copy, move, dnd-ask and context-menu show the arrow, each with one log line the first time (FR-007). The hotspot of each sits where FR-008 says (checked against the cell borders). |

## US2: an installed theme takes priority

| # | Do | Expect |
| --- | --- | --- |
| Q7 | `sudo pacman -S adwaita-cursors`. Restart RubickWay with no `cursor` block. Repeat Q2 and Q3, and use the move, resize and pan gestures. | `pointer images from cursor theme "default"`. Adwaita's images for kitty's shapes and for RubickWay's gestures. If Adwaita lacks some shapes, one line names them, and those show the built-in images. |
| Q8 | Add `cursor { theme "Adwaitaa"; }` (a typo). Restart. | `cursor theme "Adwaitaa" from the configuration is not installed: using the default cursor theme`, then Adwaita's images as in Q7. |
| Q9 | Change it to `cursor { theme "Adwaita"; }`. Restart. Then, optionally, `sudo pacman -Rs adwaita-cursors` to go back to the laptop's usual state, and restart once more. | With Adwaita installed: `pointer images from cursor theme "Adwaita"`. After removing it: `cursor theme "Adwaita" from the configuration is not installed, and the default cursor theme has no pointer images: RubickWay draws its own pointer images` (the laptop's `default` theme is still there, inheriting the removed Adwaita), then the built-in images as in Q1-Q3. |

## Other applications

| # | Do | Expect |
| --- | --- | --- |
| Q10 | If a GTK 4 or Qt 6 program is at hand (for example `gtk4-widget-factory` from `gtk4-demos`), hover over its text fields, links, buttons and window borders. | The debug lines say which shapes it names, and those are shown. If it names none, it shows the arrow, as before; record which. |

## US3: nothing else changes

| # | Do | Expect |
| --- | --- | --- |
| Q11 | Over kitty's text (I-beam), move kitty with Mod+left drag, resize with Mod+right drag, pan with Mod+Shift+left drag. Let go with the pointer still over kitty. | During each gesture, RubickWay's own image (four arrows, the resize arrow of the corner, the closed hand). After it, the I-beam again (FR-005). |
| Q12 | Play a video in mpv: `mpv --loop <file>`. Keep the pointer still over it for 2 seconds, then move it. | Hidden while still, shown when moved, as before (US3 scenario 2). |
| Q13 | With the pointer over kitty's text, close kitty with its own shortcut (`Ctrl+Shift+Q`), with another window or the background under it. | The pointer shows what is under it now: that window's shape, or the arrow (US3 scenario 4). |
| Q14 | Move the pointer back and forth across kitty's border 100 times, at debug log level. | No `frame work` line and no `libinput error: ... lagging behind` line on stderr during it (SC-004). |

## Misbehaving applications

All misbehaving cases ([contracts/protocols.md](contracts/protocols.md)) are automated in
`tests/protocol/`. None needs real hardware.
