use super::*;
use crate::{
    files::{Document, Entry, SaveFailure, TextFile},
    git::{Comparison, Repository},
    work::{Event, Job, Layout, Worker},
};
use rune_kit::{
    Tree, TreeItem, TreeModel, TreeStyle,
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
    pub preview: bool,
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

pub(super) struct TreeEntry {
    entry: Option<Entry>,
    label: String,
    badge: String,
}

pub(super) struct Workspace {
    pub terminal_focus: bool,
    pub explorer_revealed: bool,
    pub show_empty_editor: bool,
    orphan_edit: TextEdit,
    pub layout: Layout,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub worker: Option<Worker>,
    pub entries: Vec<Entry>,
    pub(super) tree: TreeModel<TreeEntry>,
    pub(super) operation_busy: bool,
    pub(super) operation_result: Option<PathBuf>,
    pub(super) operation_id: u64,
    pub(super) operation_name: TextEdit,
    pub(super) operation_destination: TextEdit,
    pub(super) operation_error: Option<String>,
    pub(super) operation_context: Option<(f32, f32)>,
    pub(super) operation_viewport: (f32, f32),
    tree_errors: Vec<(PathBuf, String)>,
    tree_dirty: bool,
    explorer_error: Option<String>,
    pub(super) outside_root: Option<PathBuf>,
    pending_reveal: Option<PathBuf>,
    revealed_directory: Option<PathBuf>,
    explorer_preferences: crate::config::ExplorerPreferences,
    pub directory: Option<PathBuf>,
    pub address: TextEdit,
    pub repo: Option<Repository>,
    pub git_error: Option<String>,
    scan_token: u64,
    scan_busy: bool,
    pub(super) last_scan: Instant,
    pub(super) last_follow: Option<PathBuf>,
    next_id: u64,
    opening: Vec<(PathBuf, bool)>,
    latest_activation: Option<PathBuf>,
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
    diff_viewport: (f32, f32),
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
            explorer_revealed: false,
            show_empty_editor: false,
            orphan_edit: TextEdit::new(""),
            layout: Layout::default(),
            tabs: Vec::new(),
            active: 0,
            worker: None,
            entries: Vec::new(),
            tree: TreeModel::new().key("file-list"),
            operation_busy: false,
            operation_result: None,
            operation_id: 0,
            operation_name: TextEdit::new(""),
            operation_destination: TextEdit::new(""),
            operation_error: None,
            operation_context: None,
            operation_viewport: (1080.0, 720.0),
            tree_errors: Vec::new(),
            tree_dirty: true,
            explorer_error: None,
            outside_root: None,
            pending_reveal: None,
            revealed_directory: None,
            explorer_preferences: crate::config::ExplorerPreferences::default(),
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
            latest_activation: None,
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
            diff_viewport: (600.0, 600.0),
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
        self.workspace.explorer_preferences = self.config.explorer.clone();
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
                self.workspace.layout = layout;
                self.workspace.layout.follow = self.config.explorer.follow_terminal;
                self.workspace.layout.hidden = self.config.explorer.show_hidden;
                for path in paths {
                    self.open_file(path);
                }
            }
            Err(e) => self.message = format!("Could not restore workspace: {e:#}"),
        }
    }
    pub(super) fn io(&mut self, job: Job) -> bool {
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
        let minimum_width = explorer_minimum_width(self);
        if self.workspace.body_width < minimum_width {
            self.workspace.explorer_revealed = !self.workspace.explorer_revealed;
            self.workspace.layout.explorer = self.workspace.explorer_revealed;
        } else {
            self.workspace.explorer_revealed = false;
            self.workspace.layout.explorer = !self.workspace.layout.explorer;
        }
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
        self.workspace.tree.clear();
        self.workspace.tree_dirty = true;
        self.workspace.layout.expanded_folders.clear();
        self.workspace.explorer_error = None;
        self.workspace.outside_root = None;
        self.workspace.pending_reveal = None;
        self.workspace.revealed_directory = None;
        self.workspace.last_follow = None;
        self.workspace.repo = None;
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }
    pub(super) fn reveal_terminal(&mut self, tx: &mut Tx) {
        let cwd = self
            .active()
            .map(Session::cwd)
            .unwrap_or_else(|| self.directory.clone());
        self.workspace.last_follow = Some(cwd.clone());
        let Some(root) = self.workspace.directory.as_ref() else {
            self.navigate(cwd);
            return;
        };
        if !cwd.starts_with(root) {
            self.workspace.outside_root = Some(cwd);
            return;
        }
        self.workspace.outside_root = None;
        self.workspace.revealed_directory = Some(cwd.clone());
        self.workspace.tree_dirty = true;
        let mut ancestor = cwd.as_path();
        while ancestor != root {
            if !self
                .workspace
                .layout
                .expanded_folders
                .iter()
                .any(|p| p == ancestor)
            {
                self.workspace
                    .layout
                    .expanded_folders
                    .push(ancestor.to_owned());
            }
            self.workspace
                .tree
                .set_expanded(&ancestor.to_string_lossy(), true);
            let Some(parent) = ancestor.parent() else {
                break;
            };
            ancestor = parent;
        }
        if cwd != *root {
            self.workspace.pending_reveal = Some(cwd.clone());
            self.workspace.tree.reveal(&cwd.to_string_lossy(), tx);
        }
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(super) fn reveal_file_result(&mut self, path: PathBuf, tx: &mut Tx) {
        if let Some(root) = self.workspace.directory.clone()
            && path.starts_with(&root)
        {
            let mut parent = path.parent();
            while let Some(folder) =
                parent.filter(|folder| *folder != root && folder.starts_with(&root))
            {
                if !self
                    .workspace
                    .layout
                    .expanded_folders
                    .iter()
                    .any(|p| p == folder)
                {
                    self.workspace
                        .layout
                        .expanded_folders
                        .push(folder.to_owned());
                }
                self.workspace
                    .tree
                    .set_expanded(&folder.to_string_lossy(), true);
                parent = folder.parent();
            }
            self.workspace.revealed_directory = Some(path.clone());
            self.workspace.pending_reveal = Some(path.clone());
            self.workspace.tree.reveal(&path.to_string_lossy(), tx);
        }
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(super) fn collapse_tree(&mut self) {
        self.workspace.tree.collapse_all();
        self.workspace.tree_dirty = true;
        self.workspace.layout.expanded_folders.clear();
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(super) fn open_tree_selection(&mut self) {
        let entry = self
            .workspace
            .tree
            .cursor()
            .and_then(|id| self.workspace.tree.get(id))
            .and_then(|n| n.entry.clone());
        if let Some(entry) = entry {
            if entry.directory {
                self.workspace.tree.toggle(&entry.path.to_string_lossy());
                self.sync_tree_expansion();
            } else {
                self.open_file(entry.path);
            }
        }
    }

    fn rebuild_tree(&mut self) {
        let workspace = &mut self.workspace;
        workspace.tree_dirty = false;
        let selected = workspace.tree.cursor().map(str::to_owned);
        workspace.tree.clear();
        let Some(root) = workspace.directory.as_ref() else {
            return;
        };
        for entry in &workspace.entries {
            let id = entry.path.to_string_lossy().into_owned();
            let parent = entry
                .path
                .parent()
                .filter(|p| *p != root)
                .map(|p| p.to_string_lossy().into_owned());
            workspace.tree.insert(
                parent.as_deref(),
                &id,
                TreeEntry {
                    entry: Some(entry.clone()),
                    label: entry.name.clone(),
                    badge: workspace
                        .repo
                        .as_ref()
                        .map(|r| r.badge(&entry.path))
                        .unwrap_or_default(),
                },
            );
            workspace
                .tree
                .set_expanded(&id, workspace.layout.expanded_folders.contains(&entry.path));
        }
        // An unloaded folder needs a disclosure affordance before its children arrive.
        for entry in &workspace.entries {
            let id = entry.path.to_string_lossy();
            if entry.directory && !workspace.tree.has_children(&id) {
                let error = workspace.tree_errors.iter().find(|(p, _)| p == &entry.path);
                let label = error
                    .map(|(_, e)| format!("Could not load: {e}"))
                    .unwrap_or_else(|| {
                        if workspace.layout.expanded_folders.contains(&entry.path) {
                            "Empty folder".into()
                        } else {
                            "Loading…".into()
                        }
                    });
                workspace.tree.insert(
                    Some(&id),
                    format!("{id}/::folder-state"),
                    TreeEntry {
                        entry: None,
                        label,
                        badge: String::new(),
                    },
                );
            }
        }
        if let Some(id) = selected {
            workspace.tree.select(&id);
        }
    }

    fn sync_tree_expansion(&mut self) -> bool {
        let mut changed = false;
        for entry in &self.workspace.entries {
            if !entry.directory {
                continue;
            }
            let expanded = self
                .workspace
                .tree
                .is_expanded(&entry.path.to_string_lossy());
            let saved = self.workspace.layout.expanded_folders.contains(&entry.path);
            if expanded != saved {
                self.workspace
                    .layout
                    .expanded_folders
                    .retain(|p| p != &entry.path);
                if expanded {
                    self.workspace
                        .layout
                        .expanded_folders
                        .push(entry.path.clone());
                }
                changed = true;
            }
        }
        if changed {
            self.workspace.tree_dirty = true;
            self.workspace.scan_token += 1;
            self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        }
        changed
    }

    pub(super) fn open_file(&mut self, path: PathBuf) {
        self.open_file_with_preview(path, false);
    }

    pub(super) fn preview_tree_selection(&mut self) {
        if let Some(entry) = self.selected_entry().filter(|e| !e.directory) {
            self.open_file_with_preview(entry.path, !self.config.single_click_keeps_open);
        }
    }

    /// Common activation boundary: restoration and explicit Open keep a file;
    /// explorer inspection may reuse only a clean, idle temporary tab.
    pub(super) fn open_file_with_preview(&mut self, path: PathBuf, preview: bool) {
        self.workspace.latest_activation = Some(path.clone());
        if let Some(i) = self
            .workspace
            .tabs
            .iter()
            .position(|tab| tab.document.path == path)
        {
            if !preview {
                self.workspace.tabs[i].preview = false;
            }
            self.show_editor(i, !preview);
            return;
        }
        if let Some((_, temporary)) = self.workspace.opening.iter_mut().find(|(p, _)| p == &path) {
            *temporary &= preview;
            return;
        }
        if self.io(Job::Open(path.clone())) {
            self.workspace.opening.push((path, preview));
            self.message = "Opening file…".into();
        }
    }
    pub(super) fn activate_editor(&mut self, index: usize) {
        self.show_editor(index, true);
    }

    fn show_editor(&mut self, index: usize, focus: bool) {
        self.settings = false;
        self.workspace.explorer_revealed = false;
        self.workspace.terminal_focus = false;
        self.workspace.active = index;
        self.workspace.layout.editor = true;
        if let Some(tab) = self.workspace.tabs.get_mut(index) {
            // Preview inspection keeps tree focus for continued keyboard
            // browsing. Explicit Open and tab activation focus the editor.
            tab.edit.focused = focus && tab.mode == View::Edit;
        }
        self.invalidate_diff(true);
    }
    pub(super) fn persist_workspace(&mut self) {
        let workspace = &mut self.workspace;
        workspace.layout.explorer_directory = workspace.directory.clone();
        workspace.layout.files = workspace
            .tabs
            .iter()
            .filter(|t| !t.preview)
            .map(|t| t.document.path.clone())
            .collect();
        workspace.layout.active_file = workspace
            .tabs
            .get(workspace.active)
            .filter(|t| !t.preview)
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
        // Explicit saving expresses intent to keep the file and prevents a
        // later inspection from replacing the tab while its save is queued.
        tab.preview = false;
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
            tab.preview = false;
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
                tab.preview = false;
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
                    tab.preview = false;
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
            if updated == tab.edit.text {
                return;
            }
            tab.preview = false;
            tab.edit.undo_group(|edit| {
                edit.anchor = 0;
                edit.focus = edit.text.len();
                edit.insert(&updated);
            });
            tab.revision += 1;
        }
    }
    pub(super) fn workspace_poll(&mut self, tx: &mut Tx) -> bool {
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
                    match result {
                        Ok(document) => {
                            if let Some(i) = self
                                .workspace
                                .tabs
                                .iter()
                                .position(|t| t.document.path == document.path)
                            {
                                self.show_editor(i, !preview);
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
                                    revision: 0,
                                    baseline: 0,
                                    after_save: None,
                                },
                            );
                            if self.workspace.restore_active.is_none()
                                || self.workspace.restore_active.as_ref()
                                    == Some(&self.workspace.tabs[index].document.path)
                            {
                                self.show_editor(index, !preview);
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
                                Ok(listing) => {
                                    self.workspace.tree_dirty |= self.workspace.entries
                                        != listing.entries
                                        || self.workspace.tree_errors != listing.errors;
                                    let path = listing.root;
                                    if !self.workspace.address.focused {
                                        self.workspace.address.replace_text(path.to_string_lossy());
                                    }
                                    self.workspace.directory = Some(path);
                                    self.workspace.entries = listing.entries;
                                    self.workspace.tree_errors = listing.errors;
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
                expanded: self.workspace.layout.expanded_folders.clone(),
                respect_ignore: self.config.explorer.respect_ignore,
                reveal: self.workspace.revealed_directory.clone(),
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
        tx.wake_after(Duration::from_millis(200));
        changed
    }
}

fn explorer(s: &State, a: &Appearance) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("explorer")
        .w(workspace.layout.explorer_width)
        .min_w(160.0)
        .h_full()
        .min_h(0.0)
        .fill(a.panel())
        .px(10.0)
        .gap(6.0)
        .child(
            row()
                .h(35.0)
                .center_y()
                .child(text("FILES").font(ui_label()).color(a.accent()))
                .child(el().grow(1.0))
                .child(button(a, "explorer-hide", "×", |s, tx| {
                    s.dispatch(AppAction::ToggleExplorer, tx)
                })),
        )
        .child(
            text_input(&workspace.address, ui_label())
                .caret(a.accent(), a.selection())
                .key("explorer-address")
                .label("Explorer folder path")
                .single_line()
                .w_full()
                .h(30.0)
                .color(a.ink())
                .on_edit(|s: &mut State| &mut s.workspace.address)
                .on_submit(|s: &mut State, tx| {
                    s.dispatch(AppAction::ExplorerSetRoot, tx);
                    tx.blur();
                }),
        )
        .child(
            row()
                .gap(4.0)
                .child(button(a, "explorer-up", "↑", |s, tx| {
                    s.dispatch(AppAction::ExplorerParent, tx)
                }))
                .child(button(a, "explorer-refresh", "↻", |s, tx| {
                    s.dispatch(AppAction::RefreshExplorer, tx)
                }))
                .child(button(
                    a,
                    "explorer-follow",
                    if workspace.layout.follow {
                        "Follow ✓"
                    } else {
                        "Follow"
                    },
                    |s, tx| s.dispatch(AppAction::ToggleFollow, tx),
                )),
        )
        .child(
            row()
                .gap(4.0)
                .child(button(
                    a,
                    "explorer-hidden",
                    if workspace.layout.hidden {
                        "Hidden ✓"
                    } else {
                        "Hidden"
                    },
                    |s, tx| s.dispatch(AppAction::ToggleHidden, tx),
                ))
                .child(button(
                    a,
                    "explorer-ignore",
                    if s.config.explorer.respect_ignore {
                        "Ignore ✓"
                    } else {
                        "Ignore"
                    },
                    |s, tx| s.dispatch(AppAction::ToggleIgnore, tx),
                )),
        );
    if let Some(cwd) = &workspace.outside_root {
        pane = pane
            .child(
                text(format!(
                    "Terminal is outside this host folder: {}",
                    cwd.display()
                ))
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
            )
            .child(button(
                a,
                "explorer-use-terminal-root",
                "Use terminal folder",
                |s, tx| s.dispatch(AppAction::ExplorerTerminalRoot, tx),
            ));
    }
    if let Some(path) = &workspace.operation_result
        && workspace
            .directory
            .as_ref()
            .is_none_or(|root| !path.starts_with(root))
    {
        pane = pane
            .child(
                text(format!("Last result: {}", path.display()))
                    .font(ui_label())
                    .color(a.muted())
                    .wrap_anywhere(),
            )
            .child(button(
                a,
                "explorer-reveal-result",
                "Show destination",
                |s, tx| s.dispatch(AppAction::RevealFileOperation, tx),
            ));
    }
    let danger = a.danger();
    let dark = a.dark;
    let mut tree_style = TreeStyle::from_kit(a.kit())
        .label(ui_label(), a.ink(), a.ink())
        .radius(6.0);
    tree_style.row_height = 28.0_f32.max(ui_label().size * 1.5 + 6.0);
    tree_style.selected = a.hover_bg();
    let list = Tree::new(
        |s: &State| &s.workspace.tree,
        |s: &mut State| &mut s.workspace.tree,
        |node| &node.label,
    )
    .name("Host files")
    .style(tree_style)
    .row(move |node, row| {
        let mut item = TreeItem::new(&node.label).badge(&node.badge);
        if let Some(entry) = &node.entry {
            item = item.icon(file_icons::resolve(
                &entry.path,
                entry.directory,
                row.expanded == Some(true),
                dark,
            ));
        }
        if node.entry.as_ref().is_some_and(|entry| entry.missing) {
            item.label_color(danger)
        } else {
            item
        }
    })
    .on_select(|s, _, tx| {
        s.workspace.terminal_focus = false;
        s.dispatch(AppAction::PreviewExplorerSelection, tx);
    })
    .on_open(|s, _, tx| s.dispatch(AppAction::ExplorerOpenSelection, tx))
    .build(s, a.kit())
    .instant_changes()
    .pb(ui_label().size * 0.25)
    .grow(1.0)
    .min_h(0.0)
    .on_secondary_press(|s: &mut State, press, tx| s.file_context_menu(press, tx));
    pane = pane.child(list);
    if workspace.operation_busy {
        pane = pane.child(text("Working on files…").font(ui_label()).color(a.muted()));
    }
    if workspace.scan_busy {
        pane = pane.child(text("Loading…").font(ui_label()).color(a.muted()));
    } else if workspace.entries.is_empty() && workspace.explorer_error.is_none() {
        pane = pane.child(text("No visible files").font(ui_label()).color(a.muted()));
    }
    if let Some(error) = &workspace.explorer_error {
        pane = pane.child(
            text(format!("Could not load folder: {error}"))
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    }
    if let Some(error) = &workspace.git_error {
        pane = pane.child(
            text(error)
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    }
    pane
}

fn editor(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("editor-pane")
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .fill(a.bg())
        .on_layout(|s: &mut State, r| s.workspace.editor_width = r[2]);
    let mut tabs = row()
        .key("editor-tabs")
        .h(38.0_f32.max(ui_label().size * 1.5 + 16.0))
        .px(6.0)
        .gap(3.0)
        .center_y()
        .w_full()
        .scroll_x()
        .fill(a.panel());
    if width < 880.0 {
        tabs = tabs.child(button(a, "return-terminal", "‹ Terminal", |s, tx| {
            s.dispatch(AppAction::ShowTerminal, tx)
        }));
    }
    let mut tab_items = Vec::new();
    for (i, tab) in workspace.tabs.iter().enumerate() {
        let id = tab.id;
        let filename = tab.document.path.file_name().unwrap_or_default();
        let duplicate = workspace
            .tabs
            .iter()
            .filter(|other| other.document.path.file_name() == Some(filename))
            .count()
            > 1;
        let name = if duplicate {
            let parent = tab.document.path.parent().unwrap_or(&tab.document.path);
            let parent_name = parent
                .file_name()
                .unwrap_or(parent.as_os_str())
                .to_string_lossy();
            // Keep extending the suffix when equal immediate parent names would still collide.
            let equal_parent = workspace.tabs.iter().any(|other| {
                other.id != id
                    && other.document.path.file_name() == Some(filename)
                    && other.document.path.parent().and_then(|p| p.file_name())
                        == parent.file_name()
            });
            format!(
                "{} — {}",
                filename.to_string_lossy(),
                if equal_parent {
                    parent.to_string_lossy()
                } else {
                    parent_name
                }
            )
        } else {
            filename.to_string_lossy().into_owned()
        };
        tab_items.push(
            row()
                .key(format!("editor-group-{id}"))
                .gap(2.0)
                .center_y()
                .child(super::tabs::interactive(
                    button(
                        a,
                        format!("editor-tab-{id}"),
                        if tab.preview {
                            format!("{name} (preview)")
                        } else {
                            name
                        },
                        move |s, tx| s.dispatch(AppAction::ActivateEditor(i), tx),
                    )
                    .on_double_press(move |s: &mut State| {
                        if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                            tab.preview = false;
                        }
                    })
                    .w(180.0)
                    .pl(30.0)
                    .child(
                        el().pin(Some(8.0), Some(0.0), None, Some(0.0))
                            .w(16.0)
                            .center()
                            .a11y_hidden()
                            .child(
                                rune::icon(
                                    file_icons::resolve(&tab.document.path, false, false, a.dark),
                                    16.0,
                                )
                                .color(a.muted())
                                .key(format!("editor-file-icon-{id}")),
                            ),
                    )
                    .fill(if i == workspace.active && !s.settings {
                        a.hover_bg()
                    } else {
                        a.panel()
                    })
                    .help(tab.document.path.to_string_lossy()),
                    super::tabs::Target::Editor(id),
                ))
                .children(tab.preview.then(|| {
                    text("Preview")
                        .key(format!("editor-preview-{id}"))
                        .font(ui_style(10.0, 500.0))
                        .color(a.accent())
                }))
                .children(
                    tab.dirty()
                        .then(|| text("•").font(ui_label()).color(a.accent())),
                )
                .child(
                    button(a, format!("editor-close-{id}"), "×", move |s, tx| {
                        s.dispatch(AppAction::CloseEditor(Some(id)), tx)
                    })
                    .px(6.0),
                ),
        );
    }
    if s.settings_open {
        tab_items.insert(
            s.tab_strips.settings_position.min(tab_items.len()),
            row()
                .key("settings-group")
                .gap(2.0)
                .center_y()
                .child(super::tabs::interactive(
                    button(a, "settings-tab", "Settings", |s, tx| {
                        s.dispatch(AppAction::Settings, tx)
                    })
                    .fill(if s.settings { a.hover_bg() } else { a.panel() }),
                    super::tabs::Target::Settings,
                ))
                .child(
                    button(a, "settings-close", "×", |s, tx| {
                        s.dispatch(AppAction::CloseSettings, tx)
                    })
                    .px(6.0),
                ),
        );
    }
    pane = pane.child(super::tabs::scroll_strip(
        tabs.children(tab_items),
        s,
        false,
    ));
    if s.settings {
        return pane.child(super::settings::content(s, a));
    }
    let Some(tab) = workspace.tabs.get(workspace.active) else {
        return pane.child(
            col().grow(1.0).center().child(
                text("Open a file from Files")
                    .font(fonts::editor())
                    .color(a.muted()),
            ),
        );
    };
    let id = tab.id;
    let mut modes = row().grow(1.0).min_w(0.0).gap(2.0).scroll_x().child(
        button(a, "file-edit", "Edit", |s, tx| {
            s.dispatch(AppAction::EditFile, tx)
        })
        .h(26.0)
        .fill(if tab.mode == View::Edit {
            a.hover_bg()
        } else {
            a.bg()
        }),
    );
    for (key, label, mode) in [
        ("diff-current", "Current", Comparison::Current),
        ("diff-unstaged", "Unstaged", Comparison::Unstaged),
        ("diff-staged", "Staged", Comparison::Staged),
    ] {
        modes = modes.child(
            button(a, key, label, move |s, tx| {
                s.dispatch(AppAction::Compare(mode), tx)
            })
            .h(26.0)
            .fill(if tab.mode == View::Diff(mode) {
                a.hover_bg()
            } else {
                a.bg()
            }),
        );
    }
    let controls = row()
        .w_full()
        .h(36.0_f32.max(ui_label().size * 1.5 + 14.0))
        .px(8.0)
        .gap(6.0)
        .center_y()
        .child(modes)
        .child(
            button(
                a,
                "file-save",
                if tab.saving { "Saving…" } else { "Save" },
                move |s, tx| s.dispatch(AppAction::SaveEditor(Some(id)), tx),
            )
            .h(26.0),
        );
    pane = pane.child(controls).child(
        text(tab.document.path.to_string_lossy())
            .font(ui_style(10.0, 400.0))
            .color(a.muted())
            .truncate()
            .w_full()
            .px(10.0)
            .py(4.0),
    );
    if let Some(error) = &tab.disk_error {
        pane = pane.child(
            text(format!("Disk: {error}"))
                .font(ui_label())
                .color(a.danger())
                .wrap_anywhere(),
        );
    }
    if tab.external.is_some() && !tab.dismissed {
        pane = pane.child(
            col()
                .w_full()
                .fill(a.warning_bg())
                .py(6.0)
                .child(
                    text("Changed on disk. Your unsaved edits are preserved.")
                        .font(ui_label())
                        .color(a.ink())
                        .wrap(),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(button(
                            a,
                            "external-reload",
                            "Reload · discard edits",
                            move |s, _| s.reload_external(id),
                        ))
                        .child(button(a, "external-keep", "Keep edits", move |s, _| {
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
                            text_input(&workspace.find_query, ui_label())
                                .caret(a.accent(), a.selection())
                                .key("editor-find")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .h(28.0)
                                .color(a.ink())
                                .placeholder("Find text", a.muted())
                                .on_edit(|s: &mut State| &mut s.workspace.find_query)
                                .on_submit(|s: &mut State, tx| s.find_editor(false, tx)),
                        )
                        .child(button(a, "find-next", "Next", |s, tx| {
                            s.find_editor(false, tx)
                        }))
                        .child(button(a, "find-close", "×", |s, _| {
                            s.workspace.find = false
                        })),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            text_input(&workspace.replacement, ui_label())
                                .caret(a.accent(), a.selection())
                                .key("editor-replace")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .h(28.0)
                                .color(a.ink())
                                .placeholder("Replace with", a.muted())
                                .on_edit(|s: &mut State| &mut s.workspace.replacement),
                        )
                        .child(button(a, "replace-next", "Replace", |s, tx| {
                            s.find_editor(true, tx)
                        }))
                        .child(button(a, "replace-all", "All", |s, _| s.replace_all())),
                ),
        );
    }
    if tab.mode == View::Edit {
        let language = syntax::lang_for_path(&tab.document.path.to_string_lossy());
        let highlights = syntax::highlight(language, &tab.edit.text);
        let spans = SyntaxTheme::from_kit(a.kit()).spans(&highlights, 0);
        let field = text_input(&tab.edit, fonts::editor())
            .caret(a.accent(), a.selection())
            .key(format!("file-editor-{id}"))
            .label("File editor")
            .w_full()
            .color(a.ink())
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
                    tab.preview = false;
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
        pane = pane.child(a.kit().scroll_bar(viewport));
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
            .font(ui_style(10.0, 400.0))
            .color(a.muted()),
        );
    } else {
        pane = pane.child(
            row()
                .gap(4.0)
                .h(32.0)
                .scroll_x()
                .child(button(a, "diff-auto", "Auto", |s, _| {
                    s.workspace.diff_layout = 0
                }))
                .child(button(a, "diff-inline", "Inline", |s, _| {
                    s.workspace.diff_layout = 1
                }))
                .child(button(a, "diff-split", "Split", |s, _| {
                    s.workspace.diff_layout = 2
                }))
                .child(button(a, "diff-prev", "↑", |s, tx| {
                    s.workspace.diff.next_hunk(false, tx);
                }))
                .child(button(a, "diff-next", "↓", |s, tx| {
                    s.workspace.diff.next_hunk(true, tx);
                })),
        );
        pane = pane.child(
            text(&workspace.diff_label)
                .font(ui_style(10.0, 400.0))
                .color(a.muted())
                .wrap(),
        );
        if let Some(error) = &workspace.diff_error {
            pane = pane.child(
                text(error)
                    .font(ui_label())
                    .color(a.muted())
                    .wrap_anywhere(),
            );
        }
        if workspace.diff_patch.is_empty() {
            pane = pane.child(el().grow(1.0));
        } else {
            // Pinned Rune Kit fixes diff rows at 13px / 20px internally. Scale its
            // coordinate space and inverse-size its viewport together: virtualization,
            // scrolling, selection and hit testing retain their native coordinates.
            let zoom = s.config.fonts.editor_size / 13.0;
            let (width, height) = workspace.diff_viewport;
            let diff = DiffView::new(
                |s: &State| &s.workspace.diff,
                |s: &mut State| &mut s.workspace.diff,
            )
            .key("git-diff")
            .build(s, a.kit())
            .focusable(true)
            .absolute(0.0, 0.0)
            .size(width / zoom, height / zoom)
            .origin(0.0, 0.0)
            .scale(zoom);
            pane = pane.child(
                el().key("git-diff-viewport")
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .clip_soft(0.0)
                    .instant_changes()
                    .on_layout(|s: &mut State, rect| s.workspace.diff_viewport = (rect[2], rect[3]))
                    .child(diff),
            );
        }
    }
    pane
}

fn editor_available(s: &State) -> bool {
    s.workspace.layout.editor
        && (!s.workspace.tabs.is_empty() || s.settings_open || s.workspace.show_empty_editor)
}

fn explorer_minimum_width(s: &State) -> f32 {
    if editor_available(s) && s.workspace.layout.terminal {
        1120.0
    } else {
        880.0
    }
}

/// Actual visible panes, including a temporarily revealed compact explorer.
pub(super) fn pane_visibility(s: &State, width: f32) -> (bool, bool, bool) {
    let editor = editor_available(s) && (width >= 880.0 || !s.workspace.terminal_focus);
    let terminal = if width < 880.0 {
        !editor
    } else {
        s.workspace.layout.terminal || !editor
    };
    let explorer = s.workspace.layout.explorer
        && (width >= explorer_minimum_width(s) || s.workspace.explorer_revealed);
    (explorer, terminal, editor)
}

pub(super) fn body(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
    let (explorer_shown, terminal, show_editor) = pane_visibility(s, width);
    let explorer_visible = explorer_shown && width >= explorer_minimum_width(s);
    let mut body = row()
        .key("workspace")
        .instant_changes()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_layout(|s: &mut State, r| s.workspace.body_width = r[2]);
    if explorer_visible {
        body = body.child(explorer(s, a)).child(
            el().key("explorer-divider")
                .w(5.0)
                .h_full()
                .center()
                .child(el().w(1.0).h_full().fill(a.hover_bg()))
                .hover(a.accent().alpha(0.3))
                .on_drag(|s: &mut State, d, _| {
                    if d.phase == Phase::Down {
                        s.workspace.drag_width = s.workspace.layout.explorer_width;
                    }
                    s.workspace.layout.explorer_width = (s.workspace.drag_width + d.dx)
                        .clamp(160.0, (s.workspace.body_width * 0.35).max(160.0));
                }),
        );
    }
    let mut main = row().grow(1.0).min_w(0.0).h_full().min_h(0.0);
    if terminal {
        let mut pane = col()
            .key("terminal-pane")
            .h_full()
            .min_h(0.0)
            .min_w(0.0)
            .child(terminal_tabs(s, a));
        if s.search {
            pane = pane.child(
                row()
                    .w_full()
                    .px(12.0)
                    .py(6.0)
                    .gap(6.0)
                    .fill(a.panel())
                    .child(
                        text_input(&s.query, ui_label())
                            .key("search-query")
                            .single_line()
                            .grow(1.0)
                            .min_w(0.0)
                            .h(29.0)
                            .placeholder("Find in terminal…", a.muted())
                            .color(a.ink())
                            .caret(a.accent(), a.selection())
                            .on_edit(|s: &mut State| &mut s.query)
                            .on_submit(|s: &mut State, tx| {
                                s.message = if s
                                    .active()
                                    .is_some_and(|session| session.find(&s.query.text))
                                {
                                    "Match found".into()
                                } else {
                                    "No match in retained scrollback".into()
                                };
                                tx.blur();
                            }),
                    )
                    .child(button(a, "close-search", "×", |s, _| s.search = false)),
            );
        }
        pane = pane.child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .px(16.0)
                .py(12.0)
                .child(grid(s, a)),
        );
        main = main.child(pane.grow(if show_editor {
            1.0 - workspace.layout.editor_share
        } else {
            1.0
        }));
    }
    if show_editor {
        if terminal {
            main = main.child(
                el().key("editor-divider")
                    .w(5.0)
                    .h_full()
                    .center()
                    .child(el().w(1.0).h_full().fill(a.hover_bg()))
                    .hover(a.accent().alpha(0.3))
                    .on_drag(|s: &mut State, d, _| {
                        if d.phase == Phase::Down {
                            s.workspace.drag_share = s.workspace.layout.editor_share;
                        }
                        let width = s.workspace.body_width
                            - if s.workspace.layout.explorer && s.workspace.body_width >= 1120.0 {
                                s.workspace.layout.explorer_width + 5.0
                            } else {
                                0.0
                            };
                        s.workspace.layout.editor_share =
                            (s.workspace.drag_share - d.dx / width.max(1.0)).clamp(0.2, 0.8);
                    }),
            );
        }
        main = main.child(editor(s, a, width).grow(if terminal {
            workspace.layout.editor_share
        } else {
            1.0
        }));
    }
    body = body.child(main);
    if explorer_shown && !explorer_visible {
        body = body.child(explorer(s, a).absolute(0.0, 0.0).border(1.0, a.hover_bg()));
    }
    body
}

pub(super) fn file_dialog(s: &State, a: &Appearance) -> Option<El> {
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
        .child(button(a, "file-cancel", "Cancel", |s, _| {
            s.cancel_file_action()
        }));
    match *prompt {
        FilePrompt::Unsaved { id, action } => {
            buttons = buttons
                .child(button(a, "file-discard", "Discard", move |s, tx| {
                    s.discard_for_action(id, action, tx)
                }))
                .child(button(a, "file-save-confirm", "Save", move |s, _| {
                    s.save_file(id, false, Some(action))
                }));
        }
        FilePrompt::Conflict { id, after } => {
            buttons = buttons
                .child(button(a, "file-reload-confirm", "Reload", move |s, _| {
                    s.reload_external(id)
                }))
                .child(button(a, "file-overwrite", "Overwrite", move |s, _| {
                    s.save_file(id, true, after)
                }));
        }
    }
    Some(el().absolute(0.0,0.0).w_full().h_full().fill(a.scrim()).center()
        .child(col().key("file-dialog").modal(false,|s:&mut State|s.cancel_file_action()).w(470.0).px(24.0).py(24.0).gap(16.0).radius(10.0).fill(a.panel())
            .child(text(title).font(ui_style(17.0,600.0)).color(a.ink()))
            .child(text(tab.document.path.to_string_lossy()).font(ui_label()).color(a.muted()).wrap_anywhere())
            .child(text(if matches!(prompt,FilePrompt::Conflict{..}){"Saving would replace changes made outside this editor. Reload discards your editor changes."}else{"Choose Save, Discard, or Cancel to continue."}).font(ui_label()).color(a.muted()).wrap())
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
        preview: false,
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
    s.workspace.terminal_focus = false;
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
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
    s.workspace.layout.expanded_folders = vec![PathBuf::from("/workspace/rodtty/src")];
    s.rebuild_tree();
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

/// Developer render scene: inspect a changed file from the real repository passed via --cwd.
/// This uses the same read-only Git boundary as the interactive workspace.
pub(super) fn real_diff_demo(s: &mut State) -> anyhow::Result<()> {
    let directory = s.directory.clone();
    let repo = crate::git::inspect(&directory)?
        .ok_or_else(|| anyhow::anyhow!("--cwd must name a Git repository"))?;
    let path = repo
        .files
        .keys()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The preview repository has no changed text file"))?;
    let document = Document::open(&path)?;
    let result = crate::git::diff(&path, &document.saved.text, Comparison::Current)?;
    s.workspace.tabs = vec![Tab {
        id: 1,
        preview: false,
        edit: TextEdit::new(&document.saved.text),
        document,
        mode: View::Diff(Comparison::Current),
        external: None,
        disk_error: None,
        dismissed: false,
        saving: false,
        revision: 0,
        baseline: 0,
        after_save: None,
    }];
    s.workspace.next_id = 2;
    s.workspace.terminal_focus = false;
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
    s.workspace.directory = Some(directory.clone());
    s.workspace
        .address
        .replace_text(directory.to_string_lossy());
    s.workspace.entries = crate::files::list_directory(&directory, false)?.1;
    s.workspace.repo = Some(repo);
    s.rebuild_tree();
    s.workspace.diff = DiffState::new(rune_kit::diff::parse(&result.patch));
    s.workspace.diff_label = result.label;
    s.workspace.diff_patch = result.patch;
    s.workspace.worker = Some(Worker::new(s.wake.clone()));
    Ok(())
}

/// Developer render scene for recognizable artwork, selected rows, and dirty editor tabs.
pub(super) fn icons_demo(s: &mut State) {
    demo(s, false);
    let root = PathBuf::from("/workspace/icons");
    s.workspace.directory = Some(root.clone());
    s.workspace.address.replace_text(root.to_string_lossy());
    s.workspace.layout.terminal = false;
    s.workspace.entries = [
        ("alpine", true),
        ("alpine/Dockerfile", false),
        ("src", true),
        ("src/index.ts", false),
        ("unknown-folder", true),
        ("Dockerfile", false),
        ("package.json", false),
        ("main.rs", false),
        ("types.d.ts", false),
        ("mystery.rodttyunknown", false),
    ]
    .into_iter()
    .map(|(name, directory)| Entry {
        path: root.join(name),
        name: PathBuf::from(name)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        directory,
        missing: false,
    })
    .collect();
    s.workspace.layout.expanded_folders = vec![root.join("alpine"), root.join("src")];
    s.workspace.tabs.clear();
    for (index, name) in [
        "main.rs",
        "client/index.ts",
        "server/index.ts",
        "Dockerfile",
        "mystery.rodttyunknown",
    ]
    .into_iter()
    .enumerate()
    {
        let saved = TextFile::decode(b"// Saved file contents\n".to_vec()).unwrap();
        let edit = TextEdit::new(if index == 0 {
            "// Unsaved changes remain visible beside the Rust artwork\n"
        } else {
            &saved.text
        });
        s.workspace.tabs.push(Tab {
            id: index as u64 + 1,
            preview: false,
            document: Document {
                path: root.join(name),
                saved,
                existed: true,
            },
            edit,
            mode: View::Edit,
            external: None,
            disk_error: None,
            dismissed: false,
            saving: false,
            revision: u64::from(index == 0),
            baseline: 0,
            after_save: None,
        });
    }
    s.workspace.active = 0;
    s.workspace.next_id = 6;
    s.rebuild_tree();
    s.workspace
        .tree
        .select(&root.join("main.rs").to_string_lossy());
}

/// Tooling-only scene: inspect a nested tree of the actual selected host folder.
pub(super) fn host_tree_demo(s: &mut State) {
    let root = s.directory.clone();
    s.workspace.layout.explorer = true;
    s.workspace.layout.expanded_folders = vec![root.join("src"), root.join("src/desktop")];
    match crate::files::list_tree(
        &root,
        &s.workspace.layout.expanded_folders,
        false,
        &[],
        None,
    ) {
        Ok(listing) => {
            s.workspace.directory = Some(listing.root.clone());
            s.workspace
                .address
                .replace_text(listing.root.to_string_lossy());
            s.workspace.entries = listing.entries;
            s.workspace.tree_errors = listing.errors;
            s.workspace.repo = crate::git::inspect(&listing.root).ok().flatten();
            s.rebuild_tree();
            s.workspace
                .tree
                .select(&root.join("src/desktop/workspace.rs").to_string_lossy());
        }
        Err(error) => s.workspace.explorer_error = Some(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_bytes(runtime: &rune::Runtime<State>, page: usize, uv: [f32; 4]) -> Vec<u8> {
        let page = &runtime.images.pages[page];
        let [x, y, w, h] = uv.map(|v| v as usize);
        (y..y + h)
            .flat_map(|row| {
                let start = (row * page.width as usize + x) * 4;
                page.data[start..start + w * 4].to_vec()
            })
            .collect()
    }

    fn assert_tab_artwork(
        runtime: &mut rune::Runtime<State>,
        id: u64,
        expected: &'static rune::Icon,
    ) {
        let elements = runtime.elements();
        let key = format!("editor-file-icon-{id}");
        let artwork = elements
            .iter()
            .find(|e| e.key.as_deref() == Some(&key))
            .expect("editor tab has artwork beside its label");
        assert_image_artwork(runtime, artwork.rect, expected);
    }

    fn assert_image_artwork(
        runtime: &mut rune::Runtime<State>,
        rect: [f32; 4],
        expected: &'static rune::Icon,
    ) {
        if expected.name == "default_file.svg" {
            let glyph = runtime
                .instances
                .iter()
                .find(|i| {
                    i.params[1] == rune::render::KIND_GLYPH
                        && (i.rect[0] - rect[0]).abs() <= 1.01
                        && (i.rect[1] - rect[1]).abs() <= 1.01
                })
                .expect("generic document silhouette paints in the theme's icon color");
            let side = if glyph.uv[2] == 0.0 {
                glyph.rect[2]
            } else {
                glyph.uv[2]
            } as usize;
            let actual_uv = [glyph.uv[0] as usize, glyph.uv[1] as usize];
            let actual_page = glyph.params[2] as usize;
            let sample = |atlas: &rune::text::Atlas, page, [x, y]: [usize; 2]| {
                let data = atlas.page_data(page).unwrap();
                (y..y + side)
                    .flat_map(|row| data[row * 2048 + x..row * 2048 + x + side].to_vec())
                    .collect::<Vec<_>>()
            };
            let actual = sample(&runtime.text.atlas, actual_page, actual_uv);
            let reference = runtime.text.icon_quad(expected, 16, [0.0; 2]).unwrap();
            let margin = (side - 16) / 2;
            let uv = reference.atlas_uv().map(|v| v as usize - margin);
            assert_eq!(
                actual,
                sample(&runtime.text.atlas, reference.atlas_page(), uv),
                "generic fallback retains the upstream document silhouette"
            );
            return;
        }
        let image = runtime
            .instances
            .iter()
            .find(|i| {
                i.params[1] == rune::render::KIND_IMAGE
                    && (i.rect[0] - rect[0]).abs() < 1.0
                    && (i.rect[1] - rect[1]).abs() < 1.0
            })
            .expect("full-color SVG image");
        let actual = image_bytes(runtime, image.params[0] as usize, image.uv);
        let reference = runtime
            .text
            .color_icon(expected, 16, Color::BLACK, false)
            .expect("reference upstream SVG")
            .clone();
        let placed = runtime.images.place(&reference, [16.0, 16.0]).unwrap();
        assert_eq!(
            actual,
            image_bytes(runtime, placed.page, placed.rect.map(|v| v as f32)),
            "wrong displayed artwork: expected {}",
            expected.name
        );
    }

    fn assert_displayed_artwork(
        runtime: &mut rune::Runtime<State>,
        label: &str,
        expected: &'static rune::Icon,
    ) {
        let elements = runtime.elements();
        let file = elements
            .iter()
            .position(|e| e.role == "treeitem" && e.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing row {label}"));
        let artwork = elements
            .iter()
            .find(|e| e.parent == Some(file) && e.key.as_deref() == Some("icon"))
            .expect("artwork before the label");
        assert_image_artwork(runtime, artwork.rect, expected);
    }

    macro_rules! artwork {
        ($name:literal) => {{
            static EXPECTED: rune::Icon = rune::Icon {
                name: $name,
                view: 32.0,
                body: include_str!(concat!("../../assets/vscode-icons/icons/", $name)),
            };
            &EXPECTED
        }};
    }

    #[test]
    fn editor_artwork_keeps_parent_labels_dirty_marks_and_tab_actions_when_appearance_changes() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        icons_demo(&mut state);
        state.config.appearance = config::Appearance::Dark;
        let mut runtime = runtime_for(state);
        runtime.set_viewport((1600.0, 800.0), 1.0);
        runtime.frame(1.0);
        assert_tab_artwork(&mut runtime, 1, artwork!("file_type_rust.svg"));
        assert_tab_artwork(&mut runtime, 4, artwork!("file_type_docker.svg"));
        assert_tab_artwork(&mut runtime, 5, artwork!("default_file.svg"));
        for label in ["index.ts — client", "index.ts — server", "•"] {
            assert!(
                runtime
                    .elements()
                    .iter()
                    .any(|e| e.text.as_deref() == Some(label)),
                "retains visible {label}"
            );
        }
        click(&mut runtime, "editor-tab-2");
        assert_eq!(runtime.state.workspace.active, 1);
        assert_tab_artwork(&mut runtime, 2, artwork!("file_type_typescript.svg"));
        runtime.update(|s, _| s.config.appearance = config::Appearance::Light);
        runtime.frame(1.0);
        assert_tab_artwork(&mut runtime, 1, artwork!("file_type_light_rust.svg"));
        assert!(runtime.state.workspace.tabs[0].dirty());
        click(&mut runtime, "editor-close-2");
        assert_eq!(runtime.state.workspace.tabs.len(), 4);
    }

    #[test]
    fn explorer_artwork_matches_exact_names_compound_extensions_folders_and_current_appearance() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        for name in [
            "Dockerfile",
            "package.json",
            "main.rs",
            "types.d.ts",
            "mystery.rodttyunknown",
        ] {
            std::fs::write(root.path().join(name), "fixture\n").unwrap();
        }
        for name in ["src", "alpine", "unrecognized-folder"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        let mut state = state(root.path());
        state.workspace.layout.explorer = true;
        state.config.appearance = config::Appearance::System;
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        assert_displayed_artwork(&mut runtime, "Dockerfile", artwork!("file_type_docker.svg"));
        assert_displayed_artwork(&mut runtime, "package.json", artwork!("file_type_npm.svg"));
        assert_displayed_artwork(
            &mut runtime,
            "types.d.ts",
            artwork!("file_type_typescriptdef.svg"),
        );
        assert_displayed_artwork(
            &mut runtime,
            "mystery.rodttyunknown",
            artwork!("default_file.svg"),
        );
        assert_displayed_artwork(&mut runtime, "main.rs", artwork!("file_type_rust.svg"));
        assert_displayed_artwork(&mut runtime, "src", artwork!("folder_type_src.svg"));
        assert_displayed_artwork(
            &mut runtime,
            "unrecognized-folder",
            artwork!("default_folder.svg"),
        );
        click(&mut runtime, root.path().join("src").to_str().unwrap());
        assert_displayed_artwork(&mut runtime, "src", artwork!("folder_type_src_opened.svg"));
        runtime.set_system_dark(false);
        runtime.frame(1.0);
        assert_displayed_artwork(
            &mut runtime,
            "main.rs",
            artwork!("file_type_light_rust.svg"),
        );
        assert_displayed_artwork(
            &mut runtime,
            "alpine",
            artwork!("folder_type_light_alpine.svg"),
        );
        assert_displayed_artwork(&mut runtime, "src", artwork!("folder_type_src_opened.svg"));
        click(&mut runtime, root.path().join("alpine").to_str().unwrap());
        assert_displayed_artwork(
            &mut runtime,
            "alpine",
            artwork!("folder_type_light_alpine_opened.svg"),
        );
        click(
            &mut runtime,
            root.path().join("unrecognized-folder").to_str().unwrap(),
        );
        assert_displayed_artwork(
            &mut runtime,
            "unrecognized-folder",
            artwork!("default_folder_opened.svg"),
        );
    }

    #[test]
    fn explorer_displays_offline_file_artwork_beside_file_labels() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut state = state(root.path());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| !s.workspace.scan_busy);
        let runtime = runtime_for(state);
        let elements = runtime.elements();
        let file = elements
            .iter()
            .position(|e| e.role == "treeitem" && e.label.as_deref() == Some("main.rs"))
            .unwrap();
        let artwork = elements
            .iter()
            .find(|e| e.parent == Some(file) && e.key.as_deref() == Some("icon"))
            .expect("file artwork before the label");
        assert!(
            runtime
                .instances
                .iter()
                .any(|i| i.params[1] == rune::render::KIND_IMAGE
                    && (i.rect[0] - artwork.rect[0]).abs() < 1.0
                    && (i.rect[1] - artwork.rect[1]).abs() < 1.0),
            "file artwork paints as a full-color SVG image"
        );
    }
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
    fn single_click_previews_reuse_one_visible_tab_and_double_click_keeps_it() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let mut state = state(root.path());
        state.navigate(root.path().to_owned());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| s.workspace.entries.len() == 3);
        let mut runtime = runtime_for(state);
        click(&mut runtime, root.path().join("a.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| !s.workspace.tabs.is_empty());
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("a.txt (preview)")
        }));
        click(&mut runtime, root.path().join("b.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| {
            s.workspace
                .tabs
                .iter()
                .any(|t| t.document.path.ends_with("b.txt"))
        });
        assert_eq!(
            runtime
                .elements()
                .iter()
                .filter(|e| e
                    .key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("editor-tab-")))
                .count(),
            1
        );
        let (x, y) = runtime
            .find(root.path().join("b.txt").to_str().unwrap())
            .unwrap();
        runtime.frame(1.0);
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.1);
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("b.txt")
        }));
        click(&mut runtime, root.path().join("c.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 2);
        runtime.update(|s, _| s.persist_workspace());
        let layout = Layout::load(root.path()).unwrap();
        assert_eq!(layout.files, vec![root.path().join("b.txt")]);
        assert!(layout.active_file.is_none());
    }

    #[test]
    fn late_preview_load_does_not_replace_a_file_explicitly_reopened_while_loading() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let mut state = state(root.path());
        state.navigate(root.path().to_owned());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| s.workspace.entries.len() == 2);
        let mut runtime = runtime_for(state);
        click(&mut runtime, root.path().join("a.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| !s.workspace.tabs.is_empty());
        runtime.update(|s, tx| s.dispatch(AppAction::ExplorerOpenSelection, tx));
        runtime.frame(0.0);
        click(&mut runtime, root.path().join("b.txt").to_str().unwrap());
        // Explicit Open targets the existing retained tab before the worker's
        // queued preview load is delivered, as a fast keyboard/palette user can.
        runtime.update(|s, tx| {
            s.workspace
                .tree
                .select(&root.path().join("a.txt").to_string_lossy());
            s.dispatch(AppAction::ExplorerOpenSelection, tx);
        });
        wait_runtime(&mut runtime, |s| s.workspace.opening.is_empty());
        assert_eq!(
            runtime.state.workspace.tabs[runtime.state.workspace.active]
                .document
                .path,
            root.path().join("a.txt")
        );
        assert!(!runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("b.txt (preview)")
        }));
    }

    #[test]
    fn preview_while_saving_keeps_the_saving_file_and_uses_normal_close_protection() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.txt");
        let b = root.path().join("b.txt");
        std::fs::write(&a, "saved").unwrap();
        std::fs::write(&b, "other").unwrap();
        let mut state = state(root.path());
        state.navigate(root.path().to_owned());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| s.workspace.entries.len() == 2);
        let mut runtime = runtime_for(state);
        click(&mut runtime, a.to_str().unwrap());
        wait_runtime(&mut runtime, |s| !s.workspace.tabs.is_empty());
        // Queue the preview first so its result is delivered while A's real
        // save is outstanding. Worker results still go through workspace_poll.
        click(&mut runtime, b.to_str().unwrap());
        runtime.update(|s, tx| {
            s.dispatch(AppAction::SaveEditor(Some(1)), tx);
            s.dispatch(AppAction::CloseEditor(Some(1)), tx);
        });
        assert_eq!(
            runtime.state.message,
            "Wait for the current save to finish."
        );
        wait_runtime(&mut runtime, |s| {
            s.workspace.opening.is_empty() && !s.workspace.tabs.iter().any(|t| t.saving)
        });
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("a.txt")
        }));
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("b.txt (preview)")
        }));
        runtime.update(|s, _| s.persist_workspace());
        assert_eq!(Layout::load(root.path()).unwrap().files, vec![a.clone()]);
        assert_eq!(std::fs::read_to_string(a).unwrap(), "saved");
    }

    #[test]
    fn editing_preview_keeps_undo_find_conflicts_and_dirty_close_decisions() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.txt");
        let b = root.path().join("b.txt");
        std::fs::write(&a, "saved text").unwrap();
        std::fs::write(&b, "another file").unwrap();
        let mut state = state(root.path());
        state.navigate(root.path().to_owned());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| s.workspace.entries.len() == 2);
        let mut runtime = runtime_for(state);
        click(&mut runtime, a.to_str().unwrap());
        wait_runtime(&mut runtime, |s| !s.workspace.tabs.is_empty());
        fill(&mut runtime, "file-editor-1", "my draft");
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("a.txt")
        }));
        press(
            &mut runtime,
            Key::Command('z'),
            rune::Mods {
                ctrl: true,
                cmd: true,
                ..Default::default()
            },
        );
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "saved text");
        assert!(
            runtime.elements().iter().any(|e| e
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("a.txt")),
            "undo does not make the tab temporary again"
        );
        fill(&mut runtime, "file-editor-1", "my draft");
        click(&mut runtime, b.to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 2);
        click(&mut runtime, "editor-tab-1");
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "my draft");
        runtime.update(|s, tx| s.dispatch(AppAction::FindEditor, tx));
        runtime.frame(0.0);
        fill(&mut runtime, "editor-find", "draft");
        press(&mut runtime, Key::Enter, Default::default());
        let edit = &runtime.state.workspace.tabs[0].edit;
        assert_eq!(
            &edit.text[edit.anchor.min(edit.focus)..edit.anchor.max(edit.focus)],
            "draft"
        );
        std::fs::write(&a, "external text").unwrap();
        runtime.update(|s, _| s.workspace.last_scan = Instant::now() - Duration::from_secs(3));
        wait_runtime(&mut runtime, |s| s.workspace.tabs[0].external.is_some());
        runtime.update(|s, tx| s.dispatch(AppAction::SaveEditor(Some(1)), tx));
        wait_runtime(&mut runtime, |s| {
            matches!(s.pending, Some(Pending::File(FilePrompt::Conflict { .. })))
        });
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "external text");
        runtime.update(|s, _| s.cancel_file_action());
        click(&mut runtime, "editor-close-1");
        assert!(matches!(
            runtime.state.pending,
            Some(Pending::File(FilePrompt::Unsaved { .. }))
        ));
        runtime.update(|s, _| s.cancel_file_action());
        runtime.update(|s, _| s.persist_workspace());
        assert_eq!(Layout::load(root.path()).unwrap().files, vec![a]);
    }

    #[test]
    fn behavior_setting_applies_live_persists_resets_and_external_json_updates() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let mut state = state(root.path());
        state.navigate(root.path().to_owned());
        state.workspace.layout.explorer = true;
        wait(&mut state, |s| s.workspace.entries.len() == 4);
        let mut runtime = runtime_for(state);
        runtime.update(|s, tx| s.dispatch(AppAction::Settings, tx));
        runtime.frame(0.0);
        fill(&mut runtime, "settings-search", "single click");
        click(&mut runtime, "single-click-keeps-open");
        assert!(Config::load(root.path()).unwrap().single_click_keeps_open);
        click(&mut runtime, root.path().join("a.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 1);
        assert!(runtime.find("settings-tab").is_some());
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("a.txt")
        }));
        click(&mut runtime, root.path().join("b.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 2);
        click(&mut runtime, "settings");
        click(&mut runtime, "reset-single-click-keeps-open");
        assert!(!Config::load(root.path()).unwrap().single_click_keeps_open);
        click(&mut runtime, root.path().join("c.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 3);
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("c.txt (preview)")
        }));
        let mut config = Config::load(root.path()).unwrap();
        config.single_click_keeps_open = true;
        config.save(root.path()).unwrap();
        runtime.update(|s, _| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            s.poll_settings();
        });
        click(&mut runtime, root.path().join("d.txt").to_str().unwrap());
        wait_runtime(&mut runtime, |s| s.workspace.tabs.len() == 4);
        assert!(runtime.elements().iter().any(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("editor-tab-"))
                && e.label.as_deref() == Some("d.txt")
        }));
        runtime.update(|s, _| s.persist_workspace());
        assert_eq!(
            Layout::load(root.path()).unwrap().files,
            ["a.txt", "b.txt", "d.txt"].map(|name| root.path().join(name))
        );
        let restored = self::state(root.path());
        let mut restored = runtime_for(restored);
        wait_runtime(&mut restored, |s| s.workspace.tabs.len() == 3);
        assert!(restored.elements().iter().all(|e| {
            !e.label
                .as_deref()
                .is_some_and(|label| label.contains("(preview)"))
        }));
    }

    #[test]
    fn editor_tabs_disambiguate_reorder_and_middle_close_through_dirty_policy() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let mut state = state(root.path());
        for (index, folder) in ["client", "host", "shared"].iter().enumerate() {
            let path = root.path().join(folder).join("config.rs");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("{folder} contents")).unwrap();
            state.open_file(path);
            wait(&mut state, |s| s.workspace.tabs.len() == index + 1);
        }
        state.workspace.tabs[1].edit.insert("unsaved ");
        state.activate_editor(1);
        state.workspace.layout.terminal = false;
        let active = state.workspace.tabs[1].id;
        let first = state.workspace.tabs[0].id;
        let last = state.workspace.tabs[2].id;
        let mut runtime = runtime_for(state);
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("config.rs — host"))
        );
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("•"))
        );
        let (x, y) = runtime.find(&format!("editor-tab-{first}")).unwrap();
        let (end, _) = runtime.find(&format!("editor-tab-{last}")).unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_move(end, y);
        runtime.pointer_up(end, y);
        runtime.frame(0.0);
        assert_eq!(runtime.state.workspace.tabs[2].id, first);
        assert_eq!(
            runtime.state.workspace.tabs[runtime.state.workspace.active].id,
            active
        );
        assert!(
            runtime
                .state
                .workspace
                .tabs
                .iter()
                .find(|t| t.id == active)
                .unwrap()
                .edit
                .text
                .contains("unsaved")
        );
        let (x, y) = runtime.find(&format!("editor-tab-{active}")).unwrap();
        runtime.button_down(x, y, rune::PointerButton::Middle);
        runtime.button_up(x, y, rune::PointerButton::Middle);
        runtime.frame(0.0);
        assert!(runtime.find("file-dialog").is_some());
        assert_eq!(runtime.state.workspace.tabs.len(), 3);
    }

    #[test]
    fn settings_tab_can_move_and_middle_close_without_closing_file_editors() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        std::fs::write(&path, "notes").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.open_settings(&mut Tx::default());
        state.workspace.layout.terminal = false;
        let mut runtime = runtime_for(state);
        let (x, y) = runtime.find("settings-tab").unwrap();
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.popup_menu().is_some());
        runtime.resolve_popup(Some("left")).unwrap();
        runtime.frame(0.0);
        let settings = runtime.find("settings-tab").unwrap();
        let file = runtime.find("editor-tab-1").unwrap();
        assert!(settings.0 < file.0);
        runtime.button_down(settings.0, settings.1, rune::PointerButton::Middle);
        runtime.button_up(settings.0, settings.1, rune::PointerButton::Middle);
        runtime.frame(0.0);
        assert!(!runtime.state.settings_open);
        assert_eq!(runtime.state.workspace.tabs.len(), 1);
    }

    #[test]
    fn overflowing_editor_tabs_scroll_with_the_mouse_wheel() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let mut state = state(root.path());
        for i in 0..8 {
            let path = root.path().join(format!("file-{i}.rs"));
            std::fs::write(&path, "contents").unwrap();
            state.open_file(path);
            wait(&mut state, |s| s.workspace.tabs.len() == i + 1);
        }
        state.workspace.layout.terminal = false;
        let mut runtime = runtime_for(state);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find("editor-tab-1").unwrap();
        runtime.wheel_xy(x, y, 0.0, -5000.0);
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let last = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("editor-tab-8") && !e.exiting)
            .unwrap();
        assert!(last.visible[2] > 0.0);
        assert_eq!(runtime.state.workspace.tabs.len(), 8);
        assert_eq!(
            runtime.state.workspace.tabs[runtime.state.workspace.active].id,
            8
        );
    }

    #[test]
    fn explorer_create_actions_write_and_reveal_files_and_folders() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        for (action, name) in [
            (AppAction::CreateFile, "notes.txt"),
            (AppAction::CreateFolder, "work"),
        ] {
            runtime.update(|s, tx| s.dispatch(action, tx));
            runtime.frame(1.0);
            assert!(runtime.find("operation-dialog").is_some());
            press(&mut runtime, Key::Text(name.into()), Default::default());
            let (x, y) = runtime.find("operation-submit").unwrap();
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(0.0);
            wait_runtime(&mut runtime, |s| {
                !s.workspace.operation_busy && s.workspace.entries.iter().any(|e| e.name == name)
            });
            runtime.update(|_, _| {});
            runtime.frame(0.0);
            assert!(
                runtime
                    .find(root.path().join(name).to_str().unwrap())
                    .is_some()
            );
        }
        assert_eq!(std::fs::read(root.path().join("notes.txt")).unwrap(), b"");
        assert!(root.path().join("work").is_dir());
    }

    fn wait_runtime(runtime: &mut rune::Runtime<State>, condition: impl Fn(&State) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(8) {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(0.02);
            if condition(&runtime.state) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("application operation timed out: {}", runtime.state.message);
    }
    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        runtime.frame(1.0);
        let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0);
    }
    fn fill(runtime: &mut rune::Runtime<State>, key: &str, value: &str) {
        use super::super::tests::press;
        runtime.update(|_, tx| tx.focus(key, true));
        runtime.frame(1.0);
        press(
            runtime,
            Key::Command('a'),
            rune::Mods {
                ctrl: true,
                cmd: true,
                ..Default::default()
            },
        );
        press(runtime, Key::Text(value.into()), Default::default());
    }

    #[test]
    fn explorer_context_copy_uses_the_clicked_item_and_keeps_unsaved_edits() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("nested/file.txt"), "copy me").unwrap();
        let editor = root.path().join("draft.txt");
        std::fs::write(&editor, "saved").unwrap();
        let mut state = state(root.path());
        open(&mut state, &editor);
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        fill(&mut runtime, "file-editor-1", "unsaved draft");
        click(&mut runtime, source.to_str().unwrap());
        wait_runtime(&mut runtime, |s| {
            !s.workspace.scan_busy
                && s.workspace
                    .entries
                    .iter()
                    .any(|e| e.path == source.join("nested"))
        });
        runtime.update(|_, _| {});
        runtime.frame(1.0);
        let (x, y) = runtime.find(source.to_str().unwrap()).unwrap();
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(1.0);
        assert!(runtime.find("file-context-menu").is_some());
        click(&mut runtime, "context-copy-file");
        fill(
            &mut runtime,
            "operation-destination",
            destination.to_str().unwrap(),
        );
        click(&mut runtime, "operation-submit");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.operation_busy && s.message.starts_with("Copied")
        });
        assert_eq!(
            std::fs::read_to_string(destination.join("source/nested/file.txt")).unwrap(),
            "copy me"
        );
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "unsaved draft");
        assert_eq!(std::fs::read_to_string(editor).unwrap(), "saved");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.scan_busy
                && s.workspace
                    .entries
                    .iter()
                    .any(|e| e.path == destination.join("source"))
        });
        runtime.update(|_, _| {});
        runtime.frame(1.0);
        assert!(
            runtime
                .find(destination.join("source").to_str().unwrap())
                .is_some()
        );
        assert!(runtime.find(source.to_str().unwrap()).is_some());
        assert!(
            runtime
                .find(source.join("nested").to_str().unwrap())
                .is_some()
        );
    }

    #[test]
    fn explorer_copy_collision_choices_never_silently_overwrite_open_content() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("report.txt");
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "new report").unwrap();
        let target = destination.join("report.txt");
        std::fs::write(&target, "existing report").unwrap();
        let mut state = state(root.path());
        open(&mut state, &target);
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        fill(&mut runtime, "file-editor-1", "unsaved report");
        for choice in [
            "operation-cancel",
            "operation-keep-both",
            "operation-replace",
        ] {
            runtime.frame(1.0);
            let (x, y) = runtime.find(source.to_str().unwrap()).unwrap();
            runtime.secondary_down(x, y);
            runtime.secondary_up(x, y);
            runtime.frame(1.0);
            click(&mut runtime, "context-copy-file");
            fill(
                &mut runtime,
                "operation-destination",
                destination.to_str().unwrap(),
            );
            click(&mut runtime, "operation-submit");
            wait_runtime(&mut runtime, |s| {
                matches!(
                    s.pending,
                    Some(Pending::Operation(
                        super::super::file_operations::OperationPrompt::Collision(_)
                    ))
                )
            });
            runtime.update(|_, _| {});
            runtime.frame(1.0);
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "existing report");
            assert!(runtime.find("operation-keep-both").is_some());
            assert!(runtime.find("operation-replace").is_some());
            if choice == "operation-cancel" {
                press(&mut runtime, Key::Text(" ".into()), Default::default());
                assert!(
                    runtime.state.pending.is_none(),
                    "Space activates the focused Cancel button"
                );
            } else {
                click(&mut runtime, choice);
            }
            wait_runtime(&mut runtime, |s| !s.workspace.operation_busy);
            if choice == "operation-replace" {
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "new report");
            } else {
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "existing report");
            }
            if choice == "operation-cancel" {
                assert!(!destination.join("report copy.txt").exists());
            } else {
                assert_eq!(
                    std::fs::read_to_string(destination.join("report copy.txt")).unwrap(),
                    "new report"
                );
            }
            assert_eq!(runtime.state.workspace.tabs[0].edit.text, "unsaved report");
        }
    }

    #[test]
    fn explorer_file_operation_errors_leave_disk_and_editor_content_intact() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(source.join("child")).unwrap();
        let editor = source.join("draft.txt");
        std::fs::write(&editor, "saved draft").unwrap();
        let mut state = state(root.path());
        open(&mut state, &editor);
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        fill(&mut runtime, "file-editor-1", "unsaved draft");
        runtime.frame(1.0);
        let (x, y) = runtime.find(source.to_str().unwrap()).unwrap();
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(1.0);
        click(&mut runtime, "context-copy-file");
        fill(
            &mut runtime,
            "operation-destination",
            source.join("child").to_str().unwrap(),
        );
        click(&mut runtime, "operation-submit");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.operation_busy && s.message.contains("cannot be copied into itself")
        });
        runtime.update(|_, _| {});
        runtime.frame(1.0);
        assert!(runtime.elements().iter().any(|e| {
            e.text
                .as_deref()
                .is_some_and(|t| t.contains("File operation failed:"))
        }));
        assert!(!source.join("child/source").exists());
        runtime.update(|s, tx| s.dispatch(AppAction::CreateFile, tx));
        runtime.frame(1.0);
        fill(&mut runtime, "operation-name", "../outside.txt");
        click(&mut runtime, "operation-submit");
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("Enter a single file or folder name."))
        );
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.state.pending.is_none());
        runtime.update(|s, tx| s.dispatch(AppAction::CreateFile, tx));
        runtime.frame(1.0);
        fill(&mut runtime, "operation-name", "new.txt");
        fill(
            &mut runtime,
            "operation-destination",
            root.path().join("missing").to_str().unwrap(),
        );
        click(&mut runtime, "operation-submit");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.operation_busy && s.message.contains("Open destination folder")
        });
        assert!(!root.path().join("missing").exists());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "unsaved draft");
        assert_eq!(std::fs::read_to_string(editor).unwrap(), "saved draft");
    }

    #[test]
    fn responsive_workspace_keeps_editor_contents_and_remembers_pane_widths() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        std::fs::write(&path, "saved notes").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.workspace.tabs[0].edit.insert("draft ");
        state.workspace.layout.explorer = true;
        state.workspace.layout.explorer_width = 224.0;
        state.workspace.layout.editor_share = 0.4;
        let draft = state.workspace.tabs[0].edit.text.clone();
        let mut runtime = runtime_for(state);
        assert!(runtime.find("explorer").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("editor-pane").is_some());
        runtime.set_viewport((1000.0, 700.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_none());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("editor-pane").is_some());
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("editor-pane").is_some());
        assert!(runtime.find("terminal-pane").is_none());
        let (x, y) = runtime.find("return-terminal").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("editor-pane").is_none());
        let (x, y) = runtime.find("toggle-editor").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.find("editor-pane").is_some());
        assert!(runtime.find("terminal-pane").is_none());
        runtime.set_viewport((1200.0, 700.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("editor-pane").is_some());
        assert_eq!(runtime.state.workspace.layout.explorer_width, 224.0);
        assert_eq!(runtime.state.workspace.layout.editor_share, 0.4);
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, draft);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "saved notes");
    }

    #[test]
    fn explicitly_hiding_compact_panes_stays_hidden_when_the_window_widens() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        std::fs::write(&path, "notes").unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.workspace.layout.explorer = true;
        let mut runtime = runtime_for(state);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        for key in ["files", "explorer-hide", "toggle-editor"] {
            let (x, y) = runtime.find(key).unwrap();
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.settle_all();
            runtime.frame(0.0);
        }
        runtime.set_viewport((1200.0, 700.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_none());
        assert!(runtime.find("editor-pane").is_none());
        assert!(runtime.find("terminal-pane").is_some());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "notes");
    }

    #[test]
    fn new_workspace_keeps_the_explorer_beside_the_terminal_until_space_is_needed() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let mut state = state(root.path());
        state.workspace.layout.explorer = true;
        // Older saved workspaces may request an editor without any open files.
        state.workspace.layout.editor = true;
        let mut runtime = runtime_for(state);
        runtime.set_viewport((1000.0, 600.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("editor-pane").is_none());
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_none());
        let (x, y) = runtime.find("files").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_some());
        let (x, y) = runtime.find("settings").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_none());
        assert!(runtime.find("settings-search").is_some());
        assert!(runtime.find("terminal-pane").is_none());
        let (x, y) = runtime.find("return-terminal").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.state.settings_open);
        runtime.set_viewport((1200.0, 700.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("settings-search").is_some());
    }

    #[test]
    fn a_small_window_with_large_ui_text_can_create_a_file_by_keyboard() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let mut state = state(root.path());
        state.config.fonts.ui_size = 24.0;
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        runtime.set_full_keyboard_access(true);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.update(|s, tx| s.dispatch(AppAction::CreateFile, tx));
        runtime.frame(1.0);
        press(
            &mut runtime,
            Key::Text("large.txt".into()),
            Default::default(),
        );
        for _ in 0..3 {
            press(&mut runtime, Key::Tab, Default::default());
        }
        runtime.settle_all();
        runtime.frame(1.0);
        let rect = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("operation-submit"))
            .unwrap()
            .rect;
        assert!(
            rect[1] >= 0.0 && rect[1] + rect[3] <= 360.0,
            "focused Create control must be visible: {rect:?}"
        );
        press(&mut runtime, Key::Text(" ".into()), Default::default());
        wait_runtime(&mut runtime, |s| {
            !s.workspace.operation_busy && s.message.starts_with("Created")
        });
        assert_eq!(std::fs::read(root.path().join("large.txt")).unwrap(), b"");
    }

    #[test]
    fn copied_items_outside_the_root_offer_an_explicit_reveal_action() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = root.path().join("notes.txt");
        std::fs::write(&source, "copied notes").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        runtime.frame(1.0);
        let (x, y) = runtime.find(source.to_str().unwrap()).unwrap();
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(1.0);
        click(&mut runtime, "context-copy-file");
        fill(
            &mut runtime,
            "operation-destination",
            outside.path().to_str().unwrap(),
        );
        click(&mut runtime, "operation-submit");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.operation_busy && s.message.starts_with("Copied")
        });
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(root.path())
        );
        click(&mut runtime, "explorer-reveal-result");
        wait_runtime(&mut runtime, |s| {
            !s.workspace.scan_busy && s.workspace.entries.iter().any(|e| e.name == "notes.txt")
        });
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(outside.path())
        );
        assert!(
            runtime
                .find(outside.path().join("notes.txt").to_str().unwrap())
                .is_some()
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied notes");
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
    fn explorer_opens_descendants_without_replacing_the_chosen_root() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("src");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/hello.txt"), "hello").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        let (x, y) = runtime.find(folder.to_str().unwrap()).unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        wait(&mut runtime.state, |s| {
            s.workspace.entries.iter().any(|e| e.name == "nested")
        });
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(root.path())
        );
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("nested"))
        );
    }

    #[test]
    fn explorer_controls_filter_real_git_ignores_and_persist_owner_preferences() {
        use super::super::tests::runtime_for;
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
        std::fs::write(root.path().join(".gitignore"), "ignored/\n*.log\n").unwrap();
        std::fs::create_dir(root.path().join("ignored")).unwrap();
        std::fs::write(root.path().join("ignored/output"), "output").unwrap();
        std::fs::write(root.path().join("output.log"), "output").unwrap();
        std::fs::write(root.path().join("visible.txt"), "hello").unwrap();
        std::fs::write(root.path().join(".secret"), "hidden").unwrap();
        std::fs::write(root.path().join(".deleted"), "tracked hidden").unwrap();
        for args in [
            vec!["add", ".deleted"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "hidden fixture",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(root.path())
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        std::fs::remove_file(root.path().join(".deleted")).unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        assert!(
            runtime
                .find(root.path().join(".deleted").to_str().unwrap())
                .is_none()
        );
        assert!(
            runtime
                .find(root.path().join("ignored").to_str().unwrap())
                .is_none()
        );
        assert!(
            runtime
                .find(root.path().join("output.log").to_str().unwrap())
                .is_none()
        );
        for key in ["explorer-ignore", "explorer-hidden"] {
            let (x, y) = runtime.find(key).unwrap();
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(0.0);
        }
        wait(&mut runtime.state, |s| {
            !s.workspace.scan_busy && s.workspace.entries.iter().any(|e| e.name == ".secret")
        });
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        for name in ["ignored", "output.log", ".secret", ".deleted"] {
            assert!(
                runtime
                    .find(root.path().join(name).to_str().unwrap())
                    .is_some(),
                "missing {name}"
            );
        }
        let settings = Config::load(root.path()).unwrap();
        let json = serde_json::to_value(settings).unwrap();
        assert_eq!(json["explorer"]["show_hidden"], true);
        assert_eq!(json["explorer"]["respect_ignore"], false);
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
        state.dispatch(AppAction::ToggleFollow, &mut Tx::default());
        state.workspace_poll(&mut Tx::default());
        wait(&mut state, |s| s.workspace.tree.cursor() == child.to_str());
        assert_eq!(state.workspace.directory.as_deref(), Some(root.path()));
        assert!(state.workspace.tree.is_expanded(child.to_str().unwrap()));
    }

    #[test]
    fn explorer_follow_reveals_a_filtered_terminal_folder_without_changing_global_filters() {
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
        let cwd = root.path().join(".hidden/ignored/nested");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(root.path().join(".gitignore"), ".hidden/\n").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        state.directory = cwd.clone();
        state.dispatch(AppAction::ToggleFollow, &mut Tx::default());
        wait(&mut state, |s| s.workspace.tree.cursor() == cwd.to_str());
        let runtime = super::super::tests::runtime_for(state);
        assert!(runtime.find(cwd.to_str().unwrap()).is_some());
        assert!(!runtime.state.config.explorer.show_hidden);
        assert!(runtime.state.config.explorer.respect_ignore);
    }

    #[test]
    fn explorer_follow_outside_root_requires_explicit_root_change() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("host.txt"), "host").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        state.directory = outside.path().to_owned();
        state.dispatch(AppAction::ToggleFollow, &mut Tx::default());
        state.workspace_poll(&mut Tx::default());
        let mut runtime = runtime_for(state);
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(root.path())
        );
        let (x, y) = runtime.find("explorer-use-terminal-root").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        wait(&mut runtime.state, |s| {
            !s.workspace.scan_busy && s.workspace.entries.iter().any(|e| e.name == "host.txt")
        });
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(outside.path())
        );
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        assert!(
            runtime
                .find(outside.path().join("host.txt").to_str().unwrap())
                .is_some()
        );
    }

    #[test]
    fn explorer_keyboard_opens_nested_files_and_restores_root_and_expansion() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("source");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        let path = folder.join("nested/hello.txt");
        std::fs::write(&path, "host file").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        runtime.update(|s, tx| s.dispatch(AppAction::ExplorerFocus, tx));
        runtime.frame(0.0);
        for name in ["source", "nested"] {
            press(&mut runtime, Key::Text(name.into()), Default::default());
            press(&mut runtime, Key::Right, Default::default());
            wait(&mut runtime.state, |s| !s.workspace.scan_busy);
            runtime.update(|_, _| {});
            runtime.frame(0.0);
            // Advance the type-ahead clock beyond its one-second prefix interval.
            runtime.frame(1.1);
        }
        press(&mut runtime, Key::End, Default::default());
        press(&mut runtime, Key::Enter, Default::default());
        wait(&mut runtime.state, |s| !s.workspace.tabs.is_empty());
        assert_eq!(runtime.state.workspace.tabs[0].document.path, path);
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "host file");
        runtime.state.persist_workspace();
        let mut restored = self::state(root.path());
        wait(&mut restored, |s| {
            !s.workspace.scan_busy && s.workspace.entries.iter().any(|e| e.name == "hello.txt")
        });
        let runtime = runtime_for(restored);
        assert!(runtime.find(path.to_str().unwrap()).is_some());
        assert_eq!(
            runtime.state.workspace.directory.as_deref(),
            Some(root.path())
        );
    }

    #[test]
    fn explorer_refresh_keeps_selection_and_shows_folder_failures_while_actions_work() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("source");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("existing.txt"), "hello").unwrap();
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        runtime.update(|s, tx| s.dispatch(AppAction::ExplorerFocus, tx));
        runtime.frame(0.0);
        press(&mut runtime, Key::Home, Default::default());
        press(&mut runtime, Key::Right, Default::default());
        wait(&mut runtime.state, |s| !s.workspace.scan_busy);
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        press(&mut runtime, Key::Down, Default::default());
        std::fs::write(folder.join("added.txt"), "added").unwrap();
        runtime.update(|s, tx| s.dispatch(AppAction::RefreshExplorer, tx));
        wait(&mut runtime.state, |s| {
            !s.workspace.scan_busy && s.workspace.entries.iter().any(|e| e.name == "added.txt")
        });
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        press(&mut runtime, Key::Enter, Default::default());
        wait(&mut runtime.state, |s| !s.workspace.tabs.is_empty());
        assert_eq!(
            runtime.state.workspace.tabs[0].document.path,
            folder.join("existing.txt")
        );
        runtime.update(|s, tx| {
            s.workspace
                .address
                .replace_text(root.path().join("missing").to_string_lossy());
            s.dispatch(AppAction::ExplorerSetRoot, tx);
            s.workspace_poll(tx);
            s.dispatch(AppAction::CommandPalette, tx);
        });
        runtime.frame(0.0);
        assert!(runtime.find("command-palette").is_some());
        wait(&mut runtime.state, |s| !s.workspace.scan_busy);
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        assert!(runtime.elements().iter().any(|e| {
            e.text
                .as_deref()
                .is_some_and(|t| t.contains("Could not load folder:"))
        }));
    }

    #[test]
    fn explorer_type_select_prefix_survives_an_unchanged_refresh() {
        use super::super::tests::{press, runtime_for};
        let root = tempfile::tempdir().unwrap();
        for name in ["sea.txt", "soap.txt"] {
            std::fs::write(root.path().join(name), "hello").unwrap();
        }
        let mut state = state(root.path());
        state.toggle_files();
        wait(&mut state, |s| !s.workspace.scan_busy);
        let mut runtime = runtime_for(state);
        runtime.update(|s, tx| s.dispatch(AppAction::ExplorerFocus, tx));
        runtime.frame(0.0);
        press(&mut runtime, Key::Text("s".into()), Default::default());
        runtime.update(|s, tx| s.dispatch(AppAction::RefreshExplorer, tx));
        wait(&mut runtime.state, |s| !s.workspace.scan_busy);
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        press(&mut runtime, Key::Text("o".into()), Default::default());
        press(&mut runtime, Key::Enter, Default::default());
        wait(&mut runtime.state, |s| {
            s.workspace
                .tabs
                .iter()
                .any(|t| t.document.path == root.path().join("soap.txt"))
        });
        assert_eq!(
            runtime.state.workspace.tabs[runtime.state.workspace.active]
                .document
                .path,
            root.path().join("soap.txt")
        );
    }

    #[test]
    fn legacy_explorer_preferences_move_to_owner_json_without_losing_workspace_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("settings.json"), "{}\n").unwrap();
        let workspace = serde_json::json!({ "explorer": true, "explorer_directory": root.path(), "follow": true, "hidden": true });
        std::fs::write(
            root.path().join("workspace-state.json"),
            serde_json::to_vec(&workspace).unwrap(),
        )
        .unwrap();
        let mut state = state(root.path());
        wait(&mut state, |s| !s.workspace.scan_busy);
        assert_eq!(state.workspace.directory.as_deref(), Some(root.path()));
        let config = Config::load(root.path()).unwrap();
        assert!(config.explorer.follow_terminal);
        assert!(config.explorer.show_hidden);
        state.persist_workspace();
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.path().join("workspace-state.json")).unwrap(),
        )
        .unwrap();
        assert!(json.get("follow").is_none());
        assert!(json.get("hidden").is_none());
    }

    #[test]
    fn workspace_controls_inspect_current_unstaged_and_staged_git_without_writes() {
        use super::super::tests::runtime_for;
        let root = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(root.path())
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        let path = root.path().join("notes.txt");
        std::fs::write(&path, "head\n").unwrap();
        git(&["add", "notes.txt"]);
        git(&["commit", "-m", "fixture"]);
        std::fs::write(&path, "index\n").unwrap();
        git(&["add", "notes.txt"]);
        std::fs::write(&path, "disk\n").unwrap();
        let index_path = root.path().join(".git/index");
        let index_before = std::fs::read(&index_path).unwrap();
        let mut state = state(root.path());
        open(&mut state, &path);
        state.workspace.tabs[0].edit.replace_text("buffer\n");
        let mut runtime = runtime_for(state);
        for (control, old, new) in [
            ("diff-current", "head", "buffer"),
            ("diff-unstaged", "index", "disk"),
            ("diff-staged", "head", "index"),
        ] {
            let (x, y) = runtime.find(control).unwrap();
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(0.0);
            runtime.update(|s, _| wait(s, |s| !s.workspace.diff_patch.is_empty()));
            runtime.settle_all();
            runtime.frame(0.0);
            assert!(runtime.find("git-diff").is_some());
            assert!(runtime.find("file-editor-1").is_none());
            assert!(
                runtime
                    .state
                    .workspace
                    .diff_patch
                    .contains(&format!("-{old}\n"))
            );
            assert!(
                runtime
                    .state
                    .workspace
                    .diff_patch
                    .contains(&format!("+{new}\n"))
            );
        }
        let (x, y) = runtime.find("diff-split").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        runtime.update(|s, tx| {
            s.workspace_poll(tx);
        });
        assert_eq!(runtime.state.workspace.diff.mode(), DiffMode::Split);
        let (x, y) = runtime.find("diff-auto").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        runtime.update(|s, tx| {
            s.workspace_poll(tx);
        });
        runtime.frame(0.0);
        for _ in 0..3 {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(0.0);
        }
        assert!(
            runtime.state.workspace.editor_width >= 620.0,
            "compact single-pane editor fills the window"
        );
        assert_eq!(runtime.state.workspace.diff.mode(), DiffMode::Unified);
        assert!(runtime.find("diff-next").is_some());
        assert!(runtime.find("return-terminal").is_some());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, "buffer\n");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "disk\n");
        assert_eq!(std::fs::read(index_path).unwrap(), index_before);
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
