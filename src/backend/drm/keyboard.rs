//! The keyboard state of the US English keymap, and the console-switch keys (research S11,
//! S17).

use crate::input::keyboard::Keyboard;
use crate::input::{InputEvent, Keysym};

/// What a key event means.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum KeyOutcome {
    /// Switch to text console `n` (Ctrl+Alt+F<n>); the press and its release are swallowed.
    SwitchConsole(i32),
    /// Events for the shell: the key, then the new modifiers if they changed.
    Events(Vec<InputEvent>),
}

/// The keyboard state RubickWay keeps itself when it reads the devices.
pub(crate) struct DrmKeyboard {
    keyboard: Keyboard,
    /// Keys whose press switched consoles; their release is swallowed too.
    swallowed: Vec<u32>,
}

impl DrmKeyboard {
    /// The US English keymap (FR-007); `None` only if it cannot be built.
    pub fn new() -> Option<DrmKeyboard> {
        Some(DrmKeyboard {
            keyboard: Keyboard::us()?,
            swallowed: Vec::new(),
        })
    }

    /// The keymap text for applications.
    pub fn keymap_text(&self) -> String {
        self.keyboard.keymap_text()
    }

    /// One key event from a device.
    pub fn key(&mut self, code: u32, pressed: bool, time_ms: u32) -> KeyOutcome {
        if pressed {
            // With Ctrl+Alt held the default rules turn F1..F12 into XF86Switch_VT_1..12;
            // checked before anything else and never configurable (FR-011).
            if let Some(vt) = console_switch(self.keyboard.modified_sym(code)) {
                self.keyboard.update_key(code, true);
                self.swallowed.push(code);
                return KeyOutcome::SwitchConsole(vt);
            }
        } else if let Some(i) = self.swallowed.iter().position(|c| *c == code) {
            self.swallowed.remove(i);
            self.keyboard.update_key(code, false);
            return KeyOutcome::Events(Vec::new());
        }
        let binding_sym = self.keyboard.binding_sym(code);
        let changed = self.keyboard.update_key(code, pressed);
        let mut events = vec![InputEvent::Key {
            keycode: code,
            binding_sym,
            pressed,
            time_ms,
        }];
        if changed {
            let (mods, raw) = self.keyboard.modifiers();
            events.push(InputEvent::Modifiers { mods, raw });
        }
        KeyOutcome::Events(events)
    }

    /// Forgets held keys (after a pause, when releases may never arrive).
    pub fn reset(&mut self) {
        self.swallowed.clear();
    }
}

/// The console number of an `XF86Switch_VT_n` symbol.
fn console_switch(sym: Keysym) -> Option<i32> {
    let first = Keysym::XF86_Switch_VT_1.raw();
    let raw = sym.raw();
    (first..first + 12)
        .contains(&raw)
        .then(|| (raw - first + 1) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_LEFTCTRL: u32 = 29;
    const KEY_LEFTALT: u32 = 56;
    const KEY_F1: u32 = 59;
    const KEY_F2: u32 = 60;
    const KEY_F12: u32 = 88;

    fn keyboard() -> DrmKeyboard {
        DrmKeyboard::new().expect("keymap")
    }

    fn press(k: &mut DrmKeyboard, code: u32) -> KeyOutcome {
        k.key(code, true, 0)
    }

    fn release(k: &mut DrmKeyboard, code: u32) -> KeyOutcome {
        k.key(code, false, 0)
    }

    #[test]
    fn ctrl_alt_fn_switches_consoles_and_is_swallowed() {
        let mut k = keyboard();
        press(&mut k, KEY_LEFTCTRL);
        press(&mut k, KEY_LEFTALT);
        assert_eq!(press(&mut k, KEY_F2), KeyOutcome::SwitchConsole(2));
        assert_eq!(release(&mut k, KEY_F2), KeyOutcome::Events(Vec::new()));
        assert_eq!(press(&mut k, KEY_F12), KeyOutcome::SwitchConsole(12));
    }

    #[test]
    fn f1_alone_or_with_ctrl_is_an_ordinary_key() {
        let mut k = keyboard();
        assert!(matches!(press(&mut k, KEY_F1), KeyOutcome::Events(e) if !e.is_empty()));
        release(&mut k, KEY_F1);
        press(&mut k, KEY_LEFTCTRL);
        assert!(matches!(press(&mut k, KEY_F1), KeyOutcome::Events(_)));
    }

    #[test]
    fn modifier_changes_follow_the_key() {
        let mut k = keyboard();
        let KeyOutcome::Events(events) = press(&mut k, KEY_LEFTCTRL) else {
            panic!("no switch");
        };
        assert!(matches!(
            events[0],
            InputEvent::Key {
                keycode: KEY_LEFTCTRL,
                pressed: true,
                ..
            }
        ));
        assert!(matches!(events[1], InputEvent::Modifiers { mods, .. } if mods.ctrl));
    }

    #[test]
    fn only_the_twelve_switch_symbols_count() {
        assert_eq!(console_switch(Keysym::XF86_Switch_VT_1), Some(1));
        assert_eq!(console_switch(Keysym::XF86_Switch_VT_12), Some(12));
        assert_eq!(console_switch(Keysym::F1), None);
    }
}
