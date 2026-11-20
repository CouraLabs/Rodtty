//! The running sessions overview and workspace switching, as palette
//! entries: `Session: <title> — <folder> · busy|idle|ended` focuses that
//! session; `Workspace: Switch to <name>` switches. Rows are built each time
//! the palette opens and resolve by stable identity when chosen, so a
//! session that ended meanwhile is reported instead of focusing another.
use super::*;

const LISTED: usize = 100;

impl State {
    pub(crate) fn refresh_session_commands(&mut self) {
        self.commands
            .retain(|c| !c.id.starts_with("session-") && !c.id.starts_with("workspace-switch-"));
        self.session_rows = self.sessions.iter().take(LISTED).map(Session::id).collect();
        let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        for (index, session) in self.sessions.iter().take(LISTED).enumerate() {
            let state = if session
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .exit
                .is_some()
            {
                "ended"
            } else if session.has_running_commands() {
                "busy"
            } else {
                "idle"
            };
            let workspace = registry
                .containing(&session.initial_cwd)
                .map(|w| format!(" [{}]", registry.label(&w.id)))
                .unwrap_or_default();
            let agent = session
                .agent_status()
                .map(|a| format!(" · agent {}", a.label()))
                .unwrap_or_default();
            let title = format!(
                "Session: {} — {} · {state}{agent}{workspace}",
                session.tab_title(),
                window::tilde_path(&session.cwd())
            );
            self.commands.push(actions::Command {
                id: Box::leak(format!("session-{index}").into_boxed_str()),
                title: Box::leak(title.into_boxed_str()),
                shortcut: "",
                action: AppAction::FocusSession(index as u16),
                context: actions::ShortcutContext::Global,
            });
        }
        self.workspace_rows = registry.workspaces.iter().map(|w| w.id.clone()).collect();
        let (active, _) = self.active_workspace();
        for (index, workspace) in registry.workspaces.iter().enumerate() {
            let current = if workspace.id == active {
                " (active)"
            } else {
                ""
            };
            let title = format!(
                "Workspace: Switch to {} — {}{current}",
                registry.label(&workspace.id),
                window::tilde_path(&workspace.root)
            );
            self.commands.push(actions::Command {
                id: Box::leak(format!("workspace-switch-{index}").into_boxed_str()),
                title: Box::leak(title.into_boxed_str()),
                shortcut: "",
                action: AppAction::SwitchWorkspace(index as u16),
                context: actions::ShortcutContext::Global,
            });
        }
    }

    pub(crate) fn focus_listed_session(&mut self, index: usize) {
        let Some(id) = self.session_rows.get(index).copied() else {
            return;
        };
        match self.sessions.iter().position(|s| s.id() == id) {
            Some(position) => {
                self.active = position;
                self.workspace.layout.terminal = true;
                self.workspace.set_terminal_focus(true);
                self.workspace.note_session_switch();
                self.selection = None;
            }
            None => self.message = "That session has ended.".into(),
        }
    }

    pub(crate) fn switch_listed_workspace(&mut self, index: usize, tx: &mut Tx) {
        let Some(id) = self.workspace_rows.get(index).cloned() else {
            return;
        };
        if let Err(error) = self.switch_workspace(&id, tx) {
            self.message = error;
        }
    }

    /// Register the explorer's folder as a named workspace.
    pub(crate) fn add_workspace(&mut self) {
        let folder = self
            .workspace
            .directory
            .clone()
            .unwrap_or_else(|| self.directory.clone());
        let mut registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        self.message = match registry
            .create("", &folder)
            .and_then(|w| registry.save(&self.root).map(|_| w))
        {
            Ok(workspace) => {
                let _ = crate::workspaces::save_selection(&self.root, Some(&workspace.id));
                format!("Added workspace {}.", registry.label(&workspace.id))
            }
            Err(error) => format!("Could not add the workspace: {error:#}"),
        };
    }

    /// Forget the active workspace. Its folder and files stay; sessions
    /// started there keep running and are listed in the sessions overview.
    pub(crate) fn remove_active_workspace(&mut self) {
        let (id, root) = self.active_workspace();
        let mut registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        let Some(removed) = registry.remove(&id) else {
            self.message = "The explorer's folder is not a saved workspace.".into();
            return;
        };
        let live = self
            .sessions
            .iter()
            .filter(|s| s.initial_cwd.starts_with(&root))
            .count();
        let dirty = self
            .workspace
            .tabs
            .iter()
            .filter(|t| t.dirty() && t.document.path.starts_with(&root))
            .count();
        self.message = match registry.save(&self.root) {
            Ok(()) => format!(
                "Removed workspace {} (its files are untouched). {live} session(s) and {dirty} unsaved file(s) there stay open until you close them.",
                removed.name
            ),
            Err(error) => format!("Could not remove the workspace: {error:#}"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_and_busy_sessions_with_one_title_are_distinct_and_selection_follows_identity() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.shell = Some(crate::test_support::shell_binary().unwrap());
        state.start_tab(root.path().into());
        state.start_tab(root.path().into());
        for s in &state.sessions {
            s.wait_for("rodtty:", std::time::Duration::from_secs(15))
                .unwrap();
        }
        state.sessions[1].send(b"sleep 20\r".to_vec()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        state.refresh_session_commands();
        let rows: Vec<&str> = state
            .commands
            .iter()
            .filter(|c| c.id.starts_with("session-"))
            .map(|c| c.title)
            .collect();
        assert!(
            rows[0].ends_with("· idle") && rows[1].ends_with("· busy"),
            "{rows:?}"
        );
        state.active = 0;
        state.focus_listed_session(1);
        assert_eq!(state.active, 1);
        // The first session ends while the overview is open: choosing it
        // reports that instead of focusing another session.
        let gone = state.sessions.remove(0);
        drop(gone);
        state.active = 0;
        state.focus_listed_session(0);
        assert!(state.message.contains("ended"));
        state.focus_listed_session(1);
        assert_eq!(
            state.active, 0,
            "the busy session is still found by identity"
        );
    }
}
