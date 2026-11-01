//! Keyboard state with xkbcommon: modifiers and the keysyms that shortcuts match on.
//!
//! Shortcuts match the key's symbol in the first layout at the base level, so they stay on the
//! same physical keys whatever layout is active (with `us,ua` and Ukrainian active, Q still
//! gives `q`, not a Cyrillic letter).

use xkbcommon::xkb;

use super::{Keysym, Modifiers};

/// Linux evdev key codes are offset by 8 in xkb.
const EVDEV_OFFSET: u32 = 8;

/// A compiled keymap and its state.
pub struct Keyboard {
    keymap: xkb::Keymap,
    state: xkb::State,
}

impl Keyboard {
    /// Compiles a text (v1) keymap, as sent by `wl_keyboard.keymap`. `None` if it does not
    /// compile.
    pub fn from_keymap_string(keymap: String) -> Option<Keyboard> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_string(
            &context,
            keymap,
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::COMPILE_NO_FLAGS,
        )?;
        let state = xkb::State::new(&keymap);
        Some(Keyboard { keymap, state })
    }

    /// Applies a modifier update (as in `wl_keyboard.modifiers`; `group` is the locked layout)
    /// and returns the effective modifiers without locked ones such as Caps Lock.
    pub fn update_mask(
        &mut self,
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    ) -> Modifiers {
        self.state
            .update_mask(depressed, latched, locked, 0, 0, group);
        let active = |name: &str| {
            self.state
                .mod_name_is_active(name, xkb::STATE_MODS_DEPRESSED | xkb::STATE_MODS_LATCHED)
        };
        Modifiers {
            ctrl: active(xkb::MOD_NAME_CTRL),
            alt: active(xkb::MOD_NAME_ALT),
            shift: active(xkb::MOD_NAME_SHIFT),
            logo: active(xkb::MOD_NAME_LOGO),
        }
    }

    /// The symbol of evdev key `code` in layout 0 at level 0 (`NoSymbol` if it has none). The
    /// code comes from the host, so one too large for an xkb key code has no symbol.
    pub fn binding_sym(&self, code: u32) -> Keysym {
        let Some(code) = code.checked_add(EVDEV_OFFSET) else {
            return Keysym::NoSymbol;
        };
        let key = xkb::Keycode::new(code);
        self.keymap
            .key_get_syms_by_level(key, 0, 0)
            .first()
            .copied()
            .unwrap_or(Keysym::NoSymbol)
    }
}
