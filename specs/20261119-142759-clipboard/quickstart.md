# Quickstart: Clipboard

Manual checks on the laptop, started from a text console (AGENTS.md, "Hardware code"). The automated
tests cover the rules headless; these checks cover real applications (spec Success Criteria).

## Prerequisites

- RubickWay built from this branch: `cargo build --release`.
- Applications: `firefox`, `kitty`, `gtk4-demo` (package `gtk4-demos`), Qt 6 Assistant (`assistant6`,
  package `qt6-tools`), `wl-clipboard`, `cliphist`, `wl-clip-persist`
  (`sudo pacman -S --needed firefox kitty gtk4-demos qt6-tools wl-clipboard cliphist wl-clip-persist`;
  `wl-clip-persist` may be in the AUR).
- Start from a text console: `RUST_LOG=rubickway=debug target/release/rubickway 2> /tmp/rubickway.log`.
- After the checks, quit (Mod+Shift+Q twice) and run
  `grep -E "frame work|lagging behind|libinput error|protocol error|WARN" /tmp/rubickway.log`.

## US1: copy and paste between applications

| # | Do | Expect |
| --- | --- | --- |
| Q1 | Copy a sentence in Firefox (select, Ctrl+C), paste in kitty (Ctrl+Shift+V); then copy a word in kitty (Ctrl+Shift+C) and paste in Firefox's address bar. 10 times each. | Exactly the copied text each time (SC-001). |
| Q2 | Same between `gtk4-demo` (any text entry) and Assistant's search field, both directions. | Exactly the copied text (SC-001). |
| Q3 | Copy an image in Firefox (right-click, Copy Image), paste in an application that takes images (for example a GTK or Qt text editor with image support, or `wl-paste -t image/png > /tmp/i.png`). | The image arrives (SC-002). |
| Q4 | Copy in Firefox, then press Ctrl+C and immediately Mod+Tab to kitty, paste. | The copied text (spec Clarifications: a quick copy is not lost). |
| Q5 | Copy in Firefox, close Firefox (without a clipboard manager running), paste in kitty. | Nothing is pasted; kitty does not hang. |
| Q6 | Copy in a window on one side of the canvas, pan and zoom far away (Mod + drag, Mod + scroll), paste in another window there. | Works the same (FR-009). |

## US2: clipboard tools and managers

| # | Do | Expect |
| --- | --- | --- |
| Q7 | In kitty: `echo hi \| wl-copy`, then paste in Firefox. Then copy in Firefox and run `wl-paste`. | `hi` in Firefox; Firefox's text from `wl-paste` (SC-003). |
| Q8 | Run `wl-paste --watch cliphist store &`, copy three different texts in different applications, then `cliphist list`. | All three are listed. |
| Q9 | Run `wl-clip-persist --clipboard regular &`, copy in Firefox, close Firefox, paste in kitty. 10 times. | The text is still pasted (SC-003). |

## US3: the primary selection

| # | Do | Expect |
| --- | --- | --- |
| Q10 | Copy a word in Firefox (Ctrl+C). Then select another word in kitty with the mouse and middle-click in Firefox's address bar; then press Ctrl+V there. | The middle click inserts the selected word; Ctrl+V the copied one (SC-004). |
| Q11 | Put `primary-selection off` in `~/.config/rubickway/config.kdl`, restart RubickWay, repeat Q10. | The middle click inserts nothing; Ctrl+V still works. Remove the line afterwards. |

## Robustness

| # | Do | Expect |
| --- | --- | --- |
| Q12 | `head -c 100M /dev/urandom \| base64 \| wl-copy`, then in kitty `wl-paste > /dev/null` while panning the canvas with Mod + drag. | Panning stays smooth; no `frame work` or `lagging behind` line in the log (SC-005). |
| Q13 | `wl-copy --foreground hi` and press Ctrl+Z in that kitty (the tool is stopped and never answers), then paste in Firefox. | Firefox waits or gives up on its own; RubickWay and other windows keep working. `fg` and Ctrl+C afterwards. |
| Q14 | Copy in Firefox, switch to another console (Ctrl+Alt+F2) and back, paste in kitty. | The text (FR-009). |
