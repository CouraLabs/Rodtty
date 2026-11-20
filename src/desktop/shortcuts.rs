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
            pane_visibility(self, self.workspace.body_width);
        let editor_field =
            focused.is_some_and(|f| f.starts_with("file-editor-") || f.starts_with("editor-"));
        let settings_field = focused.is_some_and(|f| f.starts_with("settings-"));
        let terminal = terminal_visible && self.workspace.terminal_focus() && focused.is_none();
        let editor = editor_visible
            && !self.settings
            && !self.search_tab
            && (editor_field || !self.workspace.terminal_focus());
        match context {
            actions::ShortcutContext::Terminal => terminal,
            actions::ShortcutContext::Editor => editor,
            actions::ShortcutContext::Settings => {
                editor_visible
                    && self.settings
                    && (settings_field || !self.workspace.terminal_focus())
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
    /// Whether `event` is the binding of command `id`.
    pub(super) fn is_shortcut(&self, id: &str, event: &KeyEvent, tx: &Tx) -> bool {
        self.commands.iter().find(|c| c.id == id).is_some_and(|c| {
            rune::Chord::parse(&self.binding(c)).is_ok_and(|b| b.matches_tx(event, tx))
        })
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

/// The Settings rows for every action the search matches: its title, and its
/// binding as a button that records a new one.
pub(super) fn rows(s: &State, a: &Appearance) -> Vec<El> {
    let query = s.settings_query.text.trim().to_lowercase();
    let mut rows = Vec::new();
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
        let shown = if binding.is_empty() {
            "Unassigned".to_owned()
        } else {
            binding
        };
        // The button shows only the binding; a reader hears which command it
        // records for.
        let name = format!("{} shortcut: {shown}", command.title);
        let mut item = col()
            .key(format!("shortcut-{id}-row"))
            .w_full()
            .py(8.0)
            .gap(4.0)
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        text(command.title)
                            .font(ui_label())
                            .color(a.ink())
                            .grow(1.0)
                            .shrink()
                            .min_w(0.0)
                            .wrap(),
                    )
                    .child(
                        button(a, format!("record-{id}"), shown, move |s, tx| {
                            s.begin_recording(id, tx)
                        })
                        .label(name)
                        .help("Record a new shortcut"),
                    ),
            );
        if !conflicts.is_empty() {
            item = item.child(
                text(format!("Also assigned: {}", conflicts.join(", ")))
                    .font(ui_label())
                    .color(a.danger())
                    .wrap(),
            );
        }
        rows.push(item);
    }
    rows
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
    Some(motion::modal(
        a,
        el().center().px(12.0).child(panel),
        Entrance::Dialog,
    ))
}

#[cfg(test)]
mod tests;
