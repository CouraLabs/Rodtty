use super::*;

impl State {
    pub(crate) fn file_io_pending(&self) -> bool {
        !self.workspace.opening.is_empty() || self.workspace.tabs.iter().any(|t| t.saving)
    }
    pub(crate) fn remove_file_references(&mut self, path: &std::path::Path) {
        let selected = self
            .workspace
            .tree
            .cursor()
            .map(PathBuf::from)
            .or_else(|| self.workspace.layout.selected_path.clone());
        self.workspace.layout.selected_path = selected.filter(|p| !p.starts_with(path));
        // Old tree nodes retain expansion/cursor keys until rebuilt. Clear them
        // before the next poll can copy removed paths back into workspace state.
        self.workspace.tree.clear();
        if self
            .workspace
            .latest_activation
            .as_ref()
            .is_some_and(|p| p.starts_with(path))
        {
            self.workspace.latest_activation = None;
        }
        if self
            .workspace
            .restore_active
            .as_ref()
            .is_some_and(|p| p.starts_with(path))
        {
            self.workspace.restore_active = None;
        }
        let active = self.workspace.tabs.get(self.workspace.active).map(|t| t.id);
        self.workspace
            .tabs
            .retain(|t| !t.document.path.starts_with(path));
        self.workspace.active = active
            .and_then(|id| self.workspace.tabs.iter().position(|t| t.id == id))
            .unwrap_or_else(|| {
                self.workspace
                    .active
                    .min(self.workspace.tabs.len().saturating_sub(1))
            });
        self.workspace.entries.retain(|e| !e.path.starts_with(path));
        self.workspace
            .layout
            .expanded_folders
            .retain(|p| !p.starts_with(path));
        self.workspace
            .tree_errors
            .retain(|(p, _)| !p.starts_with(path));
        self.workspace.tree_listed.retain(|p| !p.starts_with(path));
        self.workspace.tree_dirty = true;
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        self.workspace.operation_result = None;
        self.workspace.revealed_directory = None;
        self.workspace.pending_reveal = None;
        if self
            .workspace
            .directory
            .as_ref()
            .is_some_and(|p| p.starts_with(path))
        {
            self.workspace.directory = path.parent().map(PathBuf::from);
        }
        if self.workspace.tabs.is_empty() {
            self.workspace.set_terminal_focus(true);
        }
        self.rebuild_tree();
        self.invalidate_diff(true);
        self.persist_file_references();
    }
    pub(crate) fn persist_file_references(&mut self) {
        self.update_workspace_snapshot();
        self.io(Job::Layout {
            root: self.root.clone(),
            layout: self.workspace.layout.clone(),
        });
    }
    pub(crate) fn update_workspace_snapshot(&mut self) {
        self.workspace.layout.empty_editor = self.workspace.show_empty_editor;
        self.workspace.layout.explorer_revealed = self.workspace.explorer_revealed;
        self.workspace.layout.terminals = self.sessions.iter().map(Session::cwd).collect();
        self.workspace.layout.active_terminal = self.active;
        self.workspace.layout.terminal_focus =
            self.workspace.terminal_focus() || self.settings || self.search_tab;
        if let Some(selected) = self.workspace.tree.cursor() {
            self.workspace.layout.selected_path = Some(PathBuf::from(selected));
        }
        self.workspace.layout.explorer_directory = self.workspace.directory.clone();
        self.workspace.layout.files = self
            .workspace
            .tabs
            .iter()
            .filter(|t| !t.preview)
            .map(|t| t.document.path.clone())
            .collect();
        self.workspace.layout.active_file = self
            .workspace
            .tabs
            .get(self.workspace.active)
            .filter(|t| !t.preview)
            .map(|t| t.document.path.clone());
    }
    pub(crate) fn workspace_restoration_ready(&self) -> bool {
        self.workspace.opening.is_empty()
    }
    pub(crate) fn workspace_setup(&mut self) {
        self.workspace.worker = Some(Worker::new(self.wake.clone()));
        self.workspace.explorer_preferences = self.config.explorer.clone();
        match Layout::load(&self.root) {
            Ok(layout) => {
                if !layout.files.is_empty() {
                    self.workspace.restore_panes = Some(RestoredPanes {
                        editor_visible: layout.editor,
                        terminal_focus: layout.terminal_focus,
                        // Rodtty opens terminal first; the owner shows the explorer.
                        explorer_revealed: false,
                    });
                }
                self.workspace.restore_active = layout.active_file.clone();
                self.workspace.directory = layout.explorer_directory.clone();
                // Migrate the former workspace-owned switches once, preserving an
                // explicit owner preference if the new explorer section exists.
                let has_preferences = self
                    .settings_seen
                    .as_deref()
                    .and_then(|raw| serde_json::from_slice::<serde_json::Value>(raw).ok())
                    .is_some_and(|value| value.get("explorer").is_some());
                if !has_preferences && self.config_valid && (layout.follow || layout.hidden) {
                    self.save_config(|config| {
                        config.explorer.follow_terminal = layout.follow;
                        config.explorer.show_hidden = layout.hidden;
                    });
                }
                let paths = layout.files.clone();
                self.workspace.show_empty_editor = layout.empty_editor;
                self.workspace.layout = layout;
                // Terminal first: every launch starts with the explorer hidden.
                // Its root, expanded folders and selection still restore for
                // when the owner opens it.
                self.workspace.layout.explorer = false;
                self.workspace.layout.explorer_revealed = false;
                self.workspace.explorer_revealed = false;
                self.workspace.layout.follow = self.config.explorer.follow_terminal;
                self.workspace.layout.hidden = self.config.explorer.show_hidden;
                if self
                    .workspace
                    .directory
                    .as_ref()
                    .is_some_and(|p| !p.is_dir())
                {
                    self.workspace.restoration_warnings.push(format!(
                        "Saved explorer folder is unavailable: {}. Showing the startup folder instead.",
                        self.workspace.directory.as_ref().unwrap().display()
                    ));
                    self.workspace.directory = Some(self.directory.clone());
                    self.workspace.layout.expanded_folders.clear();
                }
                for path in paths {
                    self.open_file(path);
                }
            }
            Err(e) => self.message = format!("Could not restore workspace: {e:#}"),
        }
    }
    pub(crate) fn io(&mut self, job: Job) -> bool {
        if let Some(worker) = &self.workspace.worker {
            if let Err(e) = worker.send(job) {
                self.message = e;
                return false;
            }
            true
        } else {
            false
        }
    }
    /// Change only path references after a committed move. Keeping the TextEdit
    /// and saved version retains selection, undo, encoding, and conflict checks.
    pub(crate) fn remap_moved_paths(&mut self, source: &std::path::Path, target: &std::path::Path) {
        fn remap(path: &mut PathBuf, source: &std::path::Path, target: &std::path::Path) {
            if let Ok(relative) = path.strip_prefix(source) {
                *path = if relative.as_os_str().is_empty() {
                    target.to_owned()
                } else {
                    target.join(relative)
                };
            }
        }
        let w = &mut self.workspace;
        if let Some(path) = w.tree.cursor() {
            w.layout.selected_path = Some(PathBuf::from(path));
        }
        for tab in &mut w.tabs {
            if tab.document.path.starts_with(source) {
                remap(&mut tab.document.path, source, target);
                tab.baseline += 1;
                let previous = tab.revision;
                tab.revision += 1;
                tab.autosave.remap_revision(previous, tab.revision);
                tab.disk_error = None;
            } else if tab.document.path.starts_with(target) {
                // Ignore scans from before replacement. The next new-path scan
                // refreshes clean destinations and protects dirty destinations.
                tab.baseline += 1;
            }
        }
        for path in &mut w.layout.expanded_folders {
            remap(path, source, target);
        }
        for path in [
            &mut w.directory,
            &mut w.layout.explorer_directory,
            &mut w.layout.active_file,
            &mut w.layout.selected_path,
            &mut w.outside_root,
            &mut w.pending_reveal,
            &mut w.revealed_directory,
            &mut w.restore_active,
            &mut w.latest_activation,
        ]
        .into_iter()
        .flatten()
        {
            remap(path, source, target);
        }
        for path in &mut w.layout.files {
            remap(path, source, target);
        }
        // Old rows must not reintroduce old expansion keys before the next scan.
        w.entries.clear();
        w.tree.clear();
        w.tree_errors.clear();
        w.tree_listed.clear();
        w.tree_dirty = true;
        w.scan_token += 1;
        w.last_scan = Instant::now() - Duration::from_secs(3);
        self.invalidate_diff(true);
    }

    /// Notes the window's place on the desktop, for the next launch to reopen
    /// it there. The host reports geometry on the UI thread only, and a drag or
    /// resize moves the window far faster than the layout needs saving: this
    /// reads once a second and records a value only when it differs from the
    /// last save, so a settled window writes once instead of every frame.
    ///
    /// Returns true when the saved geometry changed, so the caller repaints.
    pub(crate) fn capture_window_geometry(&mut self) -> bool {
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
        if self.window_saved.is_some()
            && self
                .window_geometry_poll
                .is_some_and(|at| at.elapsed() < INTERVAL)
        {
            return false;
        }
        self.window_geometry_poll = Some(std::time::Instant::now());
        // A maximized or full-screen frame is not where the owner wants the
        // window: restoring to it would leave no smaller frame to return to.
        // Keep the last normal frame instead.
        if self.window_maximized {
            return false;
        }
        let Some(placement) = self.window.as_ref().and_then(|w| w.placement()) else {
            return false;
        };
        if placement.minimized || placement.full_screen {
            return false;
        }
        let (x, y) = placement.restore.map_or(placement.position, |(p, _)| p);
        let size = placement.restore.map_or(placement.content_size, |(_, s)| s);
        let next = WindowGeometry {
            x,
            y,
            width: size.0,
            height: size.1,
            display: placement.display,
        };
        if self.window_saved == Some(next) || self.workspace.layout.window == Some(next) {
            return false;
        }
        self.window_saved = Some(next);
        self.workspace.layout.window = Some(next);
        true
    }

    pub(crate) fn persist_workspace(&mut self) {
        // Never publish a partial list while saved files are still loading.
        if self.shutdown_saved || !self.workspace_restoration_ready() {
            return;
        }
        self.update_workspace_snapshot();
        let workspace = &mut self.workspace;
        workspace.last_persist = Instant::now();
        let next = serde_json::to_vec(&workspace.layout).ok();
        if next != workspace.last_saved {
            match workspace.layout.save(&self.root) {
                Ok(()) => workspace.last_saved = next,
                Err(e) => self.message = format!("Could not save workspace: {e:#}"),
            }
        }
    }
    pub(crate) fn request_action(&mut self, action: Action, tx: &mut Tx) {
        if self.workspace.removal.is_some() {
            self.message = "Finish or cancel the deletion first.".into();
            return;
        }
        if self.workspace.tabs.iter().any(|tab| {
            tab.saving
                && match action {
                    Action::Editor(id) => tab.id == id,
                    _ => true,
                }
        }) {
            self.message = "Wait for the current save to finish.".into();
            return;
        }
        let dirty = self
            .workspace
            .tabs
            .iter()
            .find(|tab| {
                tab.dirty()
                    && match action {
                        Action::Editor(id) => tab.id == id,
                        _ => true,
                    }
            })
            .map(|tab| tab.id);
        if let Some(id) = dirty {
            self.pending = Some(Pending::File(FilePrompt::Unsaved { id, action }));
            return;
        }
        match action {
            Action::Editor(id) => {
                let active = self
                    .workspace
                    .tabs
                    .get(self.workspace.active)
                    .map(|tab| tab.id);
                self.workspace.tabs.retain(|t| t.id != id);
                self.workspace.active = active
                    .and_then(|active| self.workspace.tabs.iter().position(|t| t.id == active))
                    .unwrap_or_else(|| {
                        self.workspace
                            .active
                            .min(self.workspace.tabs.len().saturating_sub(1))
                    });
                if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
                    tab.edit.focused = tab.mode == View::Edit;
                } else {
                    self.workspace.set_terminal_focus(true);
                }
                self.invalidate_diff(true);
            }
            Action::Window => match self.config.behavior.window_close {
                crate::config::WindowClosePolicy::KeepSessionsRunning => {
                    self.request_action(Action::KeepWindow, tx)
                }
                crate::config::WindowClosePolicy::AskEachTime => {
                    self.pending = Some(Pending::Window)
                }
                crate::config::WindowClosePolicy::QuitRodtty => {
                    self.request_action(Action::Quit, tx)
                }
            },
            Action::KeepWindow => {
                self.persist_workspace();
                self.window_close_approved = true;
                tx.close_window();
            }
            Action::Quit => {
                self.persist_workspace();
                self.pending = Some(Pending::Quit);
                if !self.sessions.iter().any(Session::has_running_commands) {
                    self.confirm(tx);
                }
            }
        }
    }
    pub(crate) fn on_window_close(&mut self, tx: &mut Tx) {
        if rune::quitting() || std::mem::take(&mut self.window_close_approved) {
            self.persist_workspace();
            return;
        }
        // Native close notifications arrive as the window hides. Reopen it for
        // the same decisions used by buttons, menus, palette and shortcuts.
        self.request_action(Action::Window, tx);
        self.window_close_approved = false;
        if (self.pending.is_some()
            || self
                .workspace
                .tabs
                .iter()
                .any(|tab| tab.saving || tab.dirty()))
            && let Some(handle) = &self.handle
        {
            handle.open();
        }
    }
}
