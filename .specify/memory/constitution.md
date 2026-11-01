# RubickWay Constitution

RubickWay is a Wayland compositor with an endless canvas where every monitor is a viewer.
It is built to be the author's daily desktop and to learn how a compositor works by
building one.

## Core Principles

### I. Own Implementation

- The project MUST NOT depend on `smithay`, `wlroots` or any other compositor framework
  crate.
- Protocol code MUST use `wayland-server` / `wayland-protocols`. Hardware, session and
  input access MUST go through system libraries (libseat, libinput, xkbcommon, drm/gbm,
  EGL).
- Smithay, anvil and niri are reading material only. A plan that mirrors one of their
  modules MUST name it as a reference.

Rationale: owning every layer is the point of the project; system libraries are used
where rewriting them would teach nothing and add risk.

### II. A Client Can Never Take the Compositor Down

- `unwrap`, `expect` and `panic!` MUST be denied crate-wide through clippy lints
  configured in `Cargo.toml`. A rare exception MUST use
  `#[expect(..., reason = "...")]` with a stated reason.
- Arithmetic on client-supplied values (sizes, offsets, strides, plane layouts) MUST use
  checked or saturating operations, and the result MUST be validated before use.
- A protocol violation MUST be answered with a protocol error to the offending client
  only. A failed buffer import MUST affect only that buffer.
- Every spec MUST list its misbehaving-client cases.

Rationale: a compositor crash takes down every application in the session; clients are
untrusted input.

### III. Test-First, Headless by Default

- Every `tasks.md` MUST include test tasks for each user story, written and seen failing
  before the implementation tasks.
- Automated tests MUST run headless, without a GPU, a host compositor or input devices.
  They cover:
  - protocol conformance through in-process clients: object lifecycles and protocol
    errors;
  - canvas and window-management logic: geometry, cameras, placement, focus changes,
    bindings and actions;
  - damage tracking;
  - rendering, by reading back offscreen output (Mesa software renderer) and asserting
    expected pixel facts - positions, spacing and colours computed from the inputs, within a
    tolerance; no reference images are stored in the repository.
- Behaviour that exists only on real hardware is verified manually: DRM/KMS hotplug and
  VT switching, driver differences, frame pacing, tearing, VRR, direct scanout, physical
  input devices, and interop with real clients (XWayland, clipboard and drag-and-drop
  with browsers). Such code MUST stay a thin layer, and each feature's `quickstart.md`
  MUST list its manual verification steps.

Rationale: most compositor logic is deterministic and testable without hardware; keeping
the hardware-only layer thin keeps the untested surface small.

### IV. A Backend-Independent Core

- The canvas model (geometry, viewers/cameras, placement) MUST be plain data with no
  Wayland, GL or input-device types, and MUST be fully unit-tested.
- Backends MUST produce backend-neutral input events and render targets. Nothing above
  the backend layer may know whether it runs nested or on DRM.

Rationale: the nested backend comes first and DRM/KMS later; the core must not change
when the backend does.

### V. Contained Unsafe

- `unsafe` is allowed only in the FFI, backend, render and plugin loader modules.
- Every `unsafe` block MUST carry a `// SAFETY:` comment stating the invariant it relies
  on.
- This is enforced by `clippy::undocumented_unsafe_blocks` and an `unsafe_code` lint at
  the crate root, configured in `Cargo.toml`.

Rationale: FFI to C libraries is unavoidable; confining it keeps memory-safety review
tractable.

### VI. No Speculative Generality

- A global MUST advertise only the protocol version the feature implements and tests.
- A trait MAY exist only with two implementations. A test double counts as one only when
  the real implementation cannot run headless.
- Configuration options MUST be added only when a spec needs them.

Rationale: unused versions, abstractions and options are untested surface that clients
and future code will rely on.

### VII. Never Block the Event Loop

- Protocol handlers and frame code MUST NOT do blocking I/O, sleep or wait on another
  thread.
- Slow work (spawning processes, reading files, loading config) MUST run off the loop and
  report back through an event source.
- Plans MUST name any work that can block.

Rationale: every stall on the loop is a dropped frame or delayed input for the whole
desktop.

### VIII. A 2D Core with Plugins

- The core is the 2D infinite-canvas compositor. It MUST run fully with no plugins
  installed and MUST NOT depend on any plugin.
- Optional features (3D, shader effects, ...) are in-process plugins: shared libraries loaded
  at startup from the plugin directory (`/usr/lib/rubickway/plugins/`), each packaged
  separately as `rubickway-features-<name>`.
- Plugins are first-party only: they live in this repository, are built together with the
  core from the same source tree using the Rust interface, and their packages pin the exact
  core version. A stable, versioned C ABI for third-party plugins MAY be added later, only by
  amendment and when there is demand.
- The plugin interface and loader MUST be designed together with the first plugin, not
  before (Principle VI).
- Plugins are trusted code in the compositor process and follow the same rules as core code
  (Principles II, V, VII). A panic inside a plugin MUST be caught at the plugin boundary and
  disable that plugin instead of taking the compositor down.

Rationale: a small core stays understandable and testable, while heavy or experimental
features can be installed only by those who want them. Without a stable Rust ABI, building
plugins together with the core is the only reliable way to load Rust code at runtime.

## Technical Constraints

- Language: Rust 1.99, edition 2024, with the toolchain pinned in the repository.
- Delivery: a `rubickway` binary plus optional plugin libraries. The nested backend (a window
  inside an existing Wayland session) comes first; the standalone DRM/KMS backend from a TTY
  comes later.
- Packaging: the core package `rubickway` and plugin packages `rubickway-features-<name>`,
  all built from the same source (on Arch: one PKGBUILD with split packages, plugins
  depending on `rubickway=$pkgver`).
- License: MIT for project code; vendored Spec Kit files keep their own MIT notice.

## Quality Gates

- Every `plan.md` MUST pass a Constitution Check against each principle above before
  design, and again after design. A violation MUST be justified in the plan's Complexity
  Tracking table or the plan MUST change.
- Every `spec.md` MUST list its misbehaving-client cases (Principle II).
- Every `quickstart.md` MUST list manual steps for hardware-only behaviour (Principle
  III).
- Lints that enforce Principles II and V MUST live in `Cargo.toml` so the full check
  enforces them without reviewer memory.

## Governance

- This constitution supersedes other project practices. Runtime guidance for agents and
  contributors (commands, workflow, boundaries, gotchas) lives in `AGENTS.md`; where the
  two conflict, this constitution wins and `AGENTS.md` is corrected.
- Amendments are made only through the `speckit-constitution` workflow, only when the
  project owner asks; each amendment updates the Last Amended date.
- The version changes only when the owner decides. A bump follows semantic versioning:
  MAJOR for removing or redefining a principle, MINOR for adding a principle or materially
  expanding guidance, PATCH for wording fixes. While the version is 0.x, the constitution is
  still settling; 1.0.0 marks the first version the owner declares stable.
- Reviews of specs, plans and code MUST check compliance with these principles.

**Version**: 0.1.0 | **Ratified**: 2026-11-01 | **Last Amended**: 2026-11-01
