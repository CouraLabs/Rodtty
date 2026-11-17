use super::*;

pub(super) fn terminal_menu(s: &mut State, point: rune::PointerPress, tx: &mut Tx) {
    if s.application_overlay_open() {
        return;
    }
    s.supersede_pending_preview();
    s.workspace.terminal_focus = true;
    tx.blur();
    if s.config.terminal_clipboard.right_click == config::TerminalRightClick::Paste {
        s.dispatch(AppAction::PasteTerminal, tx);
        return;
    }
    let menu = rune::PopupMenu::new()
        .item(
            rune::PopupItem::new(
                "copy-terminal",
                s.action_help(AppAction::CopyTerminal, "Copy selection"),
            )
            .enabled(s.action_disabled(AppAction::CopyTerminal).is_none()),
        )
        .item(
            rune::PopupItem::new(
                "paste-terminal",
                s.action_help(AppAction::PasteTerminal, "Paste"),
            )
            .enabled(s.action_disabled(AppAction::PasteTerminal).is_none()),
        );
    s.terminal_context_open = true;
    tx.popup_menu(menu, point, |s: &mut State, chosen, tx| {
        s.terminal_context_open = false;
        match chosen {
            Some("copy-terminal") => s.dispatch(AppAction::CopyTerminal, tx),
            Some("paste-terminal") => s.dispatch(AppAction::PasteTerminal, tx),
            _ => {}
        }
    });
}

/// Exercise the actual asynchronous host clipboard worker and a real terminal session.
/// The external native harness preserves arbitrary offered MIME data, verifies the copy,
/// and supplies only the public fixture below; owner clipboard contents never enter reports.
pub(super) fn native_check(
    handle: &rune::AppHandle<State>,
    report: &std::path::Path,
) -> Result<serde_json::Value> {
    use std::time::{Duration, Instant};
    handle.update(|s, _| {
        s.active()
            .unwrap()
            .send("printf '\\033[2J\\033[HOlá 世界 🦀'\r".as_bytes().to_vec())
            .unwrap();
    });
    wait_app(handle, |s| {
        s.with_screen(vt100::Screen::contents)
            .starts_with("Olá 世界 🦀")
    })?;
    let (id_tx, id_rx) = std::sync::mpsc::channel();
    handle.update(move |s, tx| {
        let _ = id_tx.send(s.active().unwrap().id());
        s.selection = Some(((0, 0), (0, 11)));
        s.dispatch(AppAction::CopyTerminal, tx);
    });
    let id = id_rx.recv_timeout(Duration::from_secs(10))?;
    std::fs::write(report.with_extension("ready.json"), b"{\"phase\":\"copy\"}")?;
    let input = report.with_extension("input.json");
    let start = Instant::now();
    while !input.exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(25));
    }
    let external: serde_json::Value = serde_json::from_slice(
        &std::fs::read(input).context("external clipboard verification timed out")?,
    )?;
    anyhow::ensure!(
        external["copy_verified"] == true,
        "external copy verification failed"
    );
    handle.update(|s,_| {
        s.active().unwrap().send(b"stty raw -echo; printf '\\033[?1h'; head -c 16 | od -An -tx1; stty sane; printf '\\033[?1l\\nCLIPBOARD_PASTE_DONE\\n'\r".to_vec()).unwrap();
    });
    wait_app(handle, |s| s.with_screen(vt100::Screen::application_cursor))?;
    handle.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
    wait_app(handle, |s| {
        let output = s.with_screen(vt100::Screen::contents);
        output.contains("4f 6c c3 a1 20 e4 b8 96 e7 95 8c 20 f0 9f a6 80")
            && output.contains("CLIPBOARD_PASTE_DONE")
    })?;
    wait_app(handle, move |s| {
        s.active().unwrap().id() == id && s.workspace.terminal_focus
    })?;
    Ok(
        serde_json::json!({"passed":true,"native_copy_exact_unicode":true,"native_paste_real_pty_exact_utf8":true,"same_session":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{open_palette, press, runtime_for};
    use super::*;

    #[test]
    fn terminal_right_click_offers_shared_copy_and_blocks_input_until_dismissed() {
        let root = tempfile::tempdir().unwrap();
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
    fn clipboard_preferences_persist_reset_and_copy_only_completed_selections() {
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(28, 100, 0);
        parser.process("Olá 世界".as_bytes());
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        let board = rune::clipboard::memory();
        board.set_text("owner sentinel");
        rt.use_clipboard(&board);
        let select = |rt: &mut rune::Runtime<State>| {
            let [x, y, _, _] = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("terminal"))
                .unwrap()
                .rect;
            let m = fonts::Metrics::measure();
            rt.pointer_down(x + 0.5 * m.width, y + 0.5 * m.height);
            rt.pointer_move(x + 8.0 * m.width, y + 0.5 * m.height);
            assert_eq!(board.text().unwrap().as_str(), "owner sentinel");
            rt.pointer_up(x + 8.0 * m.width, y + 0.5 * m.height);
            rt.frame(0.0);
        };
        select(&mut rt);
        assert_eq!(board.text().unwrap().as_str(), "owner sentinel");
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
        click(&mut rt, "terminal-right-click-paste");
        click(&mut rt, "copy-on-selection");
        let saved = Config::load(root.path()).unwrap();
        assert_eq!(
            saved.terminal_clipboard.right_click,
            config::TerminalRightClick::Paste
        );
        assert!(saved.terminal_clipboard.copy_on_selection);
        rt.update(|s, tx| s.dispatch(AppAction::ShowTerminal, tx));
        rt.frame(0.0);
        select(&mut rt);
        assert_eq!(board.text().unwrap().as_str(), "Olá 世界");
        rt.update(|s, tx| {
            s.open_settings(tx);
            s.settings_query.replace_text("clipboard");
        });
        rt.frame(0.0);
        click(&mut rt, "reset-terminal-right-click");
        assert!(
            Config::load(root.path())
                .unwrap()
                .terminal_clipboard
                .copy_on_selection
        );
        click(&mut rt, "reset-copy-on-selection");
        assert_eq!(
            Config::load(root.path()).unwrap().terminal_clipboard,
            config::TerminalClipboardPreferences::default()
        );
    }

    #[test]
    fn terminal_clipboard_actions_reach_real_pty_and_overlay_keeps_input_out() {
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
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
        let identity = state.active().unwrap().id();
        state.active().unwrap().send(b"stty raw -echo; printf '\\033[?1hREADY'; head -c 16 | od -An -tx1; stty sane; printf '\\033[?1l\\nPASTE_DONE\\n'\r".to_vec()).unwrap();
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
        board.set_text("printf 'OPTIONAL_RIGHT_CLICK_OK\\n'\n");
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
        assert!(rt.state.workspace.terminal_focus);
        rt.update(|_, tx| assert!(tx.focused().is_none(), "focused {:?}", tx.focused()));
        rt.frame(0.1);
        rt.state
            .active()
            .unwrap()
            .wait_for("OPTIONAL_RIGHT_CLICK_OK", std::time::Duration::from_secs(5))
            .unwrap();
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
            .send(b"printf 'INTERRUPT_OK\\n'\r".to_vec())
            .unwrap();
        rt.state
            .active()
            .unwrap()
            .wait_for("INTERRUPT_OK", std::time::Duration::from_secs(5))
            .unwrap();
    }
}
