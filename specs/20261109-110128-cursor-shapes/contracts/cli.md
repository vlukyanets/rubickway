# Contract: log lines

Adds to `specs/20261102-143401-drm-backend/contracts/cli.md` and
`specs/20261104-004828-gpu-buffers/contracts/cli.md`. The command line and exit statuses do not
change. Each fallback leaves one line in plain words (FR-012), once per run per case (SC-003).

## At startup (info)

When a theme is used, also after the line saying a configured theme is missing:

```text
pointer images from cursor theme "<name>"
```

## At startup (warn)

At most one line for the choice of theme:

```text
cursor theme "<name>" from the configuration is not installed: using the default cursor theme
cursor theme "<name>" from the configuration is not installed, and there is no default cursor theme: RubickWay draws its own pointer images
cursor theme "<name>" from the configuration is not installed, and the default cursor theme has no pointer images: RubickWay draws its own pointer images
no cursor theme found: RubickWay draws its own pointer images
cursor theme "<name>" has no pointer images (it inherits from <names>, not installed): RubickWay draws its own
```

The last line is the owner's laptop today: `default` inherits from `Adwaita`, which is not installed.
Without missing inherited themes, the bracket is left out.

When a theme is used, at most one line for the shapes it lacks, naming all of them:

```text
cursor theme "<name>" has no images for <shape>, <shape>, ...: RubickWay draws its own for those
```

One line per damaged file:

```text
cursor theme "<name>": <path> is damaged (<reason>): RubickWay draws its own <shape> pointer
```

`<reason>` is one of `not a cursor file`, `cut short`, `has no image`, `image larger than 256 pixels`.

## While running (info, once per run per shape)

The first time one of the FR-007 shapes (`context-menu`, `alias`, `copy`, `move`, `dnd-ask`) is
shown with the arrow because neither the theme nor RubickWay has an image for it:

```text
an application asked for the <shape> pointer, which RubickWay has no image for: showing the arrow
```

## While running (debug, on each change only)

```text
application <id> asks for the <shape> pointer
application <id> hides the pointer
application <id> shows its own pointer image (the arrow is drawn)
```

`<shape>` is the CSS name (`text`, `e-resize`, `dnd-ask`, ...). `<id>` is the client id used in
other application lines.
