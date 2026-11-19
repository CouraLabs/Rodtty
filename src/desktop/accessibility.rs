//! Accessibility tree audit for the desktop workspace: helpers that read
//! Rune's AccessKit tree as `--tree` reports it, and the tests that assert
//! the names, roles, states, focus order and activation of the workspace's
//! interactive elements.
use super::*;

/// The accessibility node keyed `key`, as `Runtime::accessibility_json`
/// reports it; panics when the tree has none.
pub(super) fn node(rt: &rune::Runtime<State>, key: &str) -> serde_json::Value {
    fn find(node: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
        if node["key"] == key {
            return Some(node.clone());
        }
        node["children"]
            .as_array()?
            .iter()
            .find_map(|child| find(child, key))
    }
    find(&rt.accessibility_json(), key).unwrap_or_else(|| panic!("no accessibility node {key}"))
}

/// The role, name and states assistive technology reads for `key`.
pub(super) fn read(rt: &rune::Runtime<State>, key: &str) -> (String, String, Vec<String>) {
    let n = node(rt, key);
    let states = n["states"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    (
        n["role"].as_str().unwrap_or_default().to_owned(),
        n["name"].as_str().unwrap_or_default().to_owned(),
        states,
    )
}

/// Whether the toggle, switch, checkbox or radio keyed `key` reads as on.
pub(super) fn checked(rt: &rune::Runtime<State>, key: &str) -> bool {
    let (_, _, states) = read(rt, key);
    assert!(
        states.iter().any(|s| s == "checked" || s == "unchecked"),
        "{key} reports no checked state: {states:?}"
    );
    states.iter().any(|s| s == "checked")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::accesskit::Action;

    fn shell() -> Option<PathBuf> {
        crate::test_support::shell_binary()
    }

    /// Two terminal sessions beside an explorer wide enough for its root row
    /// controls, in an isolated root.
    fn workspace(root: &std::path::Path, explorer_width: f32) -> rune::Runtime<State> {
        std::fs::write(root.join("notes.txt"), "notes").unwrap();
        let mut state = State::with_root(root.to_owned());
        state.directory = root.to_owned();
        state.workspace_setup();
        state.shell = shell();
        state.start_tab(root.into());
        state.start_tab(root.into());
        state.toggle_files();
        state.workspace.layout.explorer_width = explorer_width;
        let mut rt = crate::desktop::tests::runtime_for(state);
        let start = std::time::Instant::now();
        while rt.state.workspace.entries.is_empty() && start.elapsed().as_secs() < 8 {
            rt.update(|s, tx| {
                s.workspace_poll(tx);
            });
            rt.frame(0.05);
        }
        settle(&mut rt);
        rt
    }

    fn settle(rt: &mut rune::Runtime<State>) {
        for _ in 0..2 {
            rt.settle_all();
            rt.frame(1.0);
        }
    }

    fn press(rt: &mut rune::Runtime<State>, key: Key) {
        rt.key(KeyEvent {
            key,
            mods: Default::default(),
        });
        // A Return delivers its press as Space on the following frame.
        rt.frame(0.0);
        rt.frame(0.0);
    }

    fn focus(rt: &mut rune::Runtime<State>, key: &'static str) {
        rt.set_full_keyboard_access(true);
        rt.update(move |_, tx| tx.focus(key, true));
        rt.frame(0.0);
        assert!(
            read(rt, key).2.iter().any(|s| s == "focused"),
            "{key} takes focus"
        );
    }

    fn act(rt: &mut rune::Runtime<State>, key: &str) {
        assert!(
            rt.accessibility_action(key, Action::Click, None),
            "{key} offers a default action"
        );
        rt.frame(0.0);
        rt.frame(0.0);
    }

    fn role_name(rt: &rune::Runtime<State>, key: &str) -> RoleName {
        let (role, name, _) = read(rt, key);
        RoleName(role, name)
    }

    #[derive(Debug, PartialEq)]
    struct RoleName(String, String);
    impl From<(&str, &str)> for RoleName {
        fn from((role, name): (&str, &str)) -> Self {
            Self(role.into(), name.into())
        }
    }

    fn actions(rt: &rune::Runtime<State>, key: &str) -> Vec<String> {
        node(rt, key)["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str().map(str::to_owned))
            .collect()
    }

    /// Every node a keyboard can reach and press carries a name that says
    /// more than a lone glyph or a bare "Reset".
    fn assert_every_control_is_named(rt: &rune::Runtime<State>, scene: &str) {
        fn walk(n: &serde_json::Value, scene: &str) {
            let acts: Vec<&str> = n["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str())
                .collect();
            if acts.contains(&"click") && acts.contains(&"focus") {
                let name = n["name"].as_str().unwrap_or_default().trim();
                assert!(
                    name.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && name != "Reset",
                    "{scene}: {} {:?} is unnamed or ambiguous: {name:?}",
                    n["role"],
                    n["key"]
                );
            }
            for child in n["children"].as_array().into_iter().flatten() {
                walk(child, scene);
            }
        }
        walk(&rt.accessibility_json(), scene);
    }

    #[test]
    fn terminal_tabs_and_header_pane_toggles_are_named_tabs_and_toggle_buttons() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = workspace(root.path(), 300.0);
        assert_eq!(
            role_name(&rt, "terminal-tabs"),
            ("tab-list", "Terminal tabs").into()
        );
        let (role, name, states) = read(&rt, "tab-1");
        assert_eq!((role.as_str(), name.as_str()), ("tab", "Terminal"));
        assert!(
            states.contains(&"selected".into()),
            "the active tab is selected"
        );
        assert!(!read(&rt, "tab-0").2.contains(&"selected".into()));
        assert_eq!(node(&rt, "tab-0")["position"], 1);
        assert_eq!(node(&rt, "tab-1")["position"], 2);
        for key in ["tab-0", "terminal-close-0"] {
            let acts = actions(&rt, key);
            assert!(acts.iter().any(|a| a == "click") && acts.iter().any(|a| a == "focus"));
        }
        assert_eq!(
            read(&rt, "terminal-close-0").1,
            "End terminal session: Terminal (tab 1 of 2)"
        );
        assert_eq!(read(&rt, "new-tab").1, "New terminal tab");
        for (key, name, on) in [
            ("files", "File explorer", true),
            ("toggle-terminal", "Terminal pane", true),
            ("toggle-editor", "Editor pane", false),
        ] {
            let (role, label, _) = read(&rt, key);
            assert_eq!((role.as_str(), label.as_str()), ("button", name));
            assert_eq!(checked(&rt, key), on, "{key}");
        }
        assert_every_control_is_named(&rt, "workspace");

        // The assistive default action, Return and Space each activate a tab.
        act(&mut rt, "tab-0");
        assert_eq!(rt.state.active, 0);
        assert!(read(&rt, "tab-0").2.contains(&"selected".into()));
        focus(&mut rt, "tab-1");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.active, 1, "Return activates the focused tab");
        focus(&mut rt, "tab-0");
        press(&mut rt, Key::Text(" ".into()));
        assert_eq!(rt.state.active, 0, "Space activates the focused tab");

        // Tab moves through the strip in reading order: each tab, then its
        // close button, then the next tab.
        focus(&mut rt, "tab-0");
        let mut order = Vec::new();
        for _ in 0..6 {
            rt.key(KeyEvent {
                key: Key::Tab,
                mods: Default::default(),
            });
            rt.frame(0.0);
            order.push(
                rt.elements()
                    .into_iter()
                    .find(|e| e.focused)
                    .and_then(|e| e.key)
                    .unwrap_or_default(),
            );
        }
        assert_eq!(
            order,
            [
                "terminal-close-0",
                "tab-1",
                "terminal-close-1",
                "new-tab",
                "explorer-search-query",
                "files"
            ]
        );

        // A header toggle presses by its default action and reports the change.
        act(&mut rt, "files");
        settle(&mut rt);
        assert!(!checked(&rt, "files"), "the explorer hides");
        focus(&mut rt, "files");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert!(checked(&rt, "files"), "Return shows it again");
    }

    #[test]
    fn explorer_root_controls_and_overflow_menu_report_toggle_state_and_activate() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = workspace(root.path(), 300.0);
        for (key, name, on) in [
            ("explorer-search-files", "Search files", None),
            ("explorer-refresh", "Refresh files", None),
            ("explorer-follow", "Follow terminal", Some(false)),
            ("explorer-hidden", "Show hidden files", Some(false)),
            ("explorer-ignore", "Respect Git ignore rules", Some(true)),
        ] {
            let (role, label, states) = read(&rt, key);
            assert_eq!((role.as_str(), label.as_str()), ("button", name));
            match on {
                Some(on) => assert_eq!(checked(&rt, key), on, "{key}"),
                None => assert!(!states.iter().any(|s| s.ends_with("checked"))),
            }
        }
        act(&mut rt, "explorer-hidden");
        settle(&mut rt);
        assert!(rt.state.config.explorer.show_hidden);
        assert!(checked(&rt, "explorer-hidden"));
        focus(&mut rt, "explorer-follow");
        press(&mut rt, Key::Enter);
        assert!(
            rt.state.config.explorer.follow_terminal,
            "Return toggles Follow"
        );
        focus(&mut rt, "explorer-follow");
        press(&mut rt, Key::Text(" ".into()));
        assert!(
            !rt.state.config.explorer.follow_terminal,
            "Space toggles it back"
        );

        // Narrow, the controls fold into a menu whose items keep their state.
        rt.update(|s, _| s.workspace.layout.explorer_width = 180.0);
        settle(&mut rt);
        let (role, name, states) = read(&rt, "explorer-more");
        assert_eq!(
            (role.as_str(), name.as_str()),
            ("button", "More explorer actions")
        );
        assert!(states.contains(&"collapsed".into()));
        act(&mut rt, "explorer-more");
        settle(&mut rt);
        // The open menu is modal: the tree holds it alone.
        assert!(rt.state.workspace.explorer_menu);
        let (role, name, _) = read(&rt, "explorer-menu");
        assert_eq!((role.as_str(), name.as_str()), ("menu", "Explorer actions"));
        assert!(
            read(&rt, "explorer-menu-search-files")
                .2
                .contains(&"focused".into())
        );
        for (key, role, name) in [
            ("explorer-menu-search-files", "menu-item", "Search files"),
            ("explorer-menu-refresh", "menu-item", "Refresh files"),
            (
                "explorer-menu-follow",
                "menu-item-check-box",
                "Follow terminal",
            ),
            (
                "explorer-menu-hidden",
                "menu-item-check-box",
                "Show hidden files",
            ),
            (
                "explorer-menu-ignore",
                "menu-item-check-box",
                "Respect Git ignore rules",
            ),
        ] {
            let (r, n, _) = read(&rt, key);
            assert_eq!((r.as_str(), n.as_str()), (role, name), "{key}");
        }
        assert!(checked(&rt, "explorer-menu-hidden"));
        assert!(checked(&rt, "explorer-menu-ignore"));
        act(&mut rt, "explorer-menu-ignore");
        settle(&mut rt);
        assert!(!rt.state.config.explorer.respect_ignore);
        assert!(
            !rt.state.workspace.explorer_menu,
            "choosing an item closes the menu"
        );
        act(&mut rt, "explorer-more");
        settle(&mut rt);
        rt.key(KeyEvent {
            key: Key::Down,
            mods: Default::default(),
        });
        rt.frame(0.0);
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert!(
            !rt.state.workspace.explorer_menu,
            "Return chooses the focused item"
        );
    }

    #[test]
    fn settings_controls_are_named_tabs_fieldsets_dropdowns_switches_and_restore_defaults() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = crate::desktop::tests::runtime_for(State::with_root(root.path().into()));
        rt.update(|s, tx| s.open_settings(tx));
        settle(&mut rt);
        assert_eq!(role_name(&rt, "settings-tab"), ("tab", "Settings").into());
        assert_eq!(
            role_name(&rt, "settings-search"),
            ("text-input", "Search settings").into()
        );
        assert_eq!(
            role_name(&rt, "settings-tabs"),
            ("tab-list", "Settings sections").into()
        );
        let quit = if cfg!(target_os = "macos") {
            "Cmd+Q"
        } else {
            "Ctrl+Q"
        };
        let quit = format!("Application: Quit Rodtty shortcut: {quit}");
        for (tab, controls) in [
            (
                "general",
                vec![
                    ("settings-page-tab-general", "tab", "General"),
                    ("settings-group-window", "group", "Window"),
                    ("window-close-trigger", "combo-box", "Window close"),
                    ("terminal-startup-trigger", "combo-box", "Terminal startup"),
                    ("history", "switch", "Save command history"),
                    ("copy-on-selection", "switch", "Copy on selection"),
                    (
                        "less-scrollback",
                        "button",
                        "Decrease scrollback by 1000 lines",
                    ),
                    ("autosave-trigger", "combo-box", "Autosave"),
                    ("settings-explorer-hidden", "switch", "Show hidden files"),
                    ("reset-tab-general", "button", "Restore General defaults"),
                ],
            ),
            (
                "appearance",
                vec![
                    ("settings-section-Appearance", "group", "Appearance"),
                    ("settings-group-terminal-text", "group", "Terminal"),
                    ("font-ui-value-trigger", "combo-box", "UI font family"),
                    (
                        "font-terminal-value-trigger",
                        "combo-box",
                        "Terminal font family",
                    ),
                    ("theme-family-value-trigger", "combo-box", "Theme family"),
                    ("appearance-trigger", "combo-box", "Appearance"),
                    ("prompt-style-trigger", "combo-box", "Prompt style"),
                    ("prompt-symbol-trigger", "combo-box", "Prompt symbol"),
                    ("less-ui-font-size", "button", "Decrease UI font size"),
                    (
                        "more-terminal-line-height",
                        "button",
                        "Increase terminal line height",
                    ),
                    ("show-footer", "switch", "Show footer"),
                    (
                        "reset-tab-appearance",
                        "button",
                        "Restore Appearance defaults",
                    ),
                ],
            ),
            (
                "commands",
                vec![
                    ("utility-cat", "switch", "cat"),
                    ("reset-tab-commands", "button", "Restore Commands defaults"),
                ],
            ),
            (
                "shortcuts",
                vec![
                    ("record-quit", "button", quit.as_str()),
                    (
                        "reset-tab-shortcuts",
                        "button",
                        "Restore Shortcuts defaults",
                    ),
                ],
            ),
        ] {
            act(&mut rt, &format!("settings-page-tab-{tab}"));
            settle(&mut rt);
            assert!(
                read(&rt, &format!("settings-page-tab-{tab}"))
                    .2
                    .contains(&"selected".into()),
                "{tab} is the selected tab"
            );
            for (key, role, name) in controls {
                let (r, n, _) = read(&rt, key);
                assert_eq!((r.as_str(), n.as_str()), (role, name), "{key}");
            }
            if tab == "commands" {
                assert!(checked(&rt, "utility-cat"));
            }
            assert_every_control_is_named(&rt, tab);
        }

        // Return opens a dropdown and chooses its focused option.
        act(&mut rt, "settings-page-tab-appearance");
        settle(&mut rt);
        assert_eq!(node(&rt, "prompt-style-trigger")["value"], "Classic");
        focus(&mut rt, "prompt-style-trigger");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert_eq!(rt.state.settings_select, Some("prompt-style"));
        rt.key(KeyEvent {
            key: Key::Down,
            mods: Default::default(),
        });
        rt.frame(0.0);
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert_eq!(rt.state.settings_select, None, "choosing closes the list");
        assert_eq!(
            rt.state.config.prompt.style,
            crate::prompt::PromptStyle::Directory
        );
        assert_eq!(node(&rt, "prompt-style-trigger")["value"], "Directory");
        // Return asks to restore the tab's defaults, and again confirms it.
        focus(&mut rt, "reset-tab-appearance");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert_eq!(
            role_name(&rt, "confirm-reset-tab"),
            ("button", "Restore Appearance defaults").into()
        );
        focus(&mut rt, "confirm-reset-tab");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert_eq!(rt.state.config.prompt.style, Default::default());
        // Return flips a switch.
        act(&mut rt, "settings-page-tab-general");
        settle(&mut rt);
        assert!(checked(&rt, "history"));
        focus(&mut rt, "history");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert!(!rt.state.config.history, "Return flips a switch");
        assert!(!checked(&rt, "history"));
        // Return opens a font family picker as Space does.
        act(&mut rt, "settings-page-tab-appearance");
        settle(&mut rt);
        focus(&mut rt, "font-ui-value-trigger");
        press(&mut rt, Key::Enter);
        settle(&mut rt);
        assert!(
            rt.state.font_ui.is_open(),
            "Return opens the UI font family picker"
        );
    }

    #[test]
    fn terminal_and_explorer_search_fields_are_named_and_focused_on_open() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = workspace(root.path(), 300.0);
        rt.update(|s, tx| s.dispatch(AppAction::FindTerminal, tx));
        settle(&mut rt);
        let (role, name, states) = read(&rt, "search-query");
        assert_eq!(
            (role.as_str(), name.as_str()),
            ("search-input", "Find in terminal")
        );
        assert!(states.contains(&"focused".into()));
        assert_eq!(read(&rt, "close-search").1, "Close terminal search");
        act(&mut rt, "close-search");
        settle(&mut rt);
        assert!(!rt.state.terminal_search.open);

        rt.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
        settle(&mut rt);
        let (role, name, states) = read(&rt, "explorer-search-query");
        assert!(role.ends_with("input"), "{role}");
        assert_eq!(name, "Search");
        assert!(states.contains(&"focused".into()));
        // A query opens the Search tab, whose close button is named.
        press(&mut rt, Key::Text("x".into()));
        settle(&mut rt);
        assert_eq!(read(&rt, "search-tab").1, "Search");
        assert_eq!(read(&rt, "search-close").1, "Close Search tab");
        assert_every_control_is_named(&rt, "search");
    }

    #[test]
    fn confirmation_dialogs_are_named_described_modal_and_keyboard_operable() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = workspace(root.path(), 300.0);
        let session = tabs::session_id(&rt.state.sessions[1]);
        for (pending, key, role, name, first, cancel) in [
            (
                Pending::Quit,
                "confirmation",
                "alert-dialog",
                "Quit Rodtty and end all sessions?",
                "cancel",
                "cancel",
            ),
            (
                Pending::Tab(session),
                "confirmation",
                "alert-dialog",
                "End this terminal session?",
                "cancel",
                "cancel",
            ),
            (
                Pending::Window,
                "window-close-choice",
                "dialog",
                "Close Rodtty window?",
                "keep-sessions",
                "window-close-cancel",
            ),
            (
                Pending::Removal(explorer::removal::RemovalPrompt::Confirm),
                "removal-dialog",
                "alert-dialog",
                "Move this item to Trash?",
                "removal-cancel",
                "removal-cancel",
            ),
        ] {
            rt.update(move |s, _| s.pending = Some(pending));
            settle(&mut rt);
            let n = node(&rt, key);
            assert_eq!(
                (n["role"].as_str(), n["name"].as_str()),
                (Some(role), Some(name))
            );
            assert!(
                n["description"].as_str().is_some_and(|d| !d.is_empty()),
                "{key} describes itself"
            );
            assert!(read(&rt, key).2.contains(&"modal".into()));
            assert!(
                read(&rt, first).2.contains(&"focused".into()),
                "{key} focuses {first}"
            );
            assert_every_control_is_named(&rt, key);
            // Return presses the focused Cancel.
            focus(&mut rt, cancel);
            press(&mut rt, Key::Enter);
            settle(&mut rt);
            assert!(rt.state.pending.is_none(), "{key}: Return cancels");
            assert_eq!(rt.state.sessions.len(), 2);
        }
        rt.update(|s, _| explorer::operations::collision_demo(s));
        settle(&mut rt);
        let n = node(&rt, "operation-dialog");
        assert_eq!(
            (n["role"].as_str(), n["name"].as_str()),
            (Some("alert-dialog"), Some("An item already exists"))
        );
        assert!(
            n["description"]
                .as_str()
                .is_some_and(|d| d.contains("Keep Both"))
        );
        assert_eq!(read(&rt, "operation-replace").1, "Replace");
        assert_eq!(read(&rt, "operation-keep-both").1, "Keep Both");
        act(&mut rt, "operation-cancel");
        settle(&mut rt);
        assert!(rt.state.pending.is_none(), "the default action cancels");
    }
}
