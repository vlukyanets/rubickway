# Contract: config.kdl

Adds one optional block to `specs/20261102-143401-drm-backend/contracts/config.md` (and the
`distant-windows` block of `specs/20261104-004828-gpu-buffers/contracts/config.md`), which otherwise
stay as they are.

## cursor (FR-010, research C6, C11)

```kdl
cursor {
    // The cursor theme for pointer images: a directory name under ~/.local/share/icons,
    // ~/.icons, /usr/share/icons or /usr/share/pixmaps, with the themes it inherits from.
    // Without it, the "default" theme. Where the theme has no image for a shape, RubickWay
    // draws its own. Read at startup.
    theme "Adwaita"
}
```

With no block, or a block without `theme`, nothing changes from today: the `default` theme, or the
built-in images.

## Errors

As everywhere in the file, each of these is a warning with its line, and the default applies:

- `theme` with an empty string;
- `theme` with a name that is `.` or `..`, or contains `/`;
- `theme` with a value that is not a string, or with more than one value;
- an unknown child, for example `size`.

A theme that is well formed but not installed is not a configuration warning: it is reported at
startup ([cli.md](cli.md)).
