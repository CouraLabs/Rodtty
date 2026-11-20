//! Shared application actions. Extend `register` for future slices; controls and the
//! palette use `dispatch`, which keeps save/close policy at the existing workspace boundary.
use super::*;
use crate::git::Comparison;
use std::time::{Duration, Instant};

mod dispatch;
mod registry;
pub(crate) use registry::register;

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
    ShowSearch,
    CloseSearch,
    AutomationApprove,
    AutomationPauseAll,
    AutomationResumeAll,
    AutomationRevokeAll,
    AutomationTakeOver,
    AutomationReturn,
    OpenBrowser,
    CodexApprove,
    ShowServices,
    EditTasks,
    TogglePasteConfirmation,
    TogglePassthrough,
    FolderTerminal(u16),
    FolderChange(u16),
    ClearFolderHistory,
    FocusSession(u16),
    CopyTerminalMarkdown,
    SwitchWorkspace(u16),
    AddWorkspace,
    RemoveWorkspace,
    RunTask(u16),
    CheckEnvironment,
    CodexDecline,
    CloseBrowser,
    PreviousCommand,
    NextCommand,
    ToggleCommandNotifications,
    FilterTerminal,
    SendToTerminal,
    PinRun,
    CompareRuns,
    ClearPinnedRun,
    ToggleChanges,
    NextChange,
    PreviousChange,
    RecoverDrafts,
    SplitRight,
    SplitDown,
    FocusOtherPane,
    CloseTab(usize),
    RemoteEnable,
    RemoteDisable,
    RemotePair,
    RemoteRevoke,
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
        !self.workspace.tabs.is_empty() || self.settings_open || self.search_tab_open
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
            AppAction::TrashFile if self.settings || self.search_tab => {
                Some("Select a file in the explorer first")
            }
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
                if self.settings || self.search_tab =>
            {
                Some("Select a file editor first")
            }
            AppAction::CloseSettings if !self.settings_open => Some("Settings is not open"),
            AppAction::CloseSearch | AppAction::ShowSearch if !self.search_tab_open => {
                Some("Search is not open")
            }
            AppAction::NewTerminal if !self.config_valid => {
                Some("Fix settings before starting a session")
            }
            AppAction::EndTerminal | AppAction::FindTerminal | AppAction::PasteTerminal
                if self.active().is_none() =>
            {
                Some("No active terminal session")
            }
            AppAction::CopyTerminal | AppAction::CopyTerminalMarkdown
                if self.selection.is_none_or(|(a, b)| a == b) =>
            {
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
}
