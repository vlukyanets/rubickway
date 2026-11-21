# Contract: Input behaviour while a constraint applies

What the user's input does while an application holds the pointer (FR-004 to FR-009). Everything
else stays as in `specs/20261118-130629-popups/contracts/input.md` and the README controls table.

## New shortcut

| Input | Effect |
| --- | --- |
| Mod+Escape | stops the lock or confinement that applies, if any; never reaches the application. A single-use one ends, a lasting one waits; until the window receives a button press (not one taken by a binding), none of its constraints applies - also one the application asks for afterwards (M11) - and the cursor shows over it even if the application hides it (research L16) |

Built in, like Mod+Tab: a configured spawn binding on Mod+Escape is refused with the existing
warning "the combination is a built-in shortcut".

## Pointer motion

| Situation | Pointer on screen | The window gets |
| --- | --- | --- |
| no constraint, pointer over a window | moves | `motion`, `relative_motion` (if subscribed) |
| pointer over the background or a title bar or border | moves | nothing |
| lock applies | stays | `relative_motion` only |
| confinement applies | moves, clamped to the effective area, sliding along its edge | `motion` (clamped position), `relative_motion` (full movement) |
| pan, window move or resize running (with or without a constraint) | moves as the operation needs | nothing |
| screen edge reached | stops at the edge | `relative_motion` keeps the full movement |

## Other input while a constraint applies

| Input | Effect |
| --- | --- |
| buttons, scroll, keys not bound | to the window, as usual |
| Mod + left-drag | pans; the constraint is suspended silently and applies again when the pan ends, the pointer on the same point of the window |
| Mod + scroll, pinch | zooms around the pointer; the constraint keeps applying to the window as now shown |
| Mod+Shift + left-drag | moves the window; suspended silently, applies again after the move |
| Mod + right-drag | resizes; suspended silently; afterwards stops (told) if the pointer is outside the area |
| Mod+Tab, a new focused window, the window closing | the constraint stops (focus) |
| Mod+Q, spawn bindings | as usual |
| Mod+0 (or another jump of the view under a still pointer) | as usual; if the pointer is no longer over the area, the constraint stops ("pointer left"); a lasting one applies again when the pointer is over the area again |
| the application maximizes, goes fullscreen or leaves it | the pointer stays on screen; the constraint applies on if the pointer is still over the area, else stops ("pointer left") |
| Ctrl+Alt+F*n* (console switch) | stops (focus); a lasting one applies again on return when the conditions hold |
| a grabbing popup of the window opens | stops (focus): a single-use one ends, a lasting one applies again once the popup closes |
| the monitor with the pointer is unplugged | the pointer moves to another monitor; the constraint stops ("pointer left"); a lasting one applies again when the conditions hold |

## Crate API (changed, for tests and backends)

- `InputEvent::PointerMoved { delta, unaccel, time_us }`
- `ClientInput::RelativeMotion { window, delta, unaccel, time_us }`
- `ClientInput::Constraint { token, change }`, `ConstraintChange`, `ConstraintEnd`
- `WindowEvent::ConstraintChanged { id, constraint }`, `ConstraintInfo`, `ConstraintKind`,
  `ConstraintToken`
- `Action::ReleasePointer`

These are public types of the library crate used by the binary and the tests; changing them is
covered by this plan (AGENTS.md, Boundaries).
