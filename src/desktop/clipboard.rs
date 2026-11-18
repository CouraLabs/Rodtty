use super::*;

pub(super) fn terminal_menu(s: &mut State, point: rune::PointerPress, tx: &mut Tx) {
    if s.application_overlay_open() {
        return;
    }
    s.supersede_pending_preview();
    s.workspace.set_terminal_focus(true);
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

/// Where a terminal paste was aimed when it was asked for. The clipboard is
/// read off the event thread; if the active session changed or terminal focus
/// moved before the content arrived, the paste is dropped rather than typed
/// into whatever holds focus by then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PasteTarget {
    session: usize,
    focus: u64,
}

/// What a terminal paste did with the clipboard's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PasteOutcome {
    /// This many file or folder paths, quoted, went to the session.
    Files(usize),
    Text,
    /// The target session or terminal focus changed before the read returned.
    Cancelled,
    /// The clipboard held only an image, which a terminal cannot take.
    ImageOnly,
    /// Nothing the terminal can use.
    Empty,
}

impl State {
    fn terminal_paste_target(&self) -> Option<PasteTarget> {
        Some(PasteTarget {
            session: self.active()?.id(),
            focus: self.workspace.focus_generation(),
        })
    }

    /// The one terminal paste handler. The keyboard shortcut, the palette and
    /// menu action, and the terminal context menu all dispatch
    /// `AppAction::PasteTerminal`, which lands here. The read targets the
    /// terminal explicitly, so a palette's exiting modal cannot capture it.
    pub(super) fn request_terminal_paste(&mut self, tx: &mut Tx) {
        let Some(target) = self.terminal_paste_target() else {
            return;
        };
        tx.read_clipboard(move |s: &mut State, content, tx| {
            s.finish_terminal_paste(target, content, tx);
        });
    }

    /// Deliver what the clipboard read found to the session `target` named.
    /// Files and folders win over the text beside them (Finder also offers
    /// their bare names as text): each becomes a quoted absolute path, joined
    /// by spaces. Nothing is submitted; the shell sees a bracketed paste when
    /// the application asked for one.
    pub(super) fn finish_terminal_paste(
        &mut self,
        target: PasteTarget,
        content: &rune::ClipboardContent,
        tx: &Tx,
    ) -> PasteOutcome {
        let (_, visible, _) = workspace::pane_visibility(self, self.workspace.body_width);
        if !visible
            || !self.workspace.terminal_focus()
            || tx.focused().is_some()
            || self.application_overlay_open()
            || self.terminal_paste_target() != Some(target)
        {
            return PasteOutcome::Cancelled;
        }
        if !content.files().is_empty() {
            let (text, pasted, skipped) = paste_paths(content.files());
            if skipped > 0 {
                self.message = format!(
                    "Skipped {skipped} clipboard {} that {} not an absolute Unicode path",
                    if skipped == 1 { "item" } else { "items" },
                    if skipped == 1 { "is" } else { "are" },
                );
            }
            if pasted == 0 {
                return PasteOutcome::Empty;
            }
            self.paste(&text);
            return PasteOutcome::Files(pasted);
        }
        if !content.text().is_empty() {
            self.paste(content.text());
            return PasteOutcome::Text;
        }
        if content.image().is_some() {
            self.message =
                "The clipboard holds an image; the terminal pastes only text and files".into();
            return PasteOutcome::ImageOnly;
        }
        PasteOutcome::Empty
    }
}

/// The clipboard's files as shell words, separated by spaces, with how many
/// went in and how many were skipped. A path that is not absolute or not valid
/// Unicode is skipped: Rodtty's shell reads Unicode input, and a relative path
/// from the clipboard has no directory to be relative to.
fn paste_paths(paths: &[std::path::PathBuf]) -> (String, usize, usize) {
    let words: Vec<String> = paths
        .iter()
        .filter(|path| path.is_absolute())
        .filter_map(|path| quote_path(path))
        .collect();
    let skipped = paths.len() - words.len();
    (words.join(" "), words.len(), skipped)
}

/// One path as a single word for Rodtty's Bash-style shell. Single quotes keep
/// every character literal, with an embedded `'` written `'\''`, as
/// `profile::quote` does. A path with control characters uses ANSI-C `$'…'`
/// quoting instead, so a newline or escape in a file name stays in the word
/// and never reaches the terminal raw, where it could submit or be stripped.
fn quote_path(path: &std::path::Path) -> Option<String> {
    let text = path.to_str()?;
    if !text.chars().any(char::is_control) {
        return Some(format!("'{}'", text.replace('\'', "'\\''")));
    }
    let mut word = String::from("$'");
    for c in text.chars() {
        match c {
            '\\' => word.push_str("\\\\"),
            '\'' => word.push_str("\\'"),
            '\n' => word.push_str("\\n"),
            '\t' => word.push_str("\\t"),
            c if c.is_control() => {
                let mut bytes = [0; 4];
                for byte in c.encode_utf8(&mut bytes).bytes() {
                    word.push_str(&format!("\\x{byte:02x}"));
                }
            }
            c => word.push(c),
        }
    }
    word.push('\'');
    Some(word)
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
        tx.blur();
    });
    let id = id_rx.recv_timeout(Duration::from_secs(10))?;
    std::fs::write(report.with_extension("ready.json"), b"{\"phase\":\"copy\"}")?;
    let copy = report.with_extension("copy.json");
    let start = Instant::now();
    while !copy.exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(25));
    }
    anyhow::ensure!(copy.exists(), "external native activation timed out");
    handle.update(|s, tx| s.dispatch(AppAction::CopyTerminal, tx));
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
        s.active().unwrap().send(b"stty raw -echo; printf '\\033[?1h'; head -c 16 | od -An -tx1 | tr -s ' '; stty sane; printf '\\033[?1l\\nCLIPBOARD_PASTE_DONE\\n'\r".to_vec()).unwrap();
    });
    wait_app(handle, |s| s.with_screen(vt100::Screen::application_cursor))?;
    handle.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
    wait_app(handle, |s| {
        let output = s.with_screen(vt100::Screen::contents);
        output.contains("4f 6c c3 a1 20 e4 b8 96 e7 95 8c 20 f0 9f 90 9a")
            && output.contains("CLIPBOARD_PASTE_DONE")
    })?;
    wait_app(handle, move |s| {
        s.active().unwrap().id() == id && s.workspace.terminal_focus()
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
        click(&mut rt, "terminal-right-click-paste");
        click(&mut rt, "copy-on-selection");
        let saved = Config::load(root.path()).unwrap();
        assert_eq!(
            saved.terminal_clipboard.right_click,
            config::TerminalRightClick::Paste
        );
        assert!(!saved.terminal_clipboard.copy_on_selection);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
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
        click(&mut rt, "reset-terminal-right-click");
        assert!(
            !Config::load(root.path())
                .unwrap()
                .terminal_clipboard
                .copy_on_selection
        );
        click(&mut rt, "reset-copy-on-selection");
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
}

#[cfg(test)]
mod selection_threshold_tests {
    use super::super::terminal_view::SELECTION_THRESHOLD;
    use super::super::tests::runtime_for;
    use super::*;

    fn grid_origin(rt: &rune::Runtime<State>) -> (f32, f32) {
        rt.find("terminal").expect("terminal grid")
    }

    /// A press that never travels is a click: it must not highlight a cell.
    #[test]
    fn a_click_selects_nothing() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"hello world\r\n");
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        let (x, y) = grid_origin(&rt);
        rt.pointer_down(x + 20.0, y + 10.0);
        rt.pointer_up(x + 20.0, y + 10.0);
        rt.frame(0.0);
        assert!(
            rt.state.selection.is_none(),
            "a click leaves no selection: {:?}",
            rt.state.selection
        );
    }

    /// Contact wobble under the threshold is still a click.
    #[test]
    fn a_press_that_barely_drifts_selects_nothing() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"hello world\r\n");
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        let (x, y) = grid_origin(&rt);
        let start = (x + 20.0, y + 10.0);
        rt.pointer_down(start.0, start.1);
        // Less than the threshold, in both axes, over several move events.
        for step in 1..=3 {
            let d = step as f32;
            rt.pointer_move(start.0 + d, start.1);
            rt.frame(0.0);
        }
        rt.pointer_up(start.0 + 3.0, start.1);
        rt.frame(0.0);
        assert!(
            rt.state.selection.is_none(),
            "drift under {SELECTION_THRESHOLD} pt is a click: {:?}",
            rt.state.selection
        );
    }

    /// Past the threshold the drag selects, and the range spans the travel.
    #[test]
    fn a_real_drag_selects_and_keeps_the_range() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"hello world\r\n");
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        let (x, y) = grid_origin(&rt);
        let start = (x + 20.0, y + 10.0);
        rt.pointer_down(start.0, start.1);
        // Well past the threshold: several cells of travel.
        rt.pointer_move(start.0 + 40.0, start.1);
        rt.frame(0.0);
        assert!(
            rt.state.selection.is_some(),
            "a drag past {SELECTION_THRESHOLD} pt selects"
        );
        let (a, b) = rt.state.selection.unwrap();
        assert_ne!(a, b, "the range covers the travel, not a single cell");
        rt.pointer_up(start.0 + 40.0, start.1);
        rt.frame(0.0);
        assert_eq!(
            rt.state.selection,
            Some((a, b)),
            "releasing the drag keeps the selection it made"
        );
    }

    /// A press on the grid drops a selection the owner left behind.
    #[test]
    fn a_press_on_the_grid_clears_a_previous_selection() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"hello world\r\n");
        state.preview = Some(parser.screen().clone());
        state.selection = Some(((0, 0), (0, 5)));
        let mut rt = runtime_for(state);
        let (x, y) = grid_origin(&rt);
        rt.pointer_down(x + 40.0, y + 30.0);
        rt.pointer_up(x + 40.0, y + 30.0);
        rt.frame(0.0);
        assert!(
            rt.state.selection.is_none(),
            "the old highlight is gone: {:?}",
            rt.state.selection
        );
    }
}

#[cfg(test)]
mod paste_tests {
    use super::super::tests::{open_palette, press, runtime_for};
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn shell_state(root: &Path) -> State {
        let mut state = State::with_root(root.into());
        state.directory = root.into();
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state
    }

    fn primary_shift(key: char) -> (Key, rune::Mods) {
        (
            Key::Command(key),
            rune::Mods {
                ctrl: !cfg!(target_os = "macos"),
                cmd: true,
                shift: !cfg!(target_os = "macos"),
                ..Default::default()
            },
        )
    }

    /// A PNG header is all `ClipboardImage` checks; no pixels are decoded.
    fn image() -> rune::ClipboardImage {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(1u32.to_be_bytes());
        png.extend(1u32.to_be_bytes());
        rune::ClipboardImage::from_png(png).unwrap()
    }

    /// Fixture paths that need quoting: spaces, single quotes, non-ASCII, and a folder.
    fn fixtures(root: &Path) -> Vec<PathBuf> {
        let paths = vec![
            root.join("with space.txt"),
            root.join("it's here.txt"),
            root.join("Olá 世界 🦀.txt"),
            root.join("a folder"),
        ];
        for file in &paths[..3] {
            std::fs::write(file, b"x").unwrap();
        }
        std::fs::create_dir(&paths[3]).unwrap();
        paths
    }

    #[test]
    fn paths_quote_as_single_bash_words() {
        let quote = |p: &str| quote_path(Path::new(p)).unwrap();
        assert_eq!(quote("/tmp/with space"), "'/tmp/with space'");
        assert_eq!(quote("/tmp/it's"), r"'/tmp/it'\''s'");
        assert_eq!(quote("/tmp/Olá 世界 🦀"), "'/tmp/Olá 世界 🦀'");
        assert_eq!(quote(r"C:\Users\o'n"), r"'C:\Users\o'\''n'");
        assert_eq!(
            quote("/tmp/line\nbreak\t'\\\x1b"),
            r"$'/tmp/line\nbreak\t\'\\\x1b'",
            "control characters never reach the terminal raw"
        );
        let (text, pasted, skipped) = paste_paths(&[
            PathBuf::from("/a b"),
            PathBuf::from("relative"),
            PathBuf::from("/c"),
        ]);
        assert_eq!((text.as_str(), pasted, skipped), ("'/a b' '/c'", 2, 1));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let invalid = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff"));
            assert_eq!(quote_path(invalid), None, "non-UTF-8 paths are skipped");
        }
    }

    /// File lists from each entry point reach a real PTY as one bracketed paste of
    /// quoted absolute paths with nothing submitted; image-only clipboards send
    /// nothing; the quoted words parse back to the same paths in the real shell.
    #[test]
    fn every_entry_point_pastes_file_lists_as_quoted_paths_without_submitting() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = shell_state(root.path());
        state.new_tab();
        let session = state.active().unwrap();
        session
            .wait_for("rodtty:", Duration::from_secs(15))
            .unwrap();
        let paths = fixtures(root.path());
        let words = paste_paths(&paths).0;
        let mut rt = runtime_for(state);
        let board = rune::clipboard::memory();
        rt.use_clipboard(&board);
        type Entry = (&'static str, fn(&mut rune::Runtime<State>));
        let entries: [Entry; 4] = [
            ("shortcut", |rt| {
                let (key, mods) = primary_shift('v');
                press(rt, key, mods);
            }),
            ("palette", |rt| {
                open_palette(rt);
                press(rt, Key::Text("Terminal: Paste".into()), Default::default());
                press(rt, Key::Enter, Default::default());
            }),
            ("action", |rt| {
                rt.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
                rt.frame(0.0);
            }),
            ("context-menu", |rt| {
                let (x, y) = rt.find("terminal").unwrap();
                rt.secondary_down(x, y);
                rt.secondary_up(x, y);
                rt.frame(0.0);
                rt.resolve_popup(Some("paste-terminal")).unwrap();
                rt.frame(0.0);
            }),
        ];
        let image_only = entries.len();
        for (round, (name, paste)) in entries.iter().chain([&entries[0]]).enumerate() {
            let expected = if round == image_only {
                board.set_content(rune::ClipboardContent::default().with_image(image()));
                b"Z".to_vec()
            } else {
                // Finder offers the bare names as text beside the files; files win.
                board.set_content(
                    rune::ClipboardContent::new("with space.txt\rit's here.txt")
                        .with_files(paths.clone()),
                );
                format!("\x1b[200~{words}\x1b[201~Z").into_bytes()
            };
            let capture = root.path().join(format!("capture-{round}"));
            rt.settle_all();
            rt.frame(0.0);
            rt.state
                .active()
                .unwrap()
                .send(
                    format!(
                        "stty raw -echo; printf '\\033[?2004hREADY_%s\\n' {round}; head -c {} > {}; printf '\\033[?2004l'; stty sane; printf 'CAPTURED_%s\\n' {round}\r",
                        expected.len(),
                        crate::profile::quote(&capture).unwrap(),
                    )
                    .into_bytes(),
                )
                .unwrap();
            rt.state
                .active()
                .unwrap()
                .wait_for(&format!("READY_{round}"), Duration::from_secs(15))
                .unwrap();
            assert!(rt.state.with_screen(vt100::Screen::bracketed_paste));
            rt.state.message.clear();
            paste(&mut rt);
            // The memory clipboard answers at the end of a frame; the next one
            // delivers the answer to the handler.
            rt.frame(0.0);
            rt.frame(0.0);
            // The sentinel follows the paste through the same ordered input queue,
            // so a submitted newline would take its place in the capture.
            rt.state.active().unwrap().send(b"Z".to_vec()).unwrap();
            rt.state
                .active()
                .unwrap()
                .wait_for(&format!("CAPTURED_{round}"), Duration::from_secs(15))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                String::from_utf8_lossy(&std::fs::read(&capture).unwrap()),
                String::from_utf8_lossy(&expected),
                "{name} (round {round})"
            );
            if round == image_only {
                assert!(rt.state.message.contains("image"), "{}", rt.state.message);
            }
        }
        // The pasted words are the paths themselves to the real shell.
        let listing = root.path().join("listing");
        board.set_content(rune::ClipboardContent::default().with_files(paths.clone()));
        rt.state
            .active()
            .unwrap()
            .send(b"printf '<%s>\\n' ".to_vec())
            .unwrap();
        rt.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
        rt.frame(0.0);
        rt.frame(0.0);
        rt.state
            .active()
            .unwrap()
            .send(
                format!(
                    " > {}; printf 'LISTED_%s\\n' OK\r",
                    crate::profile::quote(&listing).unwrap()
                )
                .into_bytes(),
            )
            .unwrap();
        rt.state
            .active()
            .unwrap()
            .wait_for("LISTED_OK", Duration::from_secs(15))
            .unwrap();
        let expected: String = paths
            .iter()
            .map(|p| format!("<{}>\n", p.display()))
            .collect();
        assert_eq!(std::fs::read_to_string(listing).unwrap(), expected);
    }

    #[test]
    fn a_pending_paste_is_dropped_when_its_session_or_focus_moves() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = shell_state(root.path());
        state.new_tab();
        state.new_tab();
        assert_eq!(state.sessions.len(), 2);
        let mut rt = runtime_for(state);
        let files = rune::ClipboardContent::default().with_files([PathBuf::from("/tmp/a b")]);
        let text = rune::ClipboardContent::new("echo hi");
        let target = |rt: &mut rune::Runtime<State>| {
            let mut target = None;
            rt.update(|s, _| target = s.terminal_paste_target());
            target.unwrap()
        };
        let finish = |rt: &mut rune::Runtime<State>, t, content: &rune::ClipboardContent| {
            let mut outcome = None;
            rt.update(|s, tx| outcome = Some(s.finish_terminal_paste(t, content, tx)));
            outcome.unwrap()
        };
        let activate = |rt: &mut rune::Runtime<State>, index| {
            rt.update(|s, tx| s.dispatch(AppAction::ActivateTerminal(index), tx));
            rt.frame(0.0);
        };
        // Unchanged target: files and text both arrive.
        let t = target(&mut rt);
        assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Files(1));
        assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Text);
        // Tab switch to another session.
        let t = target(&mut rt);
        activate(&mut rt, 0);
        assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Cancelled);
        // Away and back to the same session still drops it.
        let t = target(&mut rt);
        activate(&mut rt, 1);
        activate(&mut rt, 0);
        assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
        // Focus leaves the terminal...
        let t = target(&mut rt);
        rt.update(|s, _| s.workspace.set_terminal_focus(false));
        assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Cancelled);
        // ...or leaves and returns before the read completes.
        rt.update(|s, _| s.workspace.set_terminal_focus(true));
        let t = target(&mut rt);
        rt.update(|s, _| {
            s.workspace.set_terminal_focus(false);
            s.workspace.set_terminal_focus(true);
        });
        assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
        // A text field took focus while the terminal kept its flag.
        let t = target(&mut rt);
        rt.update(|s, tx| s.dispatch(AppAction::FindTerminal, tx));
        rt.settle_all();
        rt.frame(0.0);
        assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
        // The session closed.
        rt.update(|s, tx| {
            s.terminal_search.open = false;
            tx.blur();
        });
        activate(&mut rt, 1);
        let t = target(&mut rt);
        rt.update(|s, _| {
            s.sessions.remove(1);
            s.active = 0;
        });
        assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
        // Image-only and empty clipboards do nothing.
        let t = target(&mut rt);
        assert_eq!(
            finish(
                &mut rt,
                t,
                &rune::ClipboardContent::default().with_image(image())
            ),
            PasteOutcome::ImageOnly
        );
        assert_eq!(
            finish(&mut rt, t, &rune::ClipboardContent::default()),
            PasteOutcome::Empty
        );
    }
}
