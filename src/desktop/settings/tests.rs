use super::*;
fn runtime(root: &std::path::Path) -> rune::Runtime<State> {
    let mut state = State::with_root(root.to_owned());
    state.workspace_setup();
    let mut runtime = rune::Runtime::new(
        state,
        view,
        (1200.0, 800.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    runtime.frame(0.0);
    runtime
}
fn open_settings(runtime: &mut rune::Runtime<State>) {
    runtime.update(|s, tx| s.dispatch(AppAction::Settings, tx));
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
}
fn click(runtime: &mut rune::Runtime<State>, key: &str) {
    let (x, y) = runtime
        .find(key)
        .unwrap_or_else(|| panic!("missing control {key}"));
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
}
fn assert_canvas(runtime: &rune::Runtime<State>, hex: &str) {
    let c = rune::theme::hex(hex).unwrap();
    let actual = runtime.clear_color();
    assert!(
        actual
            .iter()
            .zip([c.r, c.g, c.b, c.a])
            .all(|(a, b)| (a - b).abs() < 0.0001),
        "canvas {actual:?}, expected {hex}"
    );
}
#[test]
fn prompt_dropdowns_and_resets_fit_inside_a_narrow_settings_pane() {
    use crate::desktop::fonts::BUNDLED;
    let _families = rune::text::with_families("Geist", "Geist Mono");
    // The default 1080 pt workspace gives Settings about 450 pt; each
    // dropdown and its reset share a line there or wrap, never clipped.
    for (size, share) in [((1080.0, 720.0), 0.42), ((1200.0, 800.0), 0.24)] {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.workspace.layout.editor_share = share;
        state.open_settings(&mut Tx::default());
        state.settings_query.replace_text("prompt");
        let mut rt = rune::Runtime::with_text(
            state,
            view,
            size,
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        for _ in 0..3 {
            rt.frame(0.0);
            rt.settle_all();
        }
        let elements = rt.elements();
        let rect = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        let section = rect("settings-section-Appearance").rect;
        let right = |r: [f32; 4]| r[0] + r[2];
        for key in ["prompt-style-trigger", "prompt-symbol-trigger"] {
            let e = rect(key);
            assert!(
                right(e.rect) <= right(section) + 0.5,
                "{size:?} {key} stays inside its section: {:?} in {section:?}",
                e.rect
            );
            assert!(
                (e.visible[2] - e.rect[2]).abs() < 0.5,
                "{size:?} {key} is not clipped: {:?}",
                e.visible
            );
        }
    }
}
#[test]
fn terminal_line_height_and_letter_spacing_resize_the_cell_grid_and_reset() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("line height"));
    rt.frame(0.0);
    let cell = |rt: &rune::Runtime<State>| {
        let m = rt.state.terminal_metrics;
        (m.width, m.height)
    };
    let (width, height) = cell(&rt);
    for _ in 0..8 {
        click(&mut rt, "more-terminal-line-height");
    }
    let saved = Config::load(root.path()).unwrap().fonts;
    assert_eq!(saved.terminal_line_height, 2.0);
    let (_, taller) = cell(&rt);
    assert!(
        (taller - height * 2.0 / 1.2).abs() <= 1.0,
        "rows grow with line height: {height} -> {taller}"
    );
    rt.update(|s, _| s.settings_query.replace_text("letter spacing"));
    rt.frame(0.0);
    for _ in 0..4 {
        click(&mut rt, "more-terminal-letter-spacing");
    }
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .fonts
            .terminal_letter_spacing,
        2.0
    );
    let (wider, _) = cell(&rt);
    assert!(
        (wider - width - 2.0).abs() <= 0.5,
        "cells widen by the spacing: {width} -> {wider}"
    );
    assert!(rt.find("settings-group-terminal-text").is_some());
    restore_tab_defaults(&mut rt, "appearance");
    let saved = Config::load(root.path()).unwrap().fonts;
    assert_eq!(
        (saved.terminal_line_height, saved.terminal_letter_spacing),
        (1.2, 0.0)
    );
    // Restoring defaults also reloads the font families, which changes
    // this headless runtime's fallback advance; the row and spacing return.
    assert_eq!(cell(&rt).1, height);
    assert_eq!(rt.state.terminal_metrics.spacing, 0.0);
    // Values stop at their limits rather than producing an invalid file.
    rt.update(|s, _| s.settings_query.replace_text("letter spacing"));
    rt.frame(0.0);
    for _ in 0..30 {
        click(&mut rt, "less-terminal-letter-spacing");
    }
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .fonts
            .terminal_letter_spacing,
        -2.0
    );
    rt.update(|s, _| s.settings_query.replace_text("letter spacing"));
    rt.frame(0.0);
    assert!(rt.find("less-terminal-letter-spacing").is_some());
    assert!(rt.find("less-terminal-line-height").is_none());
    assert!(rt.find("less-terminal-font-size").is_none());
    for invalid in [
        r#"{"fonts":{"terminal_line_height":0.5}}"#,
        r#"{"fonts":{"terminal_letter_spacing":11}}"#,
    ] {
        std::fs::write(root.path().join("settings.json"), invalid).unwrap();
        assert!(Config::load(root.path()).is_err(), "{invalid} is rejected");
    }
}

#[test]
fn owner_hides_the_footer_from_appearance_and_restores_it_with_the_tab_defaults() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    assert!(rt.find("app-footer").is_some(), "shown by default");
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("footer"));
    rt.frame(0.0);
    click(&mut rt, "show-footer");
    assert!(rt.find("app-footer").is_none());
    assert!(!Config::load(root.path()).unwrap().show_footer);
    assert!(
        runtime(root.path()).find("app-footer").is_none(),
        "stays hidden on reopen"
    );
    restore_tab_defaults(&mut rt, "appearance");
    assert!(rt.find("app-footer").is_some());
    assert!(Config::load(root.path()).unwrap().show_footer);
}

#[test]
fn an_open_dropdown_list_is_at_least_as_wide_as_its_field() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    click(&mut rt, "window-close-trigger");
    let rect = |key: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .unwrap_or_else(|| panic!("missing {key}"))
            .rect
    };
    let field = rect("window-close-trigger");
    let list = rect("window-close-content");
    assert!(
        list[2] >= field[2] - 0.5,
        "list {list:?} narrower than field {field:?}"
    );
    assert!(
        (list[0] + list[2] - field[0] - field[2]).abs() < 0.5,
        "the list lines up with the field's edge"
    );
}

#[test]
fn tabs_show_one_group_at_a_time_and_search_spans_every_tab() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    assert!(
        rt.find("window-close-trigger").is_some(),
        "General opens first"
    );
    assert!(rt.find("appearance-trigger").is_none());
    assert!(
        rt.find("autosave-trigger").is_some(),
        "General holds the editor's behavior"
    );
    click(&mut rt, "settings-page-tab-appearance");
    assert!(rt.find("appearance-trigger").is_some());
    assert!(rt.find("window-close-trigger").is_none());
    rt.update(|s, _| s.settings_query.replace_text("font size"));
    rt.frame(0.0);
    for key in [
        "less-ui-font-size",
        "less-terminal-font-size",
        "less-editor-font-size",
    ] {
        assert!(rt.find(key).is_some(), "search finds {key} in its tab");
    }
    click(&mut rt, "settings-page-tab-general");
    assert!(
        rt.state.settings_query.text.is_empty(),
        "a tab clears the search"
    );
    assert!(rt.find("autosave-trigger").is_some());
    assert!(rt.find("less-ui-font-size").is_none());
}

#[test]
fn file_switches_flip_individually_and_general_restores_their_defaults() {
    let root = crate::test_support::tempdir().unwrap();
    let mut config = Config::default();
    config.explorer.confirm_deletion = false;
    config.explorer.follow_terminal = true;
    config.explorer.show_hidden = true;
    config.explorer.respect_ignore = false;
    config.save(root.path()).unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    // Each switch flips only its own preference.
    for (query, control, expected) in [
        (
            "Trash",
            "settings-confirm-deletion",
            [true, true, true, false],
        ),
        (
            "Follow Terminal",
            "settings-explorer-follow",
            [true, false, true, false],
        ),
        (
            "hidden files",
            "settings-explorer-hidden",
            [true, false, false, false],
        ),
        (
            "ignore rules",
            "settings-explorer-ignore",
            [true, false, false, true],
        ),
    ] {
        rt.update(|s, _| s.settings_query.replace_text(query));
        rt.frame(0.0);
        click(&mut rt, control);
        rt.update(|s, tx| {
            s.workspace_poll(tx);
        });
        let saved = Config::load(root.path()).unwrap().explorer;
        assert_eq!(
            [
                saved.confirm_deletion,
                saved.follow_terminal,
                saved.show_hidden,
                saved.respect_ignore
            ],
            expected,
            "{control}"
        );
        assert_eq!(rt.state.workspace.layout.follow, expected[1]);
        assert_eq!(rt.state.workspace.layout.hidden, expected[2]);
    }
    rt.update(|s, _| s.settings_query.replace_text("hidden files"));
    rt.frame(0.0);
    click(&mut rt, "settings-explorer-hidden");
    rt.update(|s, tx| {
        s.workspace_poll(tx);
    });
    assert!(rt.state.workspace.layout.hidden);
    // General's Restore Defaults brings every file preference back.
    restore_tab_defaults(&mut rt, "general");
    rt.update(|s, tx| {
        s.workspace_poll(tx);
    });
    assert_eq!(
        Config::load(root.path()).unwrap().explorer,
        crate::config::ExplorerPreferences::default()
    );
    assert!(!rt.state.workspace.layout.hidden);
}

#[test]
fn behavior_settings_save_and_reset_terminal_close_and_directory_choices() {
    use crate::config::{NewTerminalDirectory, TerminalClosePolicy};
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("terminal tab close"));
    rt.frame(0.0);
    choose(&mut rt, "terminal-tab-close", "Always confirm");
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .behavior
            .terminal_tab_close,
        TerminalClosePolicy::AlwaysConfirm
    );
    choose(&mut rt, "terminal-tab-close", "Close immediately");
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .behavior
            .terminal_tab_close,
        TerminalClosePolicy::CloseImmediately
    );
    rt.update(|s, _| s.settings_query.replace_text("new terminal directory"));
    rt.frame(0.0);
    choose(&mut rt, "new-terminal-directory", "Explorer folder");
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .behavior
            .new_terminal_directory,
        NewTerminalDirectory::Explorer
    );
    choose(&mut rt, "new-terminal-directory", "Specified folder");
    click(&mut rt, "specified-directory");
    rt.key(KeyEvent {
        key: Key::Text(root.path().display().to_string()),
        mods: Default::default(),
    });
    rt.frame(0.0);
    assert_eq!(
        Config::load(root.path())
            .unwrap()
            .behavior
            .specified_directory,
        root.path()
    );
    rt.update(|s, _| s.settings_query.replace_text("startup folder"));
    rt.frame(0.0);
    assert!(
        rt.find("startup-specified-directory").is_none(),
        "the startup folder field shows only for Specified folder"
    );
    choose(&mut rt, "startup-directory", "Specified folder");
    click(&mut rt, "startup-specified-directory");
    rt.key(KeyEvent {
        key: Key::Text(root.path().display().to_string()),
        mods: Default::default(),
    });
    rt.frame(0.0);
    let behavior = Config::load(root.path()).unwrap().behavior;
    assert_eq!(
        behavior.startup_directory,
        crate::config::StartupDirectory::Specified
    );
    assert_eq!(behavior.startup_specified_directory, root.path());
    // General's Restore Defaults resets the tab close and directory
    // choices and empties the specified folders.
    restore_tab_defaults(&mut rt, "general");
    let behavior = Config::load(root.path()).unwrap().behavior;
    assert_eq!(
        behavior.terminal_tab_close,
        TerminalClosePolicy::ConfirmRunning
    );
    assert_eq!(
        behavior.new_terminal_directory,
        NewTerminalDirectory::Active
    );
    assert!(behavior.specified_directory.as_os_str().is_empty());
    assert!(rt.state.behavior_directory.text.is_empty());
    assert_eq!(
        behavior.startup_directory,
        crate::config::StartupDirectory::Home
    );
    assert!(behavior.startup_specified_directory.as_os_str().is_empty());
    assert!(rt.state.startup_directory.text.is_empty());
    assert!(
        rt.find("specified-directory").is_none(),
        "the folder field shows only for Specified folder"
    );
    let mut external = Config::load(root.path()).unwrap();
    external.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
    external.behavior.specified_directory = root.path().into();
    external.behavior.terminal_tab_close = TerminalClosePolicy::AlwaysConfirm;
    external.save(root.path()).unwrap();
    rt.update(|s, _| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        s.poll_settings();
    });
    assert_eq!(
        rt.state.config.behavior.terminal_tab_close,
        TerminalClosePolicy::AlwaysConfirm
    );
}

#[test]
fn behavior_settings_search_apply_live_persist_and_reset_window_policy() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("window close"));
    rt.frame(0.0);
    choose(&mut rt, "window-close", "Ask each time");
    assert_eq!(
        Config::load(root.path()).unwrap().behavior.window_close,
        crate::config::WindowClosePolicy::AskEachTime
    );
    rt.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    rt.frame(0.0);
    rt.settle_all();
    rt.frame(0.0);
    assert!(rt.find("window-close-choice").is_some());
    click(&mut rt, "window-close-cancel");
    restore_tab_defaults(&mut rt, "general");
    assert_eq!(
        Config::load(root.path()).unwrap().behavior.window_close,
        crate::config::WindowClosePolicy::KeepSessionsRunning
    );
    rt.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    assert_eq!(rt.shell_request.take(), Some(rune::Shell::Close));
}

#[test]
fn owner_selects_dracula_family_and_keeps_both_appearances_on_reopen() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    runtime.update(|s, _| s.settings_query.replace_text("theme"));
    runtime.frame(0.0);
    click(&mut runtime, "theme-family-trigger");
    click(&mut runtime, "theme-family-option-Dracula");
    assert_canvas(&runtime, "#282a36");
    choose(&mut runtime, "appearance", "Light");
    assert_canvas(&runtime, "#fffbeb");
    let reopened = self::runtime(root.path());
    assert_canvas(&reopened, "#fffbeb");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["theme_family"], "dracula");
    assert_eq!(saved["appearance"], "light");
}

#[test]
fn all_families_apply_both_previews_live_system_json_and_restore_defaults() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("theme"));
    rt.frame(0.0);
    // Independently specified upstream canvas values, not derived from the resolver.
    let families = [
        (
            "Catppuccin",
            "catppuccin",
            "#eff1f5",
            "#1e1e2e",
            "Catppuccin Latte",
            "Catppuccin Mocha",
        ),
        (
            "Dracula",
            "dracula",
            "#fffbeb",
            "#282a36",
            "Dracula Alucard",
            "Dracula",
        ),
        (
            "Tokyo Night",
            "tokyo_night",
            "#e6e7ed",
            "#1a1b26",
            "Tokyo Night Light",
            "Tokyo Night Night",
        ),
        (
            "Nord",
            "nord",
            "#eceff4",
            "#2e3440",
            "Nord Light",
            "Nord Dark",
        ),
        (
            "Gruvbox",
            "gruvbox",
            "#fbf1c7",
            "#282828",
            "Gruvbox Light (medium)",
            "Gruvbox Dark (medium)",
        ),
        (
            "One Dark Pro",
            "one_dark_pro",
            "#fafafa",
            "#282c34",
            "One Dark Pro Light (Rodtty adaptation)",
            "One Dark Pro Dark",
        ),
        (
            "Night Owl",
            "night_owl",
            "#fbfbfb",
            "#011627",
            "Light Owl",
            "Night Owl",
        ),
        ("Ayu", "ayu", "#fafafa", "#0a0e14", "Ayu Light", "Ayu Dark"),
        (
            "Solarized",
            "solarized",
            "#fdf6e3",
            "#002b36",
            "Solarized Light",
            "Solarized Dark",
        ),
        (
            "Rosé Pine",
            "rose_pine",
            "#faf4ed",
            "#191724",
            "Rosé Pine Dawn",
            "Rosé Pine",
        ),
        (
            "Kanagawa",
            "kanagawa",
            "#f2ecbc",
            "#1f1f28",
            "Kanagawa Lotus",
            "Kanagawa",
        ),
        (
            "Everforest",
            "everforest",
            "#fdf0d5",
            "#2f383e",
            "Everforest Light",
            "Everforest",
        ),
    ];
    assert_eq!(crate::config::ThemeFamily::ALL.len(), 12);
    for (label, json_name, light, dark, light_name, dark_name) in families {
        click(&mut rt, "theme-family-trigger");
        rt.paste(label);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        click(&mut rt, &format!("theme-family-option-{label}"));
        for name in [light_name, dark_name] {
            assert!(
                rt.elements()
                    .iter()
                    .any(|e| e.text.as_deref() == Some(name)),
                "missing preview {name}"
            );
        }
        for (control, value, canvas) in [("Light", "light", light), ("Dark", "dark", dark)] {
            choose(&mut rt, "appearance", control);
            assert_canvas(&rt, canvas);
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                    .unwrap();
            assert_eq!(saved["theme_family"], json_name);
            assert_eq!(saved["appearance"], value);
            assert_canvas(&runtime(root.path()), canvas);
        }
        choose(&mut rt, "appearance", "System");
        for size in [(640.0, 360.0), (1440.0, 900.0)] {
            rt.set_viewport(size, 1.0);
            for (host_dark, canvas) in [(false, light), (true, dark), (false, light)] {
                rt.set_system_dark(host_dark);
                for _ in 0..45 {
                    rt.frame(1.0 / 60.0);
                }
                assert_canvas(&rt, canvas);
                assert!(rt.find("theme-family-trigger").is_some());
            }
        }
        rt.set_system_dark(true);
        // External JSON changes enter through the actual file polling boundary.
        let mut external = Config::load(root.path()).unwrap();
        external.appearance = crate::config::Appearance::Light;
        external.save(root.path()).unwrap();
        rt.update(|s, _| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            s.poll_settings();
        });
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        assert_canvas(&rt, light);
        choose(&mut rt, "appearance", "System");
        assert_canvas(&rt, dark);
        assert_eq!(
            Config::load(root.path()).unwrap().theme_family.label(),
            label,
            "brightness changes keep the family"
        );
        restore_tab_defaults(&mut rt, "appearance");
        assert_canvas(&rt, "#1e1e2e");
        assert_eq!(
            Config::load(root.path()).unwrap().appearance,
            crate::config::Appearance::System
        );
    }
}

#[test]
fn every_family_recolors_rendered_ansi_without_rewriting_program_rgb_or_terminal_selection() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    rt.update(|s, _| {
        let mut parser = vt100::Parser::new(8, 30, 0);
        parser
            .process(b"\x1b[41m \x1b[101m \x1b[48;5;196m \x1b[48;2;12;34;56m \x1b[0m output stays");
        s.preview = Some(parser.screen().clone());
        s.selection = Some(((1, 0), (1, 5)));
    });
    // Normal / bright red literals from the pinned primary palettes; custom light
    // ANSI pairs are the explicitly documented Rodtty adaptation contracts.
    for (family, light, dark) in [
        ("catppuccin", ["#d20f39", "#de293e"], ["#f38ba8", "#f37799"]),
        ("dracula", ["#cb3a2a", "#cb3a2a"], ["#ff5555", "#ff6e6e"]),
        (
            "tokyo_night",
            ["#8c4351", "#8c4351"],
            ["#f7768e", "#f7768e"],
        ),
        ("nord", ["#994950", "#994950"], ["#bf616a", "#bf616a"]),
        ("gruvbox", ["#cc241d", "#9d0006"], ["#cc241d", "#fb4934"]),
        (
            "one_dark_pro",
            ["#c23f4a", "#c23f4a"],
            ["#e05561", "#ff616e"],
        ),
        ("night_owl", ["#de3d3b", "#de3d3b"], ["#EF5350", "#EF5350"]),
        ("ayu", ["#f07171", "#f07171"], ["#f07178", "#f07178"]),
        ("solarized", ["#dc322f", "#cb4b16"], ["#dc322f", "#cb4b16"]),
    ] {
        for (mode, expected) in [("light", light), ("dark", dark)] {
            let config: Config = serde_json::from_value(
                serde_json::json!({"theme_family":family,"appearance":mode}),
            )
            .unwrap();
            config.save(root.path()).unwrap();
            rt.update(|s, _| {
                s.settings_poll = Instant::now() - Duration::from_secs(1);
                s.poll_settings();
            });
            for _ in 0..45 {
                rt.frame(1.0 / 60.0);
            }
            let terminal = rt.presented("terminal").unwrap().0;
            let metrics = rt.state.terminal_metrics;
            for (column, expected) in [expected[0], expected[1], "#ff0000", "#0c2238"]
                .iter()
                .enumerate()
            {
                let x = terminal[0] + (column as f32 + 0.5) * metrics.width;
                let fill = rt
                    .instances
                    .iter()
                    .find(|i| {
                        (i.rect[1] - terminal[1]).abs() < 0.01
                            && i.rect[3] == metrics.height
                            && i.rect[0] <= x
                            && x < i.rect[0] + i.rect[2]
                            && i.rect[2] < 2.0 * metrics.width
                    })
                    .expect("rendered terminal cell")
                    .fill;
                let c = rune::theme::hex(expected).unwrap();
                assert_eq!(
                    fill,
                    [c.r, c.g, c.b, c.a],
                    "{family} {mode} column {column}"
                );
            }
            assert_eq!(rt.state.selection, Some(((1, 0), (1, 5))));
        }
    }
}

#[test]
fn ayu_light_settings_labels_remain_readable_on_the_light_canvas() {
    let root = crate::test_support::tempdir().unwrap();
    let config: Config =
        serde_json::from_value(serde_json::json!({"theme_family":"ayu","appearance":"light"}))
            .unwrap();
    config.save(root.path()).unwrap();
    let mut rt = runtime(root.path());
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("theme"));
    for _ in 0..45 {
        rt.frame(1.0 / 60.0);
    }
    let label = rt
        .elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some("Theme family"))
        .unwrap()
        .rect;
    let glyph = rt
        .instances
        .iter()
        .find(|i| {
            i.params[1] == rune::render::KIND_GLYPH
                && i.rect[0] >= label[0]
                && i.rect[0] < label[0] + label[2]
                && i.rect[1] >= label[1]
                && i.rect[1] < label[1] + label[3]
        })
        .expect("drawn theme label");
    let luminance = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let fg = luminance(glyph.fill);
    let bg = luminance(rt.clear_color());
    assert!(
        (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05) >= 4.5,
        "theme label contrast: {:?}",
        glyph.fill
    );
}

#[test]
fn editor_and_read_only_diff_draw_the_selected_familys_keyword_palette() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    for (family, light, dark) in [
        ("catppuccin", "#8839ef", "#cba6f7"),
        ("dracula", "#a3144d", "#ff79c6"),
        ("tokyo_night", "#65359d", "#bb9af7"),
        ("one_dark_pro", "#884bb3", "#c678dd"),
        ("night_owl", "#994cc3", "#c792ea"),
    ] {
        for (mode, expected) in [("light", light), ("dark", dark)] {
            let config: Config = serde_json::from_value(
                serde_json::json!({"theme_family":family,"appearance":mode}),
            )
            .unwrap();
            config.save(root.path()).unwrap();
            for diff in [false, true] {
                rt.update(move |s, _| {
                    s.settings_poll = Instant::now() - Duration::from_secs(1);
                    s.poll_settings();
                    s.workspace.tabs.clear();
                    demo(s, diff);
                });
                for _ in 0..45 {
                    rt.frame(1.0 / 60.0);
                }
                let c = rune::theme::hex(expected).unwrap();
                assert!(
                    rt.instances
                        .iter()
                        .any(|i| i.params[1] == rune::render::KIND_GLYPH
                            && i.fill == [c.r, c.g, c.b, c.a]),
                    "missing {family} {mode} keyword in diff={diff}"
                );
            }
        }
    }
}

#[test]
fn narrow_settings_theme_dropdown_is_searchable_and_selectable() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path());
    rt.set_viewport((680.0, 800.0), 1.0);
    for _ in 0..45 {
        rt.frame(1.0 / 60.0);
    }
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("theme"));
    for _ in 0..45 {
        rt.frame(1.0 / 60.0);
    }
    click(&mut rt, "theme-family-trigger");
    assert!(rt.find("theme-family-option-Dracula").is_some());
    rt.paste("solar");
    for _ in 0..45 {
        rt.frame(1.0 / 60.0);
    }
    click(&mut rt, "theme-family-option-Solarized");
    assert_canvas(&rt, "#002b36");
    assert_eq!(
        Config::load(root.path()).unwrap().theme_family.label(),
        "Solarized"
    );
}

#[test]
fn owner_changes_independent_live_font_sizes_and_restores_defaults() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    runtime.update(|s, _| s.settings_query.replace_text("font size"));
    runtime.frame(0.0);
    let before = runtime
        .elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some("Settings"))
        .unwrap()
        .rect[3];
    click(&mut runtime, "more-ui-font-size");
    let after = runtime
        .elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some("Settings"))
        .unwrap()
        .rect[3];
    assert!(
        after > before,
        "UI text applies the chosen size immediately"
    );
    click(&mut runtime, "more-terminal-font-size");
    click(&mut runtime, "more-editor-font-size");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["fonts"]["ui_size"], 14.0);
    assert_eq!(saved["fonts"]["terminal_size"], 15.0);
    assert_eq!(saved["fonts"]["editor_size"], 15.0);
    restore_tab_defaults(&mut runtime, "appearance");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["fonts"]["ui_size"], 13.0);
    assert_eq!(saved["fonts"]["terminal_size"], 14.0);
    assert_eq!(saved["fonts"]["editor_size"], 14.0);
}
#[test]
fn system_tracks_live_host_changes_while_explicit_appearance_stays_fixed() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    assert_canvas(&runtime, "#1e1e2e");
    runtime.set_system_dark(false);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    assert_canvas(&runtime, "#eff1f5");
    open_settings(&mut runtime);
    click(&mut runtime, "settings-page-tab-appearance");
    choose(&mut runtime, "appearance", "Dark");
    runtime.set_system_dark(true);
    runtime.frame(0.0);
    runtime.set_system_dark(false);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    assert_canvas(&runtime, "#1e1e2e");
    choose(&mut runtime, "appearance", "Light");
    runtime.set_system_dark(true);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    assert_canvas(&runtime, "#eff1f5");
    choose(&mut runtime, "appearance", "System");
    assert_canvas(&runtime, "#1e1e2e");
    runtime.set_system_dark(false);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    assert_canvas(&runtime, "#eff1f5");
    assert_eq!(
        Config::load(root.path()).unwrap().appearance,
        crate::config::Appearance::System
    );
    runtime.update(|s, _| s.settings_query.replace_text("latte"));
    runtime.frame(0.0);
    assert!(runtime.find("appearance-trigger").is_some());
}
#[test]
fn switching_appearance_recolors_ansi_but_preserves_extended_output_and_selection() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    runtime.update(|s, _| {
        let mut parser = vt100::Parser::new(8, 30, 0);
        parser.process(b"\x1b[41m \x1b[48;5;196m \x1b[48;2;12;34;56m \x1b[0m output stays");
        s.preview = Some(parser.screen().clone());
        s.selection = Some(((1, 0), (1, 5)));
    });
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    let cell_background = |runtime: &rune::Runtime<State>, col: usize| {
        let r = runtime.presented("terminal").unwrap().0;
        let metrics = runtime.state.terminal_metrics;
        let x = r[0] + (col as f32 + 0.5) * metrics.width;
        runtime
            .instances
            .iter()
            .find(|i| {
                (i.rect[1] - r[1]).abs() < 0.01
                    && i.rect[3] == metrics.height
                    && i.rect[0] <= x
                    && x < i.rect[0] + i.rect[2]
                    && i.rect[2] < 2.0 * metrics.width
            })
            .expect("terminal cell background")
            .fill
    };
    let rgb = |hex: &str| {
        let c = rune::theme::hex(hex).unwrap();
        [c.r, c.g, c.b, c.a]
    };
    assert_eq!(cell_background(&runtime, 0), rgb("#f38ba8"));
    assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
    assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
    open_settings(&mut runtime);
    click(&mut runtime, "settings-page-tab-appearance");
    choose(&mut runtime, "appearance", "Light");
    assert_eq!(cell_background(&runtime, 0), rgb("#d20f39"));
    assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
    assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
    assert!(
        runtime
            .state
            .with_screen(|screen| screen.contents().contains("output stays"))
    );
    assert_eq!(runtime.state.selection, Some(((1, 0), (1, 5))));
}
#[test]
fn owner_selects_persists_and_restores_autosave_preferences() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    runtime.update(|s, _| s.settings_query.replace_text("autosave"));
    runtime.frame(0.0);
    choose(&mut runtime, "autosave", "After Delay");
    click(&mut runtime, "less-autosave-delay");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["autosave"]["policy"], "after_delay");
    assert_eq!(saved["autosave"]["delay_ms"], 900);
    choose(&mut runtime, "autosave", "Editor Focus Loss");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["autosave"]["policy"], "editor_focus_loss");
    assert_eq!(saved["autosave"]["delay_ms"], 900, "the delay stays");
    restore_tab_defaults(&mut runtime, "general");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["autosave"]["policy"], "explicit");
    assert_eq!(saved["autosave"]["delay_ms"], 1000);
}

#[test]
fn owner_changes_appearance_live_and_reopens_with_saved_choice() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = self::runtime(root.path());
    open_settings(&mut runtime);
    click(&mut runtime, "settings-page-tab-appearance");
    choose(&mut runtime, "appearance", "Light");
    assert_canvas(&runtime, "#eff1f5");
    choose(&mut runtime, "appearance", "Dark");
    assert_canvas(&runtime, "#1e1e2e");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["appearance"], "dark");
    assert_eq!(saved["theme_family"], "catppuccin");
    let mut reopened = self::runtime(root.path());
    reopened.set_system_dark(false);
    for _ in 0..45 {
        reopened.frame(1.0 / 60.0);
    }
    assert_canvas(&reopened, "#1e1e2e");
}
#[test]
fn prompt_style_and_symbol_preview_reset_persist_and_reach_a_running_shell() {
    use crate::prompt::{PromptStyle, PromptSymbol};
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.directory = root.path().into();
    state.shell = std::env::current_exe()
        .unwrap()
        .ancestors()
        .map(|p| p.join("rodtty-shell"))
        .find(|p| p.is_file());
    state.workspace_setup();
    if state.sessions.is_empty() {
        state.new_tab();
    }
    // Bash's `\w` shows a directory under $HOME as `~/…`; the README gate
    // keeps TMPDIR under target/, which is inside $HOME for most checkouts.
    let dir = match std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| root.path().strip_prefix(home).ok().map(|p| p.to_owned()))
    {
        Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => root.path().display().to_string(),
    };
    fn terminal(rt: &rune::Runtime<State>) -> &Session {
        rt.state.active().unwrap()
    }
    let wait_prompt = |rt: &mut rune::Runtime<State>, expected: &str| {
        let start = Instant::now();
        loop {
            // The settings pane narrows the terminal, so prompts may wrap.
            let row = terminal(rt).with_screen(|s| s.contents().trim_end().to_owned());
            if row.ends_with(expected.trim_end()) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "screen {row:?}, expected prompt {expected:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let mut rt = rune::Runtime::new(
        state,
        view,
        (1200.0, 800.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.frame(0.0);
    wait_prompt(&mut rt, &format!("rodtty:{dir}$ "));
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("prompt"));
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("history").is_none(),
        "search narrows to prompt rows"
    );
    let labelled = |rt: &rune::Runtime<State>, label: &str| {
        rt.elements()
            .iter()
            .any(|e| e.label.as_deref() == Some(label))
    };
    assert!(rt.find("prompt-style-preview").is_some());
    assert!(rt.find("prompt-symbol-preview").is_some());
    assert!(
        labelled(&rt, "rodtty:~/project$ "),
        "default Classic $ preview"
    );

    choose(&mut rt, "prompt-style", "Two-line");
    choose(&mut rt, "prompt-symbol", "Chevron ›");
    let saved = Config::load(root.path()).unwrap().prompt;
    assert_eq!(saved.style, PromptStyle::TwoLine);
    assert_eq!(saved.symbol, PromptSymbol::Chevron);
    assert!(labelled(&rt, "~/project\n› "), "live style preview");
    assert!(labelled(&rt, "› "), "live symbol preview");
    terminal(&rt).send(b"\r".to_vec()).unwrap();
    wait_prompt(&mut rt, "› ");

    choose(&mut rt, "prompt-style", "Classic");
    let saved = Config::load(root.path()).unwrap().prompt;
    assert_eq!(saved.style, PromptStyle::Classic);
    assert_eq!(
        saved.symbol,
        PromptSymbol::Chevron,
        "the choices are independent"
    );
    assert!(labelled(&rt, "rodtty:~/project› "));
    terminal(&rt).send(b"\r".to_vec()).unwrap();
    wait_prompt(&mut rt, &format!("rodtty:{dir}› "));

    // External JSON edits apply to running shells as well.
    let mut external = Config::load(root.path()).unwrap();
    external.prompt.style = PromptStyle::Rodtty;
    external.save(root.path()).unwrap();
    rt.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    rt.frame(1.0 / 60.0);
    assert!(labelled(&rt, "Rodtty › "));
    terminal(&rt).send(b"\r".to_vec()).unwrap();
    wait_prompt(&mut rt, "Rodtty › ");

    restore_tab_defaults(&mut rt, "appearance");
    assert_eq!(
        Config::load(root.path()).unwrap().prompt,
        Default::default()
    );
    terminal(&rt).send(b"\r".to_vec()).unwrap();
    wait_prompt(&mut rt, &format!("rodtty:{dir}$ "));
}
#[test]
fn owner_searches_resets_and_edits_actual_json_beside_the_terminal() {
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    assert!(runtime.find("editor-pane").is_some());
    assert!(runtime.find("terminal-pane").is_some());
    assert!(runtime.find("settings-tab").is_some());
    runtime.key(KeyEvent {
        key: Key::Text("history".into()),
        mods: Default::default(),
    });
    runtime.frame(0.0);
    assert!(runtime.find("history").is_some());
    assert!(runtime.find("utility-cat").is_none());
    click(&mut runtime, "history");
    assert!(
        !Config::load(root.path()).unwrap().history,
        "{}; active history {}",
        runtime.state.message,
        runtime.state.config.history
    );
    runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
    runtime.frame(1.0 / 60.0);
    click(&mut runtime, "more-scrollback");
    assert_eq!(Config::load(root.path()).unwrap().scrollback, 11_000);
    restore_tab_defaults(&mut runtime, "general");
    let saved = Config::load(root.path()).unwrap();
    assert!(saved.history);
    assert_eq!(saved.scrollback, 10_000);
    click(&mut runtime, "settings-page-tab-commands");
    click(&mut runtime, "utility-cat");
    assert_eq!(Config::load(root.path()).unwrap().disabled, ["cat"]);
    restore_tab_defaults(&mut runtime, "commands");
    assert!(Config::load(root.path()).unwrap().disabled.is_empty());
    click(&mut runtime, "open-settings-json");
    let start = Instant::now();
    while runtime.state.workspace.tabs.is_empty() && start.elapsed() < Duration::from_secs(5) {
        runtime.update(|s, tx| {
            s.workspace_poll(tx);
        });
        runtime.frame(0.0);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        runtime.state.workspace.tabs[0]
            .document
            .path
            .ends_with("settings.json")
    );
    assert!(runtime.find("file-editor-1").is_some());
    assert!(runtime.find("terminal-pane").is_some());
    runtime.update(|_, tx| tx.focus("file-editor-1", true));
    runtime.frame(1.0 / 60.0);
    runtime.key(KeyEvent {
        key: Key::Command('a'),
        mods: rune::Mods {
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    });
    runtime.key(KeyEvent {
        key: Key::Text(r#"{"history":false,"scrollback":3000}"#.into()),
        mods: Default::default(),
    });
    runtime.frame(1.0 / 60.0);
    click(&mut runtime, "file-save");
    let start = Instant::now();
    while runtime.state.workspace.tabs[0].saving && start.elapsed() < Duration::from_secs(5) {
        runtime.update(|s, tx| {
            s.workspace_poll(tx);
        });
        runtime.frame(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(10));
    }
    runtime.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    assert!(!Config::load(root.path()).unwrap().history);
    assert!(!runtime.state.config.history);
    assert_eq!(runtime.state.config.scrollback, 3000);
}
#[test]
fn external_json_updates_preferences_but_invalid_edits_keep_the_last_valid_choices() {
    let root = crate::test_support::tempdir().unwrap();
    Config::default().save(root.path()).unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    let path = root.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"appearance":"light","history":false,"scrollback":2000,"future":{"choice":7}}"#,
    )
    .unwrap();
    runtime.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    runtime.frame(1.0 / 60.0);
    assert!(!runtime.state.config.history);
    assert_eq!(runtime.state.config.scrollback, 2000);
    for _ in 0..45 {
        runtime.frame(1.0 / 60.0);
    }
    assert_canvas(&runtime, "#eff1f5");
    std::fs::write(&path, "{invalid JSON").unwrap();
    runtime.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    runtime.frame(1.0 / 60.0);
    assert!(!runtime.state.config.history);
    assert!(
        runtime
            .state
            .message
            .contains("keeping last valid preferences")
    );
    assert_canvas(&runtime, "#eff1f5");
    // A UI action must not destroy the invalid JSON being repaired externally.
    runtime.update(|s, _| {
        s.settings_query.replace_text("history");
    });
    runtime.frame(1.0 / 60.0);
    click(&mut runtime, "history");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{invalid JSON");
    assert!(!runtime.state.config.history);
    std::fs::write(&path, r#"{"history":false,"future":{"choice":7}}"#).unwrap();
    runtime.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    runtime.frame(1.0 / 60.0);
    click(&mut runtime, "history");
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["history"], true);
    assert_eq!(saved["future"]["choice"], 7);
    assert!(runtime.state.settings_error.is_none());
}
#[test]
fn a_ui_edit_preserves_unrelated_external_changes_before_the_next_poll() {
    let root = crate::test_support::tempdir().unwrap();
    Config::default().save(root.path()).unwrap();
    let mut runtime = runtime(root.path());
    open_settings(&mut runtime);
    runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
    runtime.frame(1.0 / 60.0);
    std::fs::write(
        root.path().join("settings.json"),
        r#"{"history":false,"scrollback":10000}"#,
    )
    .unwrap();
    click(&mut runtime, "more-scrollback");
    let saved = Config::load(root.path()).unwrap();
    assert!(!saved.history);
    assert_eq!(saved.scrollback, 11000);
}
