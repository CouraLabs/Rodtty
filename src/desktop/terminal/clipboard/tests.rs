use super::*;
use crate::desktop::tests::{open_palette, press, runtime_for};

#[test]
fn terminal_right_click_offers_shared_copy_and_blocks_input_until_dismissed() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(28, 100, 0);
    parser.process("Olá 世界 e\u{301} 🦀".as_bytes());
    state.preview = Some(parser.screen().clone());
    state.selection = Some(((0, 0), (0, 14)));
    let mut rt = runtime_for(state);
    let board = rune::clipboard::memory();
    board.set_text("unchanged");
    rt.use_clipboard(&board);
    let (x, y) = rt.find("terminal").unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(0.0);
    assert!(rt.popup_menu().is_some(), "right-click defaults to a menu");
    rt.resolve_popup(Some("copy-terminal")).unwrap();
    rt.frame(0.0);
    assert_eq!(board.text().unwrap().as_str(), "Olá 世界 e\u{301} 🦀");
    assert!(rt.state.selection.is_some());
    board.set_text("sentinel");
    press(
        &mut rt,
        Key::Command('c'),
        rune::Mods {
            ctrl: !cfg!(target_os = "macos"),
            cmd: true,
            shift: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    );
    assert_eq!(board.text().unwrap().as_str(), "Olá 世界 e\u{301} 🦀");
    board.set_text("sentinel");
    open_palette(&mut rt);
    press(
        &mut rt,
        Key::Text("Terminal: Copy Selection".into()),
        Default::default(),
    );
    press(&mut rt, Key::Enter, Default::default());
    assert_eq!(board.text().unwrap().as_str(), "Olá 世界 e\u{301} 🦀");
}
#[test]
fn copy_on_selection_defaults_on_copies_only_finished_nonempty_drags_and_honors_off() {
    let root = crate::test_support::tempdir().unwrap();
    assert!(!root.path().join("settings.json").exists());
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(28, 100, 0);
    parser.process("Olá 世界".as_bytes());
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    assert!(rt.state.config.terminal_clipboard.copy_on_selection);
    let board = rune::clipboard::memory();
    rt.use_clipboard(&board);
    let grid = |rt: &rune::Runtime<State>| {
        let [x, y, _, _] = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("terminal"))
            .unwrap()
            .rect;
        (x, y, fonts::Metrics::measure())
    };
    // Drag across cells `from..to` on the first row, checking that nothing is
    // copied while the drag is still in progress.
    let drag = |rt: &mut rune::Runtime<State>, from: f32, to: f32| {
        let (x, y, m) = grid(rt);
        let before = board.text().map(|t| t.as_str().to_owned());
        rt.pointer_down(x + from * m.width, y + 0.5 * m.height);
        rt.pointer_move(x + to * m.width, y + 0.5 * m.height);
        rt.frame(0.0);
        assert!(rt.state.selection.is_some(), "the drag selects");
        assert_eq!(
            board.text().map(|t| t.as_str().to_owned()),
            before,
            "no copy during the drag"
        );
        rt.pointer_up(x + to * m.width, y + 0.5 * m.height);
        rt.frame(0.0);
    };
    board.set_text("owner sentinel");
    let (x, y, m) = grid(&rt);
    rt.pointer_down(x + 2.5 * m.width, y + 0.5 * m.height);
    rt.pointer_up(x + 2.5 * m.width, y + 0.5 * m.height);
    rt.frame(0.0);
    assert_eq!(
        board.text().unwrap().as_str(),
        "owner sentinel",
        "a click copies nothing"
    );
    drag(&mut rt, 12.5, 18.5);
    assert_eq!(
        board.text().unwrap().as_str(),
        "owner sentinel",
        "selecting only empty cells leaves the clipboard unchanged"
    );
    drag(&mut rt, 0.5, 8.0);
    assert_eq!(
        board.text().unwrap().as_str(),
        "Olá 世界",
        "a finished nonempty selection copies with no saved preference"
    );

    rt.update(|s, tx| {
        s.open_settings(tx);
        s.settings_query.replace_text("clipboard");
    });
    rt.frame(0.0);
    let click = |rt: &mut rune::Runtime<State>, key: &str| {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    };
    crate::desktop::settings::choose(&mut rt, "terminal-right-click", "Paste");
    click(&mut rt, "copy-on-selection");
    let saved = Config::load(root.path()).unwrap();
    assert_eq!(
        saved.terminal_clipboard.right_click,
        config::TerminalRightClick::Paste
    );
    assert!(!saved.terminal_clipboard.copy_on_selection);
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(json["terminal_clipboard"]["copy_on_selection"], false);
    rt.update(|s, tx| s.dispatch(AppAction::ShowTerminal, tx));
    rt.frame(0.0);
    board.set_text("owner sentinel");
    drag(&mut rt, 0.5, 8.0);
    assert_eq!(
        board.text().unwrap().as_str(),
        "owner sentinel",
        "a saved false is honored"
    );

    // A fresh start reads the saved false back from settings.json.
    let fresh = State::with_root(root.path().into());
    assert!(!fresh.config.terminal_clipboard.copy_on_selection);
    drop(fresh);

    rt.update(|s, tx| {
        s.open_settings(tx);
        s.settings_query.replace_text("clipboard");
    });
    rt.frame(0.0);
    crate::desktop::settings::restore_tab_defaults(&mut rt, "general");
    assert_eq!(
        Config::load(root.path()).unwrap().terminal_clipboard,
        config::TerminalClipboardPreferences::default()
    );
    assert!(
        Config::load(root.path())
            .unwrap()
            .terminal_clipboard
            .copy_on_selection
    );
}

#[test]
fn terminal_clipboard_actions_reach_real_pty_and_overlay_keeps_input_out() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.directory = root.path().into();
    state.shell = crate::test_support::shell_binary();
    state.new_tab();
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    let identity = state.active().unwrap().id();
    state.active().unwrap().send(b"stty raw -echo; printf '\\033[?1hREADY'; head -c 16 | od -An -tx1 | tr -s ' '; stty sane; printf '\\033[?1l\\nPASTE_DONE\\n'\r".to_vec()).unwrap();
    state
        .active()
        .unwrap()
        .wait_for("READY", std::time::Duration::from_secs(15))
        .unwrap();
    let mut rt = runtime_for(state);
    let board = rune::clipboard::memory();
    board.set_text("Olá 世界 🦀");
    rt.use_clipboard(&board);
    let (x, y) = rt.find("terminal").unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(0.0);
    press(&mut rt, Key::Text("BAD".into()), Default::default());
    rt.paste("BAD");
    rt.frame(0.0);
    rt.resolve_popup(Some("paste-terminal")).unwrap();
    rt.frame(0.0);
    rt.state
        .active()
        .unwrap()
        .wait_for("PASTE_DONE", std::time::Duration::from_secs(15))
        .unwrap();
    let output = rt.state.with_screen(vt100::Screen::contents);
    assert!(
        output.contains("4f 6c c3 a1 20 e4 b8 96 e7 95 8c 20 f0 9f a6 80"),
        "{output}"
    );
    press(
        &mut rt,
        Key::Command('c'),
        rune::Mods {
            ctrl: true,
            ..Default::default()
        },
    );
    open_palette(&mut rt);
    press(
        &mut rt,
        Key::Text("Terminal: Paste".into()),
        Default::default(),
    );
    press(&mut rt, Key::Escape, Default::default());
    rt.update(|s, _| {
        s.save_config(|c| c.terminal_clipboard.right_click = config::TerminalRightClick::Paste);
    });
    assert_eq!(rt.state.active().unwrap().id(), identity);
    rt.settle_all();
    rt.frame(0.0);
    board.set_text("printf 'OPTIONAL_%s\\n' RIGHT_CLICK_OK\n");
    let (x, y) = rt.find("terminal").unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(0.0);
    assert!(rt.popup_menu().is_none());
    assert!(!rt.state.application_overlay_open(), "overlay remains");
    assert_eq!(
        rt.state.config.terminal_clipboard.right_click,
        config::TerminalRightClick::Paste
    );
    assert!(rt.state.workspace.terminal_focus());
    rt.update(|_, tx| assert!(tx.focused().is_none(), "focused {:?}", tx.focused()));
    rt.frame(0.1);
    rt.state
        .active()
        .unwrap()
        .wait_for("OPTIONAL_RIGHT_CLICK_OK", std::time::Duration::from_secs(5))
        .unwrap();
    for (via_palette, command, expected) in [
        (
            true,
            "printf 'PALETTE_%s\\n' PASTE_OK\n",
            "PALETTE_PASTE_OK",
        ),
        (
            false,
            "printf 'SHORTCUT_%s\\n' PASTE_OK\n",
            "SHORTCUT_PASTE_OK",
        ),
    ] {
        board.set_text(command);
        if via_palette {
            open_palette(&mut rt);
            press(
                &mut rt,
                Key::Text("Terminal: Paste".into()),
                Default::default(),
            );
            press(&mut rt, Key::Enter, Default::default());
        } else {
            rt.settle_all();
            rt.frame(0.0);
            press(
                &mut rt,
                Key::Command('v'),
                rune::Mods {
                    ctrl: !cfg!(target_os = "macos"),
                    cmd: true,
                    shift: !cfg!(target_os = "macos"),
                    ..Default::default()
                },
            );
        }
        rt.frame(0.0);
        rt.state
            .active()
            .unwrap()
            .wait_for(expected, std::time::Duration::from_secs(5))
            .unwrap();
    }
    rt.state
        .active()
        .unwrap()
        .send(b"sleep 30\r".to_vec())
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    press(
        &mut rt,
        Key::Command('c'),
        rune::Mods {
            ctrl: true,
            ..Default::default()
        },
    );
    rt.state
        .active()
        .unwrap()
        .send(b"printf 'INTERRUPT_%s\\n' OK\r".to_vec())
        .unwrap();
    rt.state
        .active()
        .unwrap()
        .wait_for("INTERRUPT_OK", std::time::Duration::from_secs(5))
        .unwrap();
}
