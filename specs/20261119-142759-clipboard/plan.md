# Implementation Plan: Clipboard

**Branch**: `worktree-clipboard` | **Date**: 2026-11-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/20261119-142759-clipboard/spec.md`

## Summary

Copy and paste between applications, the primary selection and clipboard tools, replacing today's
stub (`src/server/data_device.rs`), which accepts everything and transfers nothing.

- **Where.** All in the server: a new `selection.rs` keeps the two slots (clipboard and primary),
  their owners and the devices to tell; the protocol modules translate requests into it. The shell
  does not change: the keyboard focus it decides already reaches the server (C2).
- **Who sees what.** The focused application's devices get the clipboard and the primary selection
  when it gains the focus (before the keyboard enter) and when they change; data-control devices get
  every change. Announcements happen once per dispatch batch, so a flood is told once (C3, C11).
- **Who may set it.** An application, with the serial of one of its 16 latest input events (focus not
  required, spec Clarifications); clipboard tools without a serial (C4).
- **Pasting.** The paste descriptor goes straight to the owner; RubickWay never reads or writes it
  (C5).
- **Protocols.** `wl_data_device_manager` 3 (selection parts), `zwp_primary_selection_device_manager_v1`
  1 (unless switched off in `config.kdl`), `ext_data_control_manager_v1` 1 and
  `zwlr_data_control_manager_v1` 2 (C1, C10). Drag-and-drop stays cancelled.

Details: [research.md](research.md) C1-C14.

## Technical Context

**Language/Version**: Rust 1.100, edition 2024

**Primary Dependencies**:
- `wayland-server` and `wayland-protocols` 0.32 (already used; `wp::primary_selection` and the staging
  `ext::data_control` modules, both already in the enabled features);
- **new**: `wayland-protocols-wlr` 0.3 with feature `server` (the older data-control protocol, C1),
  and with feature `client` as a dev-dependency for the tests. Needs the owner's approval of this plan
  (AGENTS.md, Boundaries).

**Storage**: none. RubickWay keeps no copy of content (FR-008). One configuration node,
`primary-selection` ([contracts/config.md](contracts/config.md)).

**Testing**: `cargo test` (debug and release).
- `tests/protocol/clipboard.rs` (new): offers to the focused application and its order before the
  keyboard enter, transfers through real pipes, replacement and cancel, serial acceptance and refusal
  (including after a focus change), stale offers, 256-type cap, primary selection, both data-control
  protocols, tools setting and reading, disconnects.
- `tests/protocol/misbehaving.rs`: the contract's misbehaving rows.
- `tests/protocol/surfaces.rs`: the stub's test (`get_data_device` accepted, nothing offered) changes
  to the new behaviour; the globals-and-versions test lists the new globals.
- `tests/config.rs`: `primary-selection`.
- Unit tests in `selection.rs` for the type list (distinct, cap) and slot generations.
- Real applications: [quickstart.md](quickstart.md) Q1-Q14.

**Target Platform**: Linux with DRM/KMS, the owner's laptop (Arch); Firefox, kitty, GTK 4, Qt 6,
wl-clipboard, cliphist, wl-clip-persist for the manual checks.

**Project Type**: desktop application (single binary + library crate for tests)

**Performance Goals**: a paste costs one event whatever its size; 100 MB pasted while panning leaves
no slow frame and no input lag line (SC-005).

**Constraints**:
- no I/O, no reading or writing descriptors, no waiting in handlers (Principle VII);
- at most one announcement per client per dispatch batch (C3);
- no new `unsafe`; descriptors are passed as owned values and closed by dropping them.

**Scale/Scope**: about 1.5-2.5k lines including tests; three new globals.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution 0.2.0.

| Principle | Status | How this plan complies |
| --- | --- | --- |
| I. Own implementation | PASS (see Complexity Tracking) | Protocol code through `wayland-server` and generated bindings: `wayland-protocols`, plus `wayland-protocols-wlr` for the older data-control protocol - a bindings crate of the same project, not a compositor framework. References named in research (wlroots, sway, mutter, Smithay's `selection` modules, niri), reading only. |
| II. Client can't take it down | PASS | RubickWay never touches the pipes, so a client that never writes or never reads affects only its peer. Type lists capped at 256. One announcement per batch bounds what one client's flood sends to others. Errors only where the protocols define them, to the offender. The spec's 9 misbehaving cases and the contract's 11 rows each get a test with a second, well-behaved client. |
| III. Test-first, headless | PASS | Every rule is protocol behaviour, tested in-process with test clients and real pipes; tests come first in each story. Real applications in the quickstart. |
| IV. Backend-independent core | PASS | Nothing in `shell/`, `canvas`, `viewer` or the renderer changes; the clipboard lives in the server, beside the other protocol state. |
| V. Contained unsafe | PASS | No new `unsafe`; descriptors are `OwnedFd`. |
| VI. No speculative generality | PASS | Only what the spec asks: no drag-and-drop, no history, no X11 bridge. The older data-control protocol is there because tools in use need it (spec Clarifications), to be dropped later. One switch (`primary-selection`), asked for by the owner. A `Source`/`Device` enum over the four protocols, no traits. |
| VII. Never block the loop | PASS | Handlers copy small values and send events; announcing is a loop over live devices once per batch; no I/O. |
| VIII. 2D core with plugins | PASS | Core only. |

Post-design re-check (after data-model, contracts and quickstart): all PASS.

## Project Structure

### Documentation (this feature)

```text
specs/20261119-142759-clipboard/
|-- spec.md
|-- plan.md              # this file
|-- research.md          # Phase 0 (C1-C14)
|-- data-model.md        # Phase 1
|-- quickstart.md        # Phase 1 (Q1-Q14 manual checks)
|-- contracts/
|   |-- protocols.md     # globals, every request and event, misbehaving cases
|   `-- config.md        # primary-selection
|-- checklists/
|   `-- requirements.md
`-- tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
|-- config.rs                 # Config.primary_selection; the `primary-selection` node
|-- standalone.rs             # server.set_primary_selection(config.primary_selection)
`-- server/
    |-- selection.rs          # NEW: SelectionState, Slot, Source, Device, SourceData, OfferData;
    |                         # set, clear, source_destroyed, announce, focus_changed, receive (C2-C7, C11)
    |-- data_device.rs        # the stub becomes wl_data_device/source/offer on top of selection.rs
    |-- primary_selection.rs  # NEW: zwp_primary_selection_* on top of selection.rs
    |-- data_control.rs       # NEW: ext_data_control_* and zwlr_data_control_* (same behaviour)
    |-- seat.rs               # input_serials (C4); keyboard_focus calls selection::focus_changed
    |                         # before wl_keyboard.enter
    `-- mod.rs                # Server::set_primary_selection; globals; selection state;
                              # selection::announce after dispatch and after deliver

tests/
|-- config.rs                 # primary-selection
`-- protocol/
    |-- main.rs               # + mod clipboard
    |-- clipboard.rs          # NEW
    |-- surfaces.rs           # stub test and globals list change on purpose
    `-- misbehaving.rs        # + the contract's cases
```

**Structure Decision**: same single crate; the clipboard is server-side protocol state like
`cursor_shape.rs` and `popup.rs`, with one shared model (`selection.rs`) under three thin protocol
modules.

### Order of work (by user story)

Each step leaves a working desktop.

1. **Foundation**: `selection.rs` with the slots, sources and devices, empty announcements; the
   `input_serials` ring in `seat.rs`; `Server::set_primary_selection`; the new dependency. No
   behaviour change yet.
2. **US1 (P1)**: the clipboard through `wl_data_device`: set with the serial rule, offers to the
   focused application before the keyboard enter, transfers, replacement, cancel, disconnects.
   Hardware check: Q1-Q6.
3. **US2 (P2)**: both data-control protocols: tools read, watch and set both slots. Hardware check:
   Q7-Q9.
4. **US3 (P3)**: the primary selection and the `primary-selection` switch. Hardware check: Q10-Q11.
5. **Polish**: the remaining misbehaving cases, logging (C12), README, AGENTS.md if a gotcha shows up,
   Q12-Q14.

### Supporting changes

- `README.md`, "Running applications on RubickWay": copy and paste, middle-click paste (and how to
  switch it off), clipboard tools; drag-and-drop is not there yet. "Configuration": the
  `primary-selection` node in the defaults block.
- `Cargo.toml`: `wayland-protocols-wlr` (dependency and dev-dependency features).
- Earlier features' contracts are left as they were; this feature's contracts say what changed.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
| --- | --- | --- |
| New dependency `wayland-protocols-wlr` | The older data-control protocol (`zwlr_data_control_manager_v1`), which the owner chose to support with the newer one (spec Clarifications), is not in `wayland-protocols`. | Generating the bindings ourselves (`wayland-scanner` in a build script) is more code to own for the same generated result; supporting only `ext_data_control` leaves tools in use today without a clipboard. |
