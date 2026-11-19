use super::super::*;
use crate::desktop::tests::{press, runtime_for};

fn click(rt: &mut rune::Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    rt.settle_all();
    rt.frame(0.0);
}
fn words(rt: &rune::Runtime<State>) -> String {
    rt.elements()
        .iter()
        .filter_map(|e| e.text.clone())
        .collect::<Vec<_>>()
        .join(" ")
}
fn settings(rt: &mut rune::Runtime<State>, query: &str) {
    rt.update(|s, tx| s.open_settings(tx));
    press(rt, Key::Text(query.into()), Default::default());
}
#[test]
fn owner_searches_records_and_restores_a_live_persisted_shortcut() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime_for(State::with_root(root.path().into()));
    settings(&mut rt, "toggle explorer");
    assert!(rt.find("record-toggle-explorer").is_some());
    assert!(rt.find("record-new-terminal").is_none());
    click(&mut rt, "record-toggle-explorer");
    assert!(rt.find("shortcut-recorder").is_some());
    press(&mut rt, Key::Function(8), Default::default());
    click(&mut rt, "shortcut-apply");
    assert!(words(&rt).contains("F8"));
    press(&mut rt, Key::Function(8), Default::default());
    assert!(rt.state.workspace.layout.explorer);
    let mut reopened = runtime_for(State::with_root(root.path().into()));
    settings(&mut reopened, "toggle explorer");
    assert!(words(&reopened).contains("F8"));
    crate::desktop::settings::restore_tab_defaults(&mut reopened, "shortcuts");
    assert!(!words(&reopened).contains("F8"));
    press(&mut reopened, Key::Function(8), Default::default());
    assert!(!reopened.state.workspace.layout.explorer);
    // The restored default is VS Code's Toggle Sidebar binding.
    assert_eq!(
        reopened.state.shortcut_label("toggle-explorer"),
        if cfg!(target_os = "macos") {
            "Cmd+B"
        } else {
            "Ctrl+B"
        }
    );
    reopened.update(|_, tx| tx.blur());
    press(
        &mut reopened,
        Key::Command('b'),
        rune::Mods {
            ctrl: !cfg!(target_os = "macos"),
            cmd: true,
            ..Default::default()
        },
    );
    assert!(reopened.state.workspace.layout.explorer);
}

#[test]
fn default_shortcuts_follow_vs_code_and_parse() {
    let state = State::with_root(crate::test_support::tempdir().unwrap().path().into());
    let mac = cfg!(target_os = "macos");
    for (id, vs_code_mac, vs_code_other) in [
        ("command-palette", "Cmd+Shift+P", "Ctrl+Shift+P"),
        ("toggle-explorer", "Cmd+B", "Ctrl+B"),
        ("explorer-focus", "Cmd+Shift+E", "Ctrl+Shift+E"),
        ("explorer-search", "Cmd+Shift+F", "Ctrl+Shift+F"),
        ("toggle-terminal", "Ctrl+`", "Ctrl+`"),
        ("new-terminal", "Ctrl+Shift+`", "Ctrl+Shift+`"),
        ("find-terminal", "Cmd+F", "Ctrl+F"),
        ("find-editor", "Cmd+F", "Ctrl+F"),
        ("replace-editor", "Cmd+Alt+F", "Ctrl+H"),
        ("save-file", "Cmd+S", "Ctrl+S"),
        ("close-editor", "Cmd+W", "Ctrl+W"),
        ("settings", "Cmd+Comma", "Ctrl+Comma"),
        ("close-window", "Cmd+Shift+W", "Ctrl+Shift+W"),
        ("quit", "Cmd+Q", "Ctrl+Q"),
        ("copy-terminal", "Cmd+C", "Ctrl+Shift+C"),
        ("paste-terminal", "Cmd+V", "Ctrl+Shift+V"),
    ] {
        let command = state.commands.iter().find(|c| c.id == id).unwrap();
        let expected = if mac { vs_code_mac } else { vs_code_other };
        assert_eq!(command.shortcut, expected, "{id}");
        assert!(
            rune::Chord::parse(expected).is_ok(),
            "{id} {expected} parses"
        );
        assert!(
            state.shortcut_conflicts(id, expected).is_empty(),
            "{id} {expected} conflicts with {:?}",
            state.shortcut_conflicts(id, expected)
        );
    }
}
#[test]
fn conflicting_recording_preserves_defaults_and_palette_uses_current_binding() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime_for(State::with_root(root.path().into()));
    settings(&mut rt, "toggle explorer");
    click(&mut rt, "record-toggle-explorer");
    press(
        &mut rt,
        Key::Command('p'),
        rune::Mods {
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            shift: true,
            ..Default::default()
        },
    );
    assert!(rt.find("shortcut-conflict").is_some());
    click(&mut rt, "shortcut-apply");
    assert!(rt.find("shortcut-recorder").is_some());
    press(&mut rt, Key::Function(9), Default::default());
    click(&mut rt, "shortcut-apply");
    crate::desktop::tests::open_palette(&mut rt);
    press(
        &mut rt,
        Key::Text("toggle explorer".into()),
        Default::default(),
    );
    assert!(words(&rt).contains("F9"), "{}", words(&rt));
    press(&mut rt, Key::Enter, Default::default());
    assert!(rt.state.workspace.layout.explorer);
}
#[test]
fn owner_records_and_uses_a_mouse_shortcut() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime_for(State::with_root(root.path().into()));
    settings(&mut rt, "toggle explorer");
    click(&mut rt, "record-toggle-explorer");
    let (x, y) = rt.find("shortcut-mouse-recorder").unwrap();
    rt.button_down(x, y, rune::PointerButton::Back);
    rt.button_up(x, y, rune::PointerButton::Back);
    rt.frame(0.0);
    click(&mut rt, "shortcut-apply");
    assert!(words(&rt).contains("MouseBack"));
    rt.button_down(600.0, 650.0, rune::PointerButton::Back);
    rt.button_up(600.0, 650.0, rune::PointerButton::Back);
    rt.frame(0.0);
    assert!(rt.state.workspace.layout.explorer);
}

#[test]
fn shared_terminal_editor_chords_follow_visible_focus_and_text_fields_keep_copy_paste() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    demo(&mut state, false);
    state
        .config
        .shortcuts
        .insert("end-terminal".into(), "mod+w".into());
    state
        .config
        .shortcuts
        .insert("copy-terminal".into(), "mod+c".into());
    state
        .config
        .shortcuts
        .insert("paste-terminal".into(), "mod+v".into());
    state
        .config
        .shortcuts
        .insert("settings-close".into(), "mod+w".into());
    let mut rt = runtime_for(state);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    rt.update(|s, tx| {
        s.workspace.set_terminal_focus(false);
        tx.focus_when_shown("file-editor-1", true);
    });
    rt.frame(0.0);
    let primary = rune::Mods {
        cmd: true,
        ctrl: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    press(&mut rt, Key::Command('a'), primary);
    press(&mut rt, Key::Command('c'), primary);
    let copied = clipboard.text().unwrap();
    assert!(!copied.is_empty());
    press(&mut rt, Key::Text("replacement".into()), Default::default());
    press(&mut rt, Key::Command('v'), primary);
    rt.update(|_, tx| tx.request_paste());
    rt.frame(0.0);
    assert!(
        rt.state.workspace.tabs[0]
            .edit
            .text
            .contains(copied.as_str())
    );
    assert!(!rt.state.workspace.terminal_focus());
    rt.update(|s, tx| s.open_settings(tx));
    rt.frame(0.0);
    press(&mut rt, Key::Text("appearance".into()), Default::default());
    press(&mut rt, Key::Command('a'), primary);
    press(&mut rt, Key::Command('c'), primary);
    assert_eq!(clipboard.text().unwrap().as_str(), "appearance");
    press(&mut rt, Key::Command('w'), primary);
    assert!(rt.find("settings-search").is_none());
}
#[test]
fn runtime_retains_real_pty_interrupt_and_fullscreen_keys_with_custom_binding() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.directory = root.path().into();
    state.shell = std::env::current_exe()
        .unwrap()
        .ancestors()
        .map(|p| p.join("rodtty-shell"))
        .find(|p| p.is_file());
    state.new_tab();
    let session = state.active().expect("companion binary built");
    session
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    session.send(b"stty raw -echo; printf '\\033[?1h'; head -c 10 | od -An -tx1 | tr -s ' '; stty sane; printf '\\033[?1l\\nKEYS_DONE\\n'\r".to_vec()).unwrap();
    let started = std::time::Instant::now();
    while !session
        .state
        .lock()
        .unwrap()
        .parser
        .screen()
        .application_cursor()
    {
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    state
        .config
        .shortcuts
        .insert("toggle-explorer".into(), "F6".into());
    let mut rt = runtime_for(state);
    settings(&mut rt, "toggle explorer");
    click(&mut rt, "record-toggle-explorer");
    press(&mut rt, Key::Text("z".into()), Default::default());
    rt.paste("unwanted terminal input");
    rt.frame(0.0);
    press(&mut rt, Key::Escape, Default::default());
    click(&mut rt, "tab-0");
    press(&mut rt, Key::Function(6), Default::default());
    assert!(rt.state.workspace.layout.explorer);
    press(&mut rt, Key::Text("a".into()), Default::default());
    press(
        &mut rt,
        Key::Command('c'),
        rune::Mods {
            ctrl: true,
            cmd: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    );
    press(&mut rt, Key::Up, Default::default());
    press(&mut rt, Key::Function(5), Default::default());
    rt.state
        .active()
        .unwrap()
        .wait_for(
            "61 03 1b 4f 41 1b 5b 31 35 7e",
            std::time::Duration::from_secs(15),
        )
        .unwrap();
}

#[test]
fn invalid_external_binding_retains_last_live_binding_and_explains_error() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime_for(State::with_root(root.path().into()));
    settings(&mut rt, "toggle explorer");
    click(&mut rt, "record-toggle-explorer");
    press(&mut rt, Key::Function(8), Default::default());
    click(&mut rt, "shortcut-apply");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    json["shortcuts"]["toggle-explorer"] = "Ctrl+Ctrl+e".into();
    std::fs::write(
        root.path().join("settings.json"),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();
    rt.update(|s, tx| {
        s.settings_poll = std::time::Instant::now() - std::time::Duration::from_secs(1);
        poll(s, 0.0, tx);
    });
    rt.frame(0.0);
    assert!(words(&rt).contains("Invalid Settings JSON"));
    press(&mut rt, Key::Function(8), Default::default());
    assert!(rt.state.workspace.layout.explorer);
}
#[test]
fn clicking_a_visible_editor_overrides_stale_terminal_pane_focus_for_close_chords() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    demo(&mut state, false);
    state.directory = root.path().into();
    state.shell = std::env::current_exe()
        .unwrap()
        .ancestors()
        .map(|p| p.join("rodtty-shell"))
        .find(|p| p.is_file());
    state.new_tab();
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    state.workspace.set_terminal_focus(true);
    state
        .config
        .shortcuts
        .insert("end-terminal".into(), "mod+w".into());
    let mut rt = runtime_for(state);
    click(&mut rt, "file-editor-1");
    press(&mut rt, Key::Text("unsaved".into()), Default::default());
    press(
        &mut rt,
        Key::Command('w'),
        rune::Mods {
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    );
    assert!(rt.find("file-dialog").is_some());
    assert!(rt.find("confirmation").is_none());
}
#[test]
fn assigned_mouse_binding_takes_precedence_over_a_tab_middle_click() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime_for(State::with_root(root.path().into()));
    settings(&mut rt, "toggle explorer");
    click(&mut rt, "record-toggle-explorer");
    let (x, y) = rt.find("shortcut-mouse-recorder").unwrap();
    rt.button_down(x, y, rune::PointerButton::Middle);
    rt.button_up(x, y, rune::PointerButton::Middle);
    rt.frame(0.0);
    click(&mut rt, "shortcut-apply");
    let (x, y) = rt.find("settings-tab").unwrap();
    rt.button_down(x, y, rune::PointerButton::Middle);
    rt.button_up(x, y, rune::PointerButton::Middle);
    rt.frame(0.0);
    assert!(rt.state.workspace.layout.explorer);
    assert!(rt.find("settings-search").is_some());
}
