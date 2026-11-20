use super::*;

impl State {
    pub(crate) fn dispatch(&mut self, action: AppAction, tx: &mut Tx) {
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
            AppAction::AutomationApprove => self.automation_approve_pending(),
            AppAction::AutomationPauseAll => self.automation_set_all(
                &[crate::automation::grants::Status::Approved],
                crate::automation::grants::Status::Paused,
            ),
            AppAction::AutomationResumeAll => self.automation_set_all(
                &[crate::automation::grants::Status::Paused],
                crate::automation::grants::Status::Approved,
            ),
            AppAction::AutomationRevokeAll => self.automation_set_all(
                &[
                    crate::automation::grants::Status::Approved,
                    crate::automation::grants::Status::Paused,
                    crate::automation::grants::Status::Pending,
                ],
                crate::automation::grants::Status::Revoked,
            ),
            AppAction::AutomationTakeOver => self.automation_take_over(),
            AppAction::CodexApprove => self.codex_decide("accept"),
            AppAction::ShowServices => self.show_services(),
            AppAction::EditTasks => self.edit_tasks(),
            AppAction::FolderTerminal(i) => self.folder_open_terminal(usize::from(i)),
            AppAction::FolderChange(i) => self.folder_change_directory(usize::from(i)),
            AppAction::FocusSession(i) => self.focus_listed_session(usize::from(i)),
            AppAction::SwitchWorkspace(i) => self.switch_listed_workspace(usize::from(i), tx),
            AppAction::AddWorkspace => self.add_workspace(),
            AppAction::RemoveWorkspace => self.remove_active_workspace(),
            AppAction::ClearFolderHistory => {
                self.folder_history.clear();
                self.refresh_folder_commands();
                self.message = "Folder history cleared.".into();
            }
            AppAction::TogglePassthrough => {
                let id = self.active().map(Session::id);
                self.passthrough = if self.passthrough.is_some() { None } else { id };
                let exit = self.shortcut_label("terminal-passthrough");
                self.message = match self.passthrough {
                    Some(_) => format!(
                        "Keyboard passthrough on: Rodtty shortcuts go to this terminal. {exit} exits."
                    ),
                    None => "Keyboard passthrough off.".into(),
                };
            }
            AppAction::TogglePasteConfirmation => {
                self.save_config(|c| {
                    c.behavior.confirm_multiline_paste = !c.behavior.confirm_multiline_paste
                });
                self.message = if self.config.behavior.confirm_multiline_paste {
                    "Multiline terminal pastes now ask before sending.".into()
                } else {
                    "Multiline terminal pastes are sent without asking.".into()
                };
            }
            AppAction::RunTask(index) => self.run_task_index(usize::from(index)),
            AppAction::CheckEnvironment => self.check_environment(),
            AppAction::FilterTerminal => self.open_log_filter(tx),
            AppAction::SendToTerminal => self.begin_send_to_terminal(),
            AppAction::PinRun => self.pin_last_run(),
            AppAction::SplitRight => self.split_active(true),
            AppAction::SplitDown => self.split_active(false),
            AppAction::FocusOtherPane => self.focus_other_pane(),
            AppAction::CloseTab(id) => {
                // A split tab ends both of its sessions, asking as the
                // tab-close policy asks for either one's running command.
                let ids = self.tab_sessions(id);
                let other = ids.iter().copied().find(|x| *x != id);
                let running = ids.iter().any(|x| {
                    self.sessions
                        .iter()
                        .find(|s| s.id() == *x)
                        .is_some_and(Session::has_running_commands)
                });
                let confirm = match self.config.behavior.terminal_tab_close {
                    crate::config::TerminalClosePolicy::ConfirmRunning => running,
                    crate::config::TerminalClosePolicy::AlwaysConfirm => true,
                    crate::config::TerminalClosePolicy::CloseImmediately => false,
                };
                tx.blur();
                self.closing_partner = other.map(|o| (id, o));
                self.pending = Some(Pending::Tab(id));
                if !confirm {
                    self.confirm(tx);
                }
            }
            AppAction::RecoverDrafts => {
                self.drafts.dialog =
                    !self.drafts.pending.is_empty() || !self.drafts.unreadable.is_empty();
                if !self.drafts.dialog {
                    self.message = "No unsaved edits from an earlier run are waiting.".into();
                }
            }
            AppAction::ToggleChanges => self.toggle_changes_view(),
            AppAction::NextChange => self.step_change(true, tx),
            AppAction::PreviousChange => self.step_change(false, tx),
            AppAction::CompareRuns => self.compare_with_pinned(),
            AppAction::ClearPinnedRun => self.clear_pinned_run(),
            AppAction::PreviousCommand => self.step_command(true),
            AppAction::NextCommand => self.step_command(false),
            AppAction::ToggleCommandNotifications => {
                self.save_config(|c| {
                    c.behavior.command_notifications = !c.behavior.command_notifications
                });
                self.message = if self.config.behavior.command_notifications {
                    "Notifications on for commands that finish in the background.".into()
                } else {
                    "Command notifications off.".into()
                };
            }
            AppAction::CodexDecline => self.codex_decide("decline"),
            #[cfg(feature = "browser")]
            AppAction::OpenBrowser => self.open_browser_view(tx),
            #[cfg(feature = "browser")]
            AppAction::CloseBrowser => self.close_browser_view(tx),
            #[cfg(not(feature = "browser"))]
            AppAction::OpenBrowser | AppAction::CloseBrowser => {
                self.message = "This Rodtty build has no embedded browser.".into()
            }
            AppAction::AutomationReturn => self.automation_return_target(),
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
            AppAction::CommandPalette => {
                self.refresh_task_commands();
                self.refresh_folder_commands();
                self.refresh_session_commands();
                self.open_palette(tx)
            }
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
                    self.closing_partner = None;
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
                if !self.editor_pane_contents() {
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
                let shown = self.settings;
                self.settings = false;
                if self.search_tab {
                    // The Search tab shows; closing Settings leaves it there.
                } else if shown && self.search_tab_open && self.workspace.tabs.is_empty() {
                    self.search_tab = true;
                } else if self.workspace.tabs.is_empty() {
                    self.workspace.set_terminal_focus(true);
                    tx.blur();
                } else {
                    self.dispatch(AppAction::EditFile, tx);
                }
            }
            AppAction::ActivateEditor(index) => self.activate_editor(index),
            AppAction::CopyTerminal => self.copy(tx),
            AppAction::CopyTerminalMarkdown => self.copy_markdown(tx),
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
            AppAction::ShowSearch => {
                tx.blur();
                self.show_search_tab();
            }
            AppAction::CloseSearch => self.close_explorer_search(tx),
        }
    }
}
