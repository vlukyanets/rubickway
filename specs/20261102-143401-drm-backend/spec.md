# Feature Specification: Standalone Session on the Hardware

**Feature Branch**: `worktree-drm-backend`

**Created**: 2026-11-02

**Status**: Draft

**Input**: User description: "Run RubickWay standalone, directly on the hardware, without a host
Wayland session. Today `cargo run` fails with "cannot connect to a Wayland session: Could not
find wayland compositor" when started from a text console (TTY) on a laptop with no GUI
installed. The user wants to log in on a TTY, start RubickWay, and get the endless canvas on the
laptop's built-in display (and any connected monitors, each a viewer), with keyboard, touchpad and
mouse input, running shared-memory client applications as the nested mode already does. The
existing nested mode must keep working; the backend should be chosen automatically (nested when a
host Wayland session is present, standalone otherwise) with an override. The user must be able to
get back to a working console: quitting RubickWay (Alt+Shift+Q, or its standalone equivalent) and
VT switching (Ctrl+Alt+F<n>) must restore the TTY, and a crash must not leave the machine with a
dead screen and keyboard."

## Clarifications

### Session 2026-11-02

- Q: Which displays show the canvas? -> A: Every connected monitor is its own viewer with its
  own view of the canvas, including monitors plugged in or removed while RubickWay runs.
- Q: How does the user start applications on a bare console? -> A: A shortcut opens a terminal,
  Mod+Return running kitty by default; both the key combination and the command can be changed
  in a configuration file. No startup command on the command line (autostart belongs in the
  configuration, in a later feature). A separate command-line control tool (IPC) is a later
  feature: it cannot start the first application, and there is nothing to script yet.
- Q: Does RubickWay set `LIBGL_ALWAYS_SOFTWARE=1` for programs it starts? -> A: No; programs get
  RubickWay's own environment plus `WAYLAND_DISPLAY`.
- Q: Is there only one spawn binding (the terminal shortcut)? -> A: No: any number of
  `binds` entries, each a key combination running a command; a terminal is an ordinary
  application. A combination bound twice keeps the first (owner, 2026-11-02).
- Q: Is Mod fixed? -> A: No, it is configurable (Super, Alt or Ctrl), default Super; one
  setting, no separate one for nested mode.
- Q: Does nested mode stay? -> A: No, it is removed in this feature; standalone is the only mode.
- Q: Does RubickWay handle the laptop lid? -> A: No, lid events are handled by another project;
  RubickWay only has to keep working across a suspend and resume.
- Q: Does a broken configuration stop RubickWay? -> A: No; the affected settings fall back to
  their defaults and each problem is logged.
- Q: Which format does the configuration file use, TOML or KDL? -> A: KDL (2.0), in
  `config.kdl`: it suits nested, repeated blocks such as bindings, per-monitor settings and
  window rules better than TOML, and niri, a reference for this project, uses it.
- Q: Should RubickWay end itself when its event loop is frozen, so the console comes back? -> A:
  Yes, with a watchdog whose timeout is configurable, default 10 seconds (niri, DriftWM and
  Hyprland have none; a frozen compositor there needs SSH, SysRq or the power button).
- Q: Where should RubickWay write its log? -> A: stderr only, as today (as niri itself); the
  user redirects it to a file when they want to keep it.
- Q: Where is the keyboard layout set? -> A: In `config.kdl` only (rules, model, layout,
  variant, options, as niri); with nothing set, the layout is US English only.
- Q: Should Mod+Shift+Q ask for confirmation before quitting RubickWay? -> A: Yes: it quits only
  when pressed twice within 2 seconds; one press does nothing visible (RubickWay cannot draw
  text yet). Signals still end RubickWay at once.

### Session 2026-11-03

- Q: Is the keyboard layout configurable in this feature? -> A: No (owner, 2026-11-03; this
  replaces the 2026-11-02 answer above): the keymap is fixed at US English and the
  configuration has no keyboard settings. Choosing input languages (a Windows-like list such as
  English, Russian, Chinese, switched by a binding, with input methods for Chinese) is a later
  feature with its own configuration, not raw XKB names.
- Q: Are the pointer gestures (pan, move, resize with Mod) fixed? -> A: No: they are pointer
  bindings in the same `binds` block, a modifier combination plus `MouseLeft`, `MouseRight` or
  `MouseMiddle` with the action `move-window`, `resize-window` or `pan-canvas` (owner,
  2026-11-03). Part of this feature. Dragging the background without Mod and dragging a title
  bar or border are unchanged.
- Q: Which pointer gestures are the defaults? -> A: Mod + left moves a window (focusing it),
  Mod+Shift + left pans the canvas, Mod + right resizes (swapped from window-management's Mod +
  left pan and Mod+Shift + left move after trying both on the laptop; owner, 2026-11-03). Mod +
  left over the background still pans.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Start on a bare console and get back to it (Priority: P1)

On a laptop with no desktop installed, the user logs in on a text console and starts RubickWay.
The endless canvas fills the laptop's display; the keyboard and touchpad pan and zoom it exactly
as in nested mode, and the pointer is visible. When the user quits, the text console comes back
and works as before: the user can type the next command straight away.

**Why this priority**: today RubickWay cannot start at all on this machine. Getting the canvas on
the screen and getting safely back to the console is the smallest useful result and the base for
everything else.

**Independent Test**: log in on tty1, start RubickWay, pan with the touchpad, zoom with Mod +
two-finger scroll and pinch, press Mod+0, press Mod+Shift+Q once (nothing happens), then twice
quickly; the console prompt is back and `ls` works.

**Acceptance Scenarios**:

1. **Given** the user is logged in on a text console with no host Wayland session, **When** they
   start RubickWay, **Then** the canvas grid fills the built-in display at its native resolution,
   with the canvas origin centred at 100%, as in nested mode.
2. **Given** RubickWay runs standalone, **When** the user moves a finger on the touchpad or moves
   a mouse, **Then** a pointer drawn by RubickWay follows it and stays inside the display.
3. **Given** RubickWay runs standalone, **When** the user pans, zooms and resets the view with
   the same gestures as nested (with Mod being Super instead of Alt), **Then** the canvas behaves
   exactly as nested.
4. **Given** RubickWay runs standalone, **When** the user presses Mod+Shift+Q twice within 2
   seconds (or RubickWay gets
   SIGINT or SIGTERM), **Then** within 2 seconds the text console is shown again with its earlier
   content, the keyboard types into it, and RubickWay exits with success status.

---

### User Story 2 - Run applications on the standalone canvas (Priority: P1)

The user runs their applications on the standalone canvas and works with them exactly as in nested
mode: typing into kitty, scrolling, moving, resizing, maximizing, fullscreen, focus cycling and
closing.

**Why this priority**: an empty canvas is only a demo; running applications makes the standalone
session usable as the author's desktop on this laptop. It reuses everything client-windows and
window-management built.

**Independent Test**: start RubickWay standalone, open two kittys with Mod+Return, type in each,
move one by its title bar, resize the other, cycle focus with Mod+Tab, make one fullscreen, close
both with Mod+Q. Then bind two commands and change Mod in the configuration file, restart
RubickWay and start both programs with their combinations.

**Acceptance Scenarios**:

1. **Given** RubickWay runs standalone with no configuration file, **When** the user presses
   Mod+Return, **Then** kitty starts and its window opens centred in the view with keyboard
   focus, as nested.
2. **Given** the configuration file binds Mod+T to `kitty` and Mod+B to `firefox`, **When**
   RubickWay starts and the user presses either combination, **Then** that command runs, and
   Mod+Return no longer does anything special (it reaches the application).
3. **Given** the configuration file sets Mod to another modifier, **When** RubickWay starts,
   **Then** every Mod shortcut and gesture uses that modifier.
4. **Given** an application window, **When** the user types, clicks, scrolls with the touchpad or
   mouse wheel, **Then** the application receives the input as it did nested, using the
   US English keymap of FR-007.
5. **Given** applications on the standalone canvas, **When** the user uses any window-management
   gesture or shortcut from window-management, **Then** it works as it did nested, with Mod being
   Super.
6. **Given** an application asks which display it is on, **When** it reads the display's
   description, **Then** it gets the real display's name, physical size, resolution and refresh
   rate.
7. **Given** no configuration file, **When** the user drags a window with Mod held, **Then**
   the window moves (focused and raised), and with Mod+Shift held the canvas pans instead
   (FR-032). **Given** the configuration binds Mod + left button to `pan-canvas` and Mod+Shift
   + left button to `move-window`, **Then** the two are the other way round.

---

### User Story 3 - Every monitor is a viewer (Priority: P2)

The user plugs a monitor into the laptop's HDMI port. It becomes a second viewer: a separate
camera on the same endless canvas, next to the built-in display. The pointer moves from one
display to the other across their shared edge; each display pans and zooms on its own; a window
dragged from one display to the other moves to the part of the canvas that display shows. When the
monitor is unplugged, its viewer goes away and nothing on the canvas is lost.

**Why this priority**: "every monitor is a viewer" is RubickWay's core idea, and standalone is the
first mode with real monitors. It comes after US1 and US2 because a single display is already a
usable desktop.

**Independent Test**: start RubickWay with the monitor connected; both show the canvas side by
side. Zoom out on one only, drag a kitty from the built-in display to the monitor, unplug the
monitor (the kitty stays on the canvas, focus cycling brings it to the built-in display), plug it
back in.

**Acceptance Scenarios**:

1. **Given** RubickWay starts with two displays connected, **When** the canvas appears, **Then**
   each display shows it at 100%, the external display to the right of the built-in one, and the
   two views continue each other as if they were one wide view.
2. **Given** two viewers, **When** the pointer moves past the shared edge, **Then** it continues
   on the other display at the matching height.
3. **Given** two viewers, **When** the user pans, zooms or resets the view, **Then** only the
   viewer under the pointer changes.
4. **Given** a window being moved, **When** the pointer crosses to the other display, **Then** the
   grabbed point of the window stays under the pointer, so the window moves to the canvas region
   that display shows (and appears at that viewer's zoom).
5. **Given** RubickWay runs, **When** a monitor is plugged in, **Then** within 2 seconds it shows
   the canvas as a new viewer to the right of the existing ones; **When** it is unplugged,
   **Then** its viewer is removed, windows stay where they are on the canvas, and the pointer
   moves to a remaining display if it was on the removed one.

---

### User Story 4 - Switch to another console and back (Priority: P2)

While RubickWay runs, the user switches to another text console with Ctrl+Alt+F<n> (for example
to check something or to fix a problem), and later switches back. The other console works
normally; on return, the canvas, the view and every application window are exactly as they were.

**Why this priority**: switching consoles is the standard escape hatch on Linux and the way to
recover from a stuck application; without it a frozen session means a reboot.

**Independent Test**: with two kittys open, press Ctrl+Alt+F2, log in and run a command there,
press Ctrl+Alt+F1; the canvas is back with both kittys, still responding, and the view unchanged.

**Acceptance Scenarios**:

1. **Given** RubickWay runs standalone, **When** the user presses Ctrl+Alt+F<n> for another
   console, **Then** that console is shown and receives the keyboard; RubickWay stops drawing and
   stops receiving input.
2. **Given** the user switched away, **When** they switch back to RubickWay's console, **Then** the
   display shows the canvas with the same view and windows within 2 seconds, and input works
   again.
3. **Given** applications were running when the user switched away, **When** the user returns,
   **Then** they are still connected and redraw; keys and buttons held at the moment of switching
   were reported to them as released.
4. **Given** a move, resize or pan was in progress, **When** the user switches away, **Then** it
   ends where it was at that moment.

---

### User Story 5 - Never lose the machine to a crash (Priority: P2)

Whatever way RubickWay ends - quitting, a fatal error such as the graphics device disappearing, an
internal failure, or being killed - the user gets a working text console back without rebooting.

**Why this priority**: a standalone compositor owns the screen and keyboard; if it dies badly the
laptop is unusable until a hard reset. This is the safety net for development on the author's
only screen.

**Independent Test**: start RubickWay standalone and end it in each of the ways listed in FR-012
(from another console: `kill`, `kill -9`); each time the console comes back and types.

**Acceptance Scenarios**:

1. **Given** RubickWay stops because of a fatal error, **When** it exits, **Then** the text console
   is restored, the reason is readable on it as one line starting with `rubickway:`, and the exit
   status is 1.
2. **Given** RubickWay is killed without a chance to clean up, **When** the user looks at the
   screen, **Then** the text console is usable again immediately or after switching consoles with
   Ctrl+Alt+F<n>; no reboot is needed.

---

### User Story 6 - Nested mode is gone (Priority: P3)

RubickWay no longer runs as a window inside another Wayland desktop: standalone is its only mode.
Started inside a running desktop, it says so and exits without touching anything.

**Why this priority**: standalone replaces nested; keeping two backends would double the
hardware-facing code to maintain for a mode that will not be used. It comes last because removing
it is only safe once standalone works (US1, US2).

**Independent Test**: start RubickWay from a terminal inside a Wayland desktop (it exits with the
message below, the desktop is unaffected); run the full automated test suite, which needs no
desktop.

**Acceptance Scenarios**:

1. **Given** RubickWay is started inside a running graphical desktop, **When** it starts, **Then**
   it exits with status 1 and one line saying another program controls the display and that it
   must be started from a text console (`contracts/cli.md`), and opens no window.
2. **Given** the automated test suite, **When** it runs, **Then** every test passes without a
   desktop, a GPU or input devices, as before (Principle III).

---

### Edge Cases

- **Not started from an active local console**: started over SSH, from a console that is not the
  one being shown, or without a login session: RubickWay exits with status 1 and a one-line
  message saying it must be started from a local console login; the screen is not touched.
- **No usable display**: no display connected (lid closed with nothing attached counts as
  connected for the built-in panel), no graphics device, or the graphics device cannot be used:
  exit 1 with a message naming what is missing; the console is untouched or restored.
- **Graphics device lost while running**: a fatal error (FR-012).
- **Console switched away during startup** (before the first frame was shown): once RubickWay
  has access, startup continues; the switch is an ordinary pause, and the canvas appears when
  RubickWay's console is shown again (FR-011). The 2-second check of FR-002 applies only to the
  first access at startup.
- **Displays connected or disconnected while running**: see US3 scenario 5 and FR-021. Unplugging
  every external monitor leaves the built-in viewer; RubickWay keeps running even with no display
  left (for example a laptop without a built-in panel), showing the canvas again when a monitor is
  connected.
- **Several monitors plugged at once, or plugged and unplugged quickly**: each change is applied
  in order; the end state matches the monitors connected at the end.
- **Displays with different resolutions or refresh rates**: each viewer runs at its own display's
  native resolution and refresh rate. The pointer moving across the shared edge towards a shorter
  display at a height that display does not have still crosses, with its height clamped to that
  display's bottom edge (the display is chosen by the pointer's horizontal position).
- **A window shown on two viewers** (both look at the same canvas region, or the window spans the
  boundary of what they show): it is drawn on both; applications are told which displays show
  their window.
- **Pan drag crossing displays**: a pan continues on the viewer where it started, until the button
  is released.
- **Resize drag crossing displays**: the dragged edge follows the pointer in canvas units of the
  viewer the pointer is on.
- **Fullscreen and maximize with several viewers**: they apply to the window's viewer (FR-022);
  other viewers keep showing the canvas, where the window appears at its fullscreen or maximized
  position and size. Each viewer may have its own fullscreen window; the rule "a later fullscreen
  request wins" from window-management applies per viewer.
- **The viewer of a fullscreen or maximized window is unplugged**: fullscreen ends (the window gets
  its previous position and size back); a maximized window stays as it is on the canvas.
- **Input devices added or removed while running** (a USB mouse plugged in or pulled): a new
  device works within 1 second; a removed device's held keys and buttons are released for
  applications; nothing else changes.
- **No pointing device at all**: RubickWay runs; the keyboard shortcuts work; the pointer stays
  where it was (at first the centre of the leftmost display), and the viewer it is on is the
  active viewer.
- **No display at all** (every display removed, or none at startup after a removal): windows
  keep their canvas positions; new windows open centred on the canvas origin; focus cycling
  focuses and raises without moving any view; maximize and fullscreen requests are ignored, as
  for a window without focus; windows are shown nowhere, so their frame callbacks wait, as for
  windows outside every view.
- **A viewer removed during an operation**: a pan on it ends; a move or resize whose pointer was
  on it ends where the window was at that moment, as when a grab is broken (window-management);
  the pointer then moves as in FR-021.
- **Fullscreen on one viewer, actions on another**: pan, zoom and Mod+0 on viewer B never change
  viewer A's fullscreen window. Focus cycling moves and leaves fullscreen only on the active
  viewer; when it reaches viewer A's fullscreen window, that window is focused and no view
  moves (it is fully inside viewer A).
- **A window carried across viewers of different zoom**: at the crossing the window jumps to the
  canvas region the new viewer shows (the grabbed point stays under the pointer, FR-023); it is
  drawn wherever its canvas position is visible, which can still include the first viewer.
- **Suspend and resume** (triggered by another program, for example on closing the lid): after
  resume the displays show the canvas again and input works, also when input devices or
  displays were reported gone and back during the suspend (FR-029).
- **Console switch shortcuts while an application has focus**: Ctrl+Alt+F1..F12 always switch
  consoles and never reach applications.
- **Console switch to the console RubickWay is on**: no change.
- **Pointer at the display edge**: the pointer stops at the edge; panning by dragging still works
  because panning follows motion, not position.
- **Fullscreen window**: the pointer is still drawn above it.
- **Quit shortcut pressed once by accident**: nothing happens; the press is not forwarded, and
  after 2 seconds or any other key the next Mod+Shift+Q counts as a first press again (FR-031).
- **Console switch between the two quit presses**: the pending quit is cancelled.
- **RubickWay frozen** (a bug makes its event loop stop): Ctrl+Alt+F<n> cannot work, because
  RubickWay itself carries out the switch; the watchdog ends RubickWay after its timeout and the
  console comes back (FR-030). A freeze inside the graphics driver may delay this until the
  driver returns, as it delays a `kill -9`.
- **Quitting while away**: SIGTERM while on another console: RubickWay exits and the other
  console stays as it is.
- **Logs**: messages RubickWay writes while it owns the screen are not visible then; they must be
  readable after exit (the final error line at least) - see FR-013.
- **Configuration problems**: the file is unreadable, not valid, has unknown settings, or binds
  a combination that is already taken (for example `Mod+Q`): RubickWay starts, the affected
  settings keep their defaults, and each problem is logged (FR-026). An empty command is
  rejected the same way.
- **Command not found or failing**: a spawn binding's command cannot be started (for example it does
  not exist): a warning is logged; when it starts but later exits with a non-zero status or a
  signal, an info line names the status. RubickWay keeps running either way and the shortcut can
  be pressed again.
- **RubickWay exits while programs it started run**: they lose their connection as any
  application does; RubickWay does not wait for them.
- **A spawn binding pressed repeatedly**: each press starts the program once more; key repeat does
  not start more.
- **Misbehaving applications** (Principle II; the protocol objects are the same as nested, so all
  earlier misbehaving-client cases stay covered; new cases from this feature):
  - an application keeps committing new content or asking for frames while RubickWay is on
    another console: its content is accepted and kept, it gets no frame-done events until
    RubickWay is shown again, and RubickWay's memory does not grow beyond the latest content per
    window;
  - an application connects, opens a window or closes while RubickWay is on another console: the
    window state is updated and shown correctly on return;
  - an application asks to start a move or resize based on a press made before a console switch:
    ignored, as for an older press in window-management;
  - an application sets a cursor image: accepted and not shown, as nested (the pointer shape is
    RubickWay's own, FR-008); hiding the pointer is honoured only over its own window;
  - an application keeps the output of a removed display and uses it (for example in a
    fullscreen request) or binds outputs many times: the removed output counts as none, with no
    error; repeated binds work;
  - an application never reads its messages while the user is away: it is disconnected as
    nested (client-windows edge case 4); RubickWay keeps working.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: RubickWay MUST run only standalone. The nested mode (a window inside a host Wayland
  desktop) MUST be removed, with the code and dependencies only it uses; the host session named
  in the environment is no longer looked up. Started inside a running graphical desktop, RubickWay
  fails as in FR-002.
- **FR-002**: Standalone RubickWay MUST run as the logged-in user, without root privileges, when
  started from the local text console that is currently shown; it gets the display and input
  devices through the user's login session. Started anywhere else, it MUST exit with status 1 and
  a one-line message saying why (one message per case: no login session or seat, console not
  shown, another program controls the display; `contracts/cli.md`), without changing what is on
  the screen. A startup failure after the display was taken (for example the graphics setup
  failing after the first modeset) MUST give the console back as in FR-012. Running as root is
  neither required nor refused.
- **FR-003**: Standalone, every connected display MUST be a viewer: its own camera (position and
  zoom) on the one shared canvas. Each display MUST run at its preferred (native) resolution and
  refresh rate, at scale 1; the viewer's size is the display's resolution.
- **FR-004**: Everything nested-canvas, client-windows and window-management provide MUST work the
  same standalone; only the input source, the displays and Mod differ from what those specs
  describe for the nested window.
- **FR-005**: Mod MUST be configurable, Super, Alt or Ctrl (FR-024); the default is Super. All
  shortcuts and gestures keep their keys otherwise (Mod+Shift+Q quits, now confirmed by a second
  press, FR-031; Mod+0 resets the view, Mod+Tab, Mod+Q, ...).
- **FR-006**: Standalone RubickWay MUST take input from the built-in keyboard and touchpad and from
  external keyboards and mice, including ones plugged in or removed while it runs. The touchpad
  MUST provide pointer motion, buttons, tap-to-click, two-finger scrolling and pinch, so all
  existing gestures work.
- **FR-007**: The keymap MUST be US English only (XKB rules `evdev`, model `pc105`, layout
  `us`, no variant, no options), whatever the environment says (`XKB_DEFAULT_*` are not read);
  it is not configurable in this feature. Applications receive the same keymap. Key repeat MUST be offered to applications at 25 keys per second after 600 ms.
- **FR-008**: Standalone RubickWay MUST draw the pointer itself, above everything (also above a
  fullscreen window), with the shapes the shell asks for (default arrow, grabbing hand, move,
  resize in each direction). It MUST use the installed cursor theme when there is one and
  built-in images otherwise, so a machine without any theme still shows every shape. Cursor
  images set by applications are still not shown (the default arrow is shown instead), but an
  application that hides the pointer over its window (`wl_pointer.set_cursor` without a
  surface, as kitty while typing and most games) MUST get it hidden while the pointer is over
  that window and no RubickWay gesture (pan, move, resize) is running (as niri and DriftWM).
- **FR-009**: The pointer MUST stay inside the area of the displays in use; it starts in the
  centre of the leftmost display.
- **FR-010**: Standalone RubickWay MUST present frames on each display in step with that display:
  at most one new frame per refresh, no tearing, a slow display never holding back another, and
  no drawing at all while nothing changes (idle CPU as in nested-canvas SC-005). A window shown
  on several displays has its frame callbacks completed by the first of them to present a frame
  showing it after the callback was requested.
- **FR-011**: Ctrl+Alt+F1 .. Ctrl+Alt+F12 MUST switch to that text console and never reach
  applications; they are checked before every other shortcut and cannot be rebound (as niri,
  DriftWM and Hyprland). While away, RubickWay MUST not draw, MUST not receive input, MUST
  report held keys and buttons as released to applications, MUST end any move, resize or pan in
  progress, and MUST keep its canvas, view, windows and connected applications. On return, the
  display and input MUST work again within 2 seconds of RubickWay's console being shown, with
  everything as it was. The same applies whenever the session becomes inactive and active again
  for another reason (another session activated by a login manager, `chvt` or `loginctl` from
  another console).
- **FR-012**: On every way of ending - Mod+Shift+Q (FR-031), SIGINT, SIGTERM, SIGHUP, a fatal
  error (the graphics device lost, event loop failure; a display being unplugged is not one,
  FR-021) or an internal panic - RubickWay MUST give the console back: the text console is shown
  with its earlier content and the keyboard types into it. When RubickWay is killed without a
  chance to clean up (SIGKILL, abort), the console MUST still be usable again without a reboot,
  at the latest after a console switch. A frozen RubickWay is ended by the watchdog (FR-030).
- **FR-013**: Log output MUST go to stderr only, as today (no log file); the user redirects it to
  keep it (`rubickway 2> rubickway.log`). On a failure, the error message MUST be the last line
  RubickWay writes. When stderr is the console, that line MUST be readable on it after RubickWay
  exits (how much of the earlier log stays visible is the kernel console's scrollback, not
  specified here), and messages written while RubickWay owns the screen MUST NOT appear on top
  of the canvas.
- **FR-014**: RubickWay MUST provide spawn bindings: key combinations that each run a command
  (any program; a terminal is just the usual first one). With none configured there is one,
  Mod+Return running `kitty`. The key press and its release are not forwarded (FR-024 to
  FR-028). Each press starts the command once; key repeat never starts more.
- **FR-015**: Applications MUST see each display as an output, described with its real name, make,
  model, physical size, resolution, refresh rate and position in the row (sent again when the
  row re-flows), with `Unknown` for a make or model the display does not report; they MUST be
  told which displays show each of their windows, and outputs appear and disappear with the
  displays. While RubickWay is paused, windows keep the outputs they were on (no leave events);
  the outputs still exist.
- **FR-016**: Standalone RubickWay MUST render on the graphics device that drives the displays;
  other graphics devices (the laptop's second GPU, with no displays) are left alone.
- **FR-017**: Standalone errors MUST be one line on stderr starting with `rubickway:` with exit
  status 1, as the existing error messages (nested-canvas `contracts/cli.md`). An internal panic
  is reported the same way, as `rubickway: internal error: <message> at <file>:<line>`, with
  exit status 1, after the console was given back.
- **FR-018**: Displays MUST be arranged side by side in a row, top edges aligned: the built-in
  display first (leftmost), external displays to its right in the order they were connected;
  displays already connected at startup are ordered as the graphics device lists its connectors.
  The pointer MUST move between displays across their shared edges.
- **FR-019**: The active viewer is the one under the pointer. Pan, zoom and reset gestures and
  shortcuts (Mod+0) MUST act on the active viewer only; a pan drag keeps acting on the viewer it
  started on. Mod+0 returns the active viewer to its home view (FR-020).
- **FR-020**: Each viewer MUST start at 100% zoom with a home view continuing the view to its
  left, as if all displays were one wide view centred on the canvas origin: with one display, the
  origin is centred as nested; with several, the row's bounding box (the sum of the widths by
  the tallest height) is centred on the origin, and each viewer's home view is centred on its
  display's part of that box, so a shorter display's home view sits higher. A display connected
  later gets the home view continuing the rightmost viewer's home view (its home view, wherever
  that viewer's camera is now).
- **FR-021**: A display connected while RubickWay runs MUST become a viewer within 2 seconds; a
  display disconnected MUST stop being a viewer without changing anything on the canvas. The
  pointer on a removed display MUST move to the centre of the leftmost remaining display; a
  pointer on another display keeps its position on that display when the row re-flows.
- **FR-022**: Everything window-management does with "the view" or "the viewer" applies to one
  viewer: new windows open centred in the active viewer; focus cycling moves the active viewer to
  a window unless the window is already fully inside some viewer; maximize and fullscreen use the
  window's viewer - the active viewer if it shows part of the window, otherwise the viewer showing
  the largest part of it, otherwise the active viewer. A fullscreen request naming an output
  uses that output's viewer instead (as niri and DriftWM); an unknown or removed output counts
  as none. Maximize moves only that viewer's camera and sizes the window to that viewer; the
  window stays maximized for that viewer when it later spans others. window-management's rule
  for a new window while a window is fullscreen (its FR-020) applies when the active viewer has
  a fullscreen window; otherwise the new window opens normally in the active viewer.
- **FR-023**: During a move or resize, the pointer's canvas position MUST be taken in the viewer
  the pointer is on, so a window can be carried from one display to another.
- **FR-024**: RubickWay MUST read a configuration file in KDL 2.0 at startup from the user's
  configuration directory: `$XDG_CONFIG_HOME/rubickway/config.kdl`, by default
  `~/.config/rubickway/config.kdl` (also when `XDG_CONFIG_HOME` is empty or not an absolute
  path, as the XDG base directory specification says). In this feature
  it holds exactly these settings: Mod (default Super), the spawn
  bindings (FR-014; any number, default Mod+Return running `kitty`; a configuration with at
  least one valid spawn binding replaces the default), the pointer bindings (FR-032; a
  configuration with at least one valid pointer binding replaces their defaults, independently
  of the spawn bindings), and the watchdog timeout (FR-030; whole seconds from 1 to 600,
  default 10). A missing file means the defaults. Changes take effect at
  the next start.
- **FR-025**: A key combination in the configuration MUST be written as modifier names and one
  key name (as XKB names it, for example `Return` or `t`) joined with `+`, and is matched like
  the built-in shortcuts. Modifier names are `Mod` (Mod as in FR-005),
  `Super`, `Alt`, `Ctrl` and `Shift`. A combination that is already a built-in shortcut or
  gesture (including Ctrl+Alt+F1 .. F12 standalone), or that names an unknown key or modifier,
  is rejected. Matching is exact, as for built-in shortcuts: Mod+Ctrl+Q is a different
  combination from Mod+Q and is allowed. A combination without any modifier (`F12`) is allowed
  (as niri); that key then never reaches applications. A combination bound twice keeps the
  first binding; the later one is rejected with a warning. A pointer combination (FR-032) is
  written the same way with `MouseLeft`, `MouseRight` or `MouseMiddle` in place of the key, and
  needs at least one modifier (a plain click always reaches the window or background).
- **FR-026**: An invalid configuration MUST NOT stop RubickWay: every rejected or unreadable
  setting falls back to its default, and each problem is logged as a warning naming the file,
  the line when there is one, and the reason (readable after exit, FR-013). Unknown nodes are
  ignored with a warning; a setting given twice uses the last one, with a warning.
- **FR-027**: The command line MUST stay as it is: `rubickway` takes no arguments (nested-canvas
  `contracts/cli.md`).
- **FR-028**: A spawn binding's command MUST be started off the event loop (Principle VII),
  detached from RubickWay (its exit never affects RubickWay, and RubickWay leaves no finished
  child processes behind), in the user's home directory, with RubickWay's own environment plus
  `WAYLAND_DISPLAY` set to RubickWay's socket; RubickWay adds or changes nothing else, except
  that the command's standard input is empty (`/dev/null`, not RubickWay's console). A
  command that cannot be started is logged as a warning and RubickWay keeps running. The
  startup line with the socket name is still printed.
- **FR-029**: Standalone RubickWay MUST keep working across a system suspend and resume: after
  resume every connected display shows the canvas again and input works, also when the system
  reported input devices or displays as removed and added again around the suspend; a suspend
  while the user is on another console is followed by the normal return of FR-011. RubickWay
  does not react to the lid itself.
- **FR-030**: RubickWay MUST end itself when its event loop makes no progress for the watchdog
  timeout (default 10 seconds, configurable, FR-024), logging one line that names the timeout;
  the console is then restored as after a kill (FR-012), and the exit status is 1. Time the
  machine spends suspended does not count. The watchdog never fires while RubickWay works
  normally, also while it is on another console, and it does end a RubickWay that froze while
  on another console. It watches the whole run: from right after the configuration is read,
  through startup (each startup step counts as progress) and while running, until the process
  ends - so an exit that hangs for longer than the timeout is ended the same way. Timeouts below
  5 seconds are allowed but may end RubickWay during a legitimately slow moment such as its
  first frame.
- **FR-031**: Mod+Shift+Q MUST quit only when pressed a second time within 2 seconds of the
  first press; a single press does nothing visible and is not forwarded. Key repeat from holding
  the combination does not count as a second press; pressing or releasing modifier keys (Shift,
  Mod) in between does not cancel; any other key press, a console switch, or 2 seconds passing
  cancels a pending quit. This holds everywhere, also during a move or resize (replacing
  window-management's "Mod+Shift+Q quits"). SIGINT, SIGTERM and SIGHUP still end RubickWay at
  once (FR-012).
- **FR-032**: The pointer gestures with modifiers MUST be pointer bindings: a pointer
  combination (FR-025) and one action, `move-window`, `resize-window` or `pan-canvas`, in the
  `binds` block. The defaults: Mod + left button moves, Mod+Shift + left button pans, Mod +
  right button and Mod+Shift + right button resize (window-management had the two left-button
  gestures the other way round; this replaces its rows for them). Pressing a
  bound combination does what its action does in window-management (contracts/input.md):
  `move-window` on a window focuses, raises and moves it, and over the background pans;
  `resize-window` on a window focuses, raises and resizes it from the nearest edge or corner,
  and over the background does nothing; `pan-canvas` pans the active viewer anywhere without
  changing focus; with a fullscreen window on the active viewer, `resize-window` on it and
  `pan-canvas` leave fullscreen and are consumed, as before. The gesture ends when its button
  is released. Modifiers match exactly; a press whose modifiers and button match no pointer
  binding is an ordinary click. Mod + scroll and pinch stay as they are (not bindings). A
  pointer action on a key combination, or `spawn` on a pointer combination, is rejected with a
  warning.

### Key Entities

- **Session**: the user's login on the local console; grants and revokes access to the display
  and input devices; active while its console is shown, inactive while the user is on another
  console.
- **Display**: a physical screen connected to the graphics device: name, make, model, physical
  size, preferred resolution and refresh rate, connected or not, and its place in the row of
  displays.
- **Viewer** (from nested-canvas): now one per display: a camera, its home view, and at most one
  fullscreen window.
- **Configuration**: the settings read at startup - in this feature Mod, the spawn bindings
  (key combination and command each), the pointer bindings and the watchdog timeout - each with
  a default.
- **Input device**: a keyboard, touchpad or mouse; appears and disappears while RubickWay runs.
- **Pointer**: RubickWay's own pointer position on the display(s) and its current shape.
- **Backend** (from nested-canvas): now only the standalone hardware; it still speaks
  backend-neutral events, and nothing above it depends on it (Principle IV).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From a console prompt on the author's laptop, the canvas is on the built-in display
  within 3 seconds of starting RubickWay.
- **SC-002**: After the second Mod+Shift+Q, the console prompt is back and accepts typing within
  2 seconds, in 10 out of 10 tries; a single press never quits (checked automatically).
- **SC-003**: 20 consecutive switches to another console and back each restore the canvas and
  input within 2 seconds of RubickWay's console being shown, with every window still there and
  every application still responding.
- **SC-004**: After each way of ending in FR-012, including `kill -9`, and after a simulated
  freeze of the event loop (FR-030), the console is usable without a reboot (manual check); the
  freeze ends RubickWay within the timeout plus 1 second (checked automatically).
- **SC-005**: Panning, zooming and moving a kitty keep up with the display's refresh rate: during
  a 10-second continuous pan the frame times in the debug log show no gap longer than two
  refresh intervals, and no tearing is visible; idle CPU stays under 1% of one core, counting
  the watchdog's once-a-second wake-up (manual check).
- **SC-006**: Every acceptance check of client-windows and window-management with kitty passes
  standalone, with Mod = Super.
- **SC-007**: Started over SSH or from a console that is not shown, RubickWay prints one clear
  line and exits with status 1, and the screen does not change.
- **SC-008**: With the HDMI monitor connected, the pointer crosses between the displays without a
  jump, and plugging and unplugging the monitor 10 times leaves every window on the canvas and
  RubickWay running (manual check); the viewer arrangement, home views, active-viewer routing and
  the window's-viewer rule are checked automatically.
- **SC-009**: All automated tests run headless (Principle III) and all existing tests keep
  passing; every misbehaving-application case in Edge Cases is covered by an automated test.
- **SC-010**: Configuration parsing and validation (valid files, every rejected case in Edge
  Cases, defaults) is covered by automated tests; with a broken configuration file RubickWay
  still starts and Mod+Return still opens kitty.

## Assumptions

- The target machine is the author's laptop: one built-in 1920x1080 display and an HDMI port on
  the Intel graphics device; a second (NVIDIA) GPU with no displays; a built-in keyboard and
  touchpad. It has a console login through the system's standard login session manager and no
  desktop installed.
- The user starts RubickWay by hand from a console prompt (`cargo run` or the built binary);
  starting it from a login manager or as a system service is out of scope.
- Scale is 1 on every display; no HiDPI or fractional scale setting in this feature.
- The display arrangement is fixed (a row, FR-018); no setting to place displays differently, to
  mirror them, or to turn one off. The built-in display stays a viewer when the lid is closed.
- Viewer cameras are not remembered between runs or across unplugging and plugging a monitor.
- The configuration file is KDL; the exact node names (for example `input { mod-key "Super" }`,
  `binds { Mod+Return { spawn "kitty"; } }`, as niri) are fixed in the plan's contracts, laid out
  so later features can add settings without breaking existing files. Only Mod, the spawn
  bindings, the pointer bindings and the watchdog timeout are configurable now: the keyboard
  layout and input languages, rebinding built-in shortcuts, autostart lists, reloading the file while running, the display
  arrangement, touchpad settings and an application launcher (it needs protocols RubickWay does
  not have yet) are later features.
- A command-line or IPC tool to control a running RubickWay is a later feature; when it comes, it
  is a subcommand of the `rubickway` binary (as `niri msg`), not a separate program.
- Mod defaults to Super, as foreseen in nested-canvas's Assumptions for the standalone backend
  (Alt was only needed because the host desktop reserved Super), and is configurable (FR-005).
  With Mod set to Ctrl, Ctrl+Tab, Ctrl+Q, Ctrl+0 and the other Mod shortcuts belong to RubickWay
  and no longer reach applications; that is accepted, without a warning.
- Removing nested mode (US6) needs the constitution amended first: its Technical Constraints
  ("the nested backend comes first") and Principle IV's rationale name the nested backend. The
  automated tests never needed a desktop: the in-process test clients and the offscreen
  rendering tests stay. Development and manual checks happen on the laptop's console.
- The laptop lid, power button and suspend policy are handled by another project; RubickWay
  only survives the suspend (FR-029).
- Touchpad settings are fixed defaults: tap-to-click on, natural scrolling off, the system's
  acceleration; not configurable in this feature.
- Typing is US English only; other layouts, switching between them and input methods come with
  the later input-languages feature.
- Applications are still shared-memory only (GPU buffers come later). The acceptance application
  is kitty, which has to be installed on the laptop first (it is not today). kitty draws with
  OpenGL; until GPU buffers exist, Mesa's EGL in kitty falls back to its software renderer by
  itself, so kitty works with the default configuration and no environment variable (confirmed
  on the laptop, 2026-11-02). kitty needs a scalable monospace font installed (for example
  `ttf-dejavu`); bitmap-only console fonts make it exit at once.
- The laptop has no cursor theme installed, hence the built-in images in FR-008.
- The pointer is drawn into each frame (no hardware cursor plane yet): moving it redraws its
  display, at most once per refresh. There is no pointer-latency or power target in this
  feature; quickstart Q21 records a baseline for the later cursor-plane feature.
- The login session's libseat works through logind without the seatd daemon on the target
  machine; this is confirmed at the start of implementation, before anything builds on it.
- The default XKB rules map Ctrl+Alt+F1 .. F12 to the console-switch keys (FR-011).
- The graphics device is the one the firmware used at boot; without that information (no PCI
  device, a virtual GPU) the first device with display connectors is used, which is enough for
  the target machine.
- Not in this feature: screen blanking, power saving of the display, brightness and media keys
  handled by RubickWay (they reach applications as ordinary keys), screen locking, idle
  detection, gamma or night light, variable refresh rate, direct scanout, rendering on the second
  GPU, XWayland, clipboard between consoles, more than one seat.
- The automated tests cannot open real displays or input devices (Principle III): the
  hardware-only layer stays thin, and the console, display, input-device and crash-recovery
  behaviour are verified by hand through the feature's `quickstart.md`.
