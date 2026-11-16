use super::*;
use crate::{
    files::{Document, Entry, SaveFailure, TextFile},
    git::{Comparison, Repository},
    work::{Event, Job, Layout, Worker},
};
use rune_kit::{
    Kit,
    diff::{DiffMode, DiffState, DiffView},
    syntax::{self, SyntaxTheme},
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Editor(u64),
    Window,
    Quit,
}
pub(super) enum FilePrompt {
    Unsaved { id: u64, action: Action },
    Conflict { id: u64, after: Option<Action> },
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Edit,
    Diff(Comparison),
}
pub(super) struct Tab {
    pub id: u64,
    pub document: Document,
    pub edit: TextEdit,
    pub mode: View,
    pub external: Option<TextFile>,
    pub disk_error: Option<String>,
    pub dismissed: bool,
    pub saving: bool,
    pub revision: u64,
    pub baseline: u64,
    pub after_save: Option<Action>,
}
impl Tab {
    pub fn dirty(&self) -> bool {
        self.document.dirty(&self.edit.text)
    }
}

pub(super) struct Workspace {
    pub terminal_focus: bool,
    orphan_edit: TextEdit,
    pub layout: Layout,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub worker: Option<Worker>,
    pub entries: Vec<Entry>,
    pub directory: Option<PathBuf>,
    pub address: TextEdit,
    pub repo: Option<Repository>,
    pub git_error: Option<String>,
    scan_token: u64,
    scan_busy: bool,
    pub(super) last_scan: Instant,
    pub(super) last_follow: Option<PathBuf>,
    next_id: u64,
    opening: Vec<PathBuf>,
    restore_active: Option<PathBuf>,
    restore_editor: Option<bool>,
    pub diff: DiffState,
    pub diff_label: String,
    pub diff_error: Option<String>,
    pub diff_patch: String,
    diff_token: u64,
    diff_pending: Option<(u64, u64, u64, u64, Comparison)>,
    last_diff: Instant,
    pub diff_layout: u8,
    pub editor_width: f32,
    pub body_width: f32,
    pub find: bool,
    pub find_query: TextEdit,
    pub replacement: TextEdit,
    drag_width: f32,
    drag_share: f32,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            terminal_focus: true,
            orphan_edit: TextEdit::new(""),
            layout: Layout::default(),
            tabs: Vec::new(),
            active: 0,
            worker: None,
            entries: Vec::new(),
            directory: None,
            address: TextEdit::new(""),
            repo: None,
            git_error: None,
            scan_token: 0,
            scan_busy: false,
            last_scan: Instant::now() - Duration::from_secs(3),
            last_follow: None,
            next_id: 1,
            opening: Vec::new(),
            restore_active: None,
            restore_editor: None,
            diff: DiffState::default(),
            diff_label: String::new(),
            diff_error: None,
            diff_patch: String::new(),
            diff_token: 0,
            diff_pending: None,
            last_diff: Instant::now() - Duration::from_secs(3),
            diff_layout: 0,
            editor_width: 400.0,
            body_width: 1080.0,
            find: false,
            find_query: TextEdit::new(""),
            replacement: TextEdit::new(""),
            drag_width: 240.0,
            drag_share: 0.52,
        }
    }
}

impl State {
    pub(super) fn workspace_setup(&mut self) {
        self.workspace.worker = Some(Worker::new(self.wake.clone()));
        match Layout::load(&self.root) {
            Ok(layout) => {
                if !layout.files.is_empty() {
                    self.workspace.restore_editor = Some(layout.editor);
                }
                self.workspace.restore_active = layout.active_file.clone();
                self.workspace.directory = layout.explorer_directory.clone();
                self.workspace.address.replace_text(
                    layout
                        .explorer_directory
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                let paths = layout.files.clone();
                self.workspace.layout = layout;
                for path in paths {
                    self.open_file(path);
                }
            }
            Err(e) => self.message = format!("Could not restore workspace: {e:#}"),
        }
    }
    fn io(&mut self, job: Job) -> bool {
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
    pub(super) fn toggle_files(&mut self) {
        self.workspace.layout.explorer = !self.workspace.layout.explorer;
        if self.workspace.layout.explorer && self.workspace.directory.is_none() {
            self.navigate(
                self.active()
                    .map(Session::cwd)
                    .unwrap_or_else(|| self.directory.clone()),
            );
        }
    }
    pub(super) fn navigate(&mut self, path: PathBuf) {
        self.workspace.directory = Some(path.clone());
        self.workspace.address.replace_text(path.to_string_lossy());
        self.workspace.entries.clear();
        self.workspace.repo = None;
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }
    pub(super) fn open_file(&mut self, path: PathBuf) {
        if let Some(i) = self
            .workspace
            .tabs
            .iter()
            .position(|tab| tab.document.path == path)
        {
            self.activate_editor(i);
            return;
        }
        if self.workspace.opening.contains(&path) {
            return;
        }
        if self.io(Job::Open(path.clone())) {
            self.workspace.opening.push(path);
            self.message = "Opening file…".into();
        }
    }
    pub(super) fn activate_editor(&mut self, index: usize) {
        self.settings = false;
        self.workspace.terminal_focus = false;
        self.workspace.active = index;
        self.workspace.layout.editor = true;
        if let Some(tab) = self.workspace.tabs.get_mut(index) {
            tab.edit.focused = tab.mode == View::Edit;
        }
        self.invalidate_diff(true);
    }
    pub(super) fn persist_workspace(&mut self) {
        let workspace = &mut self.workspace;
        workspace.layout.explorer_directory = workspace.directory.clone();
        workspace.layout.files = workspace
            .tabs
            .iter()
            .map(|t| t.document.path.clone())
            .collect();
        workspace.layout.active_file = workspace
            .tabs
            .get(workspace.active)
            .map(|t| t.document.path.clone());
        if let Err(e) = workspace.layout.save(&self.root) {
            self.message = format!("Could not save workspace: {e:#}");
        }
    }
    pub(super) fn request_action(&mut self, action: Action, tx: &mut Tx) {
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
                    self.workspace.terminal_focus = true;
                }
                self.invalidate_diff(true);
            }
            Action::Window => {
                self.persist_workspace();
                tx.close_window();
            }
            Action::Quit => {
                self.persist_workspace();
                self.pending = Some(Pending::Quit);
            }
        }
    }
    pub(super) fn on_window_close(&mut self, tx: &mut Tx) {
        self.persist_workspace();
        if self.workspace.tabs.iter().any(Tab::dirty) {
            self.request_action(Action::Window, tx);
            if let Some(handle) = &self.handle {
                handle.open();
            }
        }
    }
    pub(super) fn save_file(&mut self, id: u64, overwrite: bool, after: Option<Action>) {
        let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if tab.saving {
            self.message = "This file is still being saved.".into();
            return;
        }
        tab.saving = true;
        tab.after_save = after;
        let job = Job::Save {
            id,
            document: tab.document.clone(),
            text: tab.edit.text.clone(),
            overwrite,
        };
        if !self.io(job)
            && let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id)
        {
            tab.saving = false;
        }
    }
    fn discard_for_action(&mut self, id: u64, action: Action, tx: &mut Tx) {
        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
            if tab.saving {
                self.message = "Wait for the save to finish.".into();
                return;
            }
            tab.edit.replace_text(tab.document.saved.text.clone());
            tab.revision += 1;
        }
        self.pending = None;
        self.request_action(action, tx);
    }
    fn reload_external(&mut self, id: u64) {
        if let Some(tab) = self.workspace.tabs.iter().find(|t| t.id == id)
            && !tab.saving
        {
            let job = Job::Reload {
                id,
                revision: tab.revision,
                path: tab.document.path.clone(),
            };
            self.io(job);
        }
        self.pending = None;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        self.invalidate_diff(false);
    }
    pub(super) fn cancel_file_action(&mut self) {
        self.pending = None;
        for tab in &mut self.workspace.tabs {
            tab.after_save = None;
        }
    }
    pub(super) fn invalidate_diff(&mut self, clear: bool) {
        self.workspace.last_diff = Instant::now() - Duration::from_secs(3);
        if clear {
            self.workspace.diff = DiffState::default();
            self.workspace.diff_patch.clear();
            self.workspace.diff_label = "Loading comparison…".into();
            self.workspace.diff_error = None;
        }
    }
    fn changed_editor(&mut self, id: u64) {
        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
            tab.revision += 1;
        }
    }
    fn file_key(&mut self, id: u64, event: &KeyEvent, tx: &mut Tx) -> bool {
        if event.is_command('s') && !event.mods.shift {
            self.dispatch(AppAction::SaveEditor(Some(id)), tx);
            return true;
        }
        if event.is_command('f') && !event.mods.shift {
            self.dispatch(AppAction::FindEditor, tx);
            return true;
        }
        if event.is_command('h') && !event.mods.shift {
            self.dispatch(AppAction::ReplaceEditor, tx);
            return true;
        }
        if event.is_command('w') && !event.mods.shift {
            self.dispatch(AppAction::CloseEditor(Some(id)), tx);
            return true;
        }
        if event.key == Key::Function(3) {
            self.find_editor(false, tx);
            return true;
        }
        if event.key == Key::Enter || event.key == Key::Tab {
            if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
                if event.key == Key::Tab {
                    tab.edit.insert("    ");
                } else {
                    let position = tab.edit.focus;
                    let before = &tab.edit.text[..position];
                    let line = before.rsplit('\n').next().unwrap_or("");
                    let indent: String = line
                        .chars()
                        .take_while(|c| *c == ' ' || *c == '\t')
                        .collect();
                    tab.edit.insert(&format!("\n{indent}"));
                }
                tab.revision += 1;
            }
            return true;
        }
        false
    }
    fn find_editor(&mut self, replace: bool, tx: &mut Tx) {
        let query = self.workspace.find_query.text.clone();
        if query.is_empty() {
            return;
        }
        let replacement = self.workspace.replacement.text.clone();
        if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
            if replace {
                let a = tab.edit.anchor.min(tab.edit.focus);
                let b = tab.edit.anchor.max(tab.edit.focus);
                if tab.edit.text.get(a..b) == Some(query.as_str()) {
                    tab.edit.insert(&replacement);
                    tab.revision += 1;
                }
            }
            let start = tab.edit.focus;
            if let Some(index) = tab.edit.text[start..]
                .find(&query)
                .map(|p| p + start)
                .or_else(|| tab.edit.text[..start].find(&query))
            {
                tab.edit.anchor = index;
                tab.edit.focus = index + query.len();
                tab.edit.break_history_group();
                tx.focus_when_shown(format!("file-editor-{}", tab.id), true);
            } else {
                self.message = "No matching text in this file.".into();
            }
        }
    }
    fn replace_all(&mut self) {
        let query = self.workspace.find_query.text.clone();
        if query.is_empty() {
            return;
        }
        if let Some(tab) = self.workspace.tabs.get_mut(self.workspace.active) {
            let updated = tab
                .edit
                .text
                .replace(&query, &self.workspace.replacement.text);
            tab.edit.undo_group(|edit| {
                edit.anchor = 0;
                edit.focus = edit.text.len();
                edit.insert(&updated);
            });
            tab.revision += 1;
        }
    }
    pub(super) fn workspace_poll(&mut self, tx: &mut Tx) -> bool {
        let events = self
            .workspace
            .worker
            .as_ref()
            .map(Worker::drain)
            .unwrap_or_default();
        let mut changed = !events.is_empty();
        for event in events {
            match event {
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
                    self.workspace.opening.retain(|p| p != &path);
                    match result {
                        Ok(document) => {
                            if let Some(i) = self
                                .workspace
                                .tabs
                                .iter()
                                .position(|t| t.document.path == document.path)
                            {
                                self.activate_editor(i);
                                continue;
                            }
                            let id = self.workspace.next_id;
                            self.workspace.next_id += 1;
                            let mode = if document.existed {
                                View::Edit
                            } else {
                                View::Diff(Comparison::Current)
                            };
                            let mut edit = TextEdit::new(&document.saved.text);
                            edit.focused = mode == View::Edit;
                            self.workspace.tabs.push(Tab {
                                id,
                                document,
                                edit,
                                mode,
                                external: None,
                                disk_error: None,
                                dismissed: false,
                                saving: false,
                                revision: 0,
                                baseline: 0,
                                after_save: None,
                            });
                            let index = self.workspace.tabs.len() - 1;
                            if self.workspace.restore_active.is_none()
                                || self.workspace.restore_active.as_ref()
                                    == Some(&self.workspace.tabs[index].document.path)
                            {
                                self.activate_editor(index);
                            }
                            self.message.clear();
                        }
                        Err(error) => self.message = error,
                    }
                    if self.workspace.opening.is_empty() {
                        self.workspace.restore_active = None;
                        if let Some(visible) = self.workspace.restore_editor.take() {
                            self.workspace.layout.editor = visible;
                            if !visible {
                                self.workspace.terminal_focus = true;
                            }
                        }
                    }
                }
                Event::Save { id, result } => {
                    if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
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
                                self.pending =
                                    Some(Pending::File(FilePrompt::Conflict { id, after }));
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
                                Ok((path, entries)) => {
                                    if !self.workspace.address.focused {
                                        self.workspace.address.replace_text(path.to_string_lossy());
                                    }
                                    self.workspace.directory = Some(path);
                                    self.workspace.entries = entries;
                                }
                                Err(error) => self.message = format!("Explorer: {error}"),
                            }
                        }
                        match git {
                            Ok(repo) => {
                                self.workspace.repo = repo;
                                self.workspace.git_error = None;
                            }
                            Err(error) => {
                                self.workspace.repo = None;
                                self.workspace.git_error = Some(error);
                            }
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
                self.workspace.last_follow = Some(cwd.clone());
                self.navigate(cwd);
                changed = true;
            }
        }
        if !self.workspace.scan_busy && self.workspace.last_scan.elapsed() > Duration::from_secs(1)
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
                files,
            }) {
                self.workspace.scan_busy = true;
                self.workspace.last_scan = Instant::now();
            }
        }
        if self.workspace.diff_pending.is_none()
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
                if self.workspace.editor_width < 620.0 {
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
        tx.wake_after(Duration::from_millis(200));
        changed
    }
}

fn explorer(s: &State) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("explorer")
        .w(workspace.layout.explorer_width)
        .min_w(160.0)
        .h_full()
        .min_h(0.0)
        .fill(panel())
        .px(10.0)
        .gap(6.0)
        .child(
            row()
                .h(35.0)
                .center_y()
                .child(text("FILES").font(LABEL).color(accent()))
                .child(el().grow(1.0))
                .child(button("explorer-hide", "×", |s, tx| {
                    s.dispatch(AppAction::ToggleExplorer, tx)
                })),
        )
        .child(
            text_input(&workspace.address, LABEL)
                .key("explorer-address")
                .label("Explorer folder path")
                .single_line()
                .w_full()
                .h(30.0)
                .color(ink())
                .on_edit(|s: &mut State| &mut s.workspace.address)
                .on_submit(|s: &mut State, tx| {
                    let path = PathBuf::from(&s.workspace.address.text);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        s.workspace
                            .directory
                            .as_ref()
                            .unwrap_or(&s.directory)
                            .join(path)
                    };
                    s.navigate(path);
                    tx.blur();
                }),
        )
        .child(
            row()
                .gap(4.0)
                .child(button("explorer-up", "↑", |s, tx| {
                    s.dispatch(AppAction::ExplorerParent, tx)
                }))
                .child(button("explorer-refresh", "↻", |s, tx| {
                    s.dispatch(AppAction::RefreshExplorer, tx)
                }))
                .child(button(
                    "explorer-follow",
                    if workspace.layout.follow {
                        "Follow ✓"
                    } else {
                        "Follow"
                    },
                    |s, tx| s.dispatch(AppAction::ToggleFollow, tx),
                ))
                .child(button(
                    "explorer-hidden",
                    if workspace.layout.hidden {
                        ".* ✓"
                    } else {
                        ".*"
                    },
                    |s, tx| s.dispatch(AppAction::ToggleHidden, tx),
                )),
        );
    let mut list = col()
        .key("file-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    for (i, entry) in workspace.entries.iter().enumerate() {
        let path = entry.path.clone();
        let directory = entry.directory;
        let badge = workspace
            .repo
            .as_ref()
            .map(|repo| repo.badge(&path))
            .unwrap_or_default();
        let label = format!(
            "{} {}",
            if entry.missing {
                "−"
            } else if directory {
                "▸"
            } else {
                "·"
            },
            entry.name
        );
        list = list.child(
            row()
                .key(format!("file-{i}"))
                .w_full()
                .h(28.0)
                .gap(4.0)
                .center_y()
                .hover(rgb(37, 49, 63))
                .on_press_tx(move |s: &mut State, tx| {
                    tx.blur();
                    if directory {
                        s.navigate(path.clone());
                    } else {
                        s.open_file(path.clone());
                    }
                })
                .child(
                    text(label)
                        .font(LABEL)
                        .color(if entry.missing {
                            rgb(238, 113, 129)
                        } else {
                            ink()
                        })
                        .truncate()
                        .shrink()
                        .grow(1.0),
                )
                .child(text(badge).font(LABEL).color(accent())),
        );
    }
    if workspace.entries.is_empty() {
        list = list.child(
            text(if workspace.scan_busy {
                "Loading…"
            } else {
                "No visible files"
            })
            .font(LABEL)
            .color(muted()),
        );
    }
    pane = pane.child(list);
    if let Some(error) = &workspace.git_error {
        pane = pane.child(text(error).font(LABEL).color(muted()).wrap_anywhere());
    }
    pane
}

fn editor(s: &State) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("editor-pane")
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .fill(bg())
        .on_layout(|s: &mut State, r| s.workspace.editor_width = r[2]);
    let mut tabs = row().h(34.0).w_full().scroll_x().fill(panel());
    for (i, tab) in workspace.tabs.iter().enumerate() {
        let id = tab.id;
        let name = tab
            .document
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        tabs = tabs
            .child(
                button(
                    format!("editor-tab-{id}"),
                    format!("{}{}", name, if tab.dirty() { " •" } else { "" }),
                    move |s, tx| s.dispatch(AppAction::ActivateEditor(i), tx),
                )
                .border(
                    1.0,
                    if i == workspace.active {
                        accent()
                    } else {
                        panel()
                    },
                ),
            )
            .child(button(format!("editor-close-{id}"), "×", move |s, tx| {
                s.dispatch(AppAction::CloseEditor(Some(id)), tx)
            }));
    }
    if s.settings_open {
        tabs = tabs
            .child(button("settings-tab", "Settings", |s, tx| {
                s.dispatch(AppAction::Settings, tx)
            }))
            .child(button("settings-close", "×", |s, tx| {
                s.dispatch(AppAction::CloseSettings, tx)
            }));
    }
    pane = pane.child(tabs);
    if s.settings {
        return pane.child(super::settings::content(s));
    }
    let Some(tab) = workspace.tabs.get(workspace.active) else {
        return pane.child(
            col()
                .grow(1.0)
                .center()
                .child(text("Open a file from Files").font(MONO).color(muted())),
        );
    };
    let id = tab.id;
    let mut controls = row()
        .w_full()
        .h(36.0)
        .gap(4.0)
        .center_y()
        .scroll_x()
        .child(button(
            "file-save",
            if tab.saving { "Saving…" } else { "Save" },
            move |s, tx| s.dispatch(AppAction::SaveEditor(Some(id)), tx),
        ))
        .child(button("file-edit", "Edit", |s, tx| {
            s.dispatch(AppAction::EditFile, tx)
        }));
    for (key, label, mode) in [
        ("diff-current", "Current", Comparison::Current),
        ("diff-unstaged", "Unstaged", Comparison::Unstaged),
        ("diff-staged", "Staged", Comparison::Staged),
    ] {
        controls = controls.child(
            button(key, label, move |s, tx| {
                s.dispatch(AppAction::Compare(mode), tx)
            })
            .border(
                1.0,
                if tab.mode == View::Diff(mode) {
                    accent().alpha(0.5)
                } else {
                    panel()
                },
            ),
        );
    }
    controls = controls
        .child(button("file-find", "Find", |s, tx| {
            s.dispatch(AppAction::FindEditor, tx)
        }))
        .child(button("editor-hide", "Hide", |s, tx| {
            s.dispatch(AppAction::ToggleEditor, tx)
        }));
    pane = pane.child(controls).child(
        text(tab.document.path.to_string_lossy())
            .font(TextStyle::new(10.0, 400.0))
            .color(muted())
            .truncate()
            .w_full(),
    );
    if let Some(error) = &tab.disk_error {
        pane = pane.child(
            text(format!("Disk: {error}"))
                .font(LABEL)
                .color(rgb(238, 113, 129))
                .wrap_anywhere(),
        );
    }
    if tab.external.is_some() && !tab.dismissed {
        pane = pane.child(
            col()
                .w_full()
                .fill(rgb(58, 43, 26))
                .py(6.0)
                .child(
                    text("Changed on disk. Your unsaved edits are preserved.")
                        .font(LABEL)
                        .color(ink())
                        .wrap(),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(button(
                            "external-reload",
                            "Reload · discard edits",
                            move |s, _| s.reload_external(id),
                        ))
                        .child(button("external-keep", "Keep edits", move |s, _| {
                            if let Some(t) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                                t.dismissed = true;
                            }
                        })),
                ),
        );
    }
    if workspace.find && tab.mode == View::Edit {
        pane = pane.child(
            col()
                .w_full()
                .gap(4.0)
                .py(4.0)
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            text_input(&workspace.find_query, LABEL)
                                .key("editor-find")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .h(28.0)
                                .color(ink())
                                .placeholder("Find text", muted())
                                .on_edit(|s: &mut State| &mut s.workspace.find_query)
                                .on_submit(|s: &mut State, tx| s.find_editor(false, tx)),
                        )
                        .child(button("find-next", "Next", |s, tx| {
                            s.find_editor(false, tx)
                        }))
                        .child(button("find-close", "×", |s, _| s.workspace.find = false)),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            text_input(&workspace.replacement, LABEL)
                                .key("editor-replace")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .h(28.0)
                                .color(ink())
                                .placeholder("Replace with", muted())
                                .on_edit(|s: &mut State| &mut s.workspace.replacement),
                        )
                        .child(button("replace-next", "Replace", |s, tx| {
                            s.find_editor(true, tx)
                        }))
                        .child(button("replace-all", "All", |s, _| s.replace_all())),
                ),
        );
    }
    if tab.mode == View::Edit {
        let language = syntax::lang_for_path(&tab.document.path.to_string_lossy());
        let highlights = syntax::highlight(language, &tab.edit.text);
        let spans = SyntaxTheme::neutral(true).spans(&highlights, 0);
        let field = text_input(&tab.edit, MONO)
            .key(format!("file-editor-{id}"))
            .label("File editor")
            .w_full()
            .color(ink())
            .wrap_anywhere()
            .spans(spans)
            .on_edit(move |s: &mut State| {
                let workspace = &mut s.workspace;
                if let Some(tab) = workspace.tabs.iter_mut().find(|t| t.id == id) {
                    &mut tab.edit
                } else {
                    &mut workspace.orphan_edit
                }
            })
            .on_input_change(move |s: &mut State, _| s.changed_editor(id))
            .on_input_key(move |s: &mut State, event, tx| s.file_key(id, event, tx))
            .on_input_paste(move |s: &mut State, clipboard, _| {
                if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                    tab.edit
                        .insert(&clipboard.text().replace("\r\n", "\n").replace('\r', "\n"));
                    tab.revision += 1;
                }
                true
            });
        let viewport = col()
            .key(format!("editor-scroll-{id}"))
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .px(10.0)
            .py(8.0)
            .child(field);
        pane = pane.child(Kit::neutral(true).scroll_bar(viewport));
        let position = tab.edit.focus;
        let line = tab.edit.text[..position]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1;
        let column = tab.edit.text[..position]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .count()
            + 1;
        pane = pane.child(
            text(format!(
                "UTF-8{} · {:?} · Ln {line}, Col {column}",
                if tab.document.saved.bom { " BOM" } else { "" },
                tab.document.saved.preferred
            ))
            .font(TextStyle::new(10.0, 400.0))
            .color(muted()),
        );
    } else {
        pane = pane.child(
            row()
                .gap(4.0)
                .h(32.0)
                .scroll_x()
                .child(button("diff-auto", "Auto", |s, _| {
                    s.workspace.diff_layout = 0
                }))
                .child(button("diff-inline", "Inline", |s, _| {
                    s.workspace.diff_layout = 1
                }))
                .child(button("diff-split", "Split", |s, _| {
                    s.workspace.diff_layout = 2
                }))
                .child(button("diff-prev", "↑", |s, tx| {
                    s.workspace.diff.next_hunk(false, tx);
                }))
                .child(button("diff-next", "↓", |s, tx| {
                    s.workspace.diff.next_hunk(true, tx);
                })),
        );
        pane = pane.child(
            text(&workspace.diff_label)
                .font(TextStyle::new(10.0, 400.0))
                .color(muted())
                .wrap(),
        );
        if let Some(error) = &workspace.diff_error {
            pane = pane.child(text(error).font(LABEL).color(muted()).wrap_anywhere());
        }
        if workspace.diff_patch.is_empty() {
            pane = pane.child(el().grow(1.0));
        } else {
            pane = pane.child(
                DiffView::new(
                    |s: &State| &s.workspace.diff,
                    |s: &mut State| &mut s.workspace.diff,
                )
                .key("git-diff")
                .build(s, &Kit::neutral(true))
                .focusable(true)
                .grow(1.0)
                .min_h(0.0),
            );
        }
    }
    pane
}

pub(super) fn body(s: &State) -> El {
    let workspace = &s.workspace;
    let show_editor = workspace.layout.editor && (!workspace.tabs.is_empty() || s.settings_open);
    let terminal = workspace.layout.terminal || !show_editor;
    let mut body = row()
        .key("workspace")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_layout(|s: &mut State, r| s.workspace.body_width = r[2]);
    if workspace.layout.explorer {
        body = body.child(explorer(s)).child(
            el().key("explorer-divider")
                .w(5.0)
                .h_full()
                .fill(panel())
                .hover(accent().alpha(0.3))
                .on_drag(|s: &mut State, d, _| {
                    if d.phase == Phase::Down {
                        s.workspace.drag_width = s.workspace.layout.explorer_width;
                    }
                    s.workspace.layout.explorer_width = (s.workspace.drag_width + d.dx)
                        .clamp(160.0, (s.workspace.body_width * 0.45).max(160.0));
                }),
        );
    }
    let mut main = row().grow(1.0).min_w(0.0).h_full().min_h(0.0);
    if terminal {
        let pane = col()
            .key("terminal-pane")
            .h_full()
            .min_h(0.0)
            .min_w(0.0)
            .px(12.0)
            .py(10.0)
            .child(grid(s));
        main = main.child(if show_editor {
            pane.grow(1.0 - workspace.layout.editor_share)
        } else {
            pane.grow(1.0)
        });
    }
    if show_editor {
        if terminal {
            main = main.child(
                el().key("editor-divider")
                    .w(5.0)
                    .h_full()
                    .fill(panel())
                    .hover(accent().alpha(0.3))
                    .on_drag(|s: &mut State, d, _| {
                        if d.phase == Phase::Down {
                            s.workspace.drag_share = s.workspace.layout.editor_share;
                        }
                        let width = s.workspace.body_width
                            - if s.workspace.layout.explorer {
                                s.workspace.layout.explorer_width + 5.0
                            } else {
                                0.0
                            };
                        s.workspace.layout.editor_share =
                            (s.workspace.drag_share - d.dx / width.max(1.0)).clamp(0.2, 0.8);
                    }),
            );
        }
        main = main.child(editor(s).grow(if terminal {
            workspace.layout.editor_share
        } else {
            1.0
        }));
    }
    body.child(main)
}

pub(super) fn file_dialog(s: &State) -> Option<El> {
    let Some(Pending::File(prompt)) = &s.pending else {
        return None;
    };
    let (id, title) = match prompt {
        FilePrompt::Unsaved { id, .. } => (*id, "Save unsaved changes?"),
        FilePrompt::Conflict { id, .. } => (*id, "File changed on disk"),
    };
    let tab = s.workspace.tabs.iter().find(|t| t.id == id)?;
    let mut buttons = row()
        .gap(8.0)
        .child(button("file-cancel", "Cancel", |s, _| {
            s.cancel_file_action()
        }));
    match *prompt {
        FilePrompt::Unsaved { id, action } => {
            buttons = buttons
                .child(button("file-discard", "Discard", move |s, tx| {
                    s.discard_for_action(id, action, tx)
                }))
                .child(button("file-save-confirm", "Save", move |s, _| {
                    s.save_file(id, false, Some(action))
                }));
        }
        FilePrompt::Conflict { id, after } => {
            buttons = buttons
                .child(button("file-reload-confirm", "Reload", move |s, _| {
                    s.reload_external(id)
                }))
                .child(button("file-overwrite", "Overwrite", move |s, _| {
                    s.save_file(id, true, after)
                }));
        }
    }
    Some(el().absolute(0.0,0.0).w_full().h_full().fill(Color::BLACK.alpha(0.65)).center()
        .child(col().key("file-dialog").modal(false,|s:&mut State|s.cancel_file_action()).w(470.0).px(24.0).py(24.0).gap(16.0).radius(10.0).fill(panel())
            .child(text(title).font(TextStyle::new(17.0,600.0)).color(ink()))
            .child(text(tab.document.path.to_string_lossy()).font(LABEL).color(muted()).wrap_anywhere())
            .child(text(if matches!(prompt,FilePrompt::Conflict{..}){"Saving would replace changes made outside this editor. Reload discards your editor changes."}else{"Choose Save, Discard, or Cancel to continue."}).font(LABEL).color(muted()).wrap())
            .child(buttons)))
}

pub(super) fn demo(s: &mut State, diff: bool) {
    let path = PathBuf::from("/workspace/rodtty/src/main.rs");
    let saved =
        TextFile::decode(b"fn main() {\n    println!(\"Hello, terminal\");\n}\n".to_vec()).unwrap();
    let document = Document {
        path: path.clone(),
        saved,
        existed: true,
    };
    let edit = TextEdit::new(
        "fn main() {\n    // Terminal, files, and code in one workspace.\n    println!(\"Hello from Rodtty\");\n}\n",
    );
    let old = document.saved.text.clone();
    let new = edit.text.clone();
    s.workspace.tabs.push(Tab {
        id: 1,
        document,
        edit,
        mode: if diff {
            View::Diff(Comparison::Current)
        } else {
            View::Edit
        },
        external: None,
        disk_error: None,
        dismissed: false,
        saving: false,
        revision: 0,
        baseline: 0,
        after_save: None,
    });
    s.workspace.next_id = 2;
    s.workspace.layout.explorer = true;
    s.workspace.directory = Some(PathBuf::from("/workspace/rodtty"));
    s.workspace.address.replace_text("/workspace/rodtty");
    s.workspace.entries = vec![
        Entry {
            path: PathBuf::from("/workspace/rodtty/src"),
            name: "src".into(),
            directory: true,
            missing: false,
        },
        Entry {
            path: PathBuf::from("/workspace/rodtty/Cargo.toml"),
            name: "Cargo.toml".into(),
            directory: false,
            missing: false,
        },
        Entry {
            path,
            name: "main.rs".into(),
            directory: false,
            missing: false,
        },
    ];
    if diff {
        s.workspace.diff = DiffState::new(vec![rune_kit::diff::FileDiff::from_texts(
            "src/main.rs",
            &old,
            &new,
        )]);
        s.workspace.diff_patch = "preview".into();
        s.workspace.diff_label = "Editor ↔ HEAD · preview includes unsaved changes".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(root: &std::path::Path) -> State {
        let mut state = State::with_root(root.to_owned());
        state.directory = root.to_owned();
        state.workspace_setup();
        state
    }
    fn wait(state: &mut State, condition: impl Fn(&State) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(8) {
            state.workspace_poll(&mut Tx::default());
            if condition(state) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("workspace operation timed out: {}", state.message);
    }
    fn open(state: &mut State, path: &std::path::Path) {
        state.open_file(path.to_owned());
        wait(state, |s| !s.workspace.tabs.is_empty());
    }

    #[test]
    fn dismissing_palette_returns_typing_to_the_editor_without_changing_its_text() {
        use super::super::tests::{open_palette, press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        let mut runtime = runtime_for(state);
        runtime.update(|_, tx| tx.focus("file-editor-1", true));
        runtime.frame(0.0);
        open_palette(&mut runtime);
        press(
            &mut runtime,
            Key::Text("no such action".into()),
            Default::default(),
        );
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("No matching actions"))
        );
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.find("command-palette").is_none());
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("file-editor-1") && e.focused)
        );
        press(&mut runtime, Key::Text("edit".into()), Default::default());
        assert!(runtime.state.workspace.tabs[0].edit.text.contains("edit"));
        assert!(
            !runtime.state.workspace.tabs[0]
                .edit
                .text
                .contains("no such action")
        );
    }

    #[test]
    fn palette_and_editor_close_control_both_preserve_unsaved_work_until_a_decision() {
        use super::super::tests::{open_palette, press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        let mut runtime = runtime_for(state);
        runtime.update(|_, tx| tx.focus("file-editor-1", true));
        runtime.frame(0.0);
        press(
            &mut runtime,
            Key::Text("unsaved".into()),
            Default::default(),
        );
        let draft = runtime.state.workspace.tabs[0].edit.text.clone();
        let (x, y) = runtime.find("editor-close-1").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.find("file-dialog").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        open_palette(&mut runtime);
        press(
            &mut runtime,
            Key::Text("close editor".into()),
            Default::default(),
        );
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("command-palette").is_none());
        assert!(runtime.find("file-dialog").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, draft);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn palette_file_actions_do_not_target_an_editor_behind_settings() {
        use super::super::tests::{open_palette, press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        let mut runtime = runtime_for(state);
        let (x, y) = runtime.find("settings").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        open_palette(&mut runtime);
        press(
            &mut runtime,
            Key::Text("save file".into()),
            Default::default(),
        );
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("Select a file editor first"))
        );
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("command-palette").is_some());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn save_control_shortcut_and_palette_save_the_same_editor_buffer() {
        use super::super::tests::{open_palette, press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        let mut runtime = runtime_for(state);
        let command = rune::Mods {
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        for (entry, expected) in [
            (0, "saved from control"),
            (1, "saved from shortcut"),
            (2, "saved from palette"),
        ] {
            runtime.update(|_, tx| tx.focus("file-editor-1", true));
            runtime.frame(0.0);
            press(&mut runtime, Key::Command('a'), command);
            press(&mut runtime, Key::Text(expected.into()), Default::default());
            match entry {
                0 => {
                    let (x, y) = runtime.find("file-save").unwrap();
                    runtime.pointer_down(x, y);
                    runtime.pointer_up(x, y);
                    runtime.frame(0.0);
                }
                1 => press(&mut runtime, Key::Command('s'), command),
                _ => {
                    open_palette(&mut runtime);
                    press(
                        &mut runtime,
                        Key::Text("save file".into()),
                        Default::default(),
                    );
                    press(&mut runtime, Key::Enter, Default::default());
                }
            }
            runtime.update(|s, _| wait(s, |s| !s.workspace.tabs[0].saving));
            runtime.frame(0.0);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
            assert!(runtime.find("file-dialog").is_none());
        }
    }

    #[test]
    fn clean_files_reload_but_dirty_files_keep_edits_and_require_conflict_resolution() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        std::fs::write(&path, "external clean").unwrap();
        state.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        wait(&mut state, |s| {
            s.workspace.tabs[0].edit.text == "external clean"
        });
        state.workspace.tabs[0].edit.insert(" local edits");
        state.workspace.tabs[0].revision += 1;
        let draft = state.workspace.tabs[0].edit.text.clone();
        std::fs::write(&path, "external again").unwrap();
        state.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        wait(&mut state, |s| s.workspace.tabs[0].external.is_some());
        assert_eq!(state.workspace.tabs[0].edit.text, draft);
        state.save_file(1, false, None);
        wait(&mut state, |s| {
            matches!(s.pending, Some(Pending::File(FilePrompt::Conflict { .. })))
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external again");
        state.reload_external(1);
        wait(&mut state, |s| {
            s.workspace.tabs[0].edit.text == "external again"
        });
        assert!(!state.workspace.tabs[0].dirty());
    }

    #[test]
    fn canceling_a_save_and_close_keeps_the_window_open_when_save_completes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.workspace.tabs[0].edit.insert(" edits");
        state.request_action(Action::Quit, &mut Tx::default());
        assert!(matches!(state.pending, Some(Pending::File(_))));
        state.save_file(1, false, Some(Action::Quit));
        state.cancel_file_action();
        wait(&mut state, |s| !s.workspace.tabs[0].saving);
        assert!(state.pending.is_none());
        assert_eq!(state.workspace.tabs.len(), 1);
        assert!(!state.workspace.tabs[0].dirty());
    }

    #[test]
    fn editor_keys_insert_newlines_support_undo_and_close_without_losing_unsaved_work() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        let mut runtime = rune::Runtime::new(
            state,
            super::super::view,
            (1200.0, 700.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        runtime.frame(0.0);
        runtime.update(|s, tx| {
            let edit = &mut s.workspace.tabs[0].edit;
            edit.anchor = edit.text.len();
            edit.focus = edit.anchor;
            tx.focus("file-editor-1", true);
        });
        runtime.frame(0.0);
        for key in [Key::Enter, Key::Tab, Key::Text("café".into())] {
            runtime.key(KeyEvent {
                key,
                mods: Default::default(),
            });
            runtime.frame(0.0);
        }
        assert_eq!(
            runtime.state.workspace.tabs[0].edit.text,
            "fn main() {}\n    café"
        );
        let edited = runtime.state.workspace.tabs[0].edit.text.clone();
        runtime.key(KeyEvent {
            key: Key::Command('z'),
            mods: rune::Mods {
                cmd: true,
                ctrl: true,
                ..Default::default()
            },
        });
        runtime.frame(0.0);
        assert_ne!(runtime.state.workspace.tabs[0].edit.text, edited);
        runtime.key(KeyEvent {
            key: Key::Command('z'),
            mods: rune::Mods {
                cmd: true,
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        });
        runtime.frame(0.0);
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, edited);
        runtime.update(|s, tx| s.request_action(Action::Editor(1), tx));
        runtime.frame(0.0);
        assert!(runtime.state.pending.is_some());
        runtime.key(KeyEvent {
            key: Key::Escape,
            mods: Default::default(),
        });
        runtime.frame(0.0);
        assert!(runtime.state.pending.is_none());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, edited);
        runtime.update(|s, tx| s.discard_for_action(1, Action::Editor(1), tx));
        runtime.frame(0.0);
        assert!(runtime.state.workspace.tabs.is_empty());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "fn main() {}");
    }

    #[test]
    fn explorer_only_tracks_terminal_location_when_follow_is_enabled() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        assert_eq!(
            state.workspace.directory,
            Some(root.path().canonicalize().unwrap())
        );
        state.directory = child.clone();
        state.workspace_poll(&mut Tx::default());
        assert_eq!(
            state.workspace.directory,
            Some(root.path().canonicalize().unwrap())
        );
        state.workspace.layout.follow = true;
        state.workspace_poll(&mut Tx::default());
        assert_eq!(state.workspace.directory.as_deref(), Some(child.as_path()));
    }

    #[test]
    fn real_git_patch_reaches_the_read_only_diff_view_and_adapts_layout() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(root.path())
                .args(["init", "-b", "main"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let path = root.path().join("space name.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.workspace.tabs[0].mode = View::Diff(Comparison::Current);
        state.invalidate_diff(true);
        wait(&mut state, |s| !s.workspace.diff_patch.is_empty());
        assert_eq!(state.workspace.diff.files().len(), 1);
        assert_eq!(state.workspace.diff.files()[0].additions(), 1);
        assert!(state.workspace.diff_label.contains("untracked"));
        state.workspace.editor_width = 1000.0;
        state.workspace_poll(&mut Tx::default());
        assert_eq!(state.workspace.diff.mode(), DiffMode::Split);
        state.workspace.editor_width = 400.0;
        state.workspace_poll(&mut Tx::default());
        assert_eq!(state.workspace.diff.mode(), DiffMode::Unified);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "fn main() {}\n");
    }
}
