//! The configuration file `config.kdl`: plain settings with defaults (research S16,
//! `contracts/config.md`). Parsing never fails: every problem is a warning and that setting keeps
//! its default.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kdl::{KdlDocument, KdlNode, KdlValue};
use xkbcommon::xkb;

use crate::input::bindings::{
    Bindings, KeyCombo, ModKey, PointerAction, PointerBind, PointerCombo, SpawnBind,
};
use crate::input::{Button, Keysym, Modifiers};

/// The settings read at startup (FR-024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Mod (FR-005).
    pub mod_key: ModKey,
    /// The spawn bindings (FR-014); never empty (the default is Mod+Return -> kitty).
    pub spawns: Vec<SpawnConfig>,
    /// The pointer bindings (FR-032); never empty (the defaults are window-management's
    /// gestures).
    pub pointer: Vec<PointerConfig>,
    /// How long the event loop may be silent before RubickWay ends itself (FR-030).
    pub watchdog_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mod_key: ModKey::Super,
            spawns: vec![SpawnConfig::default()],
            pointer: default_pointer(),
            watchdog_timeout: Duration::from_secs(10),
        }
    }
}

/// A pointer binding as written: `Mod` not yet resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerConfig {
    pub combo: PointerSpec,
    pub action: PointerAction,
}

/// A pointer combination as written (`Mod+Shift+MouseLeft`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerSpec {
    pub mods: ModSpec,
    pub button: Button,
}

impl PointerSpec {
    /// The exact modifiers once `Mod` is known.
    pub fn resolve(&self, mod_key: ModKey) -> PointerCombo {
        PointerCombo {
            mods: self.mods.resolve(mod_key),
            button: self.button,
        }
    }
}

/// The modifiers of a combination as written; `mod_` is the configured Mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModSpec {
    pub mod_: bool,
    pub super_: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl ModSpec {
    /// The exact modifiers once `Mod` is known.
    pub fn resolve(&self, mod_key: ModKey) -> Modifiers {
        let m = if self.mod_ {
            mod_key.modifiers()
        } else {
            Modifiers::NONE
        };
        Modifiers {
            ctrl: m.ctrl || self.ctrl,
            alt: m.alt || self.alt,
            shift: m.shift || self.shift,
            logo: m.logo || self.super_,
        }
    }

    fn any(&self) -> bool {
        self.mod_ || self.super_ || self.alt || self.ctrl || self.shift
    }
}

/// The default pointer bindings, as `contracts/config.md` writes them.
fn default_pointer() -> Vec<PointerConfig> {
    let m = ModSpec {
        mod_: true,
        ..ModSpec::default()
    };
    let ms = ModSpec { shift: true, ..m };
    let b = |mods, button, action| PointerConfig {
        combo: PointerSpec { mods, button },
        action,
    };
    vec![
        b(m, Button::Left, PointerAction::MoveWindow),
        b(ms, Button::Left, PointerAction::PanCanvas),
        b(m, Button::Right, PointerAction::ResizeWindow),
        b(ms, Button::Right, PointerAction::ResizeWindow),
    ]
}

/// A spawn binding as written: `Mod` not yet resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnConfig {
    pub combo: ComboSpec,
    /// Program and arguments, run without a shell; never empty.
    pub command: Vec<String>,
}

impl Default for SpawnConfig {
    /// Mod+Return running `kitty`, used when no binding is configured.
    fn default() -> Self {
        SpawnConfig {
            combo: ComboSpec {
                mod_: true,
                super_: false,
                alt: false,
                ctrl: false,
                shift: false,
                sym: Keysym::Return,
            },
            command: vec!["kitty".into()],
        }
    }
}

/// A key combination as written in the configuration (`Mod+Shift+t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboSpec {
    pub mod_: bool,
    pub super_: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    /// The key's symbol, letters lowered (it is matched on the level-0 symbol).
    pub sym: Keysym,
}

impl ComboSpec {
    /// The exact modifiers once `Mod` is known.
    pub fn resolve(&self, mod_key: ModKey) -> KeyCombo {
        let mods = ModSpec {
            mod_: self.mod_,
            super_: self.super_,
            alt: self.alt,
            ctrl: self.ctrl,
            shift: self.shift,
        };
        KeyCombo {
            mods: mods.resolve(mod_key),
            sym: self.sym,
        }
    }
}

/// One problem found in the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    /// 1-based line, when the problem has one.
    pub line: Option<usize>,
    pub message: String,
}

impl ConfigWarning {
    /// The log line: `<file>:<line>: <reason>, using the default` (`contracts/cli.md`).
    pub fn render(&self, file: &Path) -> String {
        match self.line {
            Some(line) => format!(
                "{}:{line}: {}, using the default",
                file.display(),
                self.message
            ),
            None => format!("{}: {}, using the default", file.display(), self.message),
        }
    }
}

/// `$XDG_CONFIG_HOME/rubickway/config.kdl`, or `~/.config/rubickway/config.kdl` when the
/// variable is unset, empty or not absolute (XDG base directory specification). `None`
/// without a home directory either.
pub fn config_path(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let base = match xdg_config_home.map(Path::new) {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => Path::new(home?).join(".config"),
    };
    Some(base.join("rubickway").join("config.kdl"))
}

impl Config {
    /// The bindings this configuration asks for (Mod, the spawn bindings in order, and the
    /// pointer bindings).
    pub fn bindings(&self) -> Bindings {
        Bindings::new(
            self.mod_key,
            self.spawns
                .iter()
                .map(|s| SpawnBind {
                    combo: s.combo.resolve(self.mod_key),
                    command: s.command.clone(),
                })
                .collect(),
        )
        .with_pointer(
            self.pointer
                .iter()
                .map(|p| PointerBind {
                    combo: p.combo.resolve(self.mod_key),
                    action: p.action,
                })
                .collect(),
        )
    }

    /// Reads the file at `path`. A missing file gives the defaults silently; an unreadable one
    /// the defaults and one warning without a line.
    pub fn load(path: &Path) -> (Config, Vec<ConfigWarning>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                (Config::default(), Vec::new())
            }
            Err(err) => (
                Config::default(),
                vec![ConfigWarning {
                    line: None,
                    message: format!("cannot read it ({err})"),
                }],
            ),
        }
    }

    /// Parses configuration text. Never fails: problems become warnings (FR-026).
    pub fn parse(text: &str) -> (Config, Vec<ConfigWarning>) {
        let mut p = Parser {
            text,
            config: Config::default(),
            warnings: Vec::new(),
            bindings: Vec::new(),
            pointer_binds: Vec::new(),
        };
        match text.parse::<KdlDocument>() {
            Ok(doc) => {
                p.document(&doc);
                p.choose_bindings();
            }
            Err(err) => {
                let first = err.diagnostics.first();
                let line = first.map(|d| p.line_of(d.span.offset()));
                let reason = first
                    .and_then(|d| d.message.clone())
                    .unwrap_or_else(|| "not valid KDL".into());
                return (
                    Config::default(),
                    vec![ConfigWarning {
                        line,
                        message: format!("syntax error: {reason}"),
                    }],
                );
            }
        }
        (p.config, p.warnings)
    }
}

/// Walks the document into a `Config`, collecting warnings.
struct Parser<'a> {
    text: &'a str,
    config: Config,
    warnings: Vec<ConfigWarning>,
    /// Well-formed spawn bindings and their lines; checked against the built-in shortcuts and
    /// each other once `mod-key` is known (it may come later in the file).
    bindings: Vec<(SpawnConfig, usize)>,
    /// Well-formed pointer bindings and their lines, checked against each other the same way.
    pointer_binds: Vec<(PointerConfig, usize)>,
}

/// A combination as written: a key, or a mouse button.
enum Combo {
    Key(ComboSpec),
    Pointer(PointerSpec),
}

/// A binding as written.
enum Bound {
    Spawn(SpawnConfig),
    Pointer(PointerConfig),
}

/// The button of a pointer combination's last part (`MouseLeft`, case-insensitive).
fn mouse_button(name: &str) -> Option<Result<Button, String>> {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("mouse")?;
    Some(match rest {
        "left" => Ok(Button::Left),
        "right" => Ok(Button::Right),
        "middle" => Ok(Button::Middle),
        _ => Err(format!(
            "unknown mouse button `{name}` (MouseLeft, MouseRight or MouseMiddle)"
        )),
    })
}

/// Parses `Mod+Shift+t` or `Mod+Shift+MouseLeft`: modifiers, then exactly one XKB key name or
/// mouse button; key letters are lowered (they are matched on the level-0 symbol).
fn parse_combo(text: &str) -> Result<Combo, String> {
    let parts: Vec<&str> = text.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        return Err("empty key combination".into());
    };
    let mut mods = ModSpec::default();
    for m in modifiers {
        match m.to_ascii_lowercase().as_str() {
            "mod" => mods.mod_ = true,
            "super" => mods.super_ = true,
            "alt" => mods.alt = true,
            "ctrl" => mods.ctrl = true,
            "shift" => mods.shift = true,
            other => return Err(format!("unknown modifier `{other}` in `{text}`")),
        }
    }
    if key.is_empty() || key.contains('\0') {
        return Err(format!("no key in `{text}`"));
    }
    if let Some(button) = mouse_button(key) {
        let button = button?;
        if !mods.any() {
            return Err(format!(
                "a pointer combination needs a modifier (`{text}` would take every click)"
            ));
        }
        return Ok(Combo::Pointer(PointerSpec { mods, button }));
    }
    let mut spec = ComboSpec {
        mod_: mods.mod_,
        super_: mods.super_,
        alt: mods.alt,
        ctrl: mods.ctrl,
        shift: mods.shift,
        sym: Keysym::NoSymbol,
    };
    let mut sym = xkb::keysym_from_name(key, xkb::KEYSYM_NO_FLAGS);
    if sym == Keysym::NoSymbol {
        sym = xkb::keysym_from_name(key, xkb::KEYSYM_CASE_INSENSITIVE);
    }
    if sym == Keysym::NoSymbol {
        return Err(format!("unknown key `{key}` in `{text}`"));
    }
    let raw = sym.raw();
    spec.sym = if (0x41..=0x5a).contains(&raw) {
        Keysym::new(raw + 0x20)
    } else {
        sym
    };
    Ok(Combo::Key(spec))
}

/// The pointer action named `name`, if it is one.
fn pointer_action(name: &str) -> Option<PointerAction> {
    match name {
        "move-window" => Some(PointerAction::MoveWindow),
        "resize-window" => Some(PointerAction::ResizeWindow),
        "pan-canvas" => Some(PointerAction::PanCanvas),
        _ => None,
    }
}

/// Why a combination cannot be a spawn binding, once Mod is known (FR-025).
fn conflict(combo: &ComboSpec, mod_key: ModKey) -> Option<String> {
    let resolved = combo.resolve(mod_key);
    if Bindings::new(mod_key, Vec::new())
        .builtin()
        .iter()
        .any(|b| b.combo == resolved)
    {
        return Some("the combination is a built-in shortcut".into());
    }
    let f1 = Keysym::F1.raw();
    let sym = resolved.sym.raw();
    if resolved.mods.ctrl && resolved.mods.alt && (f1..f1 + 12).contains(&sym) {
        return Some("Ctrl+Alt+F1 .. F12 switch consoles".into());
    }
    None
}

impl Parser<'_> {
    /// The 1-based line of a byte offset (KDL spans count bytes).
    fn line_of(&self, offset: usize) -> usize {
        self.text
            .bytes()
            .take(offset)
            .filter(|b| *b == b'\n')
            .count()
            + 1
    }

    fn warn(&mut self, node: &KdlNode, message: impl Into<String>) {
        let line = Some(self.line_of(node.span().offset()));
        self.warnings.push(ConfigWarning {
            line,
            message: message.into(),
        });
    }

    /// Children of a block, warning on nodes given twice: only the last of each name is
    /// returned, in document order.
    fn each<'n>(&mut self, doc: &'n KdlDocument) -> Vec<&'n KdlNode> {
        let nodes = doc.nodes();
        let mut seen: Vec<&str> = Vec::new();
        for node in nodes {
            let name = node.name().value();
            if seen.contains(&name) {
                self.warn(node, format!("`{name}` given again; the last one counts"));
            } else {
                seen.push(name);
            }
        }
        nodes
            .iter()
            .enumerate()
            .filter(|(i, node)| {
                let name = node.name().value();
                !nodes[i + 1..].iter().any(|n| n.name().value() == name)
            })
            .map(|(_, node)| node)
            .collect()
    }

    fn document(&mut self, doc: &KdlDocument) {
        for node in self.each(doc) {
            match node.name().value() {
                "input" => self.input(node),
                "binds" => self.binds(node),
                "watchdog-timeout-sec" => self.watchdog(node),
                other => self.warn(node, format!("unknown setting `{other}`, ignored")),
            }
        }
    }

    fn input(&mut self, node: &KdlNode) {
        let Some(children) = self.block(node) else {
            return;
        };
        for child in self.each(children) {
            match child.name().value() {
                "mod-key" => self.mod_key(child),
                other => self.warn(child, format!("unknown setting `input > {other}`, ignored")),
            }
        }
    }

    /// Spawn bindings `<key combo> { spawn "program" "args"...; }` (FR-014) and pointer
    /// bindings `<pointer combo> { move-window; }` (FR-032).
    fn binds(&mut self, node: &KdlNode) {
        let Some(children) = self.block(node) else {
            return;
        };
        for child in children.nodes() {
            let line = self.line_of(child.span().offset());
            match self.binding(child) {
                Some(Bound::Spawn(spawn)) => self.bindings.push((spawn, line)),
                Some(Bound::Pointer(pointer)) => self.pointer_binds.push((pointer, line)),
                None => {}
            }
        }
    }

    fn binding(&mut self, node: &KdlNode) -> Option<Bound> {
        let combo = match parse_combo(node.name().value()) {
            Ok(c) => c,
            Err(reason) => {
                self.warn(node, reason);
                return None;
            }
        };
        if !node.entries().is_empty() {
            self.warn(node, "a binding takes no arguments");
            return None;
        }
        match combo {
            Combo::Key(combo) => self.spawn_binding(node, combo).map(Bound::Spawn),
            Combo::Pointer(combo) => self.pointer_binding(node, combo).map(Bound::Pointer),
        }
    }

    /// `<pointer combo> { <action>; }`: exactly one action, without arguments or children.
    fn pointer_binding(&mut self, node: &KdlNode, combo: PointerSpec) -> Option<PointerConfig> {
        const NEEDS: &str = "a pointer binding needs one of `{ move-window; }`, `{ resize-window; }`, `{ pan-canvas; }`";
        let actions = node.children().map(KdlDocument::nodes).unwrap_or_default();
        let [action] = actions else {
            self.warn(node, NEEDS);
            return None;
        };
        let name = action.name().value();
        let Some(pointer) = pointer_action(name) else {
            let reason = if name == "spawn" {
                "`spawn` needs a key combination, not a mouse button".to_string()
            } else {
                format!("unknown action `{name}` (move-window, resize-window or pan-canvas)")
            };
            self.warn(action, reason);
            return None;
        };
        if !action.entries().is_empty() || action.children().is_some() {
            self.warn(action, format!("`{name}` takes no arguments"));
            return None;
        }
        Some(PointerConfig {
            combo,
            action: pointer,
        })
    }

    /// `<key combo> { spawn "program" "args"...; }`.
    fn spawn_binding(&mut self, node: &KdlNode, combo: ComboSpec) -> Option<SpawnConfig> {
        let Some(children) = node.children() else {
            self.warn(node, "a binding needs `{ spawn \"program\"; }`");
            return None;
        };
        let mut command = None;
        for action in children.nodes() {
            let name = action.name().value();
            if pointer_action(name).is_some() {
                self.warn(
                    action,
                    format!("`{name}` needs a mouse button, such as `Mod+MouseLeft`"),
                );
                return None;
            }
            if name != "spawn" {
                self.warn(action, format!("unknown action `{name}` (only `spawn`)"));
                return None;
            }
            if command.is_some() {
                self.warn(action, "a binding takes one `spawn`");
                return None;
            }
            let args: Option<Vec<String>> = action
                .entries()
                .iter()
                .map(|e| match (e.name(), e.value()) {
                    (None, KdlValue::String(s)) if !s.contains('\0') => Some(s.clone()),
                    _ => None,
                })
                .collect();
            match args {
                Some(args)
                    if args.first().is_some_and(|p| !p.is_empty())
                        && action.children().is_none() =>
                {
                    command = Some(args);
                }
                _ => {
                    self.warn(
                        action,
                        "`spawn` expects a program and arguments, as strings, the program not empty",
                    );
                    return None;
                }
            }
        }
        match command {
            Some(command) => Some(SpawnConfig { combo, command }),
            None => {
                self.warn(node, "a binding needs `{ spawn \"program\"; }`");
                None
            }
        }
    }

    /// Keeps every binding that shadows no built-in shortcut and no earlier binding; the
    /// others are reported. At least one kept binding replaces the default (FR-014, FR-025).
    fn choose_bindings(&mut self) {
        let mod_key = self.config.mod_key;
        let mut kept: Vec<SpawnConfig> = Vec::new();
        for (spawn, line) in std::mem::take(&mut self.bindings) {
            let resolved = spawn.combo.resolve(mod_key);
            let problem = conflict(&spawn.combo, mod_key).or_else(|| {
                kept.iter()
                    .any(|k| k.combo.resolve(mod_key) == resolved)
                    .then(|| "the combination is bound twice; the first binding counts".into())
            });
            match problem {
                Some(message) => self.warnings.push(ConfigWarning {
                    line: Some(line),
                    message,
                }),
                None => kept.push(spawn),
            }
        }
        if !kept.is_empty() {
            self.config.spawns = kept;
        }
        // Pointer bindings: the first of a combination counts; at least one kept replaces the
        // pointer defaults, independently of the spawn bindings (FR-032).
        let mut pointer: Vec<PointerConfig> = Vec::new();
        for (bind, line) in std::mem::take(&mut self.pointer_binds) {
            let resolved = bind.combo.resolve(mod_key);
            if pointer.iter().any(|k| k.combo.resolve(mod_key) == resolved) {
                self.warnings.push(ConfigWarning {
                    line: Some(line),
                    message: "the combination is bound twice; the first binding counts".into(),
                });
            } else {
                pointer.push(bind);
            }
        }
        if !pointer.is_empty() {
            self.config.pointer = pointer;
        }
    }

    /// Whole seconds, 1 to 600 (FR-024, FR-030).
    fn watchdog(&mut self, node: &KdlNode) {
        let value = match (node.entries(), node.children()) {
            ([entry], None) if entry.name().is_none() => entry.value().as_integer(),
            _ => None,
        };
        match value.and_then(|v| u64::try_from(v).ok()) {
            Some(secs) if (1..=600).contains(&secs) => {
                self.config.watchdog_timeout = Duration::from_secs(secs);
            }
            _ => self.warn(
                node,
                "`watchdog-timeout-sec` expects whole seconds from 1 to 600",
            ),
        }
    }

    fn mod_key(&mut self, node: &KdlNode) {
        let Some(value) = self.one_string(node) else {
            return;
        };
        match value.to_ascii_lowercase().as_str() {
            "super" => self.config.mod_key = ModKey::Super,
            "alt" => self.config.mod_key = ModKey::Alt,
            "ctrl" => self.config.mod_key = ModKey::Ctrl,
            _ => self.warn(
                node,
                format!("unknown mod-key \"{value}\" (Super, Alt or Ctrl)"),
            ),
        }
    }

    /// A node that must be a block with no arguments.
    fn block<'n>(&mut self, node: &'n KdlNode) -> Option<&'n KdlDocument> {
        if !node.entries().is_empty() {
            self.warn(
                node,
                format!("`{}` takes no arguments", node.name().value()),
            );
        }
        let children = node.children();
        if children.is_none() {
            self.warn(node, format!("`{}` needs a block", node.name().value()));
        }
        children
    }

    /// The single string argument of a leaf node.
    fn one_string(&mut self, node: &KdlNode) -> Option<String> {
        match (node.entries(), node.children()) {
            ([entry], None) if entry.name().is_none() => match entry.value() {
                KdlValue::String(s) => Some(s.clone()),
                _ => {
                    self.warn(node, format!("`{}` expects a string", node.name().value()));
                    None
                }
            },
            _ => {
                self.warn(
                    node,
                    format!("`{}` expects exactly one string", node.name().value()),
                );
                None
            }
        }
    }
}
