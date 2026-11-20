use super::*;

/// Registrations are stable IDs plus visible metadata and an application action. Later
/// slices can append registrations without changing palette search, input or rendering.
/// Default bindings follow VS Code's for the same actions on each platform.
pub(crate) fn register(commands: &mut Vec<Command>) {
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
            app("Ctrl+Shift+`", "Ctrl+Shift+`"),
            AppAction::NewTerminal,
        ),
        (
            "end-terminal",
            "Terminal: End Session",
            "",
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
            app("Ctrl+B", "Cmd+B"),
            AppAction::ToggleExplorer,
        ),
        (
            "toggle-terminal",
            "View: Toggle Terminal",
            app("Ctrl+`", "Ctrl+`"),
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
            app("Ctrl+F", "Cmd+F"),
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
            app("Ctrl+H", "Cmd+Alt+F"),
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
            app("Ctrl+Shift+E", "Cmd+Shift+E"),
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
            "Search: Files and Terminals",
            app("Ctrl+Shift+F", "Cmd+Shift+F"),
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
            app("Ctrl+Comma", "Cmd+Comma"),
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
            app("Ctrl+Shift+W", "Cmd+Shift+W"),
            AppAction::CloseWindow,
        ),
        (
            "command-palette",
            "View: Show Command Palette",
            app("Ctrl+Shift+P", "Cmd+Shift+P"),
            AppAction::CommandPalette,
        ),
        (
            "open-browser",
            "Browser: Open Browser",
            "",
            AppAction::OpenBrowser,
        ),
        (
            "close-browser",
            "Browser: Close Browser",
            "",
            AppAction::CloseBrowser,
        ),
        (
            "copy-terminal-markdown",
            "Terminal: Copy Selection as Markdown",
            "",
            AppAction::CopyTerminalMarkdown,
        ),
        (
            "add-workspace",
            "Workspaces: Add Explorer Folder as Workspace",
            "",
            AppAction::AddWorkspace,
        ),
        (
            "remove-workspace",
            "Workspaces: Remove Active Workspace (keeps its files)",
            "",
            AppAction::RemoveWorkspace,
        ),
        (
            "clear-folder-history",
            "Folders: Clear Recent Folders",
            "",
            AppAction::ClearFolderHistory,
        ),
        (
            "terminal-passthrough",
            "Terminal: Toggle Keyboard Passthrough",
            "Ctrl+Shift+F12",
            AppAction::TogglePassthrough,
        ),
        (
            "remote-enable",
            "Remote: Enable Access over Tailscale",
            "",
            AppAction::RemoteEnable,
        ),
        (
            "remote-disable",
            "Remote: Disable Access",
            "",
            AppAction::RemoteDisable,
        ),
        (
            "remote-pair",
            "Remote: Pair Device…",
            "",
            AppAction::RemotePair,
        ),
        (
            "remote-revoke",
            "Remote: Disconnect and Revoke All Devices",
            "",
            AppAction::RemoteRevoke,
        ),
        (
            "split-right",
            "Terminal: Split Right",
            app("Ctrl+Shift+5", "Cmd+\\"),
            AppAction::SplitRight,
        ),
        (
            "split-down",
            "Terminal: Split Down",
            "",
            AppAction::SplitDown,
        ),
        (
            "focus-other-pane",
            "Terminal: Focus Other Pane",
            app("Ctrl+Alt+O", "Cmd+Alt+O"),
            AppAction::FocusOtherPane,
        ),
        (
            "recover-drafts",
            "File: Recover Unsaved Edits…",
            "",
            AppAction::RecoverDrafts,
        ),
        (
            "toggle-changes",
            "Git: Show Changes",
            app("Ctrl+Shift+G", "Ctrl+Shift+G"),
            AppAction::ToggleChanges,
        ),
        (
            "next-change",
            "Git: Open Next Changed File",
            "",
            AppAction::NextChange,
        ),
        (
            "previous-change",
            "Git: Open Previous Changed File",
            "",
            AppAction::PreviousChange,
        ),
        (
            "pin-run",
            "Terminal: Pin Last Command Output for Comparison",
            "",
            AppAction::PinRun,
        ),
        (
            "compare-runs",
            "Terminal: Compare Last Command with Pinned Run",
            "",
            AppAction::CompareRuns,
        ),
        (
            "clear-pinned-run",
            "Terminal: Clear Pinned Run",
            "",
            AppAction::ClearPinnedRun,
        ),
        (
            "send-to-terminal",
            "Editor: Send Selection or Diff Hunk to Terminal…",
            "",
            AppAction::SendToTerminal,
        ),
        (
            "filter-terminal",
            "Terminal: Filter Output…",
            app("Ctrl+Shift+L", "Cmd+Shift+L"),
            AppAction::FilterTerminal,
        ),
        (
            "previous-command",
            "Terminal: Previous Command",
            app("Ctrl+Up", "Cmd+Up"),
            AppAction::PreviousCommand,
        ),
        (
            "next-command",
            "Terminal: Next Command",
            app("Ctrl+Down", "Cmd+Down"),
            AppAction::NextCommand,
        ),
        (
            "toggle-command-notifications",
            "Terminal: Toggle Notifications for Long Commands",
            "",
            AppAction::ToggleCommandNotifications,
        ),
        (
            "toggle-paste-confirmation",
            "Terminal: Toggle Multiline Paste Confirmation",
            "",
            AppAction::TogglePasteConfirmation,
        ),
        (
            "edit-tasks",
            "Tasks: Edit Project Tasks",
            "",
            AppAction::EditTasks,
        ),
        (
            "show-services",
            "Services: Show Local Services",
            "",
            AppAction::ShowServices,
        ),
        (
            "check-environment",
            "Project: Check Environment",
            "",
            AppAction::CheckEnvironment,
        ),
        (
            "codex-approve",
            "Codex: Approve Pending Request",
            "",
            AppAction::CodexApprove,
        ),
        (
            "codex-decline",
            "Codex: Decline Pending Request",
            "",
            AppAction::CodexDecline,
        ),
        (
            "automation-approve",
            "Automation: Approve Pending Client",
            "",
            AppAction::AutomationApprove,
        ),
        (
            "automation-pause",
            "Automation: Pause All Clients",
            "",
            AppAction::AutomationPauseAll,
        ),
        (
            "automation-resume",
            "Automation: Resume Paused Clients",
            "",
            AppAction::AutomationResumeAll,
        ),
        (
            "automation-revoke",
            "Automation: Revoke All Clients",
            "",
            AppAction::AutomationRevokeAll,
        ),
        (
            "automation-take-over",
            "Automation: Take Over Agent Target",
            "",
            AppAction::AutomationTakeOver,
        ),
        (
            "automation-return",
            "Automation: Return Target to Agent",
            "",
            AppAction::AutomationReturn,
        ),
        (
            "quit",
            "Application: Quit Rodtty",
            app("Ctrl+Q", "Cmd+Q"),
            AppAction::Quit,
        ),
    ] {
        if matches!(action, AppAction::OpenBrowser | AppAction::CloseBrowser)
            && !browser_available()
        {
            continue;
        }
        commands.push(Command {
            id,
            title,
            shortcut,
            action,
            context: match action {
                AppAction::CopyTerminal
                | AppAction::PasteTerminal
                | AppAction::EndTerminal
                | AppAction::FindTerminal
                | AppAction::PreviousCommand
                | AppAction::NextCommand => ShortcutContext::Terminal,
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

/// Whether the Browser actions exist in this build: compiled with CEF and
/// shipped with its runtime.
fn browser_available() -> bool {
    #[cfg(feature = "browser")]
    {
        crate::desktop::browser::available()
    }
    #[cfg(not(feature = "browser"))]
    {
        false
    }
}
