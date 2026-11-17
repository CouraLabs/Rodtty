//! Shared application actions. Extend `register` for future slices; controls and the
//! palette use `dispatch`, which keeps save/close policy at the existing workspace boundary.
use super::*;
use crate::git::Comparison;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) enum AppAction {
    CommandPalette,
    NewTerminal,
    EndTerminal,
    ActivateTerminal(usize),
    ToggleExplorer,
    ToggleTerminal,
    ShowTerminal,
    ToggleEditor,
    FindTerminal,
    FindEditor,
    ReplaceEditor,
    Settings,
    OpenSettingsJson,
    CloseSettings,
    ActivateEditor(usize),
    CopyTerminal,
    PasteTerminal,
    SaveEditor(Option<u64>),
    CloseEditor(Option<u64>),
    EditFile,
    Compare(Comparison),
    CloseWindow,
    Quit,
    ExplorerParent,
    RefreshExplorer,
    ToggleFollow,
    ToggleHidden,
}

pub(super) struct Command {
    pub id: &'static str,
    pub title: &'static str,
    pub shortcut: &'static str,
    pub action: AppAction,
}

/// Registrations are stable IDs plus visible metadata and an application action. Later
/// slices can append registrations without changing palette search, input or rendering.
pub(super) fn register(commands: &mut Vec<Command>) {
    let app = |linux, mac| {
        if cfg!(target_os = "macos") {
            mac
        } else {
            linux
        }
    };
    for (id, title, shortcut, action) in [
        (
            "new-terminal",
            "Terminal: New Tab",
            app("Ctrl+Shift+T", "Cmd+T"),
            AppAction::NewTerminal,
        ),
        (
            "end-terminal",
            "Terminal: End Session",
            app("Ctrl+Shift+W", "Cmd+Shift+W"),
            AppAction::EndTerminal,
        ),
        (
            "save-file",
            "File: Save File",
            app("Ctrl+S", "Cmd+S"),
            AppAction::SaveEditor(None),
        ),
        (
            "close-editor",
            "File: Close Editor",
            app("Ctrl+W", "Cmd+W"),
            AppAction::CloseEditor(None),
        ),
        (
            "toggle-explorer",
            "View: Toggle Explorer",
            app("Ctrl+Shift+E", "Cmd+E"),
            AppAction::ToggleExplorer,
        ),
        (
            "toggle-terminal",
            "View: Toggle Terminal",
            "",
            AppAction::ToggleTerminal,
        ),
        (
            "show-terminal",
            "View: Return to Terminal",
            "",
            AppAction::ShowTerminal,
        ),
        (
            "toggle-editor",
            "View: Toggle Editor",
            "",
            AppAction::ToggleEditor,
        ),
        (
            "find-terminal",
            "Terminal: Find in Scrollback",
            app("Ctrl+Shift+F", "Cmd+Shift+F"),
            AppAction::FindTerminal,
        ),
        (
            "find-editor",
            "File: Find in Editor",
            app("Ctrl+F", "Cmd+F"),
            AppAction::FindEditor,
        ),
        (
            "replace-editor",
            "File: Replace in Editor",
            app("Ctrl+H", "Cmd+H"),
            AppAction::ReplaceEditor,
        ),
        ("edit-file", "File: Show Editor", "", AppAction::EditFile),
        (
            "diff-current",
            "Git: Compare Current File with HEAD",
            "",
            AppAction::Compare(Comparison::Current),
        ),
        (
            "diff-unstaged",
            "Git: Show Unstaged Changes",
            "",
            AppAction::Compare(Comparison::Unstaged),
        ),
        (
            "diff-staged",
            "Git: Show Staged Changes",
            "",
            AppAction::Compare(Comparison::Staged),
        ),
        (
            "copy-terminal",
            "Terminal: Copy Selection",
            app("Ctrl+Shift+C", "Cmd+C"),
            AppAction::CopyTerminal,
        ),
        (
            "paste-terminal",
            "Terminal: Paste",
            app("Ctrl+Shift+V", "Cmd+V"),
            AppAction::PasteTerminal,
        ),
        (
            "explorer-parent",
            "Explorer: Open Parent Folder",
            "",
            AppAction::ExplorerParent,
        ),
        (
            "explorer-refresh",
            "Explorer: Refresh",
            "",
            AppAction::RefreshExplorer,
        ),
        (
            "explorer-follow",
            "Explorer: Toggle Follow Terminal",
            "",
            AppAction::ToggleFollow,
        ),
        (
            "explorer-hidden",
            "Explorer: Toggle Hidden Files",
            "",
            AppAction::ToggleHidden,
        ),
        (
            "settings",
            "Settings: Open Settings",
            "",
            AppAction::Settings,
        ),
        (
            "settings-json",
            "Settings: Open Settings JSON",
            "",
            AppAction::OpenSettingsJson,
        ),
        (
            "settings-close",
            "Settings: Close Settings Tab",
            "",
            AppAction::CloseSettings,
        ),
        (
            "close-window",
            "Window: Close and Keep Sessions Running",
            "",
            AppAction::CloseWindow,
        ),
        (
            "command-palette",
            "View: Show Command Palette",
            app("Ctrl+Shift+P", "Cmd+Shift+P"),
            AppAction::CommandPalette,
        ),
        (
            "quit",
            "Application: Quit Rodtty",
            app("Ctrl+Shift+Q", "Cmd+Q"),
            AppAction::Quit,
        ),
    ] {
        commands.push(Command {
            id,
            title,
            shortcut,
            action,
        });
    }
}

impl State {
    fn editor_id(&self, id: Option<u64>) -> Option<u64> {
        id.or_else(|| self.workspace.tabs.get(self.workspace.active).map(|t| t.id))
    }

    pub(super) fn action_disabled(&self, action: AppAction) -> Option<&'static str> {
        match action {
            AppAction::SaveEditor(None)
            | AppAction::CloseEditor(None)
            | AppAction::FindEditor
            | AppAction::ReplaceEditor
            | AppAction::EditFile
            | AppAction::Compare(_)
                if self.settings =>
            {
                Some("Select a file editor first")
            }
            AppAction::CloseSettings if !self.settings_open => Some("Settings is not open"),
            AppAction::NewTerminal if !self.config_valid => {
                Some("Fix settings before starting a session")
            }
            AppAction::EndTerminal | AppAction::FindTerminal | AppAction::PasteTerminal
                if self.active().is_none() =>
            {
                Some("No active terminal session")
            }
            AppAction::CopyTerminal if self.selection.is_none() => Some("No terminal selection"),
            AppAction::SaveEditor(id)
                if self
                    .editor_id(id)
                    .and_then(|id| self.workspace.tabs.iter().find(|t| t.id == id))
                    .is_some_and(|t| t.saving) =>
            {
                Some("Save in progress")
            }
            AppAction::SaveEditor(id) | AppAction::CloseEditor(id)
                if self.editor_id(id).is_none() =>
            {
                Some("No open editor")
            }
            AppAction::FindEditor
            | AppAction::ReplaceEditor
            | AppAction::EditFile
            | AppAction::Compare(_)
                if self.workspace.tabs.is_empty() =>
            {
                Some("No open editor")
            }
            AppAction::ExplorerParent
                if self
                    .workspace
                    .directory
                    .as_ref()
                    .and_then(|p| p.parent())
                    .is_none() =>
            {
                Some("No parent folder")
            }
            _ => None,
        }
    }

    pub(super) fn dispatch(&mut self, action: AppAction, tx: &mut Tx) {
        if let Some(reason) = self.action_disabled(action) {
            self.message = reason.into();
            return;
        }
        self.workspace_menu = false;
        match action {
            AppAction::CommandPalette => self.open_palette(tx),
            AppAction::NewTerminal => {
                tx.blur();
                self.workspace.layout.terminal = true;
                self.new_tab();
            }
            AppAction::EndTerminal => {
                tx.blur();
                self.pending = Some(Pending::Tab(self.active));
            }
            AppAction::ActivateTerminal(index) => {
                if index < self.sessions.len() {
                    tx.blur();
                    self.active = index;
                    self.workspace.terminal_focus = true;
                    self.workspace.layout.terminal = true;
                    self.selection = None;
                }
            }
            AppAction::ToggleExplorer => self.toggle_files(),
            AppAction::ShowTerminal => {
                self.workspace.layout.terminal = true;
                self.workspace.terminal_focus = true;
                self.workspace.explorer_revealed = false;
                tx.blur();
            }
            AppAction::ToggleTerminal => {
                self.workspace.layout.terminal = !self.workspace.layout.terminal;
                if !self.workspace.layout.terminal
                    && (self.settings_open || !self.workspace.tabs.is_empty())
                {
                    self.workspace.layout.editor = true;
                    self.workspace.terminal_focus = false;
                }
            }
            AppAction::ToggleEditor => {
                if self.workspace.tabs.is_empty()
                    && !self.settings_open
                    && !self.workspace.show_empty_editor
                {
                    self.workspace.show_empty_editor = true;
                    self.workspace.layout.editor = true;
                    self.workspace.terminal_focus = false;
                } else if self.workspace.body_width < 880.0 && self.workspace.terminal_focus {
                    self.workspace.layout.editor = true;
                    self.workspace.terminal_focus = false;
                } else {
                    self.workspace.layout.editor = !self.workspace.layout.editor;
                }
                if !self.workspace.layout.editor {
                    self.workspace.layout.terminal = true;
                    self.workspace.terminal_focus = true;
                    tx.blur();
                }
            }
            AppAction::FindTerminal => {
                self.search = true;
                tx.focus_when_shown("search-query", true);
            }
            AppAction::FindEditor | AppAction::ReplaceEditor => {
                self.workspace.layout.editor = true;
                self.workspace.terminal_focus = false;
                self.workspace.find = true;
                if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
                    tab.mode = workspace::View::Edit;
                }
                tx.focus_when_shown(
                    if matches!(action, AppAction::ReplaceEditor) {
                        "editor-replace"
                    } else {
                        "editor-find"
                    },
                    true,
                );
            }
            AppAction::Settings => self.open_settings(tx),
            AppAction::OpenSettingsJson => self.open_settings_json(),
            AppAction::CloseSettings => {
                self.settings_open = false;
                self.settings = false;
                if self.workspace.tabs.is_empty() {
                    self.workspace.terminal_focus = true;
                    tx.blur();
                } else {
                    self.dispatch(AppAction::EditFile, tx);
                }
            }
            AppAction::ActivateEditor(index) => self.activate_editor(index),
            AppAction::CopyTerminal => self.copy(tx),
            AppAction::PasteTerminal => {
                self.workspace.terminal_focus = true;
                tx.blur();
                tx.request_paste();
            }
            AppAction::SaveEditor(id) => {
                if let Some(id) = self.editor_id(id) {
                    self.save_file(id, false, None);
                    if self
                        .workspace
                        .tabs
                        .iter()
                        .any(|t| t.id == id && t.mode == workspace::View::Edit)
                    {
                        self.workspace.terminal_focus = false;
                        self.workspace.layout.editor = true;
                        tx.focus_when_shown(format!("file-editor-{id}"), true);
                    }
                }
            }
            AppAction::CloseEditor(id) => {
                if let Some(id) = self.editor_id(id) {
                    self.request_action(workspace::Action::Editor(id), tx);
                }
            }
            AppAction::EditFile => {
                if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
                    tab.mode = workspace::View::Edit;
                    self.workspace.layout.editor = true;
                    self.workspace.terminal_focus = false;
                    tx.focus_when_shown(format!("file-editor-{}", tab.id), true);
                }
            }
            AppAction::Compare(mode) => {
                if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
                    tab.mode = workspace::View::Diff(mode);
                }
                self.workspace.layout.editor = true;
                self.workspace.terminal_focus = false;
                tx.blur();
                self.invalidate_diff(true);
            }
            AppAction::CloseWindow => self.request_action(workspace::Action::Window, tx),
            AppAction::Quit => self.request_action(workspace::Action::Quit, tx),
            AppAction::ExplorerParent => {
                if let Some(parent) = self
                    .workspace
                    .directory
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(PathBuf::from)
                {
                    self.navigate(parent);
                }
            }
            AppAction::RefreshExplorer => {
                self.workspace.last_scan = Instant::now() - Duration::from_secs(3)
            }
            AppAction::ToggleFollow => {
                self.workspace.layout.follow = !self.workspace.layout.follow;
                self.workspace.last_follow = None;
            }
            AppAction::ToggleHidden => {
                self.workspace.layout.hidden = !self.workspace.layout.hidden;
                self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
            }
        }
    }
}
