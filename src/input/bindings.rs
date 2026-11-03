//! Compositor shortcuts: the built-in table with a configurable Mod, plus the spawn bindings
//! from the configuration (research S15), and the pointer bindings for the Mod pointer
//! gestures (FR-032, research S24).

use super::{Button, Keysym, Modifiers};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Return the active viewer to its home view.
    ResetView,
    /// One press of the quit shortcut (quitting needs two, FR-031).
    Quit,
    /// Focus the next window in most-recently-used order.
    NextWindow,
    /// Focus the previous window in most-recently-used order.
    PreviousWindow,
    /// Ask the focused window's application to close it.
    CloseWindow,
    /// Run the command of spawn binding `n` (FR-014).
    Spawn(usize),
}

/// The modifier used as Mod (FR-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKey {
    Super,
    Alt,
    Ctrl,
}

impl ModKey {
    /// Mod alone, as effective modifiers.
    pub fn modifiers(self) -> Modifiers {
        match self {
            ModKey::Super => Modifiers {
                logo: true,
                ..Modifiers::NONE
            },
            ModKey::Alt => Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            ModKey::Ctrl => Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        }
    }

    /// Whether Mod is held in `mods`.
    pub fn held(self, mods: Modifiers) -> bool {
        match self {
            ModKey::Super => mods.logo,
            ModKey::Alt => mods.alt,
            ModKey::Ctrl => mods.ctrl,
        }
    }
}

/// A key combination: exact modifiers plus the key's binding symbol (level 0 of the first
/// layout, as `InputEvent::Key::binding_sym`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub mods: Modifiers,
    pub sym: Keysym,
}

/// A spawn binding: a combination and the command it runs (FR-014).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnBind {
    pub combo: KeyCombo,
    /// The program and its arguments, run without a shell; never empty.
    pub command: Vec<String>,
}

/// One built-in shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub combo: KeyCombo,
    pub action: Action,
}

/// What a pointer binding does while its button is held (FR-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerAction {
    /// On a window: focus, raise and move it; over the background: pan.
    MoveWindow,
    /// On a window: focus, raise and resize it from the nearest edge or corner.
    ResizeWindow,
    /// Pan the active viewer, without changing focus.
    PanCanvas,
}

/// A pointer combination: exact modifiers plus a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerCombo {
    pub mods: Modifiers,
    pub button: Button,
}

/// A pointer binding: a combination and its gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerBind {
    pub combo: PointerCombo,
    pub action: PointerAction,
}

/// The shortcuts in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    mod_key: ModKey,
    builtin: Vec<Binding>,
    spawns: Vec<SpawnBind>,
    pointer: Vec<PointerBind>,
}

impl Default for Bindings {
    /// Mod = Super and no spawn bindings; RubickWay builds its bindings from the
    /// configuration (FR-005, FR-014).
    fn default() -> Self {
        Bindings::new(ModKey::Super, Vec::new())
    }
}

impl Bindings {
    /// The built-in shortcuts and the default pointer bindings with `mod_key` as Mod, plus the
    /// spawn bindings.
    pub fn new(mod_key: ModKey, spawns: Vec<SpawnBind>) -> Bindings {
        Bindings {
            mod_key,
            builtin: builtin_table(mod_key),
            spawns,
            pointer: default_pointer_table(mod_key),
        }
    }

    /// These bindings with `pointer` instead of the default pointer bindings.
    #[must_use]
    pub fn with_pointer(mut self, pointer: Vec<PointerBind>) -> Bindings {
        self.pointer = pointer;
        self
    }

    /// The pointer bindings in effect.
    pub fn pointer(&self) -> &[PointerBind] {
        &self.pointer
    }

    /// The gesture for a button press with `mods`, if any: exactly the binding's modifiers.
    pub fn pointer_lookup(&self, mods: Modifiers, button: Button) -> Option<PointerAction> {
        let combo = PointerCombo { mods, button };
        self.pointer
            .iter()
            .find(|b| b.combo == combo)
            .map(|b| b.action)
    }

    /// The built-in shortcuts with Mod resolved (for rejecting a configured combination that
    /// would shadow one, FR-025).
    pub fn builtin(&self) -> &[Binding] {
        &self.builtin
    }

    /// The modifier used as Mod.
    pub fn mod_key(&self) -> ModKey {
        self.mod_key
    }

    /// The spawn bindings; `Action::Spawn(n)` refers to the `n`th.
    pub fn spawns(&self) -> &[SpawnBind] {
        &self.spawns
    }

    /// Whether Mod is held (for Mod + drag and Mod + scroll).
    pub fn mod_held(&self, mods: Modifiers) -> bool {
        self.mod_key.held(mods)
    }

    /// The action for a key event, if any. Only presses with exactly the shortcut's modifiers
    /// match; releases never do.
    pub fn lookup(&self, mods: Modifiers, sym: Keysym, pressed: bool) -> Option<Action> {
        if !pressed {
            return None;
        }
        let combo = KeyCombo { mods, sym };
        if let Some(b) = self.builtin.iter().find(|b| b.combo == combo) {
            return Some(b.action);
        }
        self.spawns
            .iter()
            .position(|s| s.combo == combo)
            .map(Action::Spawn)
    }
}

/// The default gestures for a given Mod (FR-032, owner 2026-11-03): Mod + left moves, Mod+Shift +
/// left pans, Mod + right resizes, with or without Shift.
fn default_pointer_table(mod_key: ModKey) -> Vec<PointerBind> {
    let m = mod_key.modifiers();
    let ms = Modifiers { shift: true, ..m };
    let b = |mods: Modifiers, button: Button, action: PointerAction| PointerBind {
        combo: PointerCombo { mods, button },
        action,
    };
    vec![
        b(m, Button::Left, PointerAction::MoveWindow),
        b(ms, Button::Left, PointerAction::PanCanvas),
        b(m, Button::Right, PointerAction::ResizeWindow),
        b(ms, Button::Right, PointerAction::ResizeWindow),
    ]
}

/// The built-in shortcuts for a given Mod.
fn builtin_table(mod_key: ModKey) -> Vec<Binding> {
    let m = mod_key.modifiers();
    let ms = Modifiers { shift: true, ..m };
    let b = |mods: Modifiers, sym: Keysym, action: Action| Binding {
        combo: KeyCombo { mods, sym },
        action,
    };
    vec![
        b(m, Keysym::_0, Action::ResetView),
        b(ms, Keysym::q, Action::Quit),
        b(m, Keysym::q, Action::CloseWindow),
        b(m, Keysym::Tab, Action::NextWindow),
        // The binding symbol is the level-0 symbol, so Shift+Tab still reports `Tab`.
        b(ms, Keysym::Tab, Action::PreviousWindow),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(m: Modifiers, shift: bool) -> Modifiers {
        Modifiers { shift, ..m }
    }

    #[test]
    fn the_builtin_table_uses_the_given_mod() {
        for key in [ModKey::Super, ModKey::Alt, ModKey::Ctrl] {
            let b = Bindings::new(key, Vec::new());
            let m = key.modifiers();
            assert_eq!(b.lookup(m, Keysym::_0, true), Some(Action::ResetView));
            assert_eq!(b.lookup(with(m, true), Keysym::q, true), Some(Action::Quit));
            assert_eq!(b.lookup(m, Keysym::q, true), Some(Action::CloseWindow));
            assert_eq!(b.lookup(m, Keysym::Tab, true), Some(Action::NextWindow));
            assert_eq!(
                b.lookup(with(m, true), Keysym::Tab, true),
                Some(Action::PreviousWindow)
            );
            assert!(b.mod_held(m));
            assert!(!b.mod_held(Modifiers::NONE));
        }
    }

    #[test]
    fn only_presses_with_exactly_the_modifiers_match() {
        let b = Bindings::new(ModKey::Super, Vec::new());
        let logo = ModKey::Super.modifiers();
        assert_eq!(b.lookup(logo, Keysym::q, false), None);
        assert_eq!(b.lookup(Modifiers::NONE, Keysym::q, true), None);
        // Alt is not Mod when Mod is Super.
        assert_eq!(b.lookup(ModKey::Alt.modifiers(), Keysym::q, true), None);
        let logo_ctrl = Modifiers { ctrl: true, ..logo };
        assert_eq!(b.lookup(logo_ctrl, Keysym::q, true), None);
    }

    #[test]
    fn each_spawn_binding_gives_its_own_index() {
        let logo = ModKey::Super.modifiers();
        let bind = |sym, program: &str| SpawnBind {
            combo: KeyCombo { mods: logo, sym },
            command: vec![program.to_string()],
        };
        let spawns = vec![bind(Keysym::Return, "kitty"), bind(Keysym::b, "firefox")];
        let b = Bindings::new(ModKey::Super, spawns.clone());
        assert_eq!(b.lookup(logo, Keysym::Return, true), Some(Action::Spawn(0)));
        assert_eq!(b.lookup(logo, Keysym::b, true), Some(Action::Spawn(1)));
        assert_eq!(b.lookup(Modifiers::NONE, Keysym::Return, true), None);
        assert_eq!(b.spawns(), &spawns[..]);
    }

    #[test]
    fn the_default_is_the_super_table_without_spawns() {
        let b = Bindings::default();
        assert_eq!(b.mod_key(), ModKey::Super);
        assert!(b.spawns().is_empty());
        assert_eq!(b.builtin().len(), 5);
    }

    // --- Pointer bindings (FR-032, research S24) ---------------------------------------------

    #[test]
    fn the_default_pointer_table_follows_mod() {
        for key in [ModKey::Super, ModKey::Alt, ModKey::Ctrl] {
            let b = Bindings::new(key, Vec::new());
            let m = key.modifiers();
            let ms = with(m, true);
            assert_eq!(
                b.pointer_lookup(m, Button::Left),
                Some(PointerAction::MoveWindow)
            );
            assert_eq!(
                b.pointer_lookup(ms, Button::Left),
                Some(PointerAction::PanCanvas)
            );
            assert_eq!(
                b.pointer_lookup(m, Button::Right),
                Some(PointerAction::ResizeWindow)
            );
            assert_eq!(
                b.pointer_lookup(ms, Button::Right),
                Some(PointerAction::ResizeWindow)
            );
            assert_eq!(b.pointer_lookup(m, Button::Middle), None);
            assert_eq!(b.pointer_lookup(Modifiers::NONE, Button::Left), None);
        }
    }

    #[test]
    fn pointer_combinations_match_exactly() {
        let b = Bindings::default();
        let logo = ModKey::Super.modifiers();
        let logo_ctrl = Modifiers { ctrl: true, ..logo };
        assert_eq!(b.pointer_lookup(logo_ctrl, Button::Left), None);
        assert_eq!(
            b.pointer_lookup(ModKey::Alt.modifiers(), Button::Left),
            None
        );
    }

    #[test]
    fn a_configured_pointer_table_replaces_the_defaults() {
        let logo = ModKey::Super.modifiers();
        let bind = |mods, button, action| PointerBind {
            combo: PointerCombo { mods, button },
            action,
        };
        let table = vec![
            bind(logo, Button::Left, PointerAction::MoveWindow),
            bind(with(logo, true), Button::Left, PointerAction::PanCanvas),
            bind(logo, Button::Middle, PointerAction::ResizeWindow),
        ];
        let b = Bindings::default().with_pointer(table.clone());
        assert_eq!(b.pointer(), &table[..]);
        assert_eq!(
            b.pointer_lookup(logo, Button::Left),
            Some(PointerAction::MoveWindow)
        );
        assert_eq!(
            b.pointer_lookup(with(logo, true), Button::Left),
            Some(PointerAction::PanCanvas)
        );
        assert_eq!(
            b.pointer_lookup(logo, Button::Middle),
            Some(PointerAction::ResizeWindow)
        );
        assert_eq!(
            b.pointer_lookup(logo, Button::Right),
            None,
            "a default gone"
        );
        // The shortcuts stay.
        assert_eq!(b.lookup(logo, Keysym::q, true), Some(Action::CloseWindow));
    }
}
