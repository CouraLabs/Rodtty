//! Shared application actions. Extend `register` for future slices; controls and the
//! palette use `dispatch`, which keeps save/close policy at the existing workspace boundary.
use super::*;
use crate::git::Comparison;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum AppAction {
    CreateFile,
    CreateFolder,
    CopyFile,
    TrashFile,
    ToggleDeletionConfirmation,
    RenameFile,
    MoveFile,
    RevealFileOperation,
    CommandPalette,
    NewTerminal,
    EndTerminal,
    CloseTerminal(usize),
    MoveTab(tabs::Target, usize),
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
    ExplorerTerminalRoot,
    ExplorerReveal,
    ExplorerFocus,
    ExplorerCollapseAll,
    ExplorerOpenSelection,
    PreviewExplorerSelection,
    ExplorerParent,
    RefreshExplorer,
    ToggleFollow,
    ToggleHidden,
    ToggleIgnore,
    SearchFiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ShortcutContext {
    Global,
    Terminal,
    Editor,
    Settings,
}
impl ShortcutContext {
    pub fn overlaps(self, other: Self) -> bool {
        self == Self::Global || other == Self::Global || self == other
    }
}

pub(super) struct Command {
    pub id: &'static str,
    pub title: &'static str,
    pub shortcut: &'static str,
    pub action: AppAction,
    pub context: ShortcutContext,
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
            "trash-file",
            "Explorer: Move Selected Item to Trash",
            "",
            AppAction::TrashFile,
        ),
        (
            "reveal-file-operation",
            "Explorer: Reveal Last File Operation",
            "",
            AppAction::RevealFileOperation,
        ),
        (
            "create-file",
            "Explorer: New File…",
            "",
            AppAction::CreateFile,
        ),
        (
            "create-folder",
            "Explorer: New Folder…",
            "",
            AppAction::CreateFolder,
        ),
        (
            "copy-file",
            "Explorer: Copy Selected Item To…",
            "",
            AppAction::CopyFile,
        ),
        (
            "rename-file",
            "Explorer: Rename Selected Item…",
            "",
            AppAction::RenameFile,
        ),
        (
            "move-file",
            "Explorer: Move Selected Item To…",
            "",
            AppAction::MoveFile,
        ),
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
            "explorer-terminal-root",
            "Explorer: Use Terminal Folder as Root",
            "",
            AppAction::ExplorerTerminalRoot,
        ),
        (
            "explorer-reveal",
            "Explorer: Reveal Terminal Folder",
            "",
            AppAction::ExplorerReveal,
        ),
        (
            "explorer-focus",
            "Explorer: Focus Tree",
            "",
            AppAction::ExplorerFocus,
        ),
        (
            "explorer-collapse",
            "Explorer: Collapse All Folders",
            "",
            AppAction::ExplorerCollapseAll,
        ),
        (
            "explorer-open",
            "Explorer: Open Selected Item",
            "",
            AppAction::ExplorerOpenSelection,
        ),
        (
            "explorer-parent",
            "Explorer: Open Parent Folder",
            "",
            AppAction::ExplorerParent,
        ),
        (
            "explorer-search",
            "Explorer: Search Files",
            "",
            AppAction::SearchFiles,
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
            "explorer-ignore",
            "Explorer: Toggle Ignore Rules",
            "",
            AppAction::ToggleIgnore,
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
            app("Ctrl+W", "Cmd+W"),
            AppAction::CloseSettings,
        ),
        (
            "close-window",
            "Window: Close Window",
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
            context: match action {
                AppAction::CopyTerminal
                | AppAction::PasteTerminal
                | AppAction::EndTerminal
                | AppAction::FindTerminal => ShortcutContext::Terminal,
                AppAction::SaveEditor(_)
                | AppAction::CloseEditor(_)
                | AppAction::FindEditor
                | AppAction::ReplaceEditor => ShortcutContext::Editor,
                AppAction::CloseSettings => ShortcutContext::Settings,
                _ => ShortcutContext::Global,
            },
        });
    }
}

impl State {
    pub(super) fn action_help(&self, action: AppAction, title: &str) -> String {
        self.commands
            .iter()
            .find(|c| c.action == action)
            .map_or_else(|| title.to_owned(), |c| self.shortcut_help(c.id, title))
    }
    fn editor_id(&self, id: Option<u64>) -> Option<u64> {
        id.or_else(|| self.workspace.tabs.get(self.workspace.active).map(|t| t.id))
    }
    /// Whether an editor pane has real content to draw: an open editor tab or
    /// a Settings tab. Used to decide whether hiding the terminal would strand
    /// the workspace behind an empty pane.
    fn editor_pane_contents(&self) -> bool {
        !self.workspace.tabs.is_empty() || self.settings_open
    }

    pub(super) fn action_disabled(&self, action: AppAction) -> Option<&'static str> {
        if self.removal_background_busy()
            && !matches!(
                action,
                AppAction::ShowTerminal
                    | AppAction::ToggleTerminal
                    | AppAction::ActivateTerminal(_)
                    | AppAction::CopyTerminal
                    | AppAction::PasteTerminal
            )
        {
            return Some(
                "Files are protected while the operation finishes; terminal input remains available",
            );
        }
        match action {
            AppAction::TrashFile if self.settings => Some("Select a file in the explorer first"),
            AppAction::RevealFileOperation if self.workspace.operation_result.is_none() => {
                Some("No completed file operation")
            }
            AppAction::CreateFile
            | AppAction::CreateFolder
            | AppAction::CopyFile
            | AppAction::TrashFile
            | AppAction::RenameFile
            | AppAction::MoveFile
                if self.workspace.operation_busy || self.workspace.removal.is_some() =>
            {
                Some("A file operation is in progress")
            }
            AppAction::CopyFile
            | AppAction::TrashFile
            | AppAction::RenameFile
            | AppAction::MoveFile
                if self.selected_entry().is_none() =>
            {
                Some("Select a file or folder first")
            }
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
            AppAction::CopyTerminal if self.selection.is_none_or(|(a, b)| a == b) => {
                Some("No terminal selection")
            }
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
        if matches!(
            action,
            AppAction::NewTerminal
                | AppAction::ActivateTerminal(_)
                | AppAction::ShowTerminal
                | AppAction::ToggleTerminal
                | AppAction::ToggleEditor
                | AppAction::FindTerminal
                | AppAction::PasteTerminal
        ) {
            self.supersede_pending_preview();
        }
        match action {
            AppAction::TrashFile => self.begin_removal(tx),
            AppAction::ToggleDeletionConfirmation => {
                self.save_config(|c| c.explorer.confirm_deletion = !c.explorer.confirm_deletion);
            }
            AppAction::CreateFile => {
                self.begin_file_operation(crate::files::operations::FileOperation::CreateFile, tx)
            }
            AppAction::CreateFolder => {
                self.begin_file_operation(crate::files::operations::FileOperation::CreateFolder, tx)
            }
            AppAction::CopyFile => {
                if let Some(entry) = self.selected_entry() {
                    self.begin_file_operation(
                        crate::files::operations::FileOperation::Copy { source: entry.path },
                        tx,
                    );
                }
            }
            AppAction::RenameFile | AppAction::MoveFile => {
                if let Some(entry) = self.selected_entry() {
                    self.begin_file_operation(
                        crate::files::operations::FileOperation::Move { source: entry.path },
                        tx,
                    );
                    tx.focus_when_shown(
                        if matches!(action, AppAction::RenameFile) {
                            "operation-name"
                        } else {
                            "operation-destination"
                        },
                        true,
                    );
                }
            }
            AppAction::RevealFileOperation => {
                if let Some(path) = self.workspace.operation_result.clone() {
                    if self
                        .workspace
                        .directory
                        .as_ref()
                        .is_none_or(|root| !path.starts_with(root))
                        && let Some(parent) = path.parent()
                    {
                        self.navigate(parent.to_owned());
                    }
                    self.workspace.layout.explorer = true;
                    self.reveal_file_result(path, tx);
                }
            }
            AppAction::CommandPalette => self.open_palette(tx),
            AppAction::NewTerminal => {
                tx.blur();
                self.workspace.layout.terminal = true;
                self.new_tab();
            }
            AppAction::EndTerminal => {
                tx.blur();
                if let Some(session) = self.active() {
                    self.dispatch(AppAction::CloseTerminal(tabs::session_id(session)), tx);
                }
            }
            AppAction::CloseTerminal(id) => {
                if self.sessions.iter().any(|s| tabs::session_id(s) == id) {
                    tx.blur();
                    let confirm = match self.config.behavior.terminal_tab_close {
                        crate::config::TerminalClosePolicy::ConfirmRunning => self
                            .sessions
                            .iter()
                            .find(|s| tabs::session_id(s) == id)
                            .is_some_and(Session::has_running_commands),
                        crate::config::TerminalClosePolicy::AlwaysConfirm => true,
                        crate::config::TerminalClosePolicy::CloseImmediately => false,
                    };
                    self.pending = Some(Pending::Tab(id));
                    if !confirm {
                        self.confirm(tx);
                    }
                }
            }
            AppAction::MoveTab(target, to) => self.move_tab(target, to),
            AppAction::ActivateTerminal(index) => {
                if index < self.sessions.len() {
                    tx.blur();
                    if index != self.active {
                        self.workspace.note_session_switch();
                    }
                    self.active = index;
                    self.workspace.set_terminal_focus(true);
                    self.workspace.layout.terminal = true;
                    self.selection = None;
                }
            }
            AppAction::ToggleExplorer => self.toggle_files_with_focus(tx),
            AppAction::ShowTerminal => {
                self.workspace.layout.terminal = true;
                self.workspace.set_terminal_focus(true);
                self.workspace.explorer_revealed = false;
                tx.blur();
            }
            AppAction::ToggleTerminal => {
                self.workspace.layout.terminal = !self.workspace.layout.terminal;
                if !self.workspace.layout.terminal {
                    // The terminal can only hide behind a real editor pane: with no
                    // editor tab or Settings, it stays visible instead of an empty pane.
                    if self.editor_pane_contents() {
                        self.workspace.layout.editor = true;
                        self.workspace.set_terminal_focus(false);
                    } else {
                        self.workspace.layout.terminal = true;
                        self.message = "Open a file or Settings to hide the terminal".into();
                    }
                }
            }
            AppAction::ToggleEditor => {
                if self.workspace.tabs.is_empty() && !self.settings_open {
                    // Nothing to show in the pane: closing the terminal behind an
                    // empty editor would strand the workspace.
                    self.message = "Open a file or Settings to fill the editor pane".into();
                } else if self.workspace.body_width < 880.0 && self.workspace.terminal_focus() {
                    self.workspace.layout.editor = true;
                    self.workspace.set_terminal_focus(false);
                } else {
                    self.workspace.layout.editor = !self.workspace.layout.editor;
                }
                if !self.workspace.layout.editor {
                    self.workspace.layout.terminal = true;
                    self.workspace.set_terminal_focus(true);
                    tx.blur();
                }
            }
            AppAction::FindTerminal => self.open_terminal_search(tx),
            AppAction::FindEditor | AppAction::ReplaceEditor => {
                self.workspace.layout.editor = true;
                self.workspace.set_terminal_focus(false);
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
                    self.workspace.set_terminal_focus(true);
                    tx.blur();
                } else {
                    self.dispatch(AppAction::EditFile, tx);
                }
            }
            AppAction::ActivateEditor(index) => self.activate_editor(index),
            AppAction::CopyTerminal => self.copy(tx),
            AppAction::PasteTerminal => {
                self.workspace.set_terminal_focus(true);
                self.workspace.layout.terminal = true;
                tx.blur();
                self.request_terminal_paste(tx);
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
                        self.workspace.set_terminal_focus(false);
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
                    let id = tab.id;
                    self.workspace.layout.editor = true;
                    self.workspace.set_terminal_focus(false);
                    tx.focus_when_shown(format!("file-editor-{id}"), true);
                }
            }
            AppAction::Compare(mode) => {
                if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
                    tab.mode = workspace::View::Diff(mode);
                }
                self.workspace.layout.editor = true;
                self.workspace.set_terminal_focus(false);
                tx.blur();
                self.invalidate_diff(true);
            }
            AppAction::CloseWindow => self.request_action(workspace::Action::Window, tx),
            AppAction::Quit => self.request_action(workspace::Action::Quit, tx),
            AppAction::ExplorerTerminalRoot => {
                let cwd = self
                    .active()
                    .map(Session::cwd)
                    .unwrap_or_else(|| self.directory.clone());
                self.navigate(cwd);
                self.workspace.layout.explorer = true;
            }
            AppAction::ExplorerReveal => {
                self.workspace.layout.explorer = true;
                self.reveal_terminal(tx);
            }
            AppAction::ExplorerFocus => {
                self.workspace.layout.explorer = true;
                self.workspace.set_terminal_focus(false);
                tx.focus_when_shown("file-list", false);
            }
            AppAction::ExplorerCollapseAll => self.collapse_tree(),
            AppAction::ExplorerOpenSelection => self.open_tree_selection(),
            AppAction::PreviewExplorerSelection => self.preview_tree_selection(),
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
                self.save_config(|c| c.explorer.follow_terminal = !c.explorer.follow_terminal);
            }
            AppAction::ToggleHidden => {
                self.save_config(|c| c.explorer.show_hidden = !c.explorer.show_hidden);
            }
            AppAction::ToggleIgnore => {
                self.save_config(|c| c.explorer.respect_ignore = !c.explorer.respect_ignore);
            }
            AppAction::SearchFiles => self.open_explorer_search(tx),
        }
    }
}
