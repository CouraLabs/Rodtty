use super::*;

fn runtime() -> rune::Runtime<State> {
    let mut state = State::init();
    state.workspace.layout.explorer = true;
    runtime_for(state)
}

pub(super) fn runtime_for(state: State) -> rune::Runtime<State> {
    let mut runtime = rune::Runtime::new(state, view, (1200.0, 700.0), rune::Env::new(true, 1.0));
    runtime.on_key = Some(on_key);
    runtime.frame(0.0);
    runtime
}

/// A runtime pinned to one platform's window chrome, for the tests that
/// exercise app-drawn or host-drawn controls regardless of the host they
/// run on. `Runtime::new` takes a plain function pointer, so the
/// placement is selected here rather than captured.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn runtime_for_frame(
    state: State,
    placement: window::frame::Placement,
) -> rune::Runtime<State> {
    fn hosted(state: &State, cx: &mut Cx<State>) -> El {
        view_with_frame(state, cx, window::frame::Placement::Hosted)
    }
    fn trailing(state: &State, cx: &mut Cx<State>) -> El {
        view_with_frame(state, cx, window::frame::Placement::Trailing)
    }
    fn native(state: &State, cx: &mut Cx<State>) -> El {
        view_with_frame(state, cx, window::frame::Placement::Native)
    }
    let view = match placement {
        window::frame::Placement::Hosted => hosted as fn(&State, &mut Cx<State>) -> El,
        window::frame::Placement::Trailing => trailing,
        window::frame::Placement::Native => native,
    };
    let mut runtime = rune::Runtime::new(state, view, (1200.0, 700.0), rune::Env::new(true, 1.0));
    runtime.on_key = Some(on_key);
    runtime.frame(0.0);
    runtime
}

pub(super) fn press(runtime: &mut rune::Runtime<State>, key: Key, mods: rune::Mods) {
    runtime.key(KeyEvent { key, mods });
    runtime.frame(0.0);
}

pub(super) fn open_palette(runtime: &mut rune::Runtime<State>) {
    press(
        runtime,
        Key::Command('p'),
        rune::Mods {
            ctrl: !cfg!(target_os = "macos"),
            cmd: true,
            shift: true,
            ..Default::default()
        },
    );
}

fn click(runtime: &mut rune::Runtime<State>, key: &str) {
    runtime.settle_all();
    runtime.frame(0.0);
    let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
}

fn close_terminal_from(runtime: &mut rune::Runtime<State>, entry: &str) {
    runtime.settle_all();
    runtime.frame(0.0);
    match entry {
        "button" => click(runtime, "terminal-close-0"),
        "middle" => {
            let (x, y) = runtime.find("tab-0").unwrap();
            runtime.button_down(x, y, rune::PointerButton::Middle);
            runtime.button_up(x, y, rune::PointerButton::Middle);
            runtime.frame(0.0);
        }
        "context" => {
            let (x, y) = runtime.find("tab-0").unwrap();
            runtime.secondary_down(x, y);
            runtime.secondary_up(x, y);
            runtime.frame(0.0);
            runtime.resolve_popup(Some("close")).unwrap();
            runtime.frame(0.0);
        }
        "remapped" => press(runtime, Key::Function(9), Default::default()),
        "menu" => {
            click(runtime, "workspace-menu");
            click(runtime, "close-tab");
        }
        "palette" => {
            open_palette(runtime);
            press(runtime, Key::Text("End Session".into()), Default::default());
            assert!(
                runtime
                    .elements()
                    .iter()
                    .any(|e| e.text.as_deref() == Some("F9"))
            );
            press(runtime, Key::Enter, Default::default());
        }
        _ => unreachable!(),
    }
}

#[test]
fn every_terminal_close_entry_point_honors_live_idle_and_running_policy_matrix() {
    use crate::config::TerminalClosePolicy as Policy;
    let shell = crate::test_support::shell_binary().unwrap();
    for policy in [
        Policy::ConfirmRunning,
        Policy::AlwaysConfirm,
        Policy::CloseImmediately,
    ] {
        for running in [false, true] {
            for entry in [
                // End Session has no default binding, as in VS Code;
                // "remapped" presses one the owner assigned.
                "button", "middle", "context", "remapped", "menu", "palette",
            ] {
                let root = crate::test_support::tempdir().unwrap();
                let mut state = State::with_root(root.path().into());
                state.shell = Some(shell.clone());
                state.start_tab(root.path().into());
                state
                    .active()
                    .unwrap()
                    .wait_for("rodtty:", std::time::Duration::from_secs(15))
                    .unwrap();
                state.save_config(|c| {
                    c.behavior.terminal_tab_close = policy;
                    if matches!(entry, "remapped" | "palette") {
                        c.shortcuts.insert("end-terminal".into(), "F9".into());
                    }
                });
                if running {
                    state.send(b"read answer\r".to_vec());
                    let start = std::time::Instant::now();
                    while !state.active().unwrap().has_running_commands() {
                        assert!(start.elapsed().as_secs() < 10);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                let id = state.active().unwrap().id();
                let mut rt = runtime_for(state);
                close_terminal_from(&mut rt, entry);
                let confirms = policy == Policy::AlwaysConfirm
                    || (policy == Policy::ConfirmRunning && running);
                if confirms {
                    assert!(
                        rt.find("confirmation").is_some(),
                        "{policy:?} running={running} {entry}"
                    );
                    press(&mut rt, Key::Escape, Default::default());
                    assert_eq!(rt.state.active().unwrap().id(), id);
                    close_terminal_from(&mut rt, entry);
                    click(&mut rt, "confirm");
                }
                assert!(
                    rt.state.sessions.is_empty(),
                    "{policy:?} running={running} {entry}"
                );
            }
        }
    }
}

#[test]
fn startup_folder_setting_chooses_the_first_terminal_folder() {
    let root = crate::test_support::tempdir().unwrap();
    let folder = crate::test_support::tempdir().unwrap();
    let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
    assert_eq!(State::with_root(root.path().into()).directory, home);
    let mut config = Config::default();
    config.behavior.startup_directory = config::StartupDirectory::Specified;
    config.behavior.startup_specified_directory = folder.path().into();
    config.save(root.path()).unwrap();
    let state = State::with_root(root.path().into());
    assert_eq!(state.directory, folder.path());
    assert!(state.message.is_empty());
    config.behavior.startup_specified_directory = folder.path().join("missing");
    config.save(root.path()).unwrap();
    let state = State::with_root(root.path().into());
    assert_eq!(state.directory, home);
    assert!(state.message.contains("Startup folder is unavailable"));
}

#[test]
fn window_quit_and_explicit_quit_keep_running_confirmation_even_with_immediate_tab_close() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.shell = crate::test_support::shell_binary();
    state.start_tab(root.path().into());
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    state.save_config(|c| {
        c.behavior.terminal_tab_close = crate::config::TerminalClosePolicy::CloseImmediately;
        c.behavior.window_close = crate::config::WindowClosePolicy::QuitRodtty;
    });
    state.send(b"read answer\r".to_vec());
    let start = std::time::Instant::now();
    while !state.active().unwrap().has_running_commands() {
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let id = state.active().unwrap().id();
    let mut rt = runtime_for(state);
    for action in [AppAction::Quit, AppAction::CloseWindow] {
        rt.update(|s, tx| s.dispatch(action, tx));
        rt.frame(0.0);
        assert!(rt.find("confirmation").is_some());
        assert!(rt.shell_request.is_none());
        press(&mut rt, Key::Escape, Default::default());
        assert_eq!(rt.state.active().unwrap().id(), id);
    }
    // An OS close callback uses the same configured quit flow.
    rt.update(State::on_window_close);
    rt.frame(0.0);
    assert!(rt.find("confirmation").is_some());
    press(&mut rt, Key::Escape, Default::default());
    assert_eq!(rt.state.active().unwrap().id(), id);
}

#[test]
fn dirty_editor_decision_precedes_each_window_policy_and_quit() {
    use crate::config::WindowClosePolicy as Policy;
    for policy in [
        Policy::KeepSessionsRunning,
        Policy::AskEachTime,
        Policy::QuitRodtty,
    ] {
        for action in [AppAction::CloseWindow, AppAction::Quit] {
            let root = crate::test_support::tempdir().unwrap();
            let mut state = State::with_root(root.path().into());
            demo(&mut state, false);
            state.workspace.tabs[0].edit.insert("draft remains ");
            let draft = state.workspace.tabs[0].edit.text.clone();
            state.save_config(|c| c.behavior.window_close = policy);
            let mut rt = runtime_for(state);
            rt.update(|s, tx| s.dispatch(action, tx));
            rt.frame(0.0);
            assert!(rt.find("file-save-confirm").is_some());
            assert!(rt.shell_request.is_none());
            click(&mut rt, "file-cancel");
            assert_eq!(rt.state.workspace.tabs[0].edit.text, draft);
            assert!(rt.state.workspace.tabs[0].dirty());
            rt.update(|s, tx| s.dispatch(action, tx));
            rt.frame(0.0);
            click(&mut rt, "file-discard");
            assert!(!rt.state.workspace.tabs[0].dirty());
            if matches!(action, AppAction::CloseWindow) && policy == Policy::AskEachTime {
                assert!(rt.find("window-close-choice").is_some());
            } else {
                assert_eq!(
                    rt.shell_request.take(),
                    Some(
                        if matches!(action, AppAction::Quit) || policy == Policy::QuitRodtty {
                            rune::Shell::Quit
                        } else {
                            rune::Shell::Close
                        }
                    )
                );
            }
        }
    }
}

#[test]
fn new_terminal_directories_follow_live_choices_without_changing_startup_or_session_snapshots() {
    use crate::config::NewTerminalDirectory;
    let root = crate::test_support::tempdir().unwrap();
    let active = root.path().join("active");
    let explorer = root.path().join("explorer");
    let specified = root.path().join("specified");
    for dir in [&active, &explorer, &specified] {
        std::fs::create_dir(dir).unwrap();
    }
    let mut state = State::with_root(root.path().into());
    state.directory = active.clone();
    state.shell = crate::test_support::shell_binary();
    state.save_config(|c| {
        c.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
        c.behavior.specified_directory = specified.clone();
    });
    setup(&mut state, Arc::new(|| {}));
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    assert_eq!(
        state.active().unwrap().cwd(),
        active,
        "first launch still uses the startup directory"
    );
    state.navigate(explorer.clone());
    let first = state.active().unwrap().id();
    let mut rt = runtime_for(state);
    rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
    assert_eq!(rt.state.active().unwrap().cwd(), specified);
    rt.update(|s, _| {
        s.save_config(|c| {
            c.behavior.new_terminal_directory = NewTerminalDirectory::Explorer;
            c.history = false;
            c.disabled = vec!["cat".into()];
        })
    });
    rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
    let newest = rt.state.active().unwrap();
    assert_eq!(newest.cwd(), explorer);
    assert!(!newest.config.history);
    assert_eq!(newest.config.disabled, ["cat"]);
    assert_eq!(rt.state.sessions[0].id(), first);
    assert!(rt.state.sessions[0].config.history);
    assert!(rt.state.sessions[0].config.disabled.is_empty());
    rt.update(|s, _| {
        s.save_config(|c| c.behavior.new_terminal_directory = NewTerminalDirectory::Active)
    });
    rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
    assert_eq!(rt.state.active().unwrap().cwd(), explorer);
    let count = rt.state.sessions.len();
    rt.update(|s, _| {
        s.save_config(|c| {
            c.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
            c.behavior.specified_directory = PathBuf::from("/missing-rodtty-folder");
        })
    });
    rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
    assert_eq!(rt.state.sessions.len(), count);
    assert!(rt.state.message.contains("directory"));
}

#[test]
fn window_close_policy_applies_live_and_ask_can_cancel_keep_or_quit() {
    use crate::config::WindowClosePolicy;
    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime_for(State::with_root(root.path().into()));
    runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Close));
    runtime
        .update(|s, _| s.save_config(|c| c.behavior.window_close = WindowClosePolicy::AskEachTime));
    runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    runtime.frame(0.0);
    assert!(runtime.find("window-close-choice").is_some());
    assert!(runtime.shell_request.is_none());
    press(&mut runtime, Key::Escape, Default::default());
    assert!(runtime.state.pending.is_none());
    runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    runtime.frame(0.0);
    runtime.settle_all();
    runtime.frame(0.0);
    let (x, y) = runtime.find("keep-sessions").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Close));
    runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    runtime.frame(0.0);
    runtime.settle_all();
    runtime.frame(0.0);
    let (x, y) = runtime.find("quit-sessions").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
    runtime
        .update(|s, _| s.save_config(|c| c.behavior.window_close = WindowClosePolicy::QuitRodtty));
    runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
}

#[test]
fn terminal_ansi_colors_follow_the_selected_theme_family_live() {
    // A preview screen with a bright-blue word and default-ink text: the
    // fills of the glyphs the terminal draws must come from the selected
    // family's palette, the way a themed prompt restyles the terminal.
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let mut parser = vt100::Parser::new(24, 112, 100);
    parser.process(b"\x1b[38;5;12mthemed\x1b[0m: prompt $ ");
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    let mut draw_in_family = |family: crate::config::ThemeFamily| {
        rt.update(|s, _| {
            s.save_config(|c| c.theme_family = family);
            s.config.appearance = crate::config::Appearance::Dark;
        });
        // Theme colors animate to their new values; let the flight land first.
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        let a = Appearance::resolve(family, crate::config::Appearance::Dark, true);
        let blue = a.terminal(vt100::Color::Idx(12), true);
        let ink = a.terminal(vt100::Color::Default, true);
        // The terminal's own cells report their element rects; find the glyph
        // instances drawn inside them and compare their fills.
        let cells: Vec<[f32; 4]> = rt
            .elements()
            .into_iter()
            .filter(|e| {
                e.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("terminal-cell-"))
            })
            .map(|e| e.rect)
            .collect();
        let fills: Vec<[f32; 4]> = rt
            .instances
            .iter()
            .filter(|i| i.params[1] == rune::render::KIND_GLYPH)
            .filter(|i| {
                cells.iter().any(|c| {
                    i.rect[0] >= c[0]
                        && i.rect[1] >= c[1]
                        && i.rect[0] + i.rect[2] <= c[0] + c[2] + 1.0
                        && i.rect[1] + i.rect[3] <= c[1] + c[3] + 1.0
                })
            })
            .map(|i| i.fill)
            .collect();
        let close = |fill: [f32; 4], expected: rune::Color| {
            (fill[0] - expected.r).abs() < 0.001
                && (fill[1] - expected.g).abs() < 0.001
                && (fill[2] - expected.b).abs() < 0.001
        };
        let has_blue = fills.iter().any(|f| close(*f, blue));
        let has_ink = fills.iter().any(|f| close(*f, ink));
        has_blue && has_ink
    };
    assert!(
        draw_in_family(crate::config::ThemeFamily::TokyoNight),
        "Tokyo Night palette missing from the terminal"
    );
    assert!(
        draw_in_family(crate::config::ThemeFamily::Everforest),
        "Everforest palette missing from the terminal"
    );
}

#[test]
fn footer_shows_bold_app_identity_then_tilde_home_path_with_a_point_frame() {
    let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
    // The owner's actual home folder collapses to `~`, whatever the host
    // calls it, and never collapses folders that merely sit beside it.
    assert_eq!(tilde_path(&home), "~");
    let project = home.join("Projects").join("CouraLabs").join("Rodtty");
    assert_eq!(tilde_path(&project), "~/Projects/CouraLabs/Rodtty");
    let elsewhere = home
        .parent()
        .unwrap()
        .join("shared-not-home")
        .join("readme");
    assert_eq!(tilde_path(&elsewhere), elsewhere.display().to_string());

    let root = crate::test_support::tempdir().unwrap();
    let mut runtime = runtime_for(State::with_root(root.path().to_owned()));
    runtime.frame(0.0);
    let identity = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("footer-app-name"))
        .expect("footer identity");
    assert_eq!(
        identity.text.as_deref(),
        Some(concat!("Rodtty ", env!("CARGO_PKG_VERSION")))
    );
    // One point of breathing room around the footer strip's content.
    let rect = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("app-footer"))
        .unwrap()
        .rect;
    let content = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("app-footer-content"))
        .unwrap()
        .rect;
    assert!((content[0] - rect[0] - 1.0).abs() < 0.001);
    assert!((content[1] - rect[1] - 1.0).abs() < 0.001);
    assert!((rect[0] + rect[2] - 1.0 - content[0] - content[2]).abs() < 0.001);
    assert!((rect[1] + rect[3] - 1.0 - content[1] - content[3]).abs() < 0.001);
}

#[test]
fn clicking_a_terminal_tab_switches_the_active_session_both_ways() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let shell = crate::test_support::shell_binary().unwrap();
    state.shell = Some(shell);
    for _ in 0..2 {
        state.start_tab(root.path().into());
    }
    let mut rt = runtime_for(state);
    rt.state.sessions.iter().for_each(|s| {
        s.wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap()
    });
    assert_eq!(rt.state.active, 1);
    let (x, y) = rt.find("tab-0").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(
        rt.state.active, 0,
        "click on tab-0 switches the active session"
    );
    let (x, y) = rt.find("tab-1").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(rt.state.active, 1, "click on tab-1 switches back");
}

#[test]
fn idle_terminal_closes_immediately_but_live_commands_offer_cancel() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.shell = crate::test_support::shell_binary();
    state.new_tab();
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    let mut runtime = runtime_for(state);
    runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    assert!(
        runtime.state.sessions.is_empty(),
        "an idle shell needs no confirmation"
    );
    runtime.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
    runtime
        .state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    runtime
        .state
        .active()
        .unwrap()
        .send(b"read pending\r".to_vec())
        .unwrap();
    let started = std::time::Instant::now();
    while !runtime.state.active().unwrap().has_running_commands() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let id = runtime.state.active().unwrap().id();
    runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    runtime.frame(0.0);
    assert!(runtime.find("confirmation").is_some());
    press(&mut runtime, Key::Escape, Default::default());
    assert_eq!(runtime.state.active().unwrap().id(), id);
    runtime.update(|s, tx| s.dispatch(AppAction::Quit, tx));
    runtime.frame(0.0);
    assert!(runtime.find("confirmation").is_some());
    press(&mut runtime, Key::Escape, Default::default());
    assert_eq!(runtime.state.active().unwrap().id(), id);
}

#[test]
fn mac_hosted_titlebar_keeps_traffic_lights_and_header_clicks() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    demo(&mut state, false);
    let mut runtime = rune::Runtime::new(
        state,
        |state, cx| view_with_frame(state, cx, window::frame::Placement::Hosted),
        (640.0, 360.0),
        rune::Env::new(true, 2.0),
    );
    runtime.frame(0.0);
    // macOS keeps the host's traffic lights: the app draws no window controls,
    // and nothing in the header sits where the lights are.
    assert!(runtime.find("window-close").is_none(), "no app-drawn close");
    assert!(
        runtime.find("window-minimize").is_none(),
        "no app-drawn minimize"
    );
    assert!(
        runtime.find("window-maximize").is_none(),
        "no app-drawn maximize"
    );
    assert!(
        runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("toggle-terminal"))
            .unwrap()
            .rect[0]
            > 0.0,
        "header controls sit past the traffic lights, not under them"
    );
    // Header controls keep their presses and fit the minimum-size frame.
    for key in ["toggle-terminal", "workspace-menu", "files"] {
        let rect = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .unwrap()
            .rect;
        assert!(
            rect[0] >= 0.0
                && rect[0] + rect[2] <= 640.0
                && rect[1] >= 0.0
                && rect[1] + rect[3] <= 54.0,
            "{key} fits inside the minimum-size frame: {rect:?}"
        );
    }
    let pane = runtime.find("toggle-terminal").unwrap();
    runtime.pointer_down(pane.0, pane.1);
    assert_eq!(runtime.take_titlebar_action(), None);
    runtime.pointer_up(pane.0, pane.1);
    runtime.frame(0.3);
    // The header band still moves the window, and a double press zooms it.
    runtime.set_viewport((1200.0, 700.0), 1.0);
    runtime.settle_all();
    runtime.frame(0.5);
    let header = runtime.find("workspace-header").unwrap();
    runtime.pointer_down(header.0, header.1);
    assert_eq!(
        runtime.take_titlebar_action(),
        Some(rune::TitlebarAction::Drag)
    );
    runtime.pointer_up(header.0, header.1);
    runtime.pointer_down(header.0, header.1);
    assert_eq!(
        runtime.take_titlebar_action(),
        Some(rune::TitlebarAction::DoubleClick)
    );
    runtime.pointer_up(header.0, header.1);
}

#[test]
fn right_frame_places_controls_right_and_header_controls_keep_their_clicks() {
    let mut runtime = rune::Runtime::new(
        State::init(),
        |state, cx| view_with_frame(state, cx, window::frame::Placement::Trailing),
        (1200.0, 700.0),
        rune::Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    let minimize = runtime
        .find("window-minimize")
        .expect("custom minimize control");
    let maximize = runtime
        .find("window-maximize")
        .expect("custom maximize control");
    let close = runtime.find("window-close").expect("custom close control");
    assert!(minimize.0 < maximize.0 && maximize.0 < close.0);
    assert!(close.0 > 1100.0);
    runtime.pointer_down(240.0, 20.0);
    assert_eq!(
        runtime.take_titlebar_action(),
        Some(rune::TitlebarAction::Drag)
    );
    runtime.pointer_up(240.0, 20.0);
    runtime.frame(0.1);
    runtime.pointer_down(240.0, 20.0);
    assert_eq!(
        runtime.take_titlebar_action(),
        Some(rune::TitlebarAction::DoubleClick)
    );
    runtime.pointer_up(240.0, 20.0);
    let (x, y) = runtime.find("toggle-terminal").unwrap();
    runtime.pointer_down(x, y);
    assert_eq!(runtime.take_titlebar_action(), None);
    runtime.pointer_up(x, y);
    runtime.frame(0.2);
    runtime.set_viewport((640.0, 360.0), 1.0);
    runtime.settle_all();
    runtime.frame(0.3);
    for key in [
        "window-minimize",
        "window-maximize",
        "window-close",
        "toggle-terminal",
        "workspace-menu",
    ] {
        let rect = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .unwrap()
            .rect;
        assert!(
            rect[0] >= 0.0
                && rect[0] + rect[2] <= 640.0
                && rect[1] >= 0.0
                && rect[1] + rect[3] <= 60.0,
            "{key} must fit narrow header: {rect:?}"
        );
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn custom_close_keeps_unsaved_editor_until_owner_decides() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    demo(&mut state, false);
    state.workspace.tabs[0].edit.insert("draft remains ");
    let draft = state.workspace.tabs[0].edit.text.clone();
    let mut runtime = runtime_for_frame(state, window::frame::Placement::Trailing);
    let (x, y) = runtime.find("window-close").unwrap();
    runtime.pointer_down(x, y);
    assert_eq!(runtime.take_titlebar_action(), None);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    runtime.settle_all();
    runtime.frame(0.05);
    assert!(runtime.find("file-save-confirm").is_some());
    assert!(runtime.find("file-discard").is_some());
    let (x, y) = runtime.find("file-cancel").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.1);
    assert!(runtime.state.pending.is_none());
    assert_eq!(runtime.state.workspace.tabs[0].edit.text, draft);
    assert!(runtime.state.workspace.tabs[0].dirty());
}

#[test]
fn terminal_tabs_show_application_title_verbatim_then_terminal_fallback_with_directory_tooltip() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let shell = crate::test_support::shell_binary().unwrap();
    let session = Session::spawn(
        &shell,
        root.path(),
        root.path(),
        Config::default(),
        28,
        100,
        Arc::new(|| {}),
    )
    .unwrap();
    session
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    session
        .send(b"printf '\\033]2;Agent title\\007'\r".to_vec())
        .unwrap();
    let start = std::time::Instant::now();
    while session.state.lock().unwrap().parser.callbacks().title != "Agent title" {
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    state.sessions.push(session);
    let mut runtime = runtime_for(state);
    let tab_help = |runtime: &rune::Runtime<State>| {
        runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("tab-0") && !e.exiting)
            .unwrap()
            .help
    };
    assert!(
        runtime
            .elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Agent title"))
    );
    assert_eq!(
        tab_help(&runtime).as_deref(),
        Some(tilde_path(root.path()).as_str()),
        "a titled tab's tooltip still names its working directory"
    );
    assert!(
        !runtime
            .elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("Terminal 1")))
    );
    assert!(runtime.find("terminal-close-0").is_some());
    let child = root.path().join("new-folder");
    std::fs::create_dir(&child).unwrap();
    runtime.state.sessions[0]
        .send(format!("cd {}\r", profile::quote(&child).unwrap()).into_bytes())
        .unwrap();
    let start = std::time::Instant::now();
    while runtime.state.sessions[0].cwd() != child {
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    runtime.frame(0.0);
    assert!(
        runtime
            .elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Agent title")),
        "the terminal title outranks the directory change"
    );
    assert_eq!(
        tab_help(&runtime).as_deref(),
        Some(tilde_path(&child).as_str())
    );
    runtime.state.sessions[0]
        .send(b"printf '\\033]2;\\007'\r".to_vec())
        .unwrap();
    let start = std::time::Instant::now();
    while !runtime.state.sessions[0]
        .state
        .lock()
        .unwrap()
        .parser
        .callbacks()
        .title
        .is_empty()
    {
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    runtime.update(|_, _| {});
    runtime.frame(0.0);
    assert!(
        runtime
            .elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Terminal"))
    );
    assert_eq!(
        runtime.state.sessions[0].tab_title(),
        "Terminal",
        "the fallback label does not name the directory"
    );
    assert_eq!(
        tab_help(&runtime).as_deref(),
        Some(tilde_path(&child).as_str())
    );
    let full = format!(
        "Application; title 日本語 with {};;",
        "preserved content ".repeat(8)
    );
    runtime.state.sessions[0]
        .send(format!("printf '\\033]2;{}\\007'\r", full).into_bytes())
        .unwrap();
    let start = std::time::Instant::now();
    while runtime.state.sessions[0].tab_title() != full {
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    runtime.update(|_, _| {});
    runtime.frame(0.0);
    let tab = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("tab-0") && !e.exiting)
        .unwrap();
    assert_eq!(tab.help.as_deref(), Some(tilde_path(&child).as_str()));
    assert!(tab.rect[2] <= 180.0);
}

#[test]
fn dragging_and_closing_earlier_terminal_tabs_preserves_active_session() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    state.config.behavior.terminal_tab_close = crate::config::TerminalClosePolicy::AlwaysConfirm;
    let shell = crate::test_support::shell_binary().unwrap();
    for _ in 0..3 {
        state.sessions.push(
            Session::spawn(
                &shell,
                root.path(),
                root.path(),
                Config::default(),
                28,
                100,
                Arc::new(|| {}),
            )
            .unwrap(),
        );
    }
    for session in &state.sessions {
        session
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
    }
    let observed: std::collections::HashMap<_, _> = state
        .sessions
        .iter()
        .map(|session| (session.id(), session.state.lock().unwrap().generation))
        .collect();
    state.generations = state.sessions.iter().map(|s| observed[&s.id()]).collect();
    state.active = 2;
    let active = state.sessions[2].state.clone();
    let first = state.sessions[0].state.clone();
    state.selection = Some(((0, 0), (0, 2)));
    let mut runtime = runtime_for(state);
    let (x, y) = runtime.find("tab-0").unwrap();
    let (end, _) = runtime.find("tab-2").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_move(end + 30.0, y);
    runtime.pointer_up(end + 30.0, y);
    runtime.frame(0.0);
    assert!(Arc::ptr_eq(&runtime.state.sessions[2].state, &first));
    assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
    assert!(runtime.state.selection.is_some());
    assert_eq!(
        runtime.state.generations,
        runtime
            .state
            .sessions
            .iter()
            .map(|s| observed[&s.id()])
            .collect::<Vec<_>>()
    );

    let (x, y) = runtime.find("tab-0").unwrap();
    runtime.button_down(x, y, rune::PointerButton::Middle);
    runtime.button_up(x, y, rune::PointerButton::Middle);
    runtime.frame(0.0);
    assert!(runtime.find("confirmation").is_some());
    let (x, y) = runtime.find("confirm").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
    assert_eq!(
        runtime.state.generations,
        runtime
            .state
            .sessions
            .iter()
            .map(|s| observed[&s.id()])
            .collect::<Vec<_>>()
    );
}

#[test]
fn terminal_overflow_scrolls_and_context_close_targets_its_session() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    state.config.behavior.terminal_tab_close = crate::config::TerminalClosePolicy::AlwaysConfirm;
    let shell = crate::test_support::shell_binary().unwrap();
    state.shell = Some(shell.clone());
    for _ in 0..5 {
        state.sessions.push(
            Session::spawn(
                &shell,
                root.path(),
                root.path(),
                Config::default(),
                28,
                100,
                Arc::new(|| {}),
            )
            .unwrap(),
        );
    }
    state.active = 2;
    let active = state.sessions[2].state.clone();
    let closing = tabs::session_id(&state.sessions[4]);
    let mut runtime = runtime_for(state);
    let board = rune::clipboard::memory();
    runtime.use_clipboard(&board);
    let paste = |runtime: &mut rune::Runtime<State>, text: &str| {
        board.set_text(text);
        press(
            runtime,
            Key::Command('v'),
            rune::Mods {
                ctrl: !cfg!(target_os = "macos"),
                cmd: true,
                shift: !cfg!(target_os = "macos"),
                ..Default::default()
            },
        );
    };
    runtime.set_viewport((640.0, 360.0), 1.0);
    runtime.settle_all();
    runtime.frame(0.0);
    let (x, y) = runtime.find("tab-0").unwrap();
    runtime.wheel_xy(x, y, 0.0, -5000.0);
    runtime.frame(0.0);
    runtime.settle_all();
    runtime.frame(0.0);
    let last = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("tab-4") && !e.exiting)
        .unwrap();
    assert!(last.visible[2] > 0.0);
    let (x, y) = runtime.find("tab-4").unwrap();
    runtime.secondary_down(x, y);
    runtime.secondary_up(x, y);
    runtime.frame(0.0);
    assert!(runtime.popup_menu().is_some());
    assert!(runtime.state.application_overlay_open());
    runtime
        .state
        .active()
        .unwrap()
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    paste(&mut runtime, "PASTE_BEHIND_TAB_MENU");
    runtime.frame(0.0);
    press(
        &mut runtime,
        Key::Text("KEY_BEHIND_TAB_MENU".into()),
        Default::default(),
    );
    assert!(
        runtime
            .state
            .active()
            .unwrap()
            .wait_for(
                "PASTE_BEHIND_TAB_MENU",
                std::time::Duration::from_millis(200)
            )
            .is_err()
    );
    runtime.resolve_popup(None).unwrap();
    runtime.frame(0.0);
    assert!(!runtime.state.application_overlay_open());
    paste(&mut runtime, "PASTE_AFTER_TAB_MENU");
    runtime.frame(0.0);
    runtime
        .state
        .active()
        .unwrap()
        .wait_for("PASTE_AFTER_TAB_MENU", std::time::Duration::from_secs(10))
        .unwrap();
    let contents = runtime.state.active().unwrap().screen().contents();
    assert!(!contents.contains("PASTE_BEHIND_TAB_MENU"));
    assert!(!contents.contains("KEY_BEHIND_TAB_MENU"));
    runtime.secondary_down(x, y);
    runtime.secondary_up(x, y);
    runtime.frame(0.0);
    runtime.resolve_popup(Some("close")).unwrap();
    runtime.frame(0.0);
    assert!(matches!(runtime.state.pending, Some(Pending::Tab(id)) if id == closing));
    assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
    runtime.settle_all();
    runtime.frame(0.0);
    let (x, y) = runtime.find("cancel").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    runtime.settle_all();
    runtime.frame(0.0);
    assert!(runtime.state.pending.is_none());
    let (x, y) = runtime.find("new-tab").unwrap();
    // The tabs share the header band now: the button sits in it, after
    // the last tab, and pressing it must not start a window drag.
    assert!(x > last.rect[0] && x < 640.0 && y < header_height());
    runtime.pointer_down(x, y);
    assert_eq!(runtime.take_titlebar_action(), None);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    assert_eq!(runtime.state.sessions.len(), 6);
    assert_eq!(runtime.state.active, 5);
    assert_eq!(runtime.state.active().unwrap().cwd(), root.path());
}

#[test]
fn compact_workspace_menu_supports_keyboard_settings_and_palette_entry() {
    let mut runtime = runtime();
    assert!(runtime.find("quit").is_none());
    assert!(runtime.find("close-tab").is_none());
    let (x, y) = runtime.find("workspace-menu").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.settle_all();
    runtime.frame(0.0);
    let popover = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("workspace-menu-popover") && !e.exiting)
        .unwrap();
    assert!(
        popover.rect[3] <= 260.0,
        "a short action menu must fit its content"
    );
    press(&mut runtime, Key::Down, Default::default());
    press(&mut runtime, Key::Enter, Default::default());
    assert!(runtime.find("workspace-menu-popover").is_none());
    assert!(runtime.find("settings-search").is_some());
    let (x, y) = runtime.find("workspace-menu").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.settle_all();
    runtime.frame(0.0);
    press(&mut runtime, Key::Escape, Default::default());
    assert!(runtime.find("workspace-menu-popover").is_none());
    assert!(
        runtime
            .elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("workspace-menu") && e.focused)
    );
    let (x, y) = runtime.find("workspace-menu").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.settle_all();
    runtime.frame(0.0);
    press(&mut runtime, Key::Enter, Default::default());
    assert!(runtime.find("command-palette").is_some());
    assert!(runtime.find("workspace-menu-popover").is_none());
}

#[test]
fn palette_search_executes_the_same_explorer_action_as_the_toolbar() {
    let mut runtime = runtime();
    assert!(runtime.find("explorer").is_some());
    open_palette(&mut runtime);
    assert!(runtime.find("command-palette").is_some());
    assert!(
        runtime
            .elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("command-query") && e.focused)
    );
    press(
        &mut runtime,
        Key::Text("toggle explorer".into()),
        Default::default(),
    );
    press(&mut runtime, Key::Enter, Default::default());
    assert!(runtime.find("command-palette").is_none());
    assert!(runtime.find("explorer").is_none());
    let (x, y) = runtime.find("files").unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(0.0);
    assert!(runtime.find("explorer").is_some());
}

#[test]
fn palette_shows_shortcuts_and_keeps_unavailable_commands_open() {
    let mut runtime = runtime();
    open_palette(&mut runtime);
    press(
        &mut runtime,
        Key::Text("save file".into()),
        Default::default(),
    );
    let text = runtime
        .elements()
        .iter()
        .filter_map(|e| e.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains(if cfg!(target_os = "macos") {
        "Cmd+S"
    } else {
        "Ctrl+S"
    }));
    assert!(text.contains("No open editor"));
    press(&mut runtime, Key::Enter, Default::default());
    assert!(runtime.find("command-palette").is_some());
    press(&mut runtime, Key::Escape, Default::default());
    assert!(runtime.find("command-palette").is_none());
}

#[test]
fn palette_keyboard_selection_remains_visible_at_the_minimum_window_size() {
    let mut runtime = runtime();
    runtime.set_viewport((640.0, 360.0), 1.0);
    runtime.frame(0.0);
    open_palette(&mut runtime);
    let palette = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("command-palette"))
        .unwrap();
    assert!(palette.rect[0] >= 0.0 && palette.rect[0] + palette.rect[2] <= 640.0);
    assert!(palette.rect[1] >= 0.0 && palette.rect[1] + palette.rect[3] <= 360.0);
    press(&mut runtime, Key::Up, Default::default());
    assert!(runtime.find("command-quit").is_some());
    press(&mut runtime, Key::Enter, Default::default());
    assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
}

#[test]
fn terminal_controls_are_distinct_from_desktop_shortcuts() {
    let event = KeyEvent {
        key: Key::Command('c'),
        mods: rune::Mods {
            ctrl: true,
            cmd: true,
            ..Default::default()
        },
    };
    assert_eq!(encode_key(&event, false), Some(vec![3]));
    // macOS reports Ctrl+letter as the letter's text with Ctrl held.
    let ctrl = |text: &str, alt: bool| KeyEvent {
        key: Key::Text(text.into()),
        mods: rune::Mods {
            ctrl: true,
            alt,
            ..Default::default()
        },
    };
    assert_eq!(encode_key(&ctrl("c", false), false), Some(vec![3]));
    assert_eq!(encode_key(&ctrl("C", false), false), Some(vec![3]));
    assert_eq!(encode_key(&ctrl("d", false), false), Some(vec![4]));
    assert_eq!(encode_key(&ctrl("z", false), false), Some(vec![26]));
    assert_eq!(encode_key(&ctrl("[", false), false), Some(vec![0x1b]));
    assert_eq!(encode_key(&ctrl(" ", false), false), Some(vec![0]));
    assert_eq!(encode_key(&ctrl("a", true), false), Some(vec![0x1b, 1]));
    assert_eq!(encode_key(&ctrl("é", false), false), None);
    let arrow = KeyEvent {
        key: Key::Up,
        mods: Default::default(),
    };
    assert_eq!(encode_key(&arrow, false), Some(b"\x1b[A".to_vec()));
    assert_eq!(encode_key(&arrow, true), Some(b"\x1bOA".to_vec()));
}
