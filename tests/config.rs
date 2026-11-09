//! `config.kdl` parsing: defaults, every setting, every rejected case (contracts/config.md).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rubickway::config::{ComboSpec, Config, ConfigWarning, config_path};
use rubickway::input::bindings::{ModKey, PointerAction};
use rubickway::input::{Button, Keysym, Modifiers};

fn parse(text: &str) -> (Config, Vec<ConfigWarning>) {
    Config::parse(text)
}

fn lines(warnings: &[ConfigWarning]) -> Vec<Option<usize>> {
    warnings.iter().map(|w| w.line).collect()
}

// --- User Story 1: framework and mod-key (T014) ----------------------------------------------

#[test]
fn an_empty_file_gives_the_defaults() {
    let (config, warnings) = parse("");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config, Config::default());
    assert_eq!(config.mod_key, ModKey::Super);
    assert_eq!(
        config.spawns[0].combo,
        ComboSpec {
            mod_: true,
            super_: false,
            alt: false,
            ctrl: false,
            shift: false,
            sym: Keysym::Return,
        }
    );
    assert_eq!(config.spawns[0].command, vec!["kitty".to_string()]);
    assert_eq!(config.watchdog_timeout, Duration::from_secs(10));
}

#[test]
fn mod_key_is_read_case_insensitively() {
    let (config, warnings) = parse("input {\n    mod-key \"Alt\"\n}\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.mod_key, ModKey::Alt);
    let (config, _) = parse("input { mod-key \"ctrl\"; }");
    assert_eq!(config.mod_key, ModKey::Ctrl);
    let (config, _) = parse("input { mod-key \"SUPER\"; }");
    assert_eq!(config.mod_key, ModKey::Super);
}

#[test]
fn an_unknown_mod_key_keeps_super_with_a_warning_on_its_line() {
    let (config, warnings) = parse("input { mod-key \"Hyper\"; }");
    assert_eq!(config.mod_key, ModKey::Super);
    assert_eq!(lines(&warnings), vec![Some(1)]);
    let (_, warnings) = parse("\n\ninput {\n    mod-key 3\n}");
    assert_eq!(lines(&warnings), vec![Some(4)]);
}

#[test]
fn a_syntax_error_gives_all_defaults_and_one_warning_naming_its_line() {
    let (config, warnings) = parse("input {\n    mod-key \"Alt\"\n\n    broken \"\n");
    assert_eq!(config, Config::default());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].line.is_some(), "{warnings:?}");
}

#[test]
fn unknown_nodes_are_ignored_with_a_warning() {
    let (config, warnings) = parse("colour \"red\"\ninput { mod-key \"Alt\"; volume 3; }");
    assert_eq!(config.mod_key, ModKey::Alt);
    assert_eq!(lines(&warnings), vec![Some(1), Some(2)]);
}

#[test]
fn a_node_given_twice_keeps_the_last_with_a_warning() {
    let (config, warnings) = parse("input { mod-key \"Alt\"; }\ninput { mod-key \"Ctrl\"; }");
    assert_eq!(config.mod_key, ModKey::Ctrl);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let (config, warnings) = parse("input {\n    mod-key \"Alt\"\n    mod-key \"Ctrl\"\n}");
    assert_eq!(config.mod_key, ModKey::Ctrl);
    assert_eq!(lines(&warnings), vec![Some(3)]);
}

#[test]
fn a_block_given_twice_counts_only_the_last_time() {
    let (config, warnings) =
        parse("binds { Mod+B { spawn \"firefox\"; } }\nbinds { Mod+E { spawn \"kitty\"; } }");
    let commands: Vec<&[String]> = config.spawns.iter().map(|s| &s.command[..]).collect();
    assert_eq!(commands, vec![&["kitty".to_string()][..]]);
    assert_eq!(lines(&warnings), vec![Some(2)]);
    let (config, warnings) = parse("input { mod-key \"Alt\"; }\ninput { }");
    assert_eq!(
        config.mod_key,
        ModKey::Super,
        "the first block is not used at all"
    );
    assert_eq!(lines(&warnings), vec![Some(2)]);
}

#[test]
fn warning_lines_are_right_after_non_ascii_text() {
    let (_, warnings) = parse(
        "// Настройки RubickWay: клавиши, мышь и всё остальное по-русски\nbinds {\n    Mod+Q { spawn \"kitty\"; }\n}\n",
    );
    assert_eq!(lines(&warnings), vec![Some(3)]);
}

#[test]
fn an_empty_program_or_two_spawns_are_rejected() {
    let (config, warnings) = parse("binds { Mod+B { spawn \"\"; } }");
    assert_eq!(config.spawns, Config::default().spawns);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let (config, warnings) =
        parse("binds {\n    Mod+B {\n        spawn \"firefox\"\n        spawn \"kitty\"\n    }\n}");
    assert_eq!(config.spawns, Config::default().spawns);
    assert_eq!(lines(&warnings), vec![Some(4)]);
}

#[test]
fn warnings_render_with_file_and_line() {
    let w = ConfigWarning {
        line: Some(4),
        message: "unknown mod-key \"Hyper\"".into(),
    };
    assert_eq!(
        w.render(Path::new("/c/config.kdl")),
        "/c/config.kdl:4: unknown mod-key \"Hyper\", using the default"
    );
    let w = ConfigWarning {
        line: None,
        message: "cannot read it".into(),
    };
    assert_eq!(
        w.render(Path::new("/c/config.kdl")),
        "/c/config.kdl: cannot read it, using the default"
    );
}

#[test]
fn a_missing_file_gives_defaults_silently() {
    let (config, warnings) = Config::load(Path::new("/nonexistent/rubickway/config.kdl"));
    assert_eq!(config, Config::default());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn an_unreadable_file_gives_defaults_and_one_warning_without_a_line() {
    // A directory cannot be read as a file.
    let dir = std::env::temp_dir();
    let (config, warnings) = Config::load(&dir);
    assert_eq!(config, Config::default());
    assert_eq!(lines(&warnings), vec![None]);
}

#[test]
fn the_config_path_follows_xdg_config_home() {
    let home = Some(OsStr::new("/home/u"));
    assert_eq!(
        config_path(Some(OsStr::new("/x/conf")), home),
        Some(PathBuf::from("/x/conf/rubickway/config.kdl"))
    );
    // Unset, empty or relative: ~/.config (XDG base directory specification).
    for xdg in [
        None,
        Some(OsStr::new("")),
        Some(OsStr::new("relative/conf")),
    ] {
        assert_eq!(
            config_path(xdg, home),
            Some(PathBuf::from("/home/u/.config/rubickway/config.kdl")),
            "{xdg:?}"
        );
    }
    assert_eq!(config_path(None, None), None);
}

// --- User Story 2: binds, xkb, key combinations (T037) ---------------------------------------

fn combo(mod_: bool, shift: bool, ctrl: bool, sym: Keysym) -> ComboSpec {
    ComboSpec {
        mod_,
        super_: false,
        alt: false,
        ctrl,
        shift,
        sym,
    }
}

#[test]
fn a_spawn_binding_with_arguments() {
    let (config, warnings) =
        parse("binds {\n    Mod+T { spawn \"kitty\" \"--single-instance\"; }\n}");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.spawns[0].combo, combo(true, false, false, Keysym::t));
    assert_eq!(config.spawns[0].command, vec!["kitty", "--single-instance"]);
}

#[test]
fn letters_match_either_case_and_env_wraps_a_command() {
    let (upper, _) = parse("binds { Mod+T { spawn \"foot\"; } }");
    let (lower, _) = parse("binds { Mod+t { spawn \"foot\"; } }");
    assert_eq!(upper.spawns[0].combo, lower.spawns[0].combo);
    let (config, warnings) =
        parse("binds { Mod+Return { spawn \"env\" \"LIBGL_ALWAYS_SOFTWARE=1\" \"kitty\"; } }");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        config.spawns[0].command,
        vec!["env", "LIBGL_ALWAYS_SOFTWARE=1", "kitty"]
    );
}

#[test]
fn modifiers_are_case_insensitive_and_no_modifier_is_allowed() {
    let (config, warnings) = parse("binds { MOD+shift+Return { spawn \"foot\"; } }");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        config.spawns[0].combo,
        combo(true, true, false, Keysym::Return)
    );
    let (config, warnings) = parse("binds { F12 { spawn \"foot\"; } }");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        config.spawns[0].combo,
        combo(false, false, false, Keysym::F12)
    );
    let (config, warnings) = parse("binds { Mod+Ctrl+Q { spawn \"foot\"; } }");
    assert!(
        warnings.is_empty(),
        "exact matching: not Mod+Q {warnings:?}"
    );
    assert_eq!(config.spawns[0].combo, combo(true, false, true, Keysym::q));
}

#[test]
fn builtin_and_console_switch_combinations_are_rejected() {
    for taken in [
        "Mod+Q",
        "Mod+Shift+Q",
        "Mod+0",
        "Mod+Tab",
        "Mod+Shift+Tab",
        "Ctrl+Alt+F3",
    ] {
        let (config, warnings) = parse(&format!("binds {{ {taken} {{ spawn \"foot\"; }} }}"));
        assert_eq!(config.spawns, Config::default().spawns, "{taken}");
        assert_eq!(warnings.len(), 1, "{taken}: {warnings:?}");
    }
    // With Mod = Alt, Mod+Ctrl+F1 is Ctrl+Alt+F1.
    let (config, warnings) =
        parse("input { mod-key \"Alt\"; }\nbinds { Mod+Ctrl+F1 { spawn \"foot\"; } }");
    assert_eq!(config.spawns, Config::default().spawns);
    assert_eq!(lines(&warnings), vec![Some(2)]);
}

#[test]
fn malformed_combinations_are_rejected() {
    for bad in ["Hyper+T", "Mod+NoSuchKey", "Mod+", "Mod+T+U", "\"\""] {
        let (config, warnings) = parse(&format!("binds {{ {bad} {{ spawn \"foot\"; }} }}"));
        assert_eq!(config.spawns, Config::default().spawns, "{bad}");
        assert_eq!(warnings.len(), 1, "{bad}: {warnings:?}");
    }
}

#[test]
fn bad_spawns_are_rejected() {
    for bad in [
        "Mod+T { }",
        "Mod+T { spawn; }",
        "Mod+T { spawn 3; }",
        "Mod+T { exec \"foot\"; }",
        "Mod+T",
    ] {
        let (config, warnings) = parse(&format!("binds {{ {bad} }}"));
        assert_eq!(config.spawns, Config::default().spawns, "{bad}");
        assert!(!warnings.is_empty(), "{bad}");
    }
}

#[test]
fn every_valid_binding_is_kept_in_order() {
    let (config, warnings) = parse(
        "binds {\n    Mod+Q { spawn \"bad\"; }\n    Mod+T { spawn \"foot\"; }\n    Mod+B { spawn \"firefox\"; }\n}",
    );
    let commands: Vec<&[String]> = config.spawns.iter().map(|s| &s.command[..]).collect();
    assert_eq!(
        commands,
        [&["foot".to_string()][..], &["firefox".to_string()][..]]
    );
    // Only the rejected Mod+Q.
    assert_eq!(lines(&warnings), vec![Some(2)]);
}

#[test]
fn a_combination_bound_twice_keeps_the_first() {
    let (config, warnings) =
        parse("binds {\n    Mod+T { spawn \"foot\"; }\n    Mod+t { spawn \"kitty\"; }\n}");
    assert_eq!(config.spawns.len(), 1);
    assert_eq!(config.spawns[0].command, vec!["foot"]);
    assert_eq!(lines(&warnings), vec![Some(3)]);
}

#[test]
fn configured_bindings_replace_the_default() {
    let (config, _) = parse("binds { Mod+B { spawn \"firefox\"; } }");
    let bindings = config.bindings();
    let logo = ModKey::Super.modifiers();
    assert_eq!(
        bindings.lookup(logo, Keysym::Return, true),
        None,
        "Mod+Return is free"
    );
    assert_eq!(
        bindings.lookup(logo, Keysym::b, true),
        Some(rubickway::input::bindings::Action::Spawn(0))
    );
}

#[test]
fn empty_binds_keep_the_default() {
    let (config, warnings) = parse("binds { }");
    assert_eq!(config.spawns, Config::default().spawns);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn the_default_binding_resolves_with_the_configured_mod() {
    let (config, _) = parse("input { mod-key \"Ctrl\"; }");
    let bindings = config.bindings();
    let mods = ModKey::Ctrl.modifiers();
    assert_eq!(
        bindings.lookup(mods, Keysym::Return, true),
        Some(rubickway::input::bindings::Action::Spawn(0))
    );
}

// --- gpu-buffers: distant windows (T047, FR-017) ------------------------------------------

#[test]
fn with_no_block_nothing_is_set_for_distant_windows() {
    assert_eq!(
        parse("").0.distant_windows,
        rubickway::distant::DistantWindows::default()
    );
}

#[test]
fn every_value_for_distant_windows_is_read() {
    let (config, warnings) = parse(
        "distant-windows {\n\
         below-zoom-percent 50\n\
         max-fps 15\n\
         copy-detail-percent 200\n\
         }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let d = config.distant_windows;
    assert_eq!(d.below_zoom_percent, Some(50));
    assert_eq!(d.max_fps, Some(15));
    assert_eq!(d.copy_detail_percent, Some(200));
}

#[test]
fn values_for_distant_windows_work_alone() {
    let (config, warnings) = parse("distant-windows {\nbelow-zoom-percent 25\nmax-fps 10\n}");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.distant_windows.copy_detail_percent, None);
}

#[test]
fn the_ends_of_each_range_for_distant_windows_are_allowed() {
    for text in [
        "below-zoom-percent 1\nmax-fps 1\ncopy-detail-percent 25",
        "below-zoom-percent 100\nmax-fps 240\ncopy-detail-percent 400",
    ] {
        let (_, warnings) = parse(&format!("distant-windows {{\n{text}\n}}"));
        assert!(warnings.is_empty(), "{text}: {warnings:?}");
    }
}

#[test]
fn a_wrong_value_for_distant_windows_is_a_warning_with_its_line() {
    for line in [
        "below-zoom-percent 0",
        "below-zoom-percent 101",
        "below-zoom-percent \"half\"",
        "max-fps 0",
        "max-fps 241",
        "copy-detail-percent 24",
        "copy-detail-percent 401",
        "copy-detail-percent 1.5",
        "copy-detail-percent 100 100",
    ] {
        let good = if line.starts_with("below") {
            "max-fps 10"
        } else {
            "below-zoom-percent 50"
        };
        let (config, warnings) = parse(&format!("distant-windows {{\n{good}\n{line}\n}}"));
        assert!(lines(&warnings).contains(&Some(3)), "{line}: {warnings:?}");
        let d = config.distant_windows;
        if line.starts_with("below") {
            assert_eq!(d.below_zoom_percent, None, "{line}");
        } else if line.starts_with("max") {
            assert_eq!(d.max_fps, None, "{line}");
        } else {
            assert_eq!(d.copy_detail_percent, None, "{line}");
        }
    }
}

#[test]
fn distant_values_without_the_zoom_are_a_warning() {
    let (_, warnings) = parse("distant-windows {\nmax-fps 10\n}");
    assert_eq!(lines(&warnings), vec![Some(1)], "{warnings:?}");
}

#[test]
fn an_unknown_setting_for_distant_windows_is_a_warning() {
    let (_, warnings) = parse("distant-windows {\nbelow-zoom-percent 50\nbrightness 3\n}");
    assert_eq!(lines(&warnings), vec![Some(3)], "{warnings:?}");
}

// --- User Story 5: the watchdog timeout (T058, FR-024, FR-030) -------------------------------

#[test]
fn the_watchdog_timeout_is_whole_seconds_from_1_to_600() {
    let (config, warnings) = parse("watchdog-timeout-sec 5");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.watchdog_timeout, Duration::from_secs(5));
    for ok in [1u64, 600] {
        let (config, warnings) = parse(&format!("watchdog-timeout-sec {ok}"));
        assert!(warnings.is_empty(), "{ok}: {warnings:?}");
        assert_eq!(config.watchdog_timeout.as_secs(), ok);
    }
    for bad in ["0", "601", "-1", "2.5", "\"10\"", ""] {
        let (config, warnings) = parse(&format!("watchdog-timeout-sec {bad}"));
        assert_eq!(config.watchdog_timeout, Duration::from_secs(10), "{bad}");
        assert_eq!(warnings.len(), 1, "{bad}: {warnings:?}");
    }
}

// --- Pointer bindings (T084, FR-032, contracts/config.md "Pointer combinations") -------------

const SUPER: Modifiers = Modifiers {
    logo: true,
    ..Modifiers::NONE
};
const SUPER_SHIFT: Modifiers = Modifiers {
    logo: true,
    shift: true,
    ..Modifiers::NONE
};

#[test]
fn without_pointer_bindings_the_defaults_apply() {
    let (config, _) = parse("");
    let b = config.bindings();
    assert_eq!(
        b.pointer_lookup(SUPER, Button::Left),
        Some(PointerAction::MoveWindow)
    );
    assert_eq!(
        b.pointer_lookup(SUPER_SHIFT, Button::Left),
        Some(PointerAction::PanCanvas)
    );
    assert_eq!(
        b.pointer_lookup(SUPER, Button::Right),
        Some(PointerAction::ResizeWindow)
    );
    assert_eq!(
        b.pointer_lookup(SUPER_SHIFT, Button::Right),
        Some(PointerAction::ResizeWindow)
    );
    // A spawn binding alone keeps them.
    let (config, warnings) = parse("binds { Mod+T { spawn \"kitty\"; } }");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        config.bindings().pointer_lookup(SUPER, Button::Left),
        Some(PointerAction::MoveWindow)
    );
}

#[test]
fn pointer_bindings_replace_the_pointer_defaults_and_keep_the_spawn_default() {
    // The opposite of the defaults for the left button, and a middle-button resize.
    let (config, warnings) = parse(
        "binds {\n\
         Mod+MouseLeft { pan-canvas; }\n\
         Mod+Shift+mouseleft { move-window; }\n\
         Mod+MouseMiddle { resize-window; }\n\
         }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let b = config.bindings();
    assert_eq!(
        b.pointer_lookup(SUPER, Button::Left),
        Some(PointerAction::PanCanvas)
    );
    assert_eq!(
        b.pointer_lookup(SUPER_SHIFT, Button::Left),
        Some(PointerAction::MoveWindow)
    );
    assert_eq!(
        b.pointer_lookup(SUPER, Button::Middle),
        Some(PointerAction::ResizeWindow)
    );
    assert_eq!(b.pointer_lookup(SUPER, Button::Right), None);
    assert_eq!(
        b.lookup(SUPER, Keysym::Return, true),
        Some(rubickway::input::bindings::Action::Spawn(0)),
        "Mod+Return stays"
    );
}

#[test]
fn pointer_bindings_resolve_mod_from_mod_key() {
    let (config, _) =
        parse("input { mod-key \"Alt\"; }\nbinds { Mod+MouseLeft { resize-window; } }");
    let alt = ModKey::Alt.modifiers();
    assert_eq!(
        config.bindings().pointer_lookup(alt, Button::Left),
        Some(PointerAction::ResizeWindow)
    );
}

#[test]
fn bad_pointer_bindings_are_rejected_one_warning_each() {
    for bad in [
        "MouseLeft { move-window; }",
        "Mod+MouseFoo { move-window; }",
        "Mod+MouseLeft { spawn \"kitty\"; }",
        "Mod+T { move-window; }",
        "Mod+MouseLeft { pan-canvas 1; }",
        "Mod+MouseLeft { pan-canvas { x; }; }",
        "Mod+MouseLeft { fly; }",
        "Mod+MouseLeft",
    ] {
        let (config, warnings) = parse(&format!("binds {{\n{bad}\n}}"));
        assert_eq!(lines(&warnings), vec![Some(2)], "{bad}: {warnings:?}");
        assert_eq!(
            config.bindings().pointer_lookup(SUPER, Button::Left),
            Some(PointerAction::MoveWindow),
            "{bad}: the defaults stay"
        );
    }
}

#[test]
fn a_pointer_combination_bound_twice_keeps_the_first() {
    let (config, warnings) = parse(
        "binds {\n\
         Mod+MouseLeft { resize-window; }\n\
         Mod+MouseLeft { pan-canvas; }\n\
         }",
    );
    assert_eq!(lines(&warnings), vec![Some(3)], "{warnings:?}");
    assert_eq!(
        config.bindings().pointer_lookup(SUPER, Button::Left),
        Some(PointerAction::ResizeWindow)
    );
}
