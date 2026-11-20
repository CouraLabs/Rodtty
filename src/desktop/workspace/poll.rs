use super::*;

impl State {
    pub(crate) fn workspace_poll(&mut self, tx: &mut Tx) -> bool {
        self.sync_tree_expansion();
        self.apply_explorer_preferences();
        let events = self
            .workspace
            .worker
            .as_ref()
            .map(Worker::drain)
            .unwrap_or_default();
        let mut changed = !events.is_empty();
        for event in events {
            match event {
                Event::PrepareRemoval { id, result } => self.finish_prepare_removal(id, result, tx),
                Event::Remove { id, result } => self.finish_removal(id, result, tx),
                Event::FileOperation { id, result } => self.finish_file_operation(id, result, tx),
                Event::Reload {
                    id,
                    revision,
                    result,
                } => self.on_reload(id, revision, result),
                Event::Open { path, result } => self.on_open(path, result),
                Event::Save { id, token, result } => self.on_save(id, token, result, tx),
                Event::Scan {
                    token,
                    listing,
                    git,
                    files,
                } => self.on_scan(token, listing, git, files, tx),
                Event::Diff { token, result } => self.on_diff(token, result),
                Event::Error(error) => self.message = error,
            }
        }
        changed |= self.follow_terminal_directory(tx);
        self.schedule_scan();
        self.schedule_diff();
        changed |= self.sync_diff_mode();
        changed |= self.surface_deferred_save_conflict();
        changed |= self.apply_pending_location(tx);
        changed |= self.poll_explorer_search(tx);
        changed |= self.poll_autosave(tx);
        self.poll_command_notifications();
        changed |= self.poll_log_filter();
        changed |= self.poll_changes();
        self.poll_drafts();
        self.prune_splits();
        tx.wake_after(Duration::from_millis(200));
        changed
    }

    /// Explorer preferences changed in Settings: restart following and rescan.
    fn apply_explorer_preferences(&mut self) {
        if self.workspace.explorer_preferences != self.config.explorer {
            self.workspace.explorer_preferences = self.config.explorer.clone();
            self.workspace.layout.follow = self.config.explorer.follow_terminal;
            self.workspace.layout.hidden = self.config.explorer.show_hidden;
            self.workspace.last_follow = None;
            self.workspace.outside_root = None;
            self.workspace.revealed_directory = None;
            self.workspace.pending_reveal = None;
            self.workspace.scan_token += 1;
            self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        }
    }

    /// Opens the explorer at the terminal's folder, and follows the active
    /// terminal there while Follow is on.
    fn follow_terminal_directory(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        if self.workspace.layout.explorer && self.workspace.directory.is_none() {
            self.navigate(
                self.active()
                    .map(Session::cwd)
                    .unwrap_or_else(|| self.directory.clone()),
            );
            changed = true;
        }
        if self.workspace.layout.explorer && self.workspace.layout.follow {
            let cwd = self
                .active()
                .map(Session::cwd)
                .unwrap_or_else(|| self.directory.clone());
            if self.workspace.last_follow.as_ref() != Some(&cwd) {
                self.reveal_terminal(tx);
                changed = true;
            }
        }
        changed
    }

    /// At most once a second, rescans the explorer, Git and open files.
    fn schedule_scan(&mut self) {
        if self.workspace.removal.is_none()
            && !self.workspace.operation_moving
            && !self.workspace.scan_busy
            && self.workspace.last_scan.elapsed() > Duration::from_secs(1)
        {
            let directory = self
                .workspace
                .layout
                .explorer
                .then(|| self.workspace.directory.clone())
                .flatten();
            let files = self
                .workspace
                .tabs
                .iter()
                .map(|t| (t.id, t.baseline, t.document.path.clone()))
                .collect();
            if self.io(Job::Scan {
                token: self.workspace.scan_token,
                directory,
                hidden: self.workspace.layout.hidden,
                expanded: self.workspace.layout.expanded_folders.clone(),
                respect_ignore: self.config.explorer.respect_ignore,
                reveal: self.workspace.revealed_directory.clone(),
                files,
            }) {
                self.workspace.scan_busy = true;
                self.workspace.last_scan = Instant::now();
            }
        }
    }

    /// Every two seconds, recomputes the active diff tab's comparison.
    fn schedule_diff(&mut self) {
        if self.workspace.removal.is_none()
            && !self.workspace.operation_moving
            && self.workspace.diff_pending.is_none()
            && self.workspace.layout.editor
            && self.workspace.last_diff.elapsed() > Duration::from_secs(2)
            && let Some(tab) = self.workspace.tabs.get(self.workspace.active)
            && let View::Diff(mode) = tab.mode
        {
            let (id, revision, baseline) = (tab.id, tab.revision, tab.baseline);
            let (path, text) = (tab.document.path.clone(), tab.edit.text.clone());
            self.workspace.diff_token += 1;
            let token = self.workspace.diff_token;
            if self.io(Job::Diff {
                token,
                path,
                text,
                mode,
            }) {
                self.workspace.diff_pending = Some((token, id, revision, baseline, mode));
                self.workspace.last_diff = Instant::now();
            }
        }
    }

    /// Unified or split diffs: the owner's choice, or by available width.
    fn sync_diff_mode(&mut self) -> bool {
        let mode = match self.workspace.diff_layout {
            1 => DiffMode::Unified,
            2 => DiffMode::Split,
            _ => {
                if self.workspace.body_width < 880.0 || self.workspace.editor_width < 620.0 {
                    DiffMode::Unified
                } else {
                    DiffMode::Split
                }
            }
        };
        if self.workspace.diff.mode() != mode {
            self.workspace.diff.set_mode(mode);
            return true;
        }
        false
    }

    /// A save conflict deferred while another prompt or operation was busy,
    /// asked once nothing else is.
    fn surface_deferred_save_conflict(&mut self) -> bool {
        if self.pending.is_none()
            && !self.application_overlay_open()
            && !self.workspace.operation_busy
            && self.workspace.removal.is_none()
            && !self.shutdown_saved
        {
            while !self.workspace.deferred_save_conflicts.is_empty() {
                let conflict = self.workspace.deferred_save_conflicts.remove(0);
                if self.workspace.tabs.iter().any(|tab| {
                    tab.id == conflict.id
                        && tab.save_token == conflict.token
                        && tab.revision == conflict.revision
                        && tab.baseline == conflict.baseline
                        && tab.dirty()
                        && !tab.saving
                }) {
                    self.pending = Some(Pending::File(FilePrompt::Conflict {
                        id: conflict.id,
                        after: conflict.after,
                    }));
                    return true;
                }
            }
        }
        false
    }
}
