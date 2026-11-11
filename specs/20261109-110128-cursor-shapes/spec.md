# Feature Specification: Cursor Shapes

**Feature Branch**: `worktree-cursor-shapes`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "Cursor shapes for applications. Today RubickWay ignores application
cursors: over kitty's border no resize cursor appears, so the owner cannot see where to drag.
Implement wp_cursor_shape_v1 so applications (kitty and modern toolkits) can name the cursor shape
they want. For shapes, draw built-in images when no cursor theme is installed (the owner's laptop has
none): I-beam for text, pointing hand, not-allowed, resize arrows and so on, in the same way as the
existing built-in arrow, grab, move and resize images; an installed cursor theme takes priority over
the built-in images. Drawing applications' own cursor surfaces (wl_pointer.set_cursor with a surface)
is out of scope for now. The hardware cursor plane is a separate later idea and also out of scope.
Every fallback (no theme, unknown shape, missing image in a theme) logs one plain-words line."

## Clarifications

### Session 2026-11-09

- Q: How is the cursor theme chosen - only the system's `default` theme as today, the standard
  `XCURSOR_THEME` setting, or a configuration option? -> A: A configuration option,
  `cursor { theme "<name>" }` in `config.kdl`; without it, the `default` theme as today.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The Pointer Shows What the Application Asks For (Priority: P1)

The user moves the pointer over an application window. The application says which pointer shape fits
the spot under it, and RubickWay shows that shape: a resize arrow over kitty's border, an I-beam over
text, a pointing hand over a link, a "not allowed" sign over something disabled. With no cursor theme
installed, as on the owner's laptop, RubickWay draws these shapes itself.

**Why this priority**: This is the reason for the feature. Without it the user cannot see where a
window's border starts, so resizing by the border is guesswork, and text and links look the same as
everything else.

**Independent Test**: On the laptop with no cursor theme, open kitty and move the pointer over each
side and corner of its border and over its text. Each spot shows the matching image, and the arrow
comes back over the background.

**Acceptance Scenarios**:

1. **Given** kitty open and no cursor theme installed, **When** the pointer moves over kitty's left
   or right border, **Then** it shows a left-right resize arrow; over the top or bottom border, an
   up-down arrow; over each corner, the diagonal arrow of that corner.
2. **Given** kitty open, **When** the pointer moves over its text, **Then** it shows an I-beam.
3. **Given** an application that names a shape, **When** the pointer moves from that application's
   window to the canvas background, **Then** the pointer shows the arrow again.
4. **Given** the pointer over a window that last named the I-beam, **When** the pointer leaves the
   window and enters another window, **Then** the pointer shows the arrow until the other window's
   application names a shape.
5. **Given** an application that names each shape the protocol has, one after another, **When** each
   is shown, **Then** each shape listed in FR-006 has its own recognisable image, and the shapes listed
   in FR-007 show the image named there.
6. **Given** a window shown on two monitors, **When** the pointer crosses from one monitor to the
   other over that window, **Then** the shape stays the one the application named.

---

### User Story 2 - An Installed Cursor Theme Takes Priority (Priority: P2)

A user who installs a cursor theme gets that theme's images for application shapes as well as for
RubickWay's own (moving, resizing, panning). Where the theme has no image for a shape, the built-in one
is shown, and the log says so once.

**Why this priority**: The built-in images are small and plain; a theme looks better and matches other
desktops. It comes after US1 because without US1 there is nothing for the theme to show.

**Independent Test**: Install a common cursor theme, select it as described in FR-010, start RubickWay
and move the pointer over kitty's border and text: the images are the theme's, not the built-in ones.

**Acceptance Scenarios**:

1. **Given** a selected theme that has images for all shapes, **When** an application names a shape,
   **Then** the theme's image is shown.
2. **Given** a selected theme missing images for some shapes, **When** RubickWay starts, **Then** the
   log has one line naming those shapes and saying they use the built-in images, and they do.
3. **Given** no theme installed at all, **When** RubickWay starts, **Then** the log has one line saying
   no cursor theme was found and the built-in images are used.
4. **Given** a theme file that is damaged, **When** RubickWay starts, **Then** that shape uses the
   built-in image and the log says which shape and why, in one line.

---

### User Story 3 - Nothing Else Changes (Priority: P3)

Everything that works today keeps working: RubickWay's own gestures show their own images,
applications that hide the pointer still hide it, and applications that do not name shapes show the
arrow as before. One rule changes on purpose: hiding the pointer now counts only with the serial of
the application's latest pointer enter, and ends at the next enter until the application hides it
again (FR-002, FR-003), as the protocol expects.

**Why this priority**: Today's behaviour must not regress.

**Independent Test**: Move, resize and pan with the Mod gestures over a window whose application names
shapes, and confirm RubickWay's images are shown during the gesture. Run an application that hides the
pointer, and one that does not name shapes.

**Acceptance Scenarios**:

1. **Given** an application that names the I-beam, **When** the user moves, resizes or pans with a
   RubickWay gesture over its window, **Then** RubickWay's move, resize or grab image is shown during
   the gesture, and the application's shape comes back after it.
2. **Given** an application that hides the pointer over its window, **When** the pointer is over it,
   **Then** no pointer is shown, as today; **When** the application then names a shape, **Then** that
   shape is shown.
3. **Given** an application that sets its own cursor image instead of naming a shape, **When** the
   pointer is over it, **Then** the arrow is shown, as today.
4. **Given** an application that names a shape, **When** the application quits while the pointer is
   over its window, **Then** the pointer shows whatever is now under it.

### Edge Cases

- An application names a shape while the pointer is over another application's window: nothing
  changes.
- An application names a shape with the serial of an older pointer enter: the request is ignored.
- An application names shapes very often (for example on every motion event): the pointer is redrawn
  only when the shown image changes, and the desktop stays smooth.
- Several applications name shapes: only the one whose window is under the pointer decides.
- An application asks for a shape-naming object twice for the same pointer: the most recent request of
  either one counts.
- A shape is named while the pointer is hidden by the application: the shape replaces the hidden
  state, as the protocol says.
- A shape is named during a RubickWay gesture: the application does not have the pointer during the
  gesture, so the request is ignored. When the gesture ends over its window, the pointer enters it
  again and the application names its shape again, as it does on every enter.
- The selected theme exists but has no images at all: the same as no theme, one line in the log.
- The configuration names a theme that is not installed, for example a typo: one line in the log
  naming it, and the `default` theme or the built-in images are used.
- The pointer leaves all windows while a shape is shown, for example into the background or a gap
  between monitors' views: the arrow is shown.

### Misbehaving Clients (Principle II)

- A shape value that does not exist, or one newer than the protocol version the application bound:
  that application gets a protocol error; nothing else is affected.
- A shape request after the pointer it was made for is gone: ignored.
- A shape request or cursor image request (including hiding) with a wrong or stale serial: ignored,
  no error (as the protocol says). Hiding with a stale serial used to count; it no longer does.
- A request for a shape-naming object for a tablet tool: RubickWay offers no tablets, so no
  application can make one; nothing to handle.
- A flood of shape requests: handled without a stall; at most one redraw per frame.
- A client disconnecting at any point: its shape stops being shown.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: RubickWay MUST let applications name a pointer shape for the pointer while it is over
  their windows, through the standard Wayland cursor shape protocol, at the newest version it
  implements and tests.
- **FR-002**: A named shape MUST be shown only while the pointer is over a surface of the application
  that named it, and only if the request carries the serial of that application's latest pointer
  enter. The same holds for hiding the pointer and for setting a cursor image.
- **FR-003**: When the pointer enters a window, the arrow MUST be shown until that window's application
  names a shape or hides the pointer.
- **FR-004**: The latest of naming a shape, hiding the pointer and setting a cursor image MUST decide
  what is shown over the application's windows; setting a cursor image shows the arrow, as today.
- **FR-005**: While a RubickWay gesture is in progress (moving, resizing, panning), RubickWay's own
  image for it MUST be shown instead of any named shape. When the gesture ends with the pointer over
  a window, RubickWay MUST send that window's application a new pointer enter, so it can name its
  shape again; until it does, the arrow is shown (FR-003).
- **FR-006**: RubickWay MUST draw a built-in image for each of these shapes, each recognisable on its
  own: default arrow, pointing hand (pointer), I-beam (text), sideways I-beam (vertical text), busy
  (wait), arrow with busy sign (progress), crosshair, cell (thick plus), help (question mark), not
  allowed (also used for no drop), grab (open hand), grabbing (closed hand), all directions (all
  scroll, all resize), the four resize axes (left-right, up-down, the two diagonals), zoom in and zoom
  out.
- **FR-007**: Shapes MUST use the image of their meaning where it is the same: single-border resizes
  use the double arrow of their axis (east and west -> left-right, north and south -> up-down,
  north-east and south-west -> that diagonal, north-west and south-east -> the other), column resize
  -> left-right, row resize -> up-down. The drag-and-drop shapes (alias, copy, move, ask) and context
  menu MUST show the default arrow when the theme has no image for them (FR-009), since RubickWay has
  no drag-and-drop yet; the first time each is shown with the arrow in a run, the log MUST say in one
  line that it has no image of its own and the arrow is shown.
- **FR-008**: The built-in images MUST be shown at the same size as today's built-in images (24
  pixels), with the point of the image that marks the pointer position at: the tip of the arrow for
  the arrow and the shapes drawn as an arrow with a sign (help, progress); the fingertip for the
  pointing hand; the centre of the lens for zoom in and out; the centre for all others.
- **FR-009**: Where a selected cursor theme has an image for a shape (named shape or RubickWay's own),
  that image MUST be shown instead of the built-in one, at the size the theme has nearest to 24 pixels,
  drawn as it is, with the theme's own pointer position. A theme without its own image for a single
  border, column or row resize uses its image for that axis (as FR-007) before the built-in one.
  RubickWay's own gestures look up the same theme images as today.
- **FR-010**: The cursor theme MUST be the one named by the configuration's `cursor { theme "<name>" }`
  setting, looked up in the standard icon directories with the themes it inherits from; without the
  setting, the `default` theme, as today. A configured theme that is not installed MUST be reported
  in one log line, and RubickWay MUST then use the `default` theme, or the built-in images if that is
  missing too. A theme that is installed but has no images (for example, it only inherits from themes
  that are not installed) counts as no theme: the built-in images, not `default`. An empty or
  malformed value (the exact rules are in the configuration contract) is reported and ignored like
  other bad settings.
- **FR-011**: The theme MUST be read once at startup, as today; a theme installed or changed while
  RubickWay runs takes effect at the next start.
- **FR-012**: Every fallback MUST log one plain-words line, once per run per case: the configured
  theme not installed; no theme found;
  shapes the theme has no image for (one line naming all of them, however many); a damaged theme file
  (naming the shape; each damaged file is its own case); a shape with no image of its own (FR-007, on
  first use).
- **FR-013**: Changing the shape MUST NOT redraw more than once per display frame, and MUST NOT redraw
  at all when the shown image does not change.
- **FR-014**: Each misbehaving-client case listed above MUST be handled as stated, affecting only the
  offending application.

### Key Entities

- **Named shape**: the shape an application asked for, belonging to that application and to its
  latest pointer enter; replaced by a newer shape, a hidden pointer or a cursor image.
- **Pointer image**: what is drawn at the pointer, from the theme or built in, with the point that
  marks the pointer position.
- **Cursor theme**: a set of images on disk, possibly inheriting from other themes; read at startup.
  Chosen by the `cursor { theme }` setting, otherwise `default`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the laptop with no theme, the pointer over each of kitty's four borders and four
  corners shows the matching resize image, and over kitty's text the I-beam, in 10 out of 10 tries
  each.
- **SC-002**: Every shape the protocol defines is shown either with its own built-in image (FR-006) or
  with the image FR-007 names for it; no shape shows nothing, and only the default shape and the
  FR-007 arrow cases show the arrow. A test program that names every shape in turn lets the owner see
  all of them, and tell the FR-006 images apart by eye.
- **SC-003**: With no usable theme (none installed, or only one without images, as on the owner's
  laptop), the log of a run has exactly one line about it, and at most one line for each other
  fallback, however long the run.
- **SC-004**: Moving the pointer back and forth across kitty's border 100 times, at debug log level,
  leaves no slow-frame line from RubickWay and no input lag line from libinput.
- **SC-005**: For each misbehaving-client case, only the offending application is affected; other
  applications and RubickWay keep running.

## Assumptions

- kitty (0.49 on the laptop) names its shapes through the cursor shape protocol; recent GTK 4 and
  Qt 6 releases are expected to as well, which the quickstart checks by hand. Applications that set their own cursor images (for example GTK 3) keep showing
  the arrow until a later feature draws those images.
- Drawing applications' own cursor images, the hardware cursor plane, cursor animation (themes' busy
  animations show their first image) and tablets are out of scope.
- The built-in images keep today's style: two colours plus transparency, drawn at low resolution and
  doubled, and fit in the same size.
- The cursor size stays a fixed 24 pixels, as today (owner's choice); the `cursor` block has no
  size setting (Principle VI).
- Applications that draw their own cursor images may read `XCURSOR_THEME` themselves; RubickWay does
  not set it for the programs it starts. Setting it (and `XCURSOR_SIZE`) belongs with showing
  applications' own cursor images, a later feature, where its effect can be seen.
- Applications set their cursor again on every pointer enter, as the protocol expects; until they do,
  the arrow is shown. Applications that hide the pointer (kitty while typing, video players) hide it
  again after each enter.
- Drag-and-drop is a stub today (drags are cancelled), so its shapes only appear when an application
  names them on its own.
