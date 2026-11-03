//! Keyboard state with xkbcommon: modifiers and the keysyms that shortcuts match on.
//!
//! Shortcuts match the key's symbol in the first layout at the base level, so they stay on the
//! same physical keys whatever layout is active (with `us,ua` and Ukrainian active, Q still
//! gives `q`, not a Cyrillic letter).

use xkbcommon::xkb;

use super::{Keysym, Modifiers, RawModifiers};

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

    /// The fixed US English keymap (FR-007, research S17): rules `evdev`, model `pc105`,
    /// layout `us`, no variant, no options; the `XKB_DEFAULT_*` environment variables are never
    /// consulted. `None` only if xkeyboard-config is missing.
    pub fn us() -> Option<Keyboard> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_ENVIRONMENT_NAMES);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "evdev",
            "pc105",
            "us",
            "",
            None,
            xkb::COMPILE_NO_FLAGS,
        )?;
        let state = xkb::State::new(&keymap);
        Some(Keyboard { keymap, state })
    }

    /// The keymap as text (v1), for `wl_keyboard.keymap`.
    pub fn keymap_text(&self) -> String {
        self.keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
    }

    /// How many layouts (groups) the keymap has.
    pub fn layout_count(&self) -> u32 {
        self.keymap.num_layouts()
    }

    /// Feeds a key press or release of evdev key `code` to the state (when RubickWay reads the
    /// devices itself). Returns whether the modifiers or the layout changed.
    pub fn update_key(&mut self, code: u32, pressed: bool) -> bool {
        let Some(code) = code.checked_add(EVDEV_OFFSET) else {
            return false;
        };
        let direction = if pressed {
            xkb::KeyDirection::Down
        } else {
            xkb::KeyDirection::Up
        };
        self.state.update_key(xkb::Keycode::new(code), direction) != 0
    }

    /// The symbol evdev key `code` produces with the current modifiers and layout (for keys
    /// such as `XF86Switch_VT_1`, which only exist with Ctrl+Alt held).
    pub fn modified_sym(&self, code: u32) -> Keysym {
        let Some(code) = code.checked_add(EVDEV_OFFSET) else {
            return Keysym::NoSymbol;
        };
        self.state.key_get_one_sym(xkb::Keycode::new(code))
    }

    /// The current effective modifiers, and the masks to forward to applications.
    pub fn modifiers(&self) -> (Modifiers, RawModifiers) {
        let raw = RawModifiers {
            depressed: self.state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            latched: self.state.serialize_mods(xkb::STATE_MODS_LATCHED),
            locked: self.state.serialize_mods(xkb::STATE_MODS_LOCKED),
            group: self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
        };
        (self.effective(), raw)
    }

    fn effective(&self) -> Modifiers {
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
        self.effective()
    }

    /// The symbol of evdev key `code` in layout 0 at level 0 (`NoSymbol` if it has none). The
    /// code comes from the device, so one too large for an xkb key code has no symbol.
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
