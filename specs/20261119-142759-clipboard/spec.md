# Feature Specification: Clipboard

**Feature Branch**: `worktree-clipboard`

**Created**: 2026-11-19

**Status**: Draft

**Input**: User description: "clipboard - copy and paste between applications (today
src/server/data_device.rs is a stub: wl_data_device_manager v3 exists so foot starts, but nothing is
ever offered or transferred and drags are cancelled at once). Scope of this feature: (1) the regular
clipboard through wl_data_device: an application sets the selection in response to a user action (its
latest input serial), the selection is offered to the application that has keyboard focus (and to any
application when it gains focus), pasting transfers the data directly between the two applications
through a pipe the compositor hands over, without RubickWay copying the data; the selection is cleared
when its source is destroyed or its application quits; (2) primary selection (select text, paste with
the middle button) through zwp_primary_selection_device_manager_v1, the same model; (3) clipboard
managers and command-line tools (wl-copy, wl-paste, cliphist, wl-clip-persist) through
ext-data-control-v1 (and wlr-data-control-unstable-v1 if tools still need it), so a manager can read
and set both selections and keep the clipboard after the source application closes - RubickWay itself
does not keep copies. Works with the pointer and keyboard on any viewer and at any zoom of the canvas.
Misbehaving clients (huge numbers of mime types, sources set without a valid serial, destroyed
sources, clients that never read their end of the pipe) must not stall the event loop or affect other
applications. Out of scope: drag-and-drop between windows (a later feature; it may pan the canvas when
a drag reaches a display edge, like niri's edge view scroll), clipboard history kept by RubickWay
itself, X11/XWayland selection bridging (no XWayland yet)."

## Clarifications

### Session 2026-11-19

- Q: May every application use the clipboard-tool protocols, which can read the clipboard in the
  background, or only programs the user allows? -> A: Every application, as on niri and sway (FR-007).
  Restricting them (an allow-list or a switch in the configuration) may come later; see Assumptions.
- Q: Can the primary selection (middle-click paste) be switched off? -> A: Yes: on by default,
  `primary-selection off` in the configuration switches it off entirely (FR-006), as GNOME allows.
- Q: If an application asks to set the clipboard just after the user switched the focus away (Ctrl+C
  then Mod+Tab at once), is the request accepted? -> A: Yes, if it refers to one of the latest input
  events that application received (a key, a button, getting the focus), even though it no longer has
  the focus; an application the user did nothing with cannot set it (FR-004), as on sway.
- Q: Besides the new protocol for clipboard tools, is the older one supported too? -> A: Yes, both,
  with the same behaviour (FR-007); the older one is dropped once the tools no longer need it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Copy in One Application, Paste in Another (Priority: P1)

The user copies text, a link or an image in one application (Ctrl+C, a menu item) and pastes it into
another (Ctrl+V, a menu item), wherever the two windows are on the canvas and whichever monitor they
are on. Today nothing pastes across applications at all.

**Why this priority**: copy and paste between programs is basic desktop use; without it RubickWay
cannot be used for daily work.

**Independent Test**: copy a line in Firefox, focus kitty, paste: the line appears. Copy text in kitty,
paste it into Firefox's address bar. Copy an image in Firefox, paste it into an application that takes
images.

**Acceptance Scenarios**:

1. **Given** two applications on the canvas, **When** the user copies text in the first and pastes in
   the second, **Then** the second receives exactly the copied text.
2. **Given** a copy made in one application, **When** the user focuses several other applications in
   turn and pastes in each, **Then** each receives the same content.
3. **Given** content copied in an application that offers it in several formats (plain text, rich
   text, an image), **When** another application pastes, **Then** it can choose the format it prefers
   among those offered.
4. **Given** something already copied, **When** the user copies something else (in the same or another
   application), **Then** the next paste anywhere gives the new content, and the application that
   owned the old content is told it no longer does.
5. **Given** content copied in an application, **When** that application closes, **Then** pasting
   gives nothing (unless a clipboard manager kept it, US2), and no application is left waiting.
6. **Given** two windows on different monitors, or the canvas panned or zoomed between copy and paste,
   **When** the user pastes, **Then** it works the same.

---

### User Story 2 - Clipboard Tools and Managers (Priority: P2)

The user runs command-line tools and clipboard managers: `wl-copy` and `wl-paste` in scripts, a
history manager such as cliphist, or a tool that keeps the clipboard after the source application
closes (wl-clip-persist). These tools read and set the clipboard without having a window or the
keyboard focus.

**Why this priority**: scripts and history are part of daily work for many users, and a manager is
the way to keep copied content after closing its application, which RubickWay itself does not do.

**Independent Test**: `echo hi | wl-copy`, then paste into kitty: `hi`. Copy text in Firefox, run
`wl-paste`: the text. With a manager that keeps the clipboard running, copy in an application, close
it, paste elsewhere: the content is still there.

**Acceptance Scenarios**:

1. **Given** a tool sets the clipboard, **When** the user pastes into any application, **Then** it
   gets the tool's content.
2. **Given** content copied in an application, **When** a tool reads the clipboard, **Then** it gets
   that content, without the user focusing anything.
3. **Given** a history manager is running, **When** the user copies several times in different
   applications, **Then** the manager is told of each new content and can read it.
4. **Given** a manager that keeps the clipboard is running, **When** the application that owned the
   copied content closes, **Then** pasting still gives the content (the manager offers it again).
5. **Given** a tool reads or sets the primary selection (US3), **Then** it works the same way as for
   the clipboard.

---

### User Story 3 - Select and Paste with the Middle Button (Priority: P3)

The user selects text with the mouse in one application and pastes it into another with a middle
click, without copying, as is common on Linux desktops (the primary selection). It is separate from
the clipboard: selecting text does not change what Ctrl+V pastes.

**Why this priority**: many Linux users rely on it, especially in terminals, but the clipboard (US1)
covers the essential need.

**Independent Test**: select a word in kitty, middle-click in Firefox's address bar: the word is
inserted; Ctrl+V there still pastes the last copied content.

**Acceptance Scenarios**:

1. **Given** text selected in one application, **When** the user middle-clicks in another
   application's text field, **Then** the selected text is inserted there.
2. **Given** content copied to the clipboard and other text then selected, **When** the user pastes
   with Ctrl+V and with a middle click, **Then** they give the copied and the selected text
   respectively.
3. **Given** text selected in an application that then closes, **When** the user middle-clicks
   elsewhere, **Then** nothing is inserted (unless a manager kept it).

---

### Edge Cases

- The user copies and then, before pasting, switches the keyboard focus several times (Mod+Tab,
  clicks): the application that ends up focused can paste; applications that only had the focus in
  between are not left with stale content.
- The application that owns the clipboard is itself focused when the user pastes in it: it pastes its
  own content.
- A menu (popup) of an application has the keyboard when the user pastes in it: the application is
  still the focused one and can paste.
- The keyboard focus is on no window (an empty canvas): nothing is offered; the next focused
  application gets the current content.
- Very large content (hundreds of megabytes, a big image): it passes between the two applications
  without RubickWay slowing down or redrawing late; how long it takes is up to the two applications.
- A paste that the source application never answers (it is busy or hung): the pasting application
  waits or gives up on its own; RubickWay and other applications are not affected.
- The user copies in an application on a monitor that is then unplugged: the content stays available.
- Two clipboard tools set the clipboard at almost the same time: the later one wins, the earlier one is
  told it no longer owns it.
- RubickWay's own console switch (Ctrl+Alt+Fn) and back: the clipboard keeps its content.
- A drag started in an application (dragging a file or a tab): it is cancelled at once, as today;
  drag-and-drop is a later feature.

### Misbehaving Clients (Principle II)

- An application sets the clipboard or primary selection without referring to one of the latest
  input events it received (a made-up or stale reference, or none at all): the request is ignored;
  the clipboard keeps its content; no error.
- A clipboard tool puts the same prepared content into a selection a second time: protocol error to
  that tool, as its protocol requires. An application doing the same (Firefox does, on every Copy)
  is not an error: the content simply stays its own (found on the laptop, 2026-11-19, when an error
  there disconnected Firefox).
- Offered content with a very large number of formats (thousands): only the first 256 distinct formats
  are kept; duplicates count once; the application keeps running.
- A paste request for content that was already replaced or withdrawn: nothing is transferred; the
  pasting application's end is closed so it does not wait forever.
- A source application that never writes or never closes its end: only the pasting application waits;
  RubickWay keeps running.
- A pasting application that never reads its end: only the source application is affected (it may
  block on its own writing); RubickWay keeps running.
- A flood of clipboard changes (thousands per second from one application or tool): handled without a
  stall; the focused application ends up with the newest content.
- An application or tool that disconnects while owning the clipboard or primary selection: the
  selection becomes empty and the focused application is told.
- Requests that belong to drag-and-drop made on clipboard content: handled as the protocol says
  (protocol error where it requires one), without affecting others.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: An application MUST be able to offer content, in one or more formats, as the clipboard
  through the standard Wayland clipboard protocol, at the version RubickWay advertises today; any other
  application MUST then be able to paste it.
- **FR-002**: The application that has the keyboard focus MUST be told what the clipboard offers when
  it gains the focus and whenever the clipboard changes while it has it. Applications without the
  keyboard focus MUST NOT be told.
- **FR-003**: Pasting MUST pass the content directly from the offering application to the pasting one;
  RubickWay MUST NOT read, store, convert or wait for the content, and the work it does for a paste
  MUST NOT depend on the content's size.
- **FR-004**: A request to set the clipboard MUST be accepted only in response to the user's input:
  it must refer to one of the latest input events that application received (a key press or release,
  a button press or release, getting the keyboard focus). It is accepted even if the application has
  lost the focus since (Ctrl+C then Mod+Tab at once). Any other request is ignored without error
  (Misbehaving Clients).
- **FR-005**: New content MUST replace the previous one; the application that owned the previous one
  MUST be told it no longer does. When the owning application withdraws its content or disconnects,
  the clipboard MUST become empty and the focused application MUST be told.
- **FR-006**: The primary selection (select, then paste with the middle button) MUST work as a second,
  independent selection under the same rules (FR-001 to FR-005) through the standard primary selection
  protocol. Middle clicks are delivered to applications like any other click; pasting is the
  application's business. It MUST be on by default; with `primary-selection off` in the
  configuration it MUST be off entirely: applications are not offered it, so a middle click pastes
  nothing (and clipboard tools see no primary selection).
- **FR-007**: Clipboard tools and managers MUST be able, through both data-control protocols used by
  `wl-clipboard` and clipboard managers (the newer standard one and the older one, with the same
  behaviour), to: be told of every change of the clipboard and the
  primary selection, read their content at any time, and set them without having a window or the
  keyboard focus. Content set by a tool MUST be offered to the focused application like any other.
  Every application MAY use these protocols (Clarifications; a possible later restriction is noted
  in Assumptions).
- **FR-008**: RubickWay MUST NOT keep copies of clipboard content: after the owner closes, the
  clipboard is empty unless a manager offers the content again (FR-007).
- **FR-009**: The clipboard MUST work the same whichever monitor the windows are on and at any pan or
  zoom of the canvas; it MUST survive monitors being unplugged and console switches.
- **FR-010**: Drag-and-drop stays unsupported in this feature: a drag MUST be cancelled at once, the
  application told, with no error (as today).
- **FR-011**: Offered content MUST be limited to 256 distinct formats; further formats are ignored and
  duplicates count once.
- **FR-012**: Each misbehaving-client case listed above MUST be handled as stated, affecting only the
  offending application.
- **FR-013**: Clipboard activity MUST NOT block the event loop or cause redraws; a paste or a change of
  the clipboard MUST NOT delay a frame.

### Key Entities

- **Selection**: one of the two shared slots, the clipboard and the primary selection; empty, or owned
  by one application or tool that offers content in a list of formats.
- **Offered content**: what an owner makes available: its formats, and the owner who supplies the data
  on request. It lives only as long as its owner keeps offering it.
- **Paste**: one transfer of the content in one chosen format from the owner to the pasting
  application, directly between the two.
- **Clipboard tool**: a program without a window (`wl-copy`, `wl-paste`, a history or persistence
  manager) that watches, reads and sets the selections.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Text copied in each of Firefox, a GTK 4 application, a Qt 6 application and kitty pastes
  correctly into each of the others, 10 out of 10 tries per pair.
- **SC-002**: An image copied in Firefox pastes into an application that accepts images, 10 out of 10
  tries.
- **SC-003**: `wl-copy` then a paste in an application, and a copy in an application then `wl-paste`,
  give the same content 10 out of 10 tries; with a clipboard-keeping manager running, content stays
  pasteable after its application closes, 10 out of 10 tries.
- **SC-004**: Text selected in one application pastes with a middle click into another, while Ctrl+V
  still gives the copied content, 10 out of 10 tries.
- **SC-005**: Pasting 100 MB of content between two applications while the user pans the canvas leaves
  no slow-frame line from RubickWay and no input-lag line from libinput in the debug log.
- **SC-006**: For each misbehaving-client case, only the offending application is affected; other
  applications and RubickWay keep running, and an application in the background never changes what
  the next paste gives.

## Assumptions

- Applications use the standard Wayland clipboard and primary selection protocols; Firefox, GTK 3 and
  4, Qt 5 and 6, kitty and foot do. The quickstart checks them by hand.
- The data formats (text encodings, images, rich text) are the applications' business; RubickWay only
  passes the list of formats and the data.
- Keeping the clipboard after its application closes, and clipboard history, are left to managers
  (FR-007, FR-008), as on niri and sway; RubickWay does not store content.
- The rule that a selection is set only in response to the user's input (FR-004) follows sway and
  other compositors built on wlroots: background programs cannot replace the clipboard, and a quick
  Ctrl+C followed by a focus change is not lost (GNOME requires the focus and would lose it). Tools go
  through FR-007 instead.
- Any application may use the clipboard-tool protocols (FR-007), so any program the user runs can
  read the clipboard in the background, for example a password copied from a password manager. This
  is accepted for now, as on niri and sway: a program running in the user's session has other ways
  to do harm, and without sandboxing RubickWay cannot reliably tell the user's tools from other
  programs. **Possible later change** (owner, 2026-11-19): restrict these protocols, for example an
  allow-list or a switch in the configuration, or allow them only to sandboxed apps that declare it.
- `wl-clipboard`, cliphist and wl-clip-persist are installed by the user; RubickWay offers the data
  control protocols those tools use, newer and older (the older one is dropped once the tools no
  longer need it; Clarifications).
- Drag-and-drop (with panning the canvas when a drag reaches a monitor edge) and the X11 clipboard
  (no XWayland yet) are later features.
- The automated tests cover the clipboard rules and transfers headless with test applications; the
  behaviour with real applications is checked by hand through the quickstart.
