//! One binding layer for the shared action registry, palette and controls.
use super::*;

pub(super) struct Recording {
    id: &'static str,
    binding: Option<String>,
    return_focus: Option<String>,
}

impl State {
    pub(super) fn shortcut_help(&self, id: &str, title: &str) -> String {
        let binding = self.shortcut_label(id);
        if binding.is_empty() {
            title.into()
        } else {
            format!("{title} ({binding})")
        }
    }
    fn binding(&self, command: &actions::Command) -> String {
        self.config
            .shortcuts
            .get(command.id)
            .cloned()
            .unwrap_or_else(|| command.shortcut.into())
    }
    pub(super) fn shortcut_label(&self, id: &str) -> String {
        self.commands
            .iter()
            .find(|c| c.id == id)
            .map(|c| self.binding(c))
            .unwrap_or_default()
    }
    fn shortcut_context(&self, context: actions::ShortcutContext, focused: Option<&str>) -> bool {
        let (_, terminal_visible, editor_visible) =
            workspace::pane_visibility(self, self.workspace.body_width);
        let editor_field =
            focused.is_some_and(|f| f.starts_with("file-editor-") || f.starts_with("editor-"));
        let settings_field = focused.is_some_and(|f| f.starts_with("settings-"));
        let terminal = terminal_visible && self.workspace.terminal_focus && focused.is_none();
        let editor =
            editor_visible && !self.settings && (editor_field || !self.workspace.terminal_focus);
        match context {
            actions::ShortcutContext::Terminal => terminal,
            actions::ShortcutContext::Editor => editor,
            actions::ShortcutContext::Settings => {
                editor_visible
                    && self.settings
                    && (settings_field || !self.workspace.terminal_focus)
            }
            actions::ShortcutContext::Global => true,
        }
    }
    pub(super) fn dispatch_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if self.application_overlay_open() {
            return false;
        }
        let action = self
            .commands
            .iter()
            .find(|c| {
                self.shortcut_context(c.context, tx.focused())
                    && self.action_disabled(c.action).is_none()
                    && rune::Chord::parse(&self.binding(c)).is_ok_and(|b| b.matches_tx(event, tx))
            })
            .map(|c| c.action);
        if let Some(action) = action {
            self.dispatch(action, tx);
            return true;
        }
        false
    }
    pub(super) fn dispatch_mouse_shortcut(
        &mut self,
        button: rune::PointerButton,
        tx: &mut Tx,
    ) -> bool {
        if self.application_overlay_open() {
            return false;
        }
        let key = match button {
            rune::PointerButton::Middle => "MouseMiddle",
            rune::PointerButton::Back => "MouseBack",
            rune::PointerButton::Forward => "MouseForward",
            _ => return false,
        };
        let action = self
            .commands
            .iter()
            .find(|c| {
                self.binding(c).eq_ignore_ascii_case(key)
                    && self.shortcut_context(c.context, tx.focused())
                    && self.action_disabled(c.action).is_none()
            })
            .map(|c| c.action);
        if let Some(action) = action {
            self.dispatch(action, tx);
            return true;
        }
        false
    }
    fn begin_recording(&mut self, id: &'static str, tx: &mut Tx) {
        self.shortcut_recording = Some(Recording {
            id,
            binding: None,
            return_focus: tx.focused().map(str::to_owned),
        });
        tx.focus_when_shown("shortcut-recorder", true);
    }
    fn dismiss_recording(&mut self, tx: &mut Tx) {
        if let Some(recording) = self.shortcut_recording.take() {
            tx.blur();
            if let Some(key) = recording.return_focus {
                tx.focus_when_shown(key, true);
            }
        }
    }
    pub(super) fn record_shortcut(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if event.key == Key::Escape {
            self.dismiss_recording(tx);
            return true;
        }
        let key = match &event.key {
            Key::Command(c) => c.to_string(),
            Key::Text(s) if s.chars().count() == 1 => {
                if s == " " {
                    "space".into()
                } else if s == "+" {
                    "plus".into()
                } else {
                    s.to_lowercase()
                }
            }
            Key::Function(n) => format!("F{n}"),
            Key::Enter => "Enter".into(),
            Key::Tab => "Tab".into(),
            Key::Backspace => "Backspace".into(),
            Key::Delete => "Delete".into(),
            Key::Left => "Left".into(),
            Key::Right => "Right".into(),
            Key::Up => "Up".into(),
            Key::Down => "Down".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            Key::Insert => "Insert".into(),
            _ => return true,
        };
        let mut binding = String::new();
        if event.mods.ctrl {
            binding.push_str("Ctrl+");
        }
        if event.mods.alt {
            binding.push_str("Alt+");
        }
        if event.mods.shift {
            binding.push_str("Shift+");
        }
        if event.mods.logo || (cfg!(target_os = "macos") && event.mods.cmd) {
            binding.push_str("Super+");
        }
        binding.push_str(&key);
        if rune::Chord::parse(&binding).is_ok() {
            self.shortcut_recording.as_mut().unwrap().binding = Some(binding);
        }
        true
    }
    fn shortcut_conflicts(&self, id: &str, binding: &str) -> Vec<&'static str> {
        if binding.is_empty() {
            return Vec::new();
        }
        let parsed = rune::Chord::parse(binding).ok();
        let context = self
            .commands
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.context)
            .unwrap_or(actions::ShortcutContext::Global);
        self.commands
            .iter()
            .filter(|c| c.id != id && context.overlaps(c.context))
            .filter(|c| {
                let other = self.binding(c);
                if let (Some(a), Ok(b)) = (parsed, rune::Chord::parse(&other)) {
                    a.same_keys(&b, rune::KeyPlatform::current())
                } else {
                    binding.eq_ignore_ascii_case(&other)
                }
            })
            .map(|c| c.title)
            .collect()
    }
}

pub(super) fn list(s: &State, a: &Appearance) -> El {
    let query = s.settings_query.text.trim().to_lowercase();
    let mut list = col().w_full().gap(8.0);
    for command in &s.commands {
        let binding = s.binding(command);
        if !format!(
            "Keyboard Shortcuts {} {} {binding}",
            command.title, command.id
        )
        .to_lowercase()
        .contains(&query)
        {
            continue;
        }
        let id = command.id;
        let conflicts = s.shortcut_conflicts(id, &binding);
        let mut item = col()
            .w_full()
            .gap(4.0)
            .child(text(command.title).font(ui_label()).color(a.ink()))
            .child(
                row()
                    .w_full()
                    .gap(6.0)
                    .child(button(
                        a,
                        format!("record-{id}"),
                        if binding.is_empty() {
                            "Unassigned".into()
                        } else {
                            binding
                        },
                        move |s, tx| s.begin_recording(id, tx),
                    ))
                    .child(button(
                        a,
                        format!("reset-shortcut-{id}"),
                        "Reset",
                        move |s, _| {
                            s.save_config(|c| {
                                c.shortcuts.remove(id);
                            })
                        },
                    )),
            );
        if !conflicts.is_empty() {
            item = item.child(
                text(format!("Also assigned: {}", conflicts.join(", ")))
                    .font(ui_label())
                    .color(a.danger())
                    .wrap(),
            );
        }
        list = list.child(item);
    }
    list
}

pub(super) fn recorder(s: &State, a: &Appearance) -> Option<El> {
    let recording = s.shortcut_recording.as_ref()?;
    let binding = recording.binding.clone();
    let conflicts = binding
        .as_ref()
        .map(|b| s.shortcut_conflicts(recording.id, b))
        .unwrap_or_default();
    let mut panel = col().key("shortcut-recorder").instant_changes().focusable(true).on_key(|s: &mut State,event,tx| s.record_shortcut(event,tx)).modal_tx(false, |s: &mut State,tx| s.dismiss_recording(tx))
        .w_full().max_w(460.0).px(20.0).py(20.0).gap(12.0).fill(a.panel()).radius(10.0)
        .child(text(format!("Record shortcut: {}", s.commands.iter().find(|c| c.id == recording.id)?.title)).font(ui_label()).color(a.ink()).wrap())
        .child(text("Press a key combination, or click the mouse recording area. Escape cancels. Plain keys replace terminal input when assigned.").font(ui_label()).color(a.muted()).wrap())
        .child(el().key("shortcut-mouse-recorder").w_full().h(56.0).fill(a.hover_bg()).center()
            .child(text("Middle / Back / Forward mouse button").font(ui_label()).color(a.ink()))
            .pointer_buttons([rune::PointerButton::Middle, rune::PointerButton::Back, rune::PointerButton::Forward])
            .on_press_tx(|s: &mut State,tx| {
                let key = match tx.button() { rune::PointerButton::Middle => "MouseMiddle", rune::PointerButton::Back => "MouseBack", rune::PointerButton::Forward => "MouseForward", _ => return };
                s.shortcut_recording.as_mut().unwrap().binding = Some(key.into());
            }))
        .child(text(binding.as_deref().unwrap_or("Waiting for input…")).font(ui_label()).color(a.accent()));
    if !conflicts.is_empty() {
        panel = panel.child(
            text(format!(
                "Conflict with {}. Choose another shortcut; existing bindings are preserved.",
                conflicts.join(", ")
            ))
            .key("shortcut-conflict")
            .font(ui_label())
            .color(a.danger())
            .wrap(),
        );
    }
    panel = panel.child(
        row()
            .gap(8.0)
            .child(button(a, "shortcut-cancel", "Cancel", |s, tx| {
                s.dismiss_recording(tx)
            }))
            .child(button(a, "shortcut-clear", "Unbind", |s, _| {
                s.shortcut_recording.as_mut().unwrap().binding = Some(String::new())
            }))
            .child(
                button(a, "shortcut-apply", "Apply", |s, tx| {
                    if let Some(recording) = &s.shortcut_recording
                        && let Some(binding) = &recording.binding
                        && s.shortcut_conflicts(recording.id, binding).is_empty()
                    {
                        let (id, binding) = (recording.id, binding.clone());
                        s.save_config(|c| {
                            c.shortcuts.insert(id.into(), binding);
                        });
                        s.dismiss_recording(tx);
                    }
                })
                .disabled(binding.is_none() || !conflicts.is_empty()),
            ),
    );
    Some(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(a.scrim())
            .center()
            .px(12.0)
            .child(panel),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, runtime_for};
    use super::super::*;

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
    fn owner_searches_records_and_resets_a_live_persisted_shortcut() {
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
        click(&mut reopened, "reset-shortcut-toggle-explorer");
        assert!(!words(&reopened).contains("F8"));
        press(&mut reopened, Key::Function(8), Default::default());
        assert!(!reopened.state.workspace.layout.explorer);
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
        super::super::tests::open_palette(&mut rt);
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
        workspace::demo(&mut state, false);
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
            s.workspace.terminal_focus = false;
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
        assert!(!rt.state.workspace.terminal_focus);
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
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
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
        workspace::demo(&mut state, false);
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
        state.workspace.terminal_focus = true;
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
}
