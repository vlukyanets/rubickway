# Quickstart: Popups

Manual checks with real applications on the laptop (Principle III). Automated tests cover the rest
with no device: placement (`src/shell/positioner.rs`), the shell's grab and dismissals
(`tests/shell.rs`), the protocol, every misbehaving case and a popup's place among its window's
surfaces (`tests/protocol/popups.rs`).

## Prerequisites

- Logged in on a text console, no desktop running; SSH session open for recovery.
- `cargo build --release --examples`.
- Keep the log: `RUST_LOG=rubickway=debug cargo run --release 2> /tmp/rubickway.log`.
- Applications with menus: Firefox, a GTK 4 program (`gtk4-demo` from Arch's `gtk4-demos`), a Qt 6
  program (for example `qt6ct` or `assistant` from `qt6-tools`). kitty has no menus. If one of them
  is not at hand, write down which checks were skipped.
- A second monitor on HDMI for Q12.

## US1: menus open and can be used

| # | Do | Expect |
| --- | --- | --- |
| Q1 | In Firefox, right-click on a page; pick "Reload" with the pointer. 10 times. | The menu appears at the pointer, above the window, at once; the page reloads; the menu closes. 10 out of 10 (SC-001). |
| Q2 | Right-click again; pick an item with the arrow keys and Enter. Then open it and press Escape. 10 times each. | The keys move through the menu, not the page (FR-008); Enter picks; Escape closes it. |
| Q3 | Open a context menu; click the canvas background. Open it again; click another application's window (kitty). Open it again; click Firefox's page outside the menu. | Background and kitty: the menu closes; nothing else happens: no pan, kitty is not focused (FR-009); a second click acts as usual. Firefox's page: Firefox gets the click and closes the menu itself. |
| Q4 | In `gtk4-demo`, open a demo with a dropdown (for example "Lists" -> "Selections"); click the dropdown, pick an item. | The list appears at the box; the item is set. |
| Q5 | In a GTK or Qt program with a menu bar, open the first menu and move the pointer along the bar. | The menus follow the pointer from one to the next. |
| Q6 | Press on a menu button, keep the button held, move onto an item, release. | The item is picked (press, drag, release). |
| Q7 | Repeat Q1-Q2 in the Qt 6 program and `gtk4-demo`. | Same results (SC-001). |

## US2: submenus and menus at the edge

| # | Do | Expect |
| --- | --- | --- |
| Q8 | Move Firefox so its right edge is at the right edge of the monitor. Right-click near that edge; open a submenu, then one more level (bookmarks folders). 10 times. | Each level is fully on the monitor; submenus open to the left when there is no room on the right (SC-002). |
| Q9 | Same at the bottom edge with a long menu (bookmarks menu with many entries). | The menu is moved up or made shorter, fully on the monitor; it scrolls if it was made shorter. |
| Q10 | Zoom the canvas to about 50% (Mod + scroll), right-click in Firefox. | The menu is drawn at 50%, like the window, right at the pointer (FR-019). It fits inside the monitor. |
| Q11 | Run `target/release/examples/popups` from kitty. Use its keys (it prints them): menu with each adjustment, the menu that allows none, nested menus, the reactive popup, the reposition, the tooltip. Do it near each monitor edge. | Each popup ends where research P3 says: flipped, slid or resized as allowed; the "no adjustment" popup stays partly off the monitor. |
| Q12 | Place Firefox across both monitors. Right-click on the part on the right monitor, then on the part on the left one. | The menu fits the monitor the pointer is on, and is shown on both monitors where they show it. |

## US3: tooltips and popups without a grab

| # | Do | Expect |
| --- | --- | --- |
| Q13 | Hover over Firefox's toolbar buttons until tooltips appear; move from one to the next. | Tooltips appear, move or change, and go away when the pointer leaves the toolbar. |
| Q14 | Type in Firefox's address bar. | The suggestion list appears; typing continues in the address bar. |
| Q15 | With a tooltip shown, click elsewhere in the window. | RubickWay does not close it; Firefox decides. |

## US4: canvas actions close popups cleanly

For each action, open a Firefox context menu first, then do it; 10 times each (SC-004).

| # | Action | Expect |
| --- | --- | --- |
| Q16 | Mod + scroll (zoom) | The menu closes; the zoom happens. |
| Q17 | Mod+Tab | The menu closes; focus moves. |
| Q18 | Mod + left drag on the window (move) | The menu closes; the window moves. |
| Q19 | Mod+0 | The menu closes; the view resets. |
| Q20 | Mod+Return (spawn kitty) | The menu closes; kitty opens. |
| Q21 | Close Firefox from kitty (`pkill firefox`) | The menu goes with the window; nothing is left on the canvas. |
| Q22 | Unplug the HDMI monitor while a menu is open on it | The menu closes. |

## Smoothness

| # | Do | Expect |
| --- | --- | --- |
| Q23 | Open and close a Firefox context menu 100 times in a row. Then `grep -E "frame work|lagging behind|libinput error" /tmp/rubickway.log`. | No slow-frame line (`frame work ... ms`), no `lagging behind`, no leftover menu (SC-003). |

## Misbehaving applications

All misbehaving cases ([contracts/protocols.md](contracts/protocols.md)) are automated in
`tests/protocol/popups.rs` and `tests/protocol/misbehaving.rs`. None needs real hardware.
