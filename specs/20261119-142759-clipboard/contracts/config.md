# Contract: configuration

One new top-level node in `config.kdl` (research C10). Everything else stays as in
`specs/20261109-110128-cursor-shapes/contracts/config.md`.

```kdl
// Middle-click paste (the primary selection): on or off. On unless set to off.
primary-selection off
```

`on` and `off` are plain strings in KDL 2.0 (any case, with or without quotes). KDL's own booleans
`#true` and `#false` are accepted too. A bare `true` or `false` is not KDL 2.0 (it was KDL 1.0, as in
niri's configuration): it is a syntax error in the file, which, as for any syntax error, is reported
and leaves every setting at its default (found while implementing, 2026-11-19).

| Written | Result |
| --- | --- |
| absent | on |
| `primary-selection on` (or `#true`) | on |
| `primary-selection off` (or `#false`) | off: applications are not offered a primary selection; clipboard tools see none and their `set_primary_selection` is ignored |
| no value, another word or a number, extra values, or children | a warning naming the line (`primary-selection` expects on or off); the default (on) |
| given twice | the last one counts, with a warning (as for other nodes) |

Read once at startup, like every setting.
