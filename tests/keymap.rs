//! Keyboard translation with real xkb keymaps, compiled from RMLVO names.
#![expect(
    clippy::expect_used,
    reason = "test setup helpers fail loudly; clippy.toml only exempts #[test] functions"
)]

use rubickway::input::bindings::{self, Action};
use rubickway::input::keyboard::Keyboard;
use rubickway::input::{Keysym, Modifiers};
use xkbcommon::xkb;

/// Linux evdev key codes.
const KEY_Q: u32 = 16;
const KEY_0: u32 = 11;
const KEY_TAB: u32 = 15;

struct Setup {
    keyboard: Keyboard,
    keymap: xkb::Keymap,
}

impl Setup {
    fn new(layouts: &str) -> Setup {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&context, "", "", layouts, "", None, xkb::COMPILE_NO_FLAGS)
                .expect("compile keymap");
        let text = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let keyboard = Keyboard::from_keymap_string(text).expect("load keymap");
        Setup { keyboard, keymap }
    }

    fn mask(&self, names: &[&str]) -> u32 {
        names
            .iter()
            .map(|n| 1u32 << self.keymap.mod_get_index(*n))
            .sum()
    }
}

#[test]
fn alt_shift_q_gives_q_and_both_modifiers() {
    let mut s = Setup::new("us");
    let depressed = s.mask(&[xkb::MOD_NAME_ALT, xkb::MOD_NAME_SHIFT]);
    let mods = s.keyboard.update_mask(depressed, 0, 0, 0);
    assert_eq!(
        mods,
        Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        }
    );
    assert_eq!(s.keyboard.binding_sym(KEY_Q), Keysym::q);
}

#[test]
fn key_codes_beyond_the_keymap_give_no_symbol() {
    // Key codes come from the host; adding the xkb offset must not overflow.
    let s = Setup::new("us");
    assert_eq!(s.keyboard.binding_sym(u32::MAX), Keysym::NoSymbol);
    assert_eq!(s.keyboard.binding_sym(u32::MAX - 7), Keysym::NoSymbol);
}

#[test]
fn first_layout_is_used_when_the_second_is_active() {
    let mut s = Setup::new("us,ua");
    let depressed = s.mask(&[xkb::MOD_NAME_ALT, xkb::MOD_NAME_SHIFT]);
    // Group 1 (Ukrainian) locked as the active layout.
    let mods = s.keyboard.update_mask(depressed, 0, 0, 1);
    assert_eq!(
        mods,
        Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        }
    );
    assert_eq!(s.keyboard.binding_sym(KEY_Q), Keysym::q);
    assert_eq!(s.keyboard.binding_sym(KEY_0), Keysym::_0);
}

#[test]
fn locked_modifiers_are_not_reported() {
    let mut s = Setup::new("us");
    let locked = s.mask(&[xkb::MOD_NAME_CAPS, xkb::MOD_NAME_NUM]);
    assert_eq!(s.keyboard.update_mask(0, 0, locked, 0), Modifiers::NONE);
}

#[test]
fn latched_modifiers_count() {
    let mut s = Setup::new("us");
    let latched = s.mask(&[xkb::MOD_NAME_SHIFT]);
    assert_eq!(
        s.keyboard.update_mask(0, latched, 0, 0),
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        }
    );
}

#[test]
fn mod_shift_q_is_quit() {
    let alt_shift = Modifiers {
        alt: true,
        shift: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        bindings::lookup(alt_shift, Keysym::q, true),
        Some(Action::Quit)
    );
    assert_eq!(bindings::lookup(alt_shift, Keysym::q, false), None);
    let with_ctrl = Modifiers {
        ctrl: true,
        ..alt_shift
    };
    assert_eq!(bindings::lookup(with_ctrl, Keysym::q, true), None);
    assert_eq!(bindings::lookup(Modifiers::NONE, Keysym::q, true), None);
}

#[test]
fn mod_q_closes_and_mod_shift_q_quits() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        bindings::lookup(alt, Keysym::q, true),
        Some(Action::CloseWindow)
    );
    let alt_shift = Modifiers { shift: true, ..alt };
    assert_eq!(
        bindings::lookup(alt_shift, Keysym::q, true),
        Some(Action::Quit)
    );
}

#[test]
fn mod_tab_and_mod_shift_tab_cycle_windows() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let alt_shift = Modifiers { shift: true, ..alt };
    assert_eq!(
        bindings::lookup(alt, Keysym::Tab, true),
        Some(Action::NextWindow)
    );
    assert_eq!(
        bindings::lookup(alt_shift, Keysym::Tab, true),
        Some(Action::PreviousWindow)
    );
    assert_eq!(bindings::lookup(Modifiers::NONE, Keysym::Tab, true), None);
    assert_eq!(bindings::lookup(alt, Keysym::Tab, false), None);
}

#[test]
fn shift_tab_reports_tab_as_its_binding_symbol() {
    // Shift+Tab produces ISO_Left_Tab at level 1; bindings use the level-0 symbol.
    let mut s = Setup::new("us");
    let shift = s.mask(&[xkb::MOD_NAME_SHIFT]);
    s.keyboard.update_mask(shift, 0, 0, 0);
    assert_eq!(s.keyboard.binding_sym(KEY_TAB), Keysym::Tab);
}

#[test]
fn mod_0_is_reset_view_with_any_active_layout() {
    for (layouts, group) in [("us", 0), ("us,ua", 1)] {
        let mut s = Setup::new(layouts);
        let mods = s
            .keyboard
            .update_mask(s.mask(&[xkb::MOD_NAME_ALT]), 0, 0, group);
        let sym = s.keyboard.binding_sym(KEY_0);
        assert_eq!(
            bindings::lookup(mods, sym, true),
            Some(Action::ResetView),
            "{layouts}"
        );
    }
}
