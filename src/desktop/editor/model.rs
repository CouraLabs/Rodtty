use crate::desktop::workspace::*;
use crate::desktop::*;
use crate::work::Job;
use rune_kit::diff::DiffState;
use std::time::{Duration, Instant};

impl State {
    pub(crate) fn open_file(&mut self, path: PathBuf) {
        self.open_file_with_preview(path, false);
    }

    /// Common activation boundary: restoration and explicit Open keep a file;
    /// explorer inspection may reuse only a clean, idle temporary tab.
    pub(crate) fn open_file_with_preview(&mut self, path: PathBuf, preview: bool) {
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
        if self
            .workspace
            .pending_location
            .as_ref()
            .is_some_and(|(pending, _)| *pending != path)
        {
            self.workspace.pending_location = None;
        }
        self.workspace.latest_activation = Some(path.clone());
        if let Some(i) = self
            .workspace
            .tabs
            .iter()
            .position(|tab| tab.document.path == path)
        {
            if !preview {
                self.workspace.tabs[i].preview = false;
            }
            self.show_editor(i, !preview);
            return;
        }
        if let Some((_, temporary)) = self.workspace.opening.iter_mut().find(|(p, _)| p == &path) {
            *temporary &= preview;
            return;
        }
        if self.io(Job::Open(path.clone())) {
            self.workspace.opening.push((path, preview));
            self.message = "Opening file…".into();
        }
    }

    pub(crate) fn activate_editor(&mut self, index: usize) {
        self.workspace.latest_activation = self
            .workspace
            .tabs
            .get(index)
            .map(|tab| tab.document.path.clone());
        self.show_editor(index, true);
    }

    pub(crate) fn supersede_pending_preview(&mut self) {
        self.workspace.latest_activation = None;
    }

    pub(crate) fn show_editor(&mut self, index: usize, focus: bool) {
        // Opening a file from the compact overlay closes it and hands the
        // keyboard to the editor; the overlay's saved focus no longer applies.
        let from_overlay = self.explorer_overlay_open();
        let focus = focus || from_overlay;
        if from_overlay {
            self.workspace.overlay_return = None;
            self.workspace.explorer_menu = false;
        }
        self.settings = false;
        self.search_tab = false;
        self.workspace.explorer_revealed = false;
        self.workspace.set_terminal_focus(false);
        self.workspace.active = index;
        self.workspace.layout.editor = true;
        if let Some(tab) = self.workspace.tabs.get_mut(index) {
            // Preview inspection keeps tree focus for continued keyboard
            // browsing. Explicit Open and tab activation focus the editor.
            tab.edit.focused = focus && tab.mode == View::Edit;
        }
        self.invalidate_diff(true);
    }
    pub(crate) fn save_file(&mut self, id: u64, overwrite: bool, after: Option<Action>) {
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
        let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if tab.saving {
            self.message = "This file is still being saved.".into();
            return;
        }
        // Explicit saving expresses intent to keep the file and prevents a
        // later inspection from replacing the tab while its save is queued.
        tab.preview = false;
        tab.saving = true;
        tab.save_token += 1;
        tab.autosave.suspend(tab.revision);
        tab.after_save = after;
        let job = Job::Save {
            id,
            token: tab.save_token,
            document: tab.document.clone(),
            text: tab.edit.text.clone(),
            overwrite,
        };
        if !self.io(job)
            && let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id)
        {
            tab.saving = false;
        }
    }
    pub(crate) fn discard_for_action(&mut self, id: u64, action: Action, tx: &mut Tx) {
        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
            if tab.saving {
                self.message = "Wait for the save to finish.".into();
                return;
            }
            tab.edit.replace_text(tab.document.saved.text.clone());
            tab.revision += 1;
        }
        self.pending = None;
        self.request_action(action, tx);
    }
    pub(crate) fn reload_external(&mut self, id: u64) {
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
        if let Some(tab) = self.workspace.tabs.iter().find(|t| t.id == id)
            && !tab.saving
        {
            let job = Job::Reload {
                id,
                revision: tab.revision,
                path: tab.document.path.clone(),
            };
            self.io(job);
        }
        self.pending = None;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        self.invalidate_diff(false);
    }
    pub(crate) fn cancel_file_action(&mut self) {
        self.pending = None;
        for tab in &mut self.workspace.tabs {
            tab.after_save = None;
            tab.autosave.suspend(tab.revision);
        }
    }
    pub(crate) fn invalidate_diff(&mut self, clear: bool) {
        self.workspace.last_diff = Instant::now() - Duration::from_secs(3);
        if clear {
            self.workspace.diff = DiffState::default();
            self.workspace.diff_patch.clear();
            self.workspace.diff_label = "Loading comparison…".into();
            self.workspace.diff_error = None;
        }
    }
    pub(crate) fn changed_editor(&mut self, id: u64) {
        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
            tab.revision += 1;
            tab.preview = false;
        }
    }
    pub(crate) fn file_key(&mut self, id: u64, event: &KeyEvent, tx: &mut Tx) -> bool {
        if self.workspace.removal.is_some() {
            return true;
        }
        if event.key == Key::Function(3) {
            self.find_editor(false, tx);
            return true;
        }
        if event.key == Key::Enter || event.key == Key::Tab {
            if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
                if event.key == Key::Tab {
                    tab.edit.insert("    ");
                } else {
                    let position = tab.edit.focus;
                    let before = &tab.edit.text[..position];
                    let line = before.rsplit('\n').next().unwrap_or("");
                    let indent: String = line
                        .chars()
                        .take_while(|c| *c == ' ' || *c == '\t')
                        .collect();
                    tab.edit.insert(&format!("\n{indent}"));
                }
                tab.revision += 1;
                tab.preview = false;
            }
            return true;
        }
        false
    }
    pub(crate) fn find_editor(&mut self, replace: bool, tx: &mut Tx) {
        if replace && self.workspace.removal.is_some() {
            return;
        }
        let query = self.workspace.find_query.text.clone();
        if query.is_empty() {
            return;
        }
        let replacement = self.workspace.replacement.text.clone();
        if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
            if replace {
                let a = tab.edit.anchor.min(tab.edit.focus);
                let b = tab.edit.anchor.max(tab.edit.focus);
                if tab.edit.text.get(a..b) == Some(query.as_str()) {
                    tab.edit.insert(&replacement);
                    tab.revision += 1;
                    tab.preview = false;
                }
            }
            let start = tab.edit.focus;
            if let Some(index) = tab.edit.text[start..]
                .find(&query)
                .map(|p| p + start)
                .or_else(|| tab.edit.text[..start].find(&query))
            {
                tab.edit.anchor = index;
                tab.edit.focus = index + query.len();
                tab.edit.break_history_group();
                tx.focus_when_shown(format!("file-editor-{}", tab.id), true);
            } else {
                self.message = "No matching text in this file.".into();
            }
        }
    }
    pub(crate) fn replace_all(&mut self) {
        if self.workspace.removal.is_some() {
            return;
        }
        let query = self.workspace.find_query.text.clone();
        if query.is_empty() {
            return;
        }
        if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
            let updated = tab
                .edit
                .text
                .replace(&query, &self.workspace.replacement.text);
            if updated == tab.edit.text {
                return;
            }
            tab.preview = false;
            tab.edit.undo_group(|edit| {
                edit.anchor = 0;
                edit.focus = edit.text.len();
                edit.insert(&updated);
            });
            tab.revision += 1;
        }
    }
}
