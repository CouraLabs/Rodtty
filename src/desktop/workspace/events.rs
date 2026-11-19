use super::*;
use crate::files::TreeListing;

impl State {
    /// A reloaded file: replaces a clean buffer's text, or reports why it could not.
    pub(crate) fn on_reload(&mut self, id: u64, revision: u64, result: Result<TextFile, String>) {
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
                    self.message = "New edits were kept while the file was reloading.".into();
                }
                Err(error) => self.message = format!("Could not reload: {error}"),
            }
        }
    }

    /// An opened file: shows its tab, or adds one, unless a newer activation superseded this preview.
    pub(crate) fn on_open(&mut self, path: PathBuf, result: Result<Document, String>) {
        let preview = self
            .workspace
            .opening
            .iter()
            .find(|(p, _)| p == &path)
            .is_some_and(|(_, preview)| *preview);
        self.workspace.opening.retain(|(p, _)| p != &path);
        if preview && self.workspace.latest_activation.as_ref() != Some(&path) {
            return;
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
                    return;
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
                    panes.terminal_focus || !panes.editor_visible || self.workspace.tabs.is_empty(),
                );
                self.workspace.explorer_revealed = panes.explorer_revealed;
                self.message = self.workspace.restoration_warnings.join("\n");
            }
        }
    }

    /// A finished save: marks the tab saved, or surfaces the failure or conflict.
    pub(crate) fn on_save(
        &mut self,
        id: u64,
        token: u64,
        result: Result<Document, SaveFailure>,
        tx: &mut Tx,
    ) {
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
                        self.pending = Some(Pending::File(FilePrompt::Conflict { id, after }));
                    } else {
                        // A background save must not replace another
                        // file's active Save/Discard/Cancel decision.
                        self.workspace
                            .deferred_save_conflicts
                            .retain(|conflict| conflict.id != id);
                        self.workspace
                            .deferred_save_conflicts
                            .push(DeferredSaveConflict {
                                id,
                                token,
                                revision: tab.revision,
                                baseline: tab.baseline,
                                after,
                            });
                    }
                    self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
                }
                Err(SaveFailure::Io(error)) => self.message = format!("Could not save: {error}"),
            }
        }
    }

    /// A periodic scan: the explorer listing, Git status and open files' disk state.
    pub(crate) fn on_scan(
        &mut self,
        token: u64,
        listing: Option<Result<TreeListing, String>>,
        git: Result<Option<Repository>, String>,
        files: Vec<(u64, u64, Result<TextFile, String>)>,
        tx: &mut Tx,
    ) {
        self.workspace.scan_busy = false;
        if token == self.workspace.scan_token {
            if let Some(listing) = listing {
                match listing {
                    Ok(listing) => {
                        self.workspace.tree_dirty |= self.workspace.entries != listing.entries
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
                    Ok(disk) if disk.raw != tab.document.saved.raw || !tab.document.existed => {
                        if tab.dirty() {
                            if tab.external.as_ref().is_none_or(|old| old.raw != disk.raw) {
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

    /// A computed diff for the active tab, applied if it is still current.
    pub(crate) fn on_diff(&mut self, token: u64, result: Result<crate::git::Diff, String>) {
        if let Some((requested, id, revision, baseline, mode)) = self.workspace.diff_pending.take()
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
}
