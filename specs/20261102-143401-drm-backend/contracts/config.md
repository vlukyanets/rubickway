# Contract: configuration file

`$XDG_CONFIG_HOME/rubickway/config.kdl`, by default (also when `XDG_CONFIG_HOME` is empty or
not absolute) `~/.config/rubickway/config.kdl`. KDL 2.0
(<https://kdl.dev>). Read once at startup; changes apply at the next start (FR-024). Every node
is optional; a missing file means all defaults, silently.

## Full example (all defaults spelled out)

```kdl
input {
    // "Super", "Alt" or "Ctrl" (FR-005).
    mod-key "Super"
}

binds {
    // Spawn bindings: each key combination runs a program (FR-014). Any number; with none
    // configured, this one is the default.
    Mod+Return { spawn "kitty"; }

    // Pointer bindings: a modifier combination and a mouse button run a pointer gesture
    // (FR-032). With none configured, these four are the defaults.
    Mod+MouseLeft        { move-window; }
    Mod+Shift+MouseLeft  { pan-canvas; }
    Mod+MouseRight       { resize-window; }
    Mod+Shift+MouseRight { resize-window; }
}

// Seconds without a response from the event loop before RubickWay ends itself (FR-030).
watchdog-timeout-sec 10
```

Example with three programs on their own keys:

```kdl
binds {
    Mod+Return { spawn "kitty"; }
    Mod+B      { spawn "firefox"; }
    Mod+E      { spawn "kitty" "-e" "yazi"; }
}
```

A `binds` block with at least one valid spawn binding replaces the spawn default (Mod+Return is
then free unless bound again); the pointer bindings keep their defaults unless the block has a
valid pointer binding too (see "Pointer combinations").

## Nodes

| Node | Arguments | Rules |
| --- | --- | --- |
| `input` | - | child `mod-key` |
| `input > mod-key` | one string | `Super`, `Alt` or `Ctrl` (case-insensitive) |
| `binds` | - | any number of children, each a key or pointer combination as the node name |
| `binds > <key combo>` | - | one child `spawn` |
| `binds > <key combo> > spawn` | one or more strings | program (not empty), then its arguments; run without a shell |
| `binds > <pointer combo>` | - | one child: `move-window`, `resize-window` or `pan-canvas` (FR-032) |
| `binds > <pointer combo> > <action>` | none | no arguments, no children |
| `watchdog-timeout-sec` | one integer | 1 to 600 |

## Key combinations

`Modifier+...+Key`: modifiers `Mod`, `Super`, `Alt`, `Ctrl`, `Shift` (case-insensitive), then
one XKB key name (`Return`, `t`, `F5`, `space`, ...; case-insensitive if no exact match). `Mod`
is the configured `mod-key`. Matched on the key's symbol without modifiers, like the built-in
shortcuts, so `Mod+T` and `Mod+t` are the same.

Rejected (warning, default kept): unknown modifier or key, no key or two keys, a combination
that equals a built-in shortcut after resolving `Mod` (Mod+0, Mod+Q, Mod+Shift+Q, Mod+Tab,
Mod+Shift+Tab), any Ctrl+Alt+F1 .. F12.

Matching is exact: `Mod+Ctrl+Q` is allowed (it is not `Mod+Q`). A combination with no modifier
(`F12`) is allowed; that key then never reaches applications.

## Pointer combinations (FR-032)

The same modifiers, then `MouseLeft`, `MouseRight` or `MouseMiddle` (case-insensitive) in place
of the key: `Mod+Shift+MouseLeft`. At least one modifier is required (a plain click always
reaches the window or background). Matching is exact; a press that matches no pointer binding
is an ordinary click.

| Action | On a window | Over the background |
| --- | --- | --- |
| `move-window` | focus, raise and move it (a fullscreen or maximized window gets its earlier size first) | pans the canvas |
| `resize-window` | focus, raise and resize it from the nearest edge or corner | nothing |
| `pan-canvas` | pans the canvas, no focus change | pans the canvas |

With a fullscreen window on the active viewer, `resize-window` on it and `pan-canvas` anywhere
leave fullscreen and are consumed. The defaults, used while `binds` has no valid pointer
binding:

```kdl
binds {
    Mod+MouseLeft        { move-window; }
    Mod+Shift+MouseLeft  { pan-canvas; }
    Mod+MouseRight       { resize-window; }
    Mod+Shift+MouseRight { resize-window; }
}
```

One valid pointer binding replaces all four defaults; the spawn default (Mod+Return) is
replaced only by a spawn binding, and the other way round. Rejected (warning): a pointer
combination without a modifier, an unknown button, an action with arguments or children,
`spawn` on a pointer combination, a pointer action on a key combination, a pointer combination
bound twice (the first stays). Mod + scroll and pinch zoom are not bindings.

## Problems

Each problem is one warning on stderr, `<file>:<line>: <reason>, using the default` (or
`<file>: <reason>, using the default` when no line applies), and only the affected setting
falls back (FR-026):

- the file exists but cannot be read; a KDL syntax error (the whole file falls back, the line
  of the error is named);
- an unknown node or an unexpected argument, property or child (ignored, not fatal);
- a node given twice (the last one wins, with a warning; an earlier block of the same name is
  not used at all);
- a wrong value type or a value out of range;
- a binding without `spawn` or with two, an empty `spawn` or an empty program, a rejected
  combination, a combination bound
  twice (the first binding stays); `binds` without any valid spawn binding keeps the spawn
  default, and without any valid pointer binding the pointer defaults.
