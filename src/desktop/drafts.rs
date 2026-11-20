//! Editor draft recovery in the window: dirty file-backed buffers are
//! snapshotted off the UI thread every few seconds; on a fresh launch the
//! drafts a previous run left behind are offered for Restore, Save as Copy
//! or Discard. A restored draft opens unsaved against the version it was
//! edited from, so a file changed on disk since is the editor's ordinary
//! version conflict and nothing is overwritten without a decision.
use super::*;
use crate::drafts::{self, DiskState, Draft};
use crate::files::Document;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SNAPSHOT_EVERY: Duration = Duration::from_secs(2);

pub(crate) struct DraftState {
    /// Drafts from an earlier run, waiting for the owner's decision.
    pub pending: Vec<Draft>,
    pub unreadable: Vec<(PathBuf, String)>,
    pub dialog: bool,
    /// The buffer revision each path's draft was last written at, for the
    /// drafts this run manages.
    written: HashMap<PathBuf, u64>,
    last: Instant,
    busy: Arc<AtomicBool>,
}

impl Default for DraftState {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            unreadable: Vec::new(),
            dialog: false,
            written: HashMap::new(),
            last: Instant::now(),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }
}

enum Job {
    Write(Draft),
    Remove(PathBuf),
}

impl State {
    fn drafts_enabled(&self) -> bool {
        !self.config.behavior.disable_draft_recovery
    }

    /// At a fresh launch: find drafts an earlier run left.
    pub(crate) fn load_pending_drafts(&mut self) {
        if !self.drafts_enabled() {
            return;
        }
        let (pending, unreadable) = drafts::load(&self.root);
        self.drafts.dialog = !pending.is_empty() || !unreadable.is_empty();
        self.drafts.pending = pending;
        self.drafts.unreadable = unreadable;
    }

    /// The snapshot work due now: drafts for dirty buffers whose text changed
    /// since their last snapshot, and removals for buffers no longer dirty.
    fn draft_jobs(&mut self) -> Vec<Job> {
        let mut jobs = Vec::new();
        let mut dirty = std::collections::HashSet::new();
        let pending: Vec<PathBuf> = self.drafts.pending.iter().map(|d| d.path.clone()).collect();
        for tab in &self.workspace.tabs {
            // A draft still waiting for a decision is never overwritten.
            if !tab.dirty() || pending.contains(&tab.document.path) {
                continue;
            }
            let path = tab.document.path.clone();
            dirty.insert(path.clone());
            if self.drafts.written.get(&path) == Some(&tab.revision) {
                continue;
            }
            self.drafts.written.insert(path.clone(), tab.revision);
            jobs.push(Job::Write(Draft {
                path,
                existed: tab.document.existed,
                saved: tab.document.saved.raw.clone(),
                text: tab.edit.text.clone(),
                saved_at_ms: drafts::now_ms(),
            }));
        }
        let gone: Vec<PathBuf> = self
            .drafts
            .written
            .keys()
            .filter(|p| !dirty.contains(*p) && !pending.contains(p))
            .cloned()
            .collect();
        for path in gone {
            self.drafts.written.remove(&path);
            jobs.push(Job::Remove(path));
        }
        jobs
    }

    /// Snapshot on a background thread, at most every two seconds.
    pub(crate) fn poll_drafts(&mut self) {
        if !self.drafts_enabled()
            || self.drafts.last.elapsed() < SNAPSHOT_EVERY
            || self.drafts.busy.load(Ordering::Acquire)
        {
            return;
        }
        self.drafts.last = Instant::now();
        let jobs = self.draft_jobs();
        if jobs.is_empty() {
            return;
        }
        let root = self.root.clone();
        let busy = self.drafts.busy.clone();
        busy.store(true, Ordering::Release);
        std::thread::spawn(move || {
            run(&root, jobs);
            busy.store(false, Ordering::Release);
        });
    }

    #[cfg(test)]
    pub(crate) fn drafts_force_due(&mut self) {
        self.drafts.last = Instant::now() - SNAPSHOT_EVERY;
    }

    /// Snapshot now, on this thread: before quitting, so a discarded buffer
    /// leaves no draft behind.
    pub(crate) fn sync_drafts(&mut self) {
        if !self.drafts_enabled() {
            return;
        }
        while self.drafts.busy.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let jobs = self.draft_jobs();
        run(&self.root, jobs);
    }

    /// Open pending draft `index` as an unsaved buffer.
    pub(crate) fn restore_draft(&mut self, index: usize) {
        let Some(draft) = self.drafts.pending.get(index).cloned() else {
            return;
        };
        let saved = match crate::files::TextFile::decode(draft.saved.clone()) {
            Ok(saved) => saved,
            Err(e) => {
                self.message =
                    format!("This draft's saved version can't be read ({e:#}); use Save as Copy.");
                return;
            }
        };
        let state = draft.disk_state();
        let document = Document {
            path: draft.path.clone(),
            saved,
            existed: draft.existed,
        };
        self.workspace.layout.editor = true;
        self.on_open(draft.path.clone(), Ok(document.clone()));
        let Some(tab) = self
            .workspace
            .tabs
            .iter_mut()
            .find(|t| t.document.path == draft.path)
        else {
            self.message = format!("{} could not be opened.", draft.path.display());
            return;
        };
        // An already restored tab takes the draft's saved version too, so the
        // conflict check compares the disk with what the draft was edited from.
        tab.document = document;
        tab.preview = false;
        tab.edit.replace_text(draft.text.clone());
        tab.revision += 1;
        tab.external = None;
        tab.dismissed = false;
        let revision = tab.revision;
        self.drafts.pending.remove(index);
        // The draft file stays until the buffer is saved or discarded.
        self.drafts
            .written
            .insert(draft.path.clone(), revision.wrapping_sub(1));
        self.message = match state {
            DiskState::Unchanged => format!(
                "Restored the unsaved draft of {}.",
                window::tilde_path(&draft.path)
            ),
            DiskState::Changed => format!(
                "Restored the draft of {}, which changed on disk since: saving reports the conflict so you can compare first, or use Save as Copy.",
                window::tilde_path(&draft.path)
            ),
            DiskState::Missing => format!(
                "Restored the draft of {}, which is no longer on disk: saving recreates it.",
                window::tilde_path(&draft.path)
            ),
        };
        self.drafts.dialog = !self.drafts.pending.is_empty() || !self.drafts.unreadable.is_empty();
    }

    /// Write pending draft `index` to a new sibling file and open it; the
    /// original file is left alone.
    pub(crate) fn save_draft_copy(&mut self, index: usize) {
        let Some(draft) = self.drafts.pending.get(index).cloned() else {
            return;
        };
        let bytes = crate::files::TextFile::decode(draft.saved.clone())
            .map(|saved| saved.encode(&draft.text))
            .unwrap_or_else(|_| draft.text.clone().into_bytes());
        let copy = drafts::copy_path(&draft.path);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&copy)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes));
        match written {
            Ok(()) => {
                drafts::remove(&self.root, &draft.path);
                self.drafts.pending.remove(index);
                self.message = format!("Saved the draft as {}.", window::tilde_path(&copy));
                self.open_file(copy);
            }
            Err(e) => self.message = format!("Could not save the copy: {e}"),
        }
        self.drafts.dialog = !self.drafts.pending.is_empty() || !self.drafts.unreadable.is_empty();
    }

    pub(crate) fn discard_draft(&mut self, index: usize) {
        if index < self.drafts.pending.len() {
            let draft = self.drafts.pending.remove(index);
            drafts::remove(&self.root, &draft.path);
        }
        self.drafts.dialog = !self.drafts.pending.is_empty() || !self.drafts.unreadable.is_empty();
    }

    pub(crate) fn discard_unreadable_drafts(&mut self) {
        for (file, _) in std::mem::take(&mut self.drafts.unreadable) {
            let _ = std::fs::remove_file(file);
        }
        self.drafts.dialog = !self.drafts.pending.is_empty();
    }
}

fn run(root: &Path, jobs: Vec<Job>) {
    for job in jobs {
        match job {
            Job::Write(draft) => {
                let _ = drafts::write(root, &draft);
            }
            Job::Remove(path) => drafts::remove(root, &path),
        }
    }
}

/// The recovery dialog.
pub(crate) fn dialog(s: &State, a: &Appearance) -> Option<El> {
    if !s.drafts.dialog {
        return None;
    }
    let now = drafts::now_ms();
    let mut list = col().key("drafts-list").role("list").gap(10.0).w_full();
    for (i, draft) in s.drafts.pending.iter().enumerate() {
        let age = commands::duration(Duration::from_millis(now.saturating_sub(draft.saved_at_ms)));
        let state = match draft.disk_state() {
            DiskState::Unchanged => "the file is unchanged since",
            DiskState::Changed => {
                "the file changed on disk since: restoring keeps both versions until you decide"
            }
            DiskState::Missing => "the file is no longer on disk",
        };
        list = list.child(
            col()
                .key(format!("draft-{i}"))
                .role("group")
                .label(window::tilde_path(&draft.path))
                .gap(4.0)
                .child(
                    text(window::tilde_path(&draft.path))
                        .font(ui_style(13.0, 600.0))
                        .color(a.ink())
                        .wrap_anywhere(),
                )
                .child(
                    text(format!("Unsaved edits from {age} ago; {state}."))
                        .font(ui_label())
                        .color(a.muted())
                        .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(button(
                            a,
                            format!("draft-restore-{i}"),
                            "Restore",
                            move |s, _| s.restore_draft(i),
                        ))
                        .child(button(
                            a,
                            format!("draft-copy-{i}"),
                            "Save as Copy",
                            move |s, _| s.save_draft_copy(i),
                        ))
                        .child(button(
                            a,
                            format!("draft-discard-{i}"),
                            "Discard",
                            move |s, _| s.discard_draft(i),
                        )),
                ),
        );
    }
    if !s.drafts.unreadable.is_empty() {
        list = list.child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(format!(
                        "{} draft file(s) could not be read.",
                        s.drafts.unreadable.len()
                    ))
                    .font(ui_label())
                    .color(a.muted())
                    .grow(1.0),
                )
                .child(button(
                    a,
                    "drafts-discard-unreadable",
                    "Remove Them",
                    |s, _| s.discard_unreadable_drafts(),
                )),
        );
    }
    Some(
        col()
            .key("draft-recovery")
            .role("dialog")
            .label("Recover unsaved edits")
            .modal(false, |s: &mut State| s.drafts.dialog = false)
            .w(560.0)
            .max_h(560.0)
            .px(24.0)
            .py(24.0)
            .gap(12.0)
            .radius(10.0)
            .fill(a.panel())
            .child(text("Recover Unsaved Edits").font(ui_style(17.0, 600.0)).color(a.ink()))
            .child(
                text("Rodtty closed without saving these files. Nothing is written to them until you choose.")
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
            )
            .child(list)
            .child(button(a, "drafts-later", "Decide Later", |s, _| s.drafts.dialog = false)),
    )
}
