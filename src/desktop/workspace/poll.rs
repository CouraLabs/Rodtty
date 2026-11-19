use super::*;

impl State {
    pub(crate) fn workspace_poll(&mut self, tx: &mut Tx) -> bool {
        self.sync_tree_expansion();
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
                } => {
                    if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
                        match result {
                            Ok(disk) if tab.revision == revision => {
                                tab.edit.replace_text(disk.text.clone());
                                tab.document.saved = disk;
                                tab.document.existed = true;
                                tab.external = None;
                                tab.disk_error = None;
                                tab.dismissed = false;
                                tab.baseline += 1;
                                tab.revision += 1;
                            }
                            Ok(disk) => {
                                tab.external = Some(disk);
                                tab.dismissed = false;
                                self.message =
                                    "New edits were kept while the file was reloading.".into();
                            }
                            Err(error) => self.message = format!("Could not reload: {error}"),
                        }
                    }
                }
                Event::Open { path, result } => {
                    let preview = self
                        .workspace
                        .opening
                        .iter()
                        .find(|(p, _)| p == &path)
                        .is_some_and(|(_, preview)| *preview);
                    self.workspace.opening.retain(|(p, _)| p != &path);
                    if preview && self.workspace.latest_activation.as_ref() != Some(&path) {
                        continue;
                    }
                    // The tab is named by the opened document's resolved path.
                    if self
                        .workspace
                        .pending_location
                        .as_ref()
                        .is_some_and(|(pending, _)| *pending == path)
                    {
                        self.workspace.pending_location = match &result {
                            Ok(document) => self
                                .workspace
                                .pending_location
                                .take()
                                .map(|(_, location)| (document.path.clone(), location)),
                            Err(_) => None,
                        };
                    }
                    match result {
                        Ok(document) => {
                            if let Some(i) = self
                                .workspace
                                .tabs
                                .iter()
                                .position(|t| t.document.path == document.path)
                            {
                                if !preview {
                                    self.workspace.tabs[i].preview = false;
                                }
                                self.show_editor(i, !preview);
                                continue;
                            }
                            let id = self.workspace.next_id;
                            self.workspace.next_id += 1;
                            // Markdown opens rendered, newly or on restoration,
                            // unless a Search Files match asked for its source.
                            let source_requested = self
                                .workspace
                                .pending_location
                                .as_ref()
                                .is_some_and(|(pending, _)| *pending == document.path);
                            let mode = if !document.existed {
                                View::Diff(Comparison::Current)
                            } else if is_markdown(&document.path) && !source_requested {
                                View::Preview
                            } else {
                                View::Edit
                            };
                            let mut edit = TextEdit::new(&document.saved.text);
                            edit.focused = mode == View::Edit;
                            let replacement = preview
                                .then(|| {
                                    self.workspace.tabs.iter().position(|t| {
                                        t.preview
                                            && !t.dirty()
                                            && !t.saving
                                            && t.external.is_none()
                                            && t.disk_error.is_none()
                                            && t.after_save.is_none()
                                    })
                                })
                                .flatten();
                            if let Some(index) = replacement {
                                self.workspace.tabs.remove(index);
                            }
                            let index = replacement.unwrap_or(self.workspace.tabs.len());
                            self.workspace.tabs.insert(
                                index,
                                Tab {
                                    id,
                                    document,
                                    preview,
                                    edit,
                                    mode,
                                    external: None,
                                    disk_error: None,
                                    dismissed: false,
                                    saving: false,
                                    save_token: 0,
                                    revision: 0,
                                    baseline: 0,
                                    after_save: None,
                                    autosave: Default::default(),
                                },
                            );
                            if self.workspace.restore_active.is_none()
                                || self.workspace.restore_active.as_ref()
                                    == Some(&self.workspace.tabs[index].document.path)
                            {
                                self.show_editor(index, !preview);
                            }
                            if self.workspace.restore_panes.is_none() {
                                self.message.clear();
                            }
                        }
                        Err(error) => {
                            if self.workspace.restore_panes.is_some() {
                                self.workspace.restoration_warnings.push(error.clone());
                            }
                            self.message = error;
                        }
                    }
                    if self.workspace.opening.is_empty() {
                        self.workspace.restore_active = None;
                        if let Some(panes) = self.workspace.restore_panes.take() {
                            self.workspace.layout.editor = panes.editor_visible;
                            self.workspace.set_terminal_focus(
                                panes.terminal_focus
                                    || !panes.editor_visible
                                    || self.workspace.tabs.is_empty(),
                            );
                            self.workspace.explorer_revealed = panes.explorer_revealed;
                            self.message = self.workspace.restoration_warnings.join("\n");
                        }
                    }
                }
                Event::Save { id, token, result } => {
                    let prompt_available = !self.application_overlay_open()
                        && self.pending.is_none()
                        && self.workspace.removal.is_none()
                        && !self.workspace.operation_busy;
                    if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id)
                        && tab.saving
                        && tab.save_token == token
                    {
                        tab.saving = false;
                        let after = tab.after_save.take();
                        match result {
                            Ok(document) => {
                                tab.document = document;
                                tab.external = None;
                                tab.disk_error = None;
                                tab.dismissed = false;
                                tab.baseline += 1;
                                self.message = "File saved.".into();
                                if matches!(self.pending,Some(Pending::File(FilePrompt::Unsaved{id:pending,..}|FilePrompt::Conflict{id:pending,..}))if pending==id)
                                {
                                    self.pending = None;
                                }
                                if let Some(action) = after {
                                    self.request_action(action, tx);
                                }
                                self.invalidate_diff(false);
                            }
                            Err(SaveFailure::Conflict) => {
                                if prompt_available
                                    || matches!(self.pending, Some(Pending::File(FilePrompt::Unsaved {id: current, ..} | FilePrompt::Conflict {id: current, ..})) if current == id)
                                {
                                    self.pending =
                                        Some(Pending::File(FilePrompt::Conflict { id, after }));
                                } else {
                                    // A background save must not replace another
                                    // file's active Save/Discard/Cancel decision.
                                    self.workspace
                                        .deferred_save_conflicts
                                        .retain(|conflict| conflict.id != id);
                                    self.workspace.deferred_save_conflicts.push(
                                        DeferredSaveConflict {
                                            id,
                                            token,
                                            revision: tab.revision,
                                            baseline: tab.baseline,
                                            after,
                                        },
                                    );
                                }
                                self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
                            }
                            Err(SaveFailure::Io(error)) => {
                                self.message = format!("Could not save: {error}")
                            }
                        }
                    }
                }
                Event::Scan {
                    token,
                    listing,
                    git,
                    files,
                } => {
                    self.workspace.scan_busy = false;
                    if token == self.workspace.scan_token {
                        if let Some(listing) = listing {
                            match listing {
                                Ok(listing) => {
                                    self.workspace.tree_dirty |= self.workspace.entries
                                        != listing.entries
                                        || self.workspace.tree_errors != listing.errors
                                        || self.workspace.tree_listed != listing.listed;
                                    let path = listing.root;
                                    self.workspace.directory = Some(path);
                                    self.workspace.entries = listing.entries;
                                    self.workspace.tree_errors = listing.errors;
                                    self.workspace.tree_listed = listing.listed;
                                    self.workspace.explorer_error = None;
                                }
                                Err(error) => self.workspace.explorer_error = Some(error),
                            }
                        }
                        match git {
                            Ok(repo) => {
                                self.workspace.tree_dirty |= self.workspace.repo != repo;
                                self.workspace.repo = repo;
                                self.workspace.git_error = None;
                            }
                            Err(error) => {
                                self.workspace.tree_dirty |= self.workspace.repo.is_some();
                                self.workspace.repo = None;
                                self.workspace.git_error = Some(error);
                            }
                        }
                    }
                    if token == self.workspace.scan_token {
                        if self.workspace.tree_dirty {
                            self.rebuild_tree();
                        }
                        if let Some(path) = self.workspace.pending_reveal.clone()
                            && self.workspace.tree.reveal(&path.to_string_lossy(), tx)
                        {
                            self.workspace.pending_reveal = None;
                        }
                    }
                    for (id, baseline, result) in files {
                        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id)
                            && tab.baseline == baseline
                            && !tab.saving
                        {
                            match result {
                                Ok(disk)
                                    if disk.raw != tab.document.saved.raw
                                        || !tab.document.existed =>
                                {
                                    if tab.dirty() {
                                        if tab
                                            .external
                                            .as_ref()
                                            .is_none_or(|old| old.raw != disk.raw)
                                        {
                                            tab.dismissed = false;
                                        }
                                        tab.external = Some(disk);
                                    } else {
                                        tab.edit.replace_text(disk.text.clone());
                                        tab.document.saved = disk;
                                        tab.document.existed = true;
                                        tab.revision += 1;
                                        tab.baseline += 1;
                                        tab.external = None;
                                        tab.dismissed = false;
                                    }
                                    tab.disk_error = None;
                                }
                                Ok(_) => {
                                    tab.external = None;
                                    tab.disk_error = None;
                                    tab.dismissed = false;
                                }
                                Err(error) => tab.disk_error = Some(error),
                            }
                        }
                    }
                }
                Event::Diff { token, result } => {
                    if let Some((requested, id, revision, baseline, mode)) =
                        self.workspace.diff_pending.take()
                        && requested == token
                        && self
                            .workspace
                            .tabs
                            .get(self.workspace.active)
                            .is_some_and(|t| {
                                t.id == id
                                    && t.revision == revision
                                    && t.baseline == baseline
                                    && t.mode == View::Diff(mode)
                            })
                    {
                        match result {
                            Ok(diff) => {
                                if diff.patch != self.workspace.diff_patch {
                                    self.workspace
                                        .diff
                                        .set_files(rune_kit::diff::parse(&diff.patch));
                                    self.workspace.diff_patch = diff.patch;
                                }
                                self.workspace.diff_label = format!(
                                    "{}{}{}",
                                    diff.label,
                                    if diff.untracked { " · untracked" } else { "" },
                                    if !diff.changed { " · no changes" } else { "" }
                                );
                                self.workspace.diff_error = None;
                            }
                            Err(error) => {
                                self.workspace.diff_error = Some(error);
                            }
                        }
                    }
                }
                Event::Error(error) => self.message = error,
            }
        }
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
            changed = true;
        }
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
                    changed = true;
                    break;
                }
            }
        }
        changed |= self.apply_pending_location(tx);
        changed |= self.poll_explorer_search(tx);
        changed |= self.poll_autosave(tx);
        tx.wake_after(Duration::from_millis(200));
        changed
    }
}
