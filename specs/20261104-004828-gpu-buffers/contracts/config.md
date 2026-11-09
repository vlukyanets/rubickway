# Contract: config.kdl

Adds one optional block to `specs/20261102-143401-drm-backend/contracts/config.md`, which otherwise
stays as it is. Settings for the composing GPU come with the later feature (research G13).

## distant-windows (FR-017, research G4)

Less work for windows far from the viewer, as games do with level of detail. Every value is optional
and works on its own; with no block, nothing changes.

```kdl
distant-windows {
    // Windows on a display at this canvas zoom or below are distant. 1 to 100. Without it the
    // block does nothing.
    below-zoom-percent 50

    // How many times a second a distant window is asked for a new frame. 1 to 240. Without it, as
    // often as the display refreshes. The application draws less, so the GPU has less to do. Some
    // programs draw on their own clock and are not slowed by this.
    max-fps 15

    // Resolution of the copy that smooths a distant window's GPU image, as a percentage of the size
    // it is drawn at. 25 to 400. Without it, the image's full size. Lower is cheaper and less sharp;
    // a copy well below the image's size may also shimmer on fine detail.
    copy-detail-percent 100
}
```

## Errors

As everywhere in the file: a wrong value is a warning with its line and that value is not used.
`max-fps` or `copy-detail-percent` without `below-zoom-percent` is also a warning, as they would do
nothing.
