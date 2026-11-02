# Quickstart: Window Management on the Canvas

How to prove the feature works. Automated checks first, then manual steps that need a real
host session and a real application (Principle III).

## Prerequisites

- Everything from `specs/20261101-175316-client-windows/quickstart.md` (kitty with
  `LIBGL_ALWAYS_SOFTWARE=1`).

## Automated

Run the *Full check* from `AGENTS.md`. Added coverage, headless:

| Area | Proves |
| --- | --- |
| shell unit + property tests | anchors (FR-007), size clamping and its precedence (FR-008), 3 x 3 edge pick with boundaries and ties (FR-006), MRU order and cycling, commit on Mod release or focus loss (FR-011), view fitting with 5% margin, no move for a fully visible window, 10% floor (FR-012), maximize and fullscreen geometry and restore, fullscreen over maximized (FR-013, FR-014) |
| shell routing tests | Mod + Shift + left moves and Mod + right resizes with no client input (FR-002, FR-006), move keeps the grabbed point under the pointer at zoom 0.1 to 10 and while zooming (FR-003, SC-001), other input during an operation (Edge Cases), opposite edge fixed for any answered size (FR-007, SC-002), request acceptance only while the button is held (FR-016), maximize/fullscreen only from the focused window (FR-019), new windows during fullscreen and dialogs ending it (FR-020), shortcuts consumed (FR-010, FR-011, FR-017), fullscreen left by navigation, move and resize gestures and the window disappearing, cursor shapes (FR-009) |
| protocol tests (in-process clients) | move/resize serial checks and `invalid_resize_edge`, configure sizes and states, coalescing with one outstanding size, min/max size applied on commit, initial state in the first configure and `set_parent` (FR-019, FR-020), `wm_capabilities`, `close`, `Updated.configured`, misbehaving cases from the spec's Edge Cases (SC-006) |
| pixel assertions | a fullscreen window drawn alone on black, no grid |

## Manual (host session required)

Start `RUST_LOG=rubickway=debug cargo run` and kittys as in client-windows. Use
`-o remember_window_size=no -o initial_window_width=60c -o initial_window_height=15c` for small
windows.

| # | Steps | Expected | Proves |
| --- | --- | --- | --- |
| W1 | Open two kittys (they overlap); drag one by kitty's own title bar | it follows the pointer exactly and stays where dropped; it is focused and on top | US1/AC1, AC5; FR-001, FR-004; SC-005 |
| W2 | Alt + Shift + left-drag inside the other kitty | it moves; kitty shows no selection or click | US1/AC2; FR-002 |
| W3 | Start a drag, then Alt + scroll to zoom out, keep dragging, zoom back in, release | the window stays under the pointer throughout and lands at the new place | US1/AC3, AC4; FR-003; SC-001 |
| W4 | Drag kitty's bottom-right border, then its top-left border | it resizes; the opposite corner stays put; kitty snaps to cells without jumping | US2/AC1, AC3; FR-005, FR-007; SC-002, SC-004, SC-005 |
| W5 | Alt + right-drag near the left edge, then near the bottom-right corner, then in the centre | left edge resize / corner resize / nearest-corner resize; the move/resize cursors show | US2/AC2; FR-006, FR-009 |
| W6 | Spread three kittys far apart, pan into empty canvas, press Alt+Tab repeatedly, then Alt+Tab twice with Alt released in between; then Alt+Tab to a window already fully on screen | each press shows the next window centred and focused; two short presses switch back and forth; the visible window is focused without the view moving | US3/AC1-AC3; FR-011, FR-012; SC-003 |
| W7 | Alt+Shift+Tab | cycles in the opposite order | US3/AC4; FR-011 |
| W8 | Focus a kitty, press Alt+Q; then run `sleep 100` in another and press Alt+Q | the first closes; the second asks for confirmation (kitty's own dialog) | US4; FR-010; SC-005 |
| W9 | Double click kitty's title bar; pan away and back; double click again | maximized: view at 100% centred on the window, which fills the viewer; it stays maximized while panning; restored to its old size and place | US5/AC1, AC3; FR-013, FR-015; SC-005 |
| W10 | Toggle kitty fullscreen (Ctrl+Shift+F11), type, then Alt + left-drag | it fills the viewer alone on black; typing works; the drag leaves fullscreen and restores the old view, window size and place | US5/AC2, AC3; FR-014 |
| W11 | Drag a maximized kitty by its title bar | it returns to its old size under the pointer and moves | US5/AC3; Edge Cases "Dragging a maximized window" |
| W12 | Resize the RubickWay host window while kitty is maximized, then fullscreen | kitty follows the new viewer size | Edge Cases "Viewer resized" |
| W13 | In a host terminal run `sleep 3; kill -9 <kitty pid>`, then start a move of that kitty and hold the button; repeat with a resize | the operation ends, the cursor returns to normal, nothing else changes | Edge Cases "Window disappears mid-drag"; SC-006 |
| W14 | Pan, zoom, Alt+0 and Alt+Shift+Q as before | unchanged behaviour | FR-017 |
| W15 | Start `kitty --start-as=maximized`, then `kitty --start-as=fullscreen` | each opens directly in that state, with no visible resize after appearing | FR-019 |
| W16 | Maximize a kitty, toggle fullscreen, toggle fullscreen again, then double click the title bar | fullscreen, back to maximized, then back to its normal size and place | FR-013, FR-014; Edge Cases "Maximized and fullscreen together" |
| W17 | In a fullscreen kitty run `sleep 2; kitty &`, then press Alt+Tab | the fullscreen kitty stays and keeps focus; Alt+Tab leaves fullscreen and shows the new kitty centred in the old view | FR-020 |
