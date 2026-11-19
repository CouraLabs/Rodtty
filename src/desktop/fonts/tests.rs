use super::*;
use rune::{Env, Runtime};

fn rect(rt: &Runtime<State>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("missing {key}"))
        .rect
}
fn frame(rt: &mut Runtime<State>) {
    for _ in 0..4 {
        rt.frame(1.0 / 60.0);
    }
}
fn open_settings(rt: &mut Runtime<State>) {
    rt.update(|s, tx| s.dispatch(AppAction::Settings, tx));
    frame(rt);
}
fn click(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    frame(rt);
}
#[test]
fn font_family_pickers_stay_inside_the_pane_and_wrap_below_their_label_when_narrow() {
    let _families = rune::text::with_families("Geist", "Geist Mono");
    // A 640 pt window (the minimum) shows the editor pane alone; a wide
    // window with a narrow editor share squeezes the Settings pane.
    for (size, share, wraps) in [((640.0, 600.0), 0.5, false), ((1200.0, 800.0), 0.24, true)] {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.workspace.layout.editor_share = share;
        state.open_settings(&mut Tx::default());
        state.settings_query.replace_text("font family");
        let mut rt = Runtime::with_text(
            state,
            view,
            size,
            Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        frame(&mut rt);
        rt.settle_all();
        frame(&mut rt);
        let element = |key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        let section = element("settings-section-Appearance").rect;
        for key in ["ui", "terminal"] {
            let picker = element(&format!("font-{key}-trigger"));
            let line = element(&format!("{key}-font-family-row")).rect;
            let right = |r: [f32; 4]| r[0] + r[2];
            assert!(
                right(picker.rect) <= right(section) + 0.5 && picker.rect[0] >= section[0] - 0.5,
                "{size:?} {key} picker stays inside its section: {:?} in {section:?}",
                picker.rect
            );
            assert!(
                (picker.visible[2] - picker.rect[2]).abs() < 0.5,
                "{size:?} {key} picker is not clipped: {:?}",
                picker.visible
            );
            let below = picker.rect[1] - line[1] > 10.0;
            assert_eq!(below, wraps, "{size:?} {key} picker wraps below its label");
        }
    }
}

#[test]
fn owner_searches_and_selects_ui_and_monospace_terminal_families() {
    let _families = rune::text::with_families("Geist", "Geist Mono");
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.open_settings(&mut Tx::default());
    state.settings_query.replace_text("font family");
    let mut rt = Runtime::with_text(
        state,
        view,
        (1200.0, 900.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    frame(&mut rt);
    click(&mut rt, "font-ui-trigger");
    assert!(rt.find("font-ui-option-Geist").is_some());
    assert!(rt.find("font-ui-option-Geist Mono").is_some());
    click(&mut rt, "font-ui-option-Geist Mono");
    assert_eq!(
        crate::config::Config::load(root.path())
            .unwrap()
            .fonts
            .ui_family,
        "Geist Mono"
    );
    click(&mut rt, "font-terminal-trigger");
    assert!(rt.find("font-terminal-option-Geist").is_none());
    assert!(rt.find("font-terminal-option-Geist Mono").is_some());
    click(&mut rt, "font-terminal-option-Geist Mono");
    click(&mut rt, "font-ui-trigger");
    click(&mut rt, "font-ui-option-Geist");
    assert_eq!(
        crate::config::Config::load(root.path())
            .unwrap()
            .fonts
            .ui_family,
        "Geist"
    );
    click(&mut rt, "font-ui-trigger");
    rt.set_viewport((640.0, 900.0), 1.0);
    frame(&mut rt);
    assert!(rt.find("font-ui-option-Geist").is_some());
    click(&mut rt, "workspace-menu");
    assert!(
        rt.find("font-ui-option-Geist").is_none(),
        "pressing outside the font dropdown must dismiss it"
    );
    click(&mut rt, "workspace-menu");
    assert!(rt.find("workspace-menu-popover").is_some());
    assert!(
        rt.find("font-ui-option-Geist").is_none(),
        "the workspace menu must suppress an already open font dropdown"
    );
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Default::default(),
    });
    frame(&mut rt);
    click(&mut rt, "font-ui-trigger");
    rt.update(|s, tx| s.dispatch(AppAction::ShowTerminal, tx));
    frame(&mut rt);
    assert!(
        rt.find("font-ui-option-Geist").is_none(),
        "a hidden compact settings pane must not retain its dropdown overlay"
    );
}
#[test]
fn file_operation_dialogs_and_tree_menus_cover_open_font_dropdowns() {
    let root = crate::test_support::tempdir().unwrap();
    let file = root.path().join("notes.txt");
    std::fs::write(&file, "notes").unwrap();
    let mut state = State::with_root(root.path().into());
    state.directory = root.path().into();
    state.workspace_setup();
    state.toggle_files();
    state.open_settings(&mut Tx::default());
    state.settings_query.replace_text("font family");
    let mut rt = Runtime::with_text(
        state,
        view,
        (1600.0, 900.0),
        Env::new(true, 1.0),
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    let started = std::time::Instant::now();
    while rt.find(file.to_str().unwrap()).is_none()
        && started.elapsed() < std::time::Duration::from_secs(8)
    {
        rt.update(|s, tx| {
            s.workspace_poll(tx);
        });
        rt.frame(0.1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    click(&mut rt, "font-ui-trigger");
    assert!(rt.find("font-ui-option-Geist").is_some());
    rt.update(|s, tx| s.dispatch(AppAction::CreateFile, tx));
    frame(&mut rt);
    assert!(rt.find("operation-dialog").is_some());
    assert!(rt.find("font-ui-option-Geist").is_none());
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Default::default(),
    });
    rt.frame(1.0);
    if rt.find("font-ui-option-Geist").is_none() {
        click(&mut rt, "font-ui-trigger");
    }
    let (x, y) = rt.find(file.to_str().unwrap()).unwrap();
    rt.update(move |s, tx| s.file_context_menu(rune::PointerPress { x, y }, tx));
    rt.frame(1.0);
    assert!(rt.find("file-context-menu").is_some());
    assert!(
        rt.find("font-ui-option-Geist").is_none(),
        "font choices must stay below a tree context menu"
    );
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Default::default(),
    });
    frame(&mut rt);
    if rt.find("font-ui-option-Geist").is_none() {
        click(&mut rt, "font-ui-trigger");
    }
    let (x, y) = rt.find("settings-tab").unwrap();
    rt.update(move |s, tx| {
        super::tabs::context_menu(
            s,
            super::tabs::Target::Settings,
            rune::PointerPress { x, y },
            tx,
        );
    });
    frame(&mut rt);
    assert!(rt.popup_menu().is_some());
    assert!(
        rt.find("font-ui-option-Geist").is_none(),
        "font choices must stay below a tab context menu"
    );
    rt.resolve_popup(None).unwrap();
    frame(&mut rt);
    assert!(!rt.state.application_overlay_open());
}

#[test]
#[ignore = "requires a native desktop host with installed proportional and monospace fonts"]
fn owner_finds_installed_fonts_by_keyboard_and_keeps_editor_size() {
    let _families = rune::text::with_families("Geist", "Geist Mono");
    let families = Catalog::discover();
    let installed_mono = families
        .iter()
        .find(|f| !f.bundled && f.monospace)
        .expect("native host has a monospace family")
        .name
        .clone();
    assert!(families.iter().any(|f| !f.bundled && !f.monospace));
    eprintln!(
        "native font inventory: {} families; {} monospace; selecting {}",
        families.len(),
        families.iter().filter(|f| f.monospace).count(),
        installed_mono
    );
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    workspace_preview(&mut state);
    state.save_config(|c| c.fonts.editor_size = 22.0);
    state.open_settings(&mut Tx::default());
    state.settings_query.replace_text("font family");
    state.font_catalog.families = families;
    state.font_catalog.ready = true;
    let mut rt = Runtime::with_text(
        state,
        view,
        (1600.0, 1000.0),
        Env {
            system_dark: false,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    frame(&mut rt);
    click(&mut rt, "font-terminal-trigger");
    rt.paste(installed_mono.clone());
    frame(&mut rt);
    assert!(
        rt.find(&format!("font-terminal-option-{installed_mono}"))
            .is_some()
    );
    assert!(
        rt.find("font-terminal-option-Geist Mono").is_none(),
        "search filters unrelated families"
    );
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Default::default(),
    });
    frame(&mut rt);
    let saved = Config::load(root.path()).unwrap();
    assert_eq!(saved.fonts.terminal_family, installed_mono);
    assert_eq!(saved.fonts.editor_size, 22.0);
    assert!(
        rt.font_families()
            .1
            .starts_with(&format!("\"{installed_mono}\""))
    );
    click(&mut rt, "font-terminal-trigger");
    click(&mut rt, "font-terminal-option-Geist Mono");
    assert!(rt.font_families().1.starts_with("\"Geist Mono\""));
    assert_eq!(Config::load(root.path()).unwrap().fonts.editor_size, 22.0);
}
#[test]
fn disappeared_saved_families_show_the_bundled_fallback_without_rewriting_preferences() {
    let _families = rune::text::with_families("Geist", "Geist Mono");
    let root = crate::test_support::tempdir().unwrap();
    let mut config = Config::default();
    config.fonts.ui_family = "Rodtty Vanished UI 1229".into();
    config.fonts.terminal_family = "Rodtty Vanished Mono 1229".into();
    config.save(root.path()).unwrap();
    let mut state = State::with_root(root.path().into());
    state.font_catalog.ready = true;
    state.open_settings(&mut Tx::default());
    state.settings_query.replace_text("font family");
    let mut rt = Runtime::with_text(
        state,
        view,
        (1200.0, 900.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    rt.update(|s, tx| {
        poll(s, 0.0, tx);
    });
    frame(&mut rt);
    assert_eq!(
        rt.font_families(),
        ("\"Geist\", \"Geist\"", "\"Geist Mono\", \"Geist Mono\"")
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_ref().is_some_and(|text| text
                .contains("Rodtty Vanished Mono 1229 is unavailable; using bundled Geist Mono")))
    );
    assert_eq!(
        Config::load(root.path()).unwrap().fonts.terminal_family,
        "Rodtty Vanished Mono 1229"
    );
}
#[test]
fn terminal_unicode_cursor_and_selection_follow_live_sizes_and_display_scale() {
    let _families = rune::text::with_families("Geist", "Geist Mono");
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(24, 80, 100);
    parser.process("Wi日本語🌿e\u{301}Z".as_bytes());
    state.preview = Some(parser.screen().clone());
    let mut rt = Runtime::with_text(
        state,
        view,
        (1200.0, 900.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    frame(&mut rt);
    let grid = rect(&rt, "terminal");
    // Geist Mono's hmtx advance is 600 / 1000 em. At 14px the cell
    // rounds to 8 device pixels; wide/combining glyphs keep VT cell indices.
    assert_eq!(rect(&rt, "terminal-cell-0-11")[0] - grid[0], 88.0);
    assert_eq!(rect(&rt, "terminal-caret")[0] - grid[0], 96.0);
    open_settings(&mut rt);
    rt.update(|s, _| s.settings_query.replace_text("font size"));
    frame(&mut rt);
    for _ in 0..6 {
        click(&mut rt, "more-terminal-font-size");
    }
    let grid = rect(&rt, "terminal");
    assert_eq!(rect(&rt, "terminal-cell-0-11")[0] - grid[0], 132.0);
    assert_eq!(rect(&rt, "terminal-caret")[0] - grid[0], 144.0);
    rt.pointer_down(grid[0] + 1.0, grid[1] + 8.0);
    rt.pointer_move(grid[0] + 145.0, grid[1] + 8.0);
    rt.pointer_up(grid[0] + 145.0, grid[1] + 8.0);
    frame(&mut rt);
    rt.update(|s, tx| {
        on_key(
            s,
            &KeyEvent {
                key: Key::Command('c'),
                mods: rune::Mods {
                    cmd: true,
                    ctrl: !cfg!(target_os = "macos"),
                    shift: !cfg!(target_os = "macos"),
                    ..Default::default()
                },
            },
            tx,
        );
    });
    assert_eq!(
        rt.copy_request.take().unwrap().content().text(),
        "Wi日本語🌿e\u{301}Z"
    );
    crate::desktop::settings::restore_tab_defaults(&mut rt, "appearance");
    rt.set_viewport((1200.0, 900.0), 1.5);
    frame(&mut rt);
    let grid = rect(&rt, "terminal");
    // 8.4 logical px rounds to 13 physical px at scale 1.5.
    assert!((rect(&rt, "terminal-caret")[0] - grid[0] - 104.0).abs() < 0.02);
}
#[test]
fn external_font_preferences_apply_with_bundled_fallback_and_keep_unsaved_editor_work() {
    use std::time::{Duration, Instant};
    let _families = rune::text::with_families("Geist", "Geist Mono");
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    workspace_preview(&mut state);
    let text = state.workspace.tabs[0].edit.text.clone();
    let mut rt = Runtime::with_text(
        state,
        view,
        (1600.0, 1000.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    frame(&mut rt);
    let before = rect(&rt, "file-editor-1")[3];
    let tree_label_height = |rt: &Runtime<State>| {
        rt.elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Cargo.toml"))
            .expect("visible explorer label")
            .rect[3]
    };
    let tree_before = tree_label_height(&rt);
    std::fs::write(root.path().join("settings.json"), r#"{"fonts":{"ui_family":"Rodtty Missing UI Face 1827","terminal_family":"Rodtty Missing Mono Face 1827","ui_size":16,"terminal_size":18,"editor_size":22}}"#).unwrap();
    rt.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    frame(&mut rt);
    assert!(rt.font_families().0.ends_with("\"Geist\""));
    assert!(rt.font_families().1.ends_with("\"Geist Mono\""));
    assert!(rect(&rt, "file-editor-1")[3] > before);
    assert!(
        tree_label_height(&rt) > tree_before,
        "explorer labels follow UI sizing"
    );
    assert_eq!(
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("file-editor-1"))
            .unwrap()
            .text
            .as_deref(),
        Some(text.as_str())
    );
    let before = rect(&rt, "terminal-caret");
    std::fs::write(
        root.path().join("settings.json"),
        r#"{"fonts":{"terminal_size":0}}"#,
    )
    .unwrap();
    rt.update(|s, tx| {
        s.settings_poll = Instant::now() - Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    frame(&mut rt);
    assert_eq!(rect(&rt, "terminal-caret"), before);
    assert!(rt.elements().iter().any(|e| {
        e.text
            .as_ref()
            .is_some_and(|t| t.contains("font size must be between"))
    }));
}
#[test]
fn git_diff_code_follows_the_live_editor_size() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    diff_preview(&mut state);
    let mut rt = Runtime::with_text(
        state,
        view,
        (1440.0, 860.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
    );
    frame(&mut rt);
    let code_height = |rt: &Runtime<State>| {
        rt.elements()
            .into_iter()
            .find(|e| e.path.contains("git-diff/") && e.text.as_deref() == Some("fn main() {"))
            .expect("rendered diff code")
            .rect[3]
    };
    let before = code_height(&rt);
    std::fs::write(
        root.path().join("settings.json"),
        r#"{"fonts":{"editor_size":22}}"#,
    )
    .unwrap();
    rt.update(|s, tx| {
        s.settings_poll = std::time::Instant::now() - std::time::Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    frame(&mut rt);
    assert!(
        code_height(&rt) > before * 1.4,
        "Git diff code must reflect editor sizing"
    );
    let code = rt
        .elements()
        .into_iter()
        .find(|e| e.path.contains("git-diff/") && e.text.as_deref() == Some("fn main() {"))
        .unwrap()
        .rect;
    rt.pointer_move(code[0] + 0.1, code[1] + code[3] / 2.0);
    rt.pointer_down(code[0] + 0.1, code[1] + code[3] / 2.0);
    frame(&mut rt);
    rt.pointer_move(code[0] + 26.5, code[1] + code[3] / 2.0);
    frame(&mut rt);
    rt.pointer_up(code[0] + 26.5, code[1] + code[3] / 2.0);
    frame(&mut rt);
    assert_eq!(rt.selected_text().as_deref(), Some("fn"));
}
