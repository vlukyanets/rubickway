//! Fixed compositor shortcuts (no configuration yet).

use super::{Keysym, Modifiers};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Return the view to the canvas origin at 100% zoom.
    ResetView,
    /// Exit the compositor.
    Quit,
}

/// One shortcut: exact modifiers plus the key's binding symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub mods: Modifiers,
    pub sym: Keysym,
    pub action: Action,
}

/// The compositor modifier ("Mod"): Alt while running nested.
const MOD: Modifiers = Modifiers {
    alt: true,
    ..Modifiers::NONE
};

const MOD_SHIFT: Modifiers = Modifiers { shift: true, ..MOD };

/// The shortcut table.
pub const BINDINGS: &[Binding] = &[
    Binding {
        mods: MOD,
        sym: Keysym::_0,
        action: Action::ResetView,
    },
    Binding {
        mods: MOD_SHIFT,
        sym: Keysym::q,
        action: Action::Quit,
    },
];

/// Whether the compositor modifier is held (for Mod + drag and Mod + scroll).
pub fn mod_held(mods: Modifiers) -> bool {
    mods.alt
}

/// The action for a key event, if any. Only presses with exactly the binding's modifiers
/// match; releases never do.
pub fn lookup(mods: Modifiers, sym: Keysym, pressed: bool) -> Option<Action> {
    if !pressed {
        return None;
    }
    BINDINGS
        .iter()
        .find(|b| b.mods == mods && b.sym == sym)
        .map(|b| b.action)
}
