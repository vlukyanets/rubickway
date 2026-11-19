# Contract: input with popups

Extends `specs/20261102-143401-drm-backend/contracts/input.md` (and through it the earlier input
contracts). `Mod` = the configured `mod-key`. Everything not listed here behaves as there.

## While an application has a grab (a menu is open)

| Where | Input | Effect |
| --- | --- | --- |
| a window or popup of that application | motion, buttons, scroll | delivered as usual (FR-008); a click on its window outside the menus goes to the application, which closes its menus itself |
| another window of that application (not the grab's window) | button press | delivered (owner events); the click focuses and raises that window as usual, and the focus change closes all popups (FR-010) |
| the grab's window or one of its popups, button held | motion | the pointer moves between that window's surfaces and its popups: press on a menu button, drag to an item, release picks it |
| anywhere else (background, another application) | motion | nothing; that window gets `leave` |
| anywhere else | button press without a binding | all popups of the grabbing application close, topmost first; the press and its release are swallowed (FR-009) |
| anywhere | a pointer binding (Mod + left, Mod + right, Mod+Shift + left) | all popups close (below), then the gesture starts at once; not swallowed |
| anywhere else | scroll without Mod | nothing (Mod + scroll zooms and closes all popups, below) |
| keyboard | keys that are not bindings | to the topmost grabbing popup (FR-008); Escape, arrows and Enter are the application's |

## Closing popups (FR-010)

All open popups of every application close, topmost first, before RubickWay does any of these, which
then work as without popups:

| Action |
| --- |
| any key binding (Mod+Tab, Mod+Q, Mod+0, spawn bindings, Mod+Shift+Q, ...) |
| a pointer binding: move (Mod + left), resize (Mod + right), pan (Mod+Shift + left) |
| a left press on the background with no grab open (it pans) |
| Mod + scroll or pinch (zoom), or anything else that changes any viewer's view |
| keyboard focus moving to another window |
| any window entering or leaving maximize or fullscreen, whoever asked (a RubickWay action or the application) |
| a display unplugged |

While a grab is open, a left press on the background has no binding of its own: it is a press outside
(above), closes the grabbing application's menus and is swallowed; it does not start a pan. A second
press does. Without a grab (only tooltips open), it pans as usual and closes the tooltips first.

## Popups without a grab (tooltips)

| Input | Effect |
| --- | --- |
| pointer over the tooltip | it gets the pointer like any surface of its application |
| clicks elsewhere | nothing happens to it (FR-014) |
| keyboard | stays with the focused window |
