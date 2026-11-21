# Implementation Plan: Pointer Lock

**Branch**: `worktree-pointer-lock` | **Date**: 2026-11-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261119-161011-pointer-lock/spec.md`

## Summary

Relative pointer motion and pointer constraints (lock and confinement) for applications, with a
release shortcut and the canvas still usable while an application holds the pointer.

- **Who decides.** The server keeps the protocol objects and their double-buffered state and reports
  each window's constraint to the shell (`WindowEvent::ConstraintChanged`); the shell decides when it
  applies, holds or clamps the pointer, and tells the server (`ClientInput::Constraint`), which sends
  `locked`/`unlocked` or `confined`/`unconfined` (research L2, L3). The rules are pure shell logic,
  tested headless.
- **Relative motion.** The backend adds libinput's unaccelerated delta and microsecond time to
  `PointerMoved`; the shell sends `RelativeMotion` to the window that has the pointer, accelerated
  delta divided by the zoom, unaccelerated unchanged, never shortened by edges (L4).
- **Lock** keeps the layout pointer still; **confinement** clamps it to the effective area (the
  constraint's area, the window's input shape and the part of the window on the pointer's monitor),
  sliding along the edge (L8, L9). A lock that ends moves the pointer to the application's release
  point if it is inside the window (L9).
- **The canvas wins.** Pans, moves and resizes suspend a constraint silently; afterwards it applies
  again if its conditions still hold (L6). Mod+Escape stops it; a lasting one waits for a click (L7).
- **Robustness.** A confinement over more than 256 rectangles uses the bounding box (L10; `wl_region`
  itself stays unlimited, as in other compositors, with limits noted as a possible later change);
  relative pointers are capped at 32 per client (L11); one constraint per surface
  (`already_constrained`).

Details: [research.md](research.md) L1-L14.

## Technical Context

**Language/Version**: Rust 1.100, edition 2024

**Primary Dependencies**: `wayland-server` 0.31 and `wayland-protocols` 0.32 (already used, feature
`unstable` already on: `wp::relative_pointer::zv1`, `wp::pointer_constraints::zv1`); `input` 0.10
(`dx_unaccelerated`, `time_usec`, already available). No new dependency.

**Storage**: none; no configuration options (Mod+Escape is built in, spec Clarifications).

**Testing**: `cargo test` (debug and release).
- `tests/pointer_constraints.rs` (new): the shell's rules through `Shell::apply` and `Shell::handle`:
  relative motion (who gets it, zoom scaling, edges, pans), lock, confinement on one and two viewers,
  activation and its conditions, single-use and lasting, Mod+Escape and click to capture again, focus
  changes, popup grabs, pans, zooms, moves and resizes, release point, area changes, window gone.
- `src/shell/constraint.rs` unit tests for `confine`, plus a proptest: from a point inside the area,
  the result is always inside.
- `tests/protocol/pointer_constraints.rs` (new): globals, creation, `already_constrained`, defunct and
  re-creation, double-buffered area and hint, `ConstraintChanged` reports (roots only, after
  `Mapped`), `locked`/`unlocked`/`confined`/`unconfined` from decisions, stale tokens, relative motion
  events (time split, fixed-point values), the relative-pointer cap.
- `tests/protocol/misbehaving.rs`: contract rows M1-M11.
- `tests/protocol/surfaces.rs`: the globals-and-versions list gains the two globals.
- `src/backend/drm/input.rs`: `translate` fills the new fields.
- `src/input/bindings.rs`: Mod+Escape for each Mod key.
- SC-005 headless: one client floods (10,000 constraints, a 10,000-rectangle area, 10,000 relative
  pointers) in one batch; a second client's input in the same step still arrives in that step.
- Real hardware: [quickstart.md](quickstart.md) Q1-Q15 (Q9a optional).

**Target Platform**: Linux with DRM/KMS, the owner's laptop (Arch) and an external monitor; Firefox,
a native SDL game, gamescope, Blender, weston-confine or a game with mouse confinement.

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: one motion event costs O(rectangles of one area) for the shell and at most 32
relative-motion sends for the server; nothing per window or per client beyond that (SC-005).

**Constraints**:
- no I/O and no waiting in handlers (Principle VII);
- all arithmetic on client values in `f64` from `i32`, no overflow; areas and hints validated against
  the window before use (Principle II);
- no new `unsafe`.

**Scale/Scope**: about 2-3k lines including tests; two new globals; public crate types change
(contracts/input.md).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS | Protocol code through `wayland-server` and `wayland-protocols`; motion data from libinput. References named in research (wlroots, sway, mutter, niri), reading only. |
| II. Client can't take it down | PASS | One constraint per surface (protocol error to the offender); confinement bounded at 256 rectangles (bounding box beyond), relative-pointer cap 32; areas and hints validated; stale tokens ignored. The spec's 7 misbehaving cases and the contract's M1-M11 each get a test with a second, well-behaved client. Mod+Escape always frees the pointer. |
| III. Test-first, headless | PASS | The rules are shell logic tested through `Shell` (no Wayland, no GPU); protocol behaviour through in-process clients. Tests first in each story. The real mouse and applications in the quickstart. |
| IV. Backend-independent core | PASS | The backend only adds two fields to a backend-neutral event. Constraint rules are in the shell on canvas types (`Rect`, `Point`); the canvas model and viewer do not change. |
| V. Contained unsafe | PASS | No new `unsafe`. |
| VI. No speculative generality | PASS | Both globals at version 1, the only version. No configuration option; reassigning Mod+Escape is noted as a possible later change, not built. No traits; one `ConstraintKind` enum for lock and confine. |
| VII. Never block the loop | PASS | Handlers store small values; per-motion work is bounded (Performance Goals); no I/O. |
| VIII. 2D core with plugins | PASS | Core only. |

Post-design re-check (after data-model, contracts and quickstart): all PASS.

## Project Structure

### Documentation (this feature)

```text
specs/20261119-161011-pointer-lock/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (L1-L14)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q15 manual checks)
|-- contracts/
|   |-- protocols.md     # globals, requests, events, misbehaving cases M1-M11
|   `-- input.md         # Mod+Escape and input while a constraint applies; crate API changes
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- input/
|   |-- mod.rs                 # PointerMoved { delta, unaccel, time_us }
|   `-- bindings.rs            # Action::ReleasePointer, Mod+Escape in builtin_table
|-- backend/drm/input.rs       # RawEvent::Motion gets dx/dy_unaccel and time_us
|-- shell/
|   |-- constraint.rs          # NEW: Constraint, ConstraintState, effective area, confine()
|   |-- route.rs               # ClientInput::RelativeMotion, ClientInput::Constraint, ConstraintEnd
|   `-- mod.rs                 # WindowEvent::ConstraintChanged, ConstraintInfo; relative_motion
|                              # holds/clamps the pointer; update_constraint after handle/apply;
|                              # Mod+Escape; click to capture again; release point
`-- server/
    |-- pointer_constraints.rs # NEW: manager, locked and confined pointers, per-surface map,
    |                          # commit of area and hint, ConstraintChanged reports, decisions
    |-- relative_pointer.rs    # NEW: manager and relative pointers, cap 32 per client
    |-- compositor.rs          # commit applies constraint state (wl_region unchanged)
    |-- seat.rs                # deliver RelativeMotion and Constraint (the shell alone keeps
    |                          # motion from a locked window, research L9)
    `-- mod.rs                 # globals; state fields

tests/
|-- pointer_constraints.rs     # NEW: shell rules
`-- protocol/
    |-- main.rs                # + mod pointer_constraints
    |-- pointer_constraints.rs # NEW
    |-- surfaces.rs            # globals list
    `-- misbehaving.rs         # + M1-M11
```

**Structure Decision**: same single crate. The rules sit in the shell next to pointer routing, like
popup placement; the server modules are thin protocol layers, like `cursor_shape.rs`.

### Order of work (by user story)

Each step leaves a working desktop.

1. **Foundation**: the backend's new fields and `PointerMoved`; the crate types
   (`ConstraintToken`, `ConstraintInfo`, `ConstraintKind`, the new `WindowEvent` and `ClientInput`
   variants) with the shell ignoring them; the two globals with inert objects. No behaviour change.
2. **US1 (P1)**: relative motion end to end; locks: server objects, reports, the shell's activation
   rules, holding the pointer, release point, Mod+Escape and click to capture again, focus changes.
   Hardware check: Q1-Q7.
3. **US2 (P2)**: confinement: `confine()`, the effective area, area changes. Hardware check: Q8-Q9a.
4. **US3 (P3)**: suspension during pans, moves and resizes; zoom; two viewers. Hardware check:
   Q10-Q13.
5. **Polish**: remaining misbehaving rows, logging (L13), README (controls table: Mod+Escape, a
   paragraph on games and pointer capture), AGENTS.md if a gotcha shows up, Q14-Q15.

### Supporting changes

- `README.md`: Mod+Escape in the controls table; under running applications, games and 3D tools can
  capture the pointer and how to take it back; note that a spawn binding on Mod+Escape is refused.
- Earlier features' contracts are left as they were; this feature's contracts say what changed.

## Complexity Tracking

No violations to justify.

Known pre-existing gap, left as is by the owner's decision (2026-11-19): `wl_region` has no limit on
rectangles, so a client can make its own input hit-test slow; other compositors set no limit either.
Limits may be added later as a separate change (research L10).
