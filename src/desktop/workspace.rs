use super::*;
use crate::{
    files::{Document, Entry, SaveFailure, TextFile},
    git::{Change, Comparison, Repository},
    work::{Event, Job, Layout, WindowGeometry, Worker},
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
    KeepWindow,
    Quit,
}
pub(super) enum FilePrompt {
    Unsaved { id: u64, action: Action },
    Conflict { id: u64, after: Option<Action> },
}
struct DeferredSaveConflict {
    id: u64,
    token: u64,
    revision: u64,
    baseline: u64,
    after: Option<Action>,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Edit,
    Diff(Comparison),
    /// Rendered markdown, offered for `.md` files.
    Preview,
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
    pub save_token: u64,
    pub revision: u64,
    pub baseline: u64,
    pub after_save: Option<Action>,
    pub autosave: autosave::Schedule,
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
    /// How the file differs from HEAD, and its changed line counts.
    change: Option<Change>,
    lines: Option<(u32, u32)>,
    /// The synthetic workspace-root row: a folder that exists in the tree but
    /// not in `entries`, labelled with the current folder's own name.
    root: bool,
}

/// Where keyboard focus goes back to when the compact Files overlay closes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FocusReturn {
    Terminal,
    /// The editor pane: the text editor of this tab, or the pane itself
    /// (a diff, preview or Settings) when `None`.
    Editor(Option<u64>),
}

/// Where Search Files opens a content match: in the file's source text, never
/// a rendered view, with the match selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceLocation {
    /// 1-based line, split as the editor splits lines.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    /// The match's length, in characters.
    pub length: usize,
}

impl SourceLocation {
    /// The location's byte range in `text`, clamped to the line it names so a
    /// buffer that changed since the search still gets a valid selection.
    pub(super) fn range(self, text: &str) -> std::ops::Range<usize> {
        let start_of_line = text
            .split_inclusive('\n')
            .take(self.line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>();
        let line = text[start_of_line..].split('\n').next().unwrap_or("");
        let offset = |from: usize, chars: usize| {
            line[from..]
                .char_indices()
                .map(|(at, _)| from + at)
                .chain([line.len()])
                .nth(chars)
                .unwrap_or(line.len())
        };
        let start = offset(0, self.column.saturating_sub(1));
        let end = offset(start, self.length);
        start_of_line + start..start_of_line + end
    }
}

struct RestoredPanes {
    editor_visible: bool,
    terminal_focus: bool,
    explorer_revealed: bool,
}

pub(super) struct Workspace {
    pub autosave_preferences: config::AutosavePreferences,
    pub autosave_context: Option<u64>,
    deferred_save_conflicts: Vec<DeferredSaveConflict>,
    /// Private so every change goes through `set_terminal_focus`, which counts it.
    terminal_focus: bool,
    /// Counts terminal focus changes and active-session switches, so an
    /// asynchronous terminal paste can tell that its target moved meanwhile.
    focus_generation: u64,
    pub explorer_revealed: bool,
    /// The root row's overflow menu is open.
    pub(super) explorer_menu: bool,
    /// Captured as the compact Files overlay opens.
    pub(super) overlay_return: Option<FocusReturn>,
    pub show_empty_editor: bool,
    orphan_edit: TextEdit,
    pub layout: Layout,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub worker: Option<Worker>,
    pub entries: Vec<Entry>,
    pub(super) tree: TreeModel<TreeEntry>,
    pub(super) operation_busy: bool,
    pub(super) removal: Option<super::removal::RemovalState>,
    pub(super) operation_moving: bool,
    pub(super) operation_result: Option<PathBuf>,
    pub(super) operation_id: u64,
    pub(super) operation_name: TextEdit,
    pub(super) operation_destination: TextEdit,
    pub(super) operation_error: Option<String>,
    pub(super) operation_context: Option<(f32, f32)>,
    pub(super) operation_viewport: (f32, f32),
    tree_errors: Vec<(PathBuf, String)>,
    /// Expanded folders the current `entries` came from reading successfully;
    /// only these can show **Empty folder**.
    tree_listed: Vec<PathBuf>,
    tree_dirty: bool,
    explorer_error: Option<String>,
    pub(super) outside_root: Option<PathBuf>,
    pending_reveal: Option<PathBuf>,
    revealed_directory: Option<PathBuf>,
    explorer_preferences: crate::config::ExplorerPreferences,
    pub directory: Option<PathBuf>,
    pub repo: Option<Repository>,
    pub git_error: Option<String>,
    scan_token: u64,
    scan_busy: bool,
    pub(super) last_scan: Instant,
    pub(super) last_follow: Option<PathBuf>,
    next_id: u64,
    opening: Vec<(PathBuf, bool)>,
    /// A Search Files match to select once its file's tab shows.
    pub(super) pending_location: Option<(PathBuf, SourceLocation)>,
    /// The match last revealed this way, while its selection stands.
    pub(super) search_reveal: Option<(u64, std::ops::Range<usize>)>,
    pub(super) latest_activation: Option<PathBuf>,
    restore_active: Option<PathBuf>,
    restore_panes: Option<RestoredPanes>,
    pub(super) restoration_warnings: Vec<String>,
    pub(super) last_persist: Instant,
    last_saved: Option<Vec<u8>>,
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
impl Workspace {
    pub(super) fn has_pending_opens(&self) -> bool {
        !self.opening.is_empty()
    }
    /// Whether keyboard input belongs to the terminal rather than the editor.
    pub(super) fn terminal_focus(&self) -> bool {
        self.terminal_focus
    }
    pub(super) fn set_terminal_focus(&mut self, focused: bool) {
        if self.terminal_focus != focused {
            self.terminal_focus = focused;
            self.focus_generation += 1;
        }
    }
    /// Record that the active terminal session changed, even if focus did not.
    pub(super) fn note_session_switch(&mut self) {
        self.focus_generation += 1;
    }
    pub(super) fn focus_generation(&self) -> u64 {
        self.focus_generation
    }
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            autosave_preferences: Default::default(),
            autosave_context: None,
            deferred_save_conflicts: Vec::new(),
            terminal_focus: true,
            focus_generation: 0,
            explorer_revealed: false,
            explorer_menu: false,
            overlay_return: None,
            show_empty_editor: false,
            orphan_edit: TextEdit::new(""),
            layout: Layout::default(),
            tabs: Vec::new(),
            active: 0,
            worker: None,
            entries: Vec::new(),
            tree: TreeModel::new().key("file-list"),
            operation_busy: false,
            removal: None,
            operation_moving: false,
            operation_result: None,
            operation_id: 0,
            operation_name: TextEdit::new(""),
            operation_destination: TextEdit::new(""),
            operation_error: None,
            operation_context: None,
            operation_viewport: (1080.0, 720.0),
            tree_errors: Vec::new(),
            tree_listed: Vec::new(),
            tree_dirty: true,
            explorer_error: None,
            outside_root: None,
            pending_reveal: None,
            revealed_directory: None,
            explorer_preferences: crate::config::ExplorerPreferences::default(),
            directory: None,
            repo: None,
            git_error: None,
            scan_token: 0,
            scan_busy: false,
            last_scan: Instant::now() - Duration::from_secs(3),
            last_follow: None,
            next_id: 1,
            opening: Vec::new(),
            pending_location: None,
            search_reveal: None,
            latest_activation: None,
            restore_active: None,
            restore_panes: None,
            restoration_warnings: Vec::new(),
            last_persist: Instant::now(),
            last_saved: None,
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
    pub(super) const WORKSPACE_ROOT_EXPANDED: bool = true;
    pub(super) fn file_io_pending(&self) -> bool {
        !self.workspace.opening.is_empty() || self.workspace.tabs.iter().any(|t| t.saving)
    }
    pub(super) fn remove_file_references(&mut self, path: &std::path::Path) {
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
    pub(super) fn persist_file_references(&mut self) {
        self.update_workspace_snapshot();
        self.io(Job::Layout {
            root: self.root.clone(),
            layout: self.workspace.layout.clone(),
        });
    }
    fn update_workspace_snapshot(&mut self) {
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
    pub(super) fn workspace_restoration_ready(&self) -> bool {
        self.workspace.opening.is_empty()
    }
    pub(super) fn workspace_setup(&mut self) {
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
    /// Files is drawn as the compact overlay above the terminal and editor.
    pub(super) fn explorer_overlay_open(&self) -> bool {
        let (shown, _, _) = pane_visibility(self, self.workspace.body_width);
        shown && self.workspace.body_width < explorer_minimum_width(self)
    }
    /// The Files toggle: also moves keyboard focus into the compact overlay as
    /// it opens, and back where it was as it closes.
    pub(super) fn toggle_files_with_focus(&mut self, tx: &mut Tx) {
        let was_overlay = self.explorer_overlay_open();
        let return_to = if self.workspace.terminal_focus() {
            FocusReturn::Terminal
        } else {
            FocusReturn::Editor(
                self.workspace
                    .tabs
                    .get(self.workspace.active)
                    .filter(|tab| !self.settings && !self.search_tab && tab.mode == View::Edit)
                    .map(|tab| tab.id),
            )
        };
        self.workspace.explorer_menu = false;
        self.toggle_files();
        let overlay = self.explorer_overlay_open();
        if overlay && !was_overlay {
            self.workspace.overlay_return = Some(return_to);
            tx.focus_when_shown("file-list", false);
        } else if was_overlay && !overlay {
            self.restore_overlay_focus(tx);
        }
    }
    fn restore_overlay_focus(&mut self, tx: &mut Tx) {
        match self.workspace.overlay_return.take() {
            Some(FocusReturn::Terminal) => {
                self.workspace.set_terminal_focus(true);
                tx.blur();
            }
            Some(FocusReturn::Editor(Some(id)))
                if self.workspace.tabs.iter().any(|tab| tab.id == id) =>
            {
                self.workspace.set_terminal_focus(false);
                if let Some(tab) = self.workspace.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.edit.focused = true;
                }
                tx.focus_when_shown(format!("file-editor-{id}"), true);
            }
            Some(FocusReturn::Editor(_)) => {
                self.workspace.set_terminal_focus(false);
                tx.blur();
            }
            None => {
                if self.workspace.terminal_focus() {
                    tx.blur();
                }
            }
        }
    }
    /// Escape over the compact overlay closes it, as the Files button does.
    pub(super) fn dismiss_explorer_overlay(&mut self, tx: &mut Tx) -> bool {
        if !self.explorer_overlay_open() {
            return false;
        }
        self.toggle_files_with_focus(tx);
        true
    }
    pub(super) fn navigate(&mut self, path: PathBuf) {
        self.workspace.directory = Some(path.clone());
        self.workspace.entries.clear();
        self.workspace.tree_listed.clear();
        self.workspace.tree.clear();
        self.workspace.tree_dirty = true;
        self.workspace.layout.expanded_folders.clear();
        self.workspace.layout.selected_path = None;
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

    /// Change only path references after a committed move. Keeping the TextEdit
    /// and saved version retains selection, undo, encoding, and conflict checks.
    pub(super) fn remap_moved_paths(&mut self, source: &std::path::Path, target: &std::path::Path) {
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
        // Collapse-all would otherwise close the synthetic root row the tree
        // shows its contents through; the workspace root stays open by design.
        if let Some(root) = self.workspace.directory.as_ref() {
            self.workspace
                .tree
                .set_expanded(&root.to_string_lossy(), Self::WORKSPACE_ROOT_EXPANDED);
        }
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
        let selected = workspace.tree.cursor().map(str::to_owned).or_else(|| {
            workspace
                .layout
                .selected_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
        });
        workspace.tree.clear();
        let Some(root) = workspace.directory.as_ref() else {
            return;
        };
        // The tree reads as a rooted folder: the first row is the workspace folder
        // itself, named as the host folder is, with every other row under it.
        let root_key = root.to_string_lossy().into_owned();
        let root_label = root
            .file_name()
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .into_owned();
        workspace.tree.insert(
            None,
            &root_key,
            TreeEntry {
                entry: None,
                label: root_label,
                badge: String::new(),
                change: None,
                lines: None,
                root: true,
            },
        );
        let root_expanded = Self::WORKSPACE_ROOT_EXPANDED;
        workspace.tree.set_expanded(&root_key, root_expanded);
        // First the whole parent-child structure: a parent precedes its
        // children (a stable sort by depth keeps each folder's sibling order),
        // so no entry is dropped for arriving before its folder and every
        // folder knows its real children before any status row is decided.
        let mut entries: Vec<&Entry> = workspace.entries.iter().collect();
        entries.sort_by_key(|entry| entry.path.components().count());
        for entry in entries {
            let id = entry.path.to_string_lossy().into_owned();
            // A nested entry stays under its own folder; only the root's direct
            // children hang from the synthetic root row.
            let parent = entry
                .path
                .parent()
                .filter(|p| *p != root)
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| root_expanded.then(|| root_key.clone()));
            workspace.tree.insert(
                parent.as_deref(),
                &id,
                TreeEntry {
                    entry: Some(entry.clone()),
                    root: false,
                    label: entry.name.clone(),
                    badge: workspace
                        .repo
                        .as_ref()
                        .map(|r| r.badge(&entry.path))
                        .unwrap_or_default(),
                    change: workspace
                        .repo
                        .as_ref()
                        .filter(|_| !entry.directory)
                        .and_then(|r| r.change(&entry.path))
                        .or(entry.missing.then_some(Change::Deleted)),
                    lines: workspace
                        .repo
                        .as_ref()
                        .filter(|_| !entry.directory)
                        .and_then(|r| r.line_changes(&entry.path)),
                },
            );
            workspace
                .tree
                .set_expanded(&id, workspace.layout.expanded_folders.contains(&entry.path));
        }
        // Then one status row per childless folder. It also gives an unloaded
        // folder its disclosure affordance before its children arrive.
        for entry in &workspace.entries {
            let id = entry.path.to_string_lossy();
            if entry.directory && !workspace.tree.has_children(&id) {
                let label = folder_status(
                    workspace
                        .tree_errors
                        .iter()
                        .find(|(p, _)| p == &entry.path)
                        .map(|(_, e)| e.as_str()),
                    workspace.tree_listed.contains(&entry.path),
                );
                workspace.tree.insert(
                    Some(&id),
                    format!("{id}/::folder-state"),
                    TreeEntry {
                        entry: None,
                        root: false,
                        label,
                        badge: String::new(),
                        change: None,
                        lines: None,
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
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
        if self
            .workspace
            .pending_location
            .as_ref()
            .is_some_and(|(pending, _)| *pending != path)
        {
            self.workspace.pending_location = None;
        }
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
        self.workspace.latest_activation = self
            .workspace
            .tabs
            .get(index)
            .map(|tab| tab.document.path.clone());
        self.show_editor(index, true);
    }

    pub(super) fn supersede_pending_preview(&mut self) {
        self.workspace.latest_activation = None;
    }

    fn show_editor(&mut self, index: usize, focus: bool) {
        // Opening a file from the compact overlay closes it and hands the
        // keyboard to the editor; the overlay's saved focus no longer applies.
        let from_overlay = self.explorer_overlay_open();
        let focus = focus || from_overlay;
        if from_overlay {
            self.workspace.overlay_return = None;
            self.workspace.explorer_menu = false;
        }
        self.settings = false;
        self.search_tab = false;
        self.workspace.explorer_revealed = false;
        self.workspace.set_terminal_focus(false);
        self.workspace.active = index;
        self.workspace.layout.editor = true;
        if let Some(tab) = self.workspace.tabs.get_mut(index) {
            // Preview inspection keeps tree focus for continued keyboard
            // browsing. Explicit Open and tab activation focus the editor.
            tab.edit.focused = focus && tab.mode == View::Edit;
        }
        self.invalidate_diff(true);
    }
    /// Notes the window's place on the desktop, for the next launch to reopen
    /// it there. The host reports geometry on the UI thread only, and a drag or
    /// resize moves the window far faster than the layout needs saving: this
    /// reads once a second and records a value only when it differs from the
    /// last save, so a settled window writes once instead of every frame.
    ///
    /// Returns true when the saved geometry changed, so the caller repaints.
    pub(super) fn capture_window_geometry(&mut self) -> bool {
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

    pub(super) fn persist_workspace(&mut self) {
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
    pub(super) fn request_action(&mut self, action: Action, tx: &mut Tx) {
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
    pub(super) fn on_window_close(&mut self, tx: &mut Tx) {
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
    pub(super) fn save_file(&mut self, id: u64, overwrite: bool, after: Option<Action>) {
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
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
        tab.save_token += 1;
        tab.autosave.suspend(tab.revision);
        tab.after_save = after;
        let job = Job::Save {
            id,
            token: tab.save_token,
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
        if self.workspace.removal.is_some() || self.workspace.operation_moving {
            self.message = "Finish the current file operation first.".into();
            return;
        }
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
            tab.autosave.suspend(tab.revision);
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
        if self.workspace.removal.is_some() {
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
        if replace && self.workspace.removal.is_some() {
            return;
        }
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
        if self.workspace.removal.is_some() {
            return;
        }
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

/// The status row under a folder that has no rows of its own. **Empty
/// folder** needs a successful listing of that folder that found nothing
/// visible; until then it is loading, and a failed listing says why.
fn folder_status(error: Option<&str>, listed: bool) -> String {
    match error {
        Some(error) => format!("Could not load: {error}"),
        None if listed => "Empty folder".into(),
        None => "Loading…".into(),
    }
}

/// An action at the trailing edge of the explorer root row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum RootControl {
    Search,
    Refresh,
    Follow,
    Hidden,
    Ignore,
}

impl RootControl {
    /// In row order. The row's width budget counts every control.
    pub(super) const ALL: [RootControl; 5] = [
        RootControl::Search,
        RootControl::Refresh,
        RootControl::Follow,
        RootControl::Hidden,
        RootControl::Ignore,
    ];
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Search => "explorer-search-files",
            Self::Refresh => "explorer-refresh",
            Self::Follow => "explorer-follow",
            Self::Hidden => "explorer-hidden",
            Self::Ignore => "explorer-ignore",
        }
    }
    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Search => &rune_kit::glyphs::SEARCH,
            Self::Refresh => &rune_kit::glyphs::REFRESH,
            Self::Follow => &rune_kit::glyphs::CROSSHAIR,
            Self::Hidden => &rune_kit::glyphs::EYE,
            Self::Ignore => &rune_kit::glyphs::FILTER,
        }
    }
    /// The overflow menu's item title.
    fn title(self) -> &'static str {
        match self {
            Self::Search => "Search Files",
            Self::Refresh => "Refresh Files",
            Self::Follow => "Follow Terminal",
            Self::Hidden => "Show Hidden Files",
            Self::Ignore => "Respect Git Ignore Rules",
        }
    }
    /// A toggle's current state; `None` for a plain action.
    fn active(self, s: &State) -> Option<bool> {
        match self {
            Self::Search | Self::Refresh => None,
            Self::Follow => Some(s.config.explorer.follow_terminal),
            Self::Hidden => Some(s.config.explorer.show_hidden),
            Self::Ignore => Some(s.config.explorer.respect_ignore),
        }
    }
    /// The accessible name; a toggle reports its state separately.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Search => "Search files",
            Self::Refresh => "Refresh files",
            Self::Follow => "Follow terminal",
            Self::Hidden => "Show hidden files",
            Self::Ignore => "Respect Git ignore rules",
        }
    }
    /// The tooltip, naming a toggle's state for sight.
    fn help(self, s: &State) -> String {
        let name = self.name();
        match self.active(s) {
            Some(true) => format!("{name}: on"),
            Some(false) => format!("{name}: off"),
            None => name.into(),
        }
    }
    fn action(self) -> AppAction {
        match self {
            Self::Search => AppAction::SearchFiles,
            Self::Refresh => AppAction::RefreshExplorer,
            Self::Follow => AppAction::ToggleFollow,
            Self::Hidden => AppAction::ToggleHidden,
            Self::Ignore => AppAction::ToggleIgnore,
        }
    }
}

/// The explorer's horizontal padding, on each side.
const EXPLORER_PADDING: f32 = 10.0;
/// A root-row control's hit target: the header's icon button width.
const ROOT_CONTROL_WIDTH: f32 = 30.0;
const ROOT_CONTROL_GAP: f32 = 2.0;
/// The tree row's own chrome before its label and after its trailing content:
/// end padding (6 each side), chevron (16), icon (16) and the gaps between (4 each).
const TREE_ROW_CHROME: f32 = 6.0 + 16.0 + 4.0 + 16.0 + 4.0 + 4.0 + 6.0;
/// The least of the root's name the row keeps beside its controls.
const ROOT_NAME_MINIMUM: f32 = 32.0;

/// Whether every root-row control fits at full hit-target size in an explorer
/// this wide, leaving the root's name some room. Otherwise one overflow
/// button stands in for them.
pub(super) fn root_controls_fit(explorer_width: f32) -> bool {
    let count = RootControl::ALL.len() as f32;
    let controls = count * ROOT_CONTROL_WIDTH + (count - 1.0) * ROOT_CONTROL_GAP;
    explorer_width - 2.0 * EXPLORER_PADDING - TREE_ROW_CHROME - controls >= ROOT_NAME_MINIMUM
}

/// The root row's trailing controls: each toggle shows whether it is on, or
/// one overflow button when the explorer is too narrow for them all.
fn root_controls(s: &State, a: &Appearance, width: f32, row_height: f32) -> El {
    let mut controls = row()
        .key("explorer-root-controls")
        .gap(ROOT_CONTROL_GAP)
        .center_y();
    let button = |key: &'static str, glyph, help: String, action: AppAction| {
        icon_button(a, key, glyph, help, move |s, tx| s.dispatch(action, tx))
            .min_h(row_height)
            .fill(Color::TRANSPARENT)
    };
    if root_controls_fit(width) {
        for control in RootControl::ALL {
            let mut button = button(
                control.key(),
                control.glyph(),
                control.help(s),
                control.action(),
            )
            .label(control.name());
            if let Some(on) = control.active(s) {
                button = button.toggle_state(a, on);
            }
            controls = controls.child(button);
        }
    } else {
        controls = controls.child(
            icon_button(
                a,
                "explorer-more",
                &rune_kit::glyphs::ELLIPSIS,
                "More explorer actions",
                |s, tx| {
                    s.workspace.explorer_menu = !s.workspace.explorer_menu;
                    if s.workspace.explorer_menu {
                        tx.focus_when_shown(
                            RootControl::ALL[0]
                                .key()
                                .replace("explorer-", "explorer-menu-"),
                            true,
                        );
                    }
                },
            )
            .min_h(row_height)
            .expanded(s.workspace.explorer_menu)
            .fill(if s.workspace.explorer_menu {
                a.hover_bg()
            } else {
                Color::TRANSPARENT
            }),
        );
    }
    controls
}

/// The overflow button's menu: each root-row action, with each toggle's state.
pub(super) fn explorer_menu(s: &State, a: &Appearance) -> Option<El> {
    if !s.workspace.explorer_menu {
        return None;
    }
    let close = |s: &mut State, tx: &mut Tx| {
        s.workspace.explorer_menu = false;
        tx.focus("explorer-more", true);
    };
    let mut menu = col()
        .key("explorer-menu")
        .role("menu")
        .label("Explorer actions")
        .w(240.0)
        .px(6.0)
        .py(6.0)
        .gap(3.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg())
        .shadow(6.0, 20.0, a.scrim())
        .enters(a, Entrance::Menu(motion::FROM_TOP_TRAILING))
        .modal_tx(true, close)
        .anchored("explorer-more", rune::Side::Bottom, rune::Align::End, 4.0);
    for control in RootControl::ALL {
        let help = control.help(s);
        let action = control.action();
        let key = control.key().replace("explorer-", "explorer-menu-");
        super::pressable(&key);
        let mut item = row()
            .key(key)
            .role(if control.active(s).is_some() {
                "menuitemcheckbox"
            } else {
                "menuitem"
            })
            .label(control.name())
            .help(help)
            .focusable(true)
            .focus_ring(2.0, 1.0, a.accent())
            .w_full()
            .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
            .px(10.0)
            .gap(8.0)
            .center_y()
            .radius(5.0)
            .hover(a.hover_bg())
            .on_press_tx(move |s: &mut State, tx| {
                s.workspace.explorer_menu = false;
                tx.focus("explorer-more", false);
                s.dispatch(action, tx);
            })
            .child(rune::icon(control.glyph(), 16.0).color(a.ink()))
            .child(
                text(control.title())
                    .font(ui_label())
                    .color(a.ink())
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate(),
            );
        if let Some(on) = control.active(s) {
            item = item.checked(on).child(
                text(if on { "On" } else { "Off" })
                    .key("state")
                    // The item's checked state says this to assistive technology.
                    .a11y_hidden()
                    .font(ui_label())
                    .color(if on { a.accent() } else { a.muted() }),
            );
        }
        menu = menu.child(item);
    }
    Some(menu)
}

fn explorer(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("explorer")
        .w(width)
        .min_w(160.0)
        .h_full()
        .min_h(0.0)
        .fill(a.panel())
        .px(EXPLORER_PADDING)
        .pt(6.0)
        .gap(6.0);
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
    let accent = a.accent();
    let success = a.success();
    let dark = a.dark;
    let workspace_root_path = workspace.directory.clone().unwrap_or_default();
    let mut tree_style = TreeStyle::from_kit(a.kit())
        .label(ui_label(), a.ink(), a.ink())
        .radius(6.0);
    tree_style.row_height = 28.0_f32.max(ui_label().size * 1.5 + 6.0);
    tree_style.selected = a.hover_bg();
    // The fill alone changes too little to mark the selected row: an accent
    // bar along its leading edge reads at 3:1 or more in every appearance.
    tree_style.selected_mark = Some(a.accent());
    // The root row's controls rest hidden and appear while the row is hovered
    // or holds keyboard focus (the row's hover track lights for both). A
    // selected row draws a fill instead of a hover track, so there they show.
    let controls = root_controls(s, a, width, tree_style.row_height);
    let list = Tree::new(
        |s: &State| &s.workspace.tree,
        |s: &mut State| &mut s.workspace.tree,
        |node| &node.label,
    )
    .name("Host files")
    .style(tree_style)
    .row(move |node, row| {
        let mut item = TreeItem::new(&node.label);
        if !node.badge.is_empty() {
            item = item.badge(&node.badge);
        }
        if node.root {
            // The workspace folder's own row: drawn open, rooted at the folder's
            // name, which truncates before the controls at its trailing edge.
            item = item
                .icon(file_icons::resolve(&workspace_root_path, true, true, dark))
                .trailing(if row.selected {
                    controls.clone()
                } else {
                    controls.clone().group_opacity(0.0, 1.0)
                });
        } else if let Some(entry) = &node.entry {
            item = item.icon(file_icons::resolve(
                &entry.path,
                entry.directory,
                row.expanded == Some(true),
                dark,
            ));
            // Git information takes the place of a size: a changed file's
            // name is tinted by its change, and an updated file counts the
            // lines it adds and removes.
            if let Some((added, removed)) =
                node.lines.filter(|_| node.change == Some(Change::Modified))
            {
                item = item.changes(added, removed);
            }
        }
        match node.change {
            Some(Change::Modified) => item.label_color(accent),
            Some(Change::Added) => item.label_color(success),
            Some(Change::Deleted) => item.label_color(danger).strike(),
            None => item,
        }
    })
    .on_select(|s, _, tx| {
        s.workspace.set_terminal_focus(false);
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
        .role("tablist")
        .label("Editor tabs")
        .h(38.0_f32.max(ui_label().size * 1.5 + 16.0))
        .px(6.0)
        .gap(3.0)
        .center_y()
        .w_full()
        .scroll_x()
        .fill(a.panel());
    if width < 880.0 {
        tabs = tabs.child(
            icon_button(
                a,
                "return-terminal",
                &rune_kit::glyphs::ARROW_LEFT,
                "Return to terminal",
                |s, tx| s.dispatch(AppAction::ShowTerminal, tx),
            )
            .fill(a.hover_bg()),
        );
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
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if i == workspace.active && !s.settings && !s.search_tab {
                    a.hover_bg()
                } else {
                    a.panel()
                })
                .hover(a.hover_bg())
                .children(
                    (i == workspace.active && !s.settings && !s.search_tab)
                        .then(|| super::tab_indicator(a, "indicator-editor-tabs")),
                )
                .child(super::tabs::interactive(
                    el().key({
                        let key = format!("editor-tab-{id}");
                        super::pressable(&key);
                        key
                    })
                    .role("tab")
                    .focus_selected(i == workspace.active && !s.settings && !s.search_tab)
                    .label(name.clone())
                    .help(tab.document.path.to_string_lossy())
                    .description(if tab.preview {
                        format!("{PREVIEW_TAB_DESCRIPTION}. {}", tab.document.path.display())
                    } else {
                        tab.document.path.to_string_lossy().into_owned()
                    })
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .max_w(190.0)
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .gap(6.0)
                    .on_press_tx(move |s: &mut State, tx| {
                        tx.blur();
                        s.dispatch(AppAction::ActivateEditor(i), tx);
                    })
                    .on_double_press(move |s: &mut State| {
                        if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                            tab.preview = false;
                        }
                    })
                    .child(
                        rune::icon(
                            file_icons::resolve(&tab.document.path, false, false, a.dark),
                            16.0,
                        )
                        .color(a.muted())
                        .key(format!("editor-file-icon-{id}")),
                    )
                    .child(
                        text(name.clone())
                            .key(format!("editor-title-{id}"))
                            .font(tab_title_font(tab.preview))
                            .color(match (tab.preview, i == workspace.active) {
                                (true, selected) => a.temporary_tab_ink(selected),
                                (false, true) => a.ink(),
                                (false, false) => a.muted(),
                            })
                            .max_w(120.0)
                            .truncate(),
                    )
                    .children(tab.dirty().then(|| {
                        text("•")
                            .key(format!("editor-dirty-{id}"))
                            .font(ui_label())
                            .color(a.accent())
                    })),
                    super::tabs::Target::Editor(id),
                ))
                .child(
                    icon_button(
                        a,
                        format!("editor-close-{id}"),
                        &rune_kit::glyphs::CLOSE,
                        "Close editor tab",
                        move |s, tx| s.dispatch(AppAction::CloseEditor(Some(id)), tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
                ),
        );
    }
    if s.settings_open {
        tab_items.insert(
            s.tab_strips.settings_position.min(tab_items.len()),
            row()
                .key("settings-group")
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if s.settings { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .children(
                    s.settings
                        .then(|| super::tab_indicator(a, "indicator-editor-tabs")),
                )
                .child(super::tabs::interactive(
                    el().key({
                        super::pressable("settings-tab");
                        "settings-tab"
                    })
                    .role("tab")
                    .focus_selected(s.settings)
                    .label("Settings")
                    .help("Settings")
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .on_press_tx(|s: &mut State, tx| {
                        tx.blur();
                        s.dispatch(AppAction::Settings, tx);
                    })
                    .child(
                        text("Settings")
                            .font(ui_label())
                            .color(if s.settings { a.ink() } else { a.muted() })
                            .truncate(),
                    ),
                    super::tabs::Target::Settings,
                ))
                .child(
                    icon_button(
                        a,
                        "settings-close",
                        &rune_kit::glyphs::CLOSE,
                        "Close Settings tab",
                        |s, tx| s.dispatch(AppAction::CloseSettings, tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
                ),
        );
    }
    if s.search_tab_open {
        let shown = s.search_tab;
        tab_items.insert(
            s.tab_strips.search_position.min(tab_items.len()),
            row()
                .key("search-group")
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if shown { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .children(shown.then(|| super::tab_indicator(a, "indicator-editor-tabs")))
                .child(super::tabs::interactive(
                    el().key({
                        super::pressable("search-tab");
                        "search-tab"
                    })
                    .role("tab")
                    .focus_selected(shown)
                    .label("Search")
                    .help(format!("Search: {}", s.explorer_search.query.text))
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .gap(6.0)
                    .on_press_tx(|s: &mut State, tx| s.dispatch(AppAction::ShowSearch, tx))
                    .child(rune::icon(&rune_kit::glyphs::SEARCH, 16.0).color(a.muted()))
                    .child(
                        text("Search")
                            .font(ui_label())
                            .color(if shown { a.ink() } else { a.muted() })
                            .truncate(),
                    ),
                    super::tabs::Target::Search,
                ))
                .child(
                    icon_button(
                        a,
                        "search-close",
                        &rune_kit::glyphs::CLOSE,
                        "Close Search tab",
                        |s, tx| s.dispatch(AppAction::CloseSearch, tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
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
    if s.search_tab {
        return pane.child(
            col()
                .key("search-content")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .fill(a.bg())
                .child(
                    col()
                        .w_full()
                        .grow(1.0)
                        .min_h(0.0)
                        .px(10.0)
                        .pt(8.0)
                        .child(explorer_search::results(s, a)),
                )
                .child(explorer_search::footer(s, a)),
        );
    }
    let Some(tab) = workspace.tabs.get(workspace.active) else {
        // Settings closed its tab but the pane is still drawn while it exits;
        // an editor pane with nothing open renders nothing rather than a placeholder.
        return pane.child(el().grow(1.0).fill(a.bg()));
    };
    let id = tab.id;
    let markdown = is_markdown(&tab.document.path);
    let mut modes = row().min_w(0.0).gap(2.0).scroll_x().child(
        icon_button(
            a,
            "file-edit",
            &rune_kit::glyphs::PENCIL,
            "Edit",
            move |s: &mut State, tx| s.dispatch(AppAction::EditFile, tx),
        )
        .toggle_state(a, tab.mode == View::Edit)
        .hover(a.hover_bg()),
    );
    if markdown {
        modes = modes.child(
            icon_button(
                a,
                "file-preview",
                &rune_kit::glyphs::PREVIEW_TEXT,
                "Preview",
                move |s: &mut State, tx| {
                    if let Some(tab) = s.workspace.tabs.get_mut(s.workspace.active) {
                        tab.mode = if tab.mode == View::Preview {
                            View::Edit
                        } else {
                            View::Preview
                        };
                    }
                    s.workspace.set_terminal_focus(false);
                    tx.blur();
                },
            )
            .toggle_state(a, tab.mode == View::Preview)
            .hover(a.hover_bg()),
        );
    }
    for (key, label, mode) in [
        (
            "diff-current",
            "Compare with saved file",
            Comparison::Current,
        ),
        (
            "diff-unstaged",
            "Compare unstaged changes",
            Comparison::Unstaged,
        ),
        ("diff-staged", "Compare staged changes", Comparison::Staged),
    ] {
        // A comparison shows only when it has changes to show. The explorer's
        // Git status decides; without it (another repository, or the explorer
        // closed) every comparison stays available. The open one stays too.
        let path = &tab.document.path;
        let available = tab.mode == View::Diff(mode)
            || workspace
                .repo
                .as_ref()
                .filter(|repo| path.starts_with(&repo.root))
                .is_none_or(|repo| {
                    repo.compares(path, mode) || (mode == Comparison::Current && tab.dirty())
                });
        if !available {
            continue;
        }
        modes = modes.child(
            icon_button(a, key, &rune_kit::glyphs::COMPARE, label, move |s, tx| {
                s.dispatch(AppAction::Compare(mode), tx)
            })
            .toggle_state(a, tab.mode == View::Diff(mode))
            .hover(a.hover_bg()),
        );
    }
    // A thin vertical rule between groups of status line controls.
    let separator = || {
        el().w(1.0)
            .h(18.0)
            .mx(4.0)
            .fill(a.kit().c.border_base)
            .a11y_hidden()
    };
    if matches!(tab.mode, View::Diff(_)) {
        // Git diff controls sit in the status line, before the save button,
        // instead of over the diff.
        modes = modes
            .child(separator())
            .child(
                row()
                    .key("diff-controls")
                    .role("radiogroup")
                    .label("Diff layout")
                    .gap(2.0)
                    .center_y()
                    .children(
                        [("auto", "Auto"), ("inline", "Inline"), ("split", "Split")]
                            .into_iter()
                            .zip(0u8..)
                            .map(|((key, title), layout)| {
                                // One of three layouts: a radio whose active
                                // choice is outlined, not only tinted.
                                let name = format!("{title} diff layout");
                                button(a, format!("diff-{key}"), title, move |s, _| {
                                    s.workspace.diff_layout = layout
                                })
                                .role("radio")
                                .label(name.clone())
                                .help(name)
                                .toggle_state(a, s.workspace.diff_layout == layout)
                                .hover(a.hover_bg())
                            }),
                    ),
            )
            .child(icon_button(
                a,
                "diff-prev",
                &rune_kit::glyphs::ARROW_UP,
                "Previous change",
                |s: &mut State, tx| {
                    s.workspace.diff.next_hunk(false, tx);
                },
            ))
            .child(icon_button(
                a,
                "diff-next",
                &rune_kit::glyphs::ARROW_DOWN,
                "Next change",
                |s: &mut State, tx| {
                    s.workspace.diff.next_hunk(true, tx);
                },
            ));
    }
    let save_help = s.shortcut_help("save-file", "Save file");
    modes = modes.child(separator()).child(
        icon_button(
            a,
            "file-save",
            &rune_kit::glyphs::SAVE,
            if tab.saving { "Saving…" } else { &save_help },
            move |s: &mut State, tx| s.dispatch(AppAction::SaveEditor(Some(id)), tx),
        )
        .disabled(tab.saving),
    );
    // The editor's status line: mode icons on the left, position and encoding on
    // the right, with breathing room on all sides.
    let status_text = if tab.mode == View::Edit {
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
        Some(format!(
            "UTF-8{} · {:?} · Ln {line}, Col {column}",
            if tab.document.saved.bom { " BOM" } else { "" },
            tab.document.saved.preferred
        ))
    } else if tab.mode == View::Preview {
        Some("Markdown preview".to_owned())
    } else {
        None
    };
    let footer = row()
        .key("editor-status")
        .w_full()
        .fill(a.panel())
        .border_sides([1.0, 0.0, 0.0, 0.0], a.hover_bg())
        .px(10.0)
        .py(4.0)
        .gap(6.0)
        .center_y()
        .child(modes)
        .child(el().grow(1.0));
    let footer = if let Some(status) = status_text {
        footer.child(text(status).font(ui_label()).color(a.muted()).shrink())
    } else if matches!(tab.mode, View::Diff(_)) {
        // What the diff compares, in place of the editor's position.
        footer.child(
            text(&workspace.diff_label)
                .key("diff-label")
                .font(ui_label())
                .color(a.muted())
                .shrink()
                .truncate(),
        )
    } else {
        footer
    };
    // Tabs and compact navigation stay above the content; the file's status,
    // Git diff controls and file actions sit below it, the actions last.
    let mut below = Vec::new();
    if let Some(error) = &tab.disk_error {
        below.push(
            text(format!("Disk: {error}"))
                .font(ui_label())
                .color(a.danger())
                .wrap_anywhere(),
        );
    }
    if tab.external.is_some() && !tab.dismissed {
        below.push(
            col()
                .key("external-change")
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
                                .px(8.0)
                                .py(4.0)
                                .color(a.ink())
                                .placeholder("Find text", a.muted())
                                .border(1.0, a.kit().c.border_base)
                                .radius(6.0)
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
                                .px(8.0)
                                .py(4.0)
                                .color(a.ink())
                                .placeholder("Replace with", a.muted())
                                .border(1.0, a.kit().c.border_base)
                                .radius(6.0)
                                .on_edit(|s: &mut State| &mut s.workspace.replacement),
                        )
                        .child(button(a, "replace-next", "Replace", |s, tx| {
                            s.find_editor(true, tx)
                        }))
                        .child(button(a, "replace-all", "All", |s, _| s.replace_all())),
                ),
        );
    }
    if tab.mode == View::Preview {
        // Rendered markdown: the same text the editor shows, laid out and styled
        // by Rune Kit, in the editor's scroll area.
        let rendered = rune_kit::Markdown::new(tab.edit.text.clone())
            .autolink(true)
            .build(a.kit());
        let viewport = col()
            .key(format!("editor-scroll-{id}"))
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .px(16.0)
            .py(12.0)
            .child(rendered);
        pane = pane.child(a.kit().scroll_bar(viewport));
    } else if tab.mode == View::Edit {
        let language = syntax::lang_for_path(&tab.document.path.to_string_lossy());
        let highlights = syntax::highlight(language, &tab.edit.text);
        let spans = SyntaxTheme::from_kit(a.kit()).spans(&highlights, 0);
        let mut field = text_input(&tab.edit, fonts::editor())
            .caret(a.accent(), a.selection())
            .key(format!("file-editor-{id}"))
            .label("File editor")
            .disabled(workspace.removal.is_some())
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
                if s.workspace.removal.is_some() {
                    return true;
                }
                if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                    tab.edit
                        .insert(&clipboard.text().replace("\r\n", "\n").replace('\r', "\n"));
                    tab.revision += 1;
                    tab.preview = false;
                }
                true
            });
        // A Search Files match keeps a keyed mark while it is still selected,
        // for scrolling to and so it reads while the editor is unfocused.
        if let Some((_, range)) = workspace
            .search_reveal
            .as_ref()
            .filter(|(tab_id, range)| *tab_id == id && tab.edit.range() == (range.start, range.end))
        {
            field = field.text_highlight(
                rune::TextHighlight::new(range.clone(), a.accent().alpha(0.22))
                    .key(explorer_search::search_match_key(id)),
            );
        }
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
    } else {
        let mut diff_below = Vec::new();
        if let Some(error) = &workspace.diff_error {
            diff_below.push(
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
        below.splice(0..0, diff_below);
    }
    pane.children(below).child(footer)
}

/// The accessible description that marks a preview editor tab, whose title
/// shows the temporary state only as italic, dimmed text.
pub(super) const PREVIEW_TAB_DESCRIPTION: &str =
    "Preview tab, replaced when another file is previewed";

/// An editor tab title's type: a preview editor tab's is italic.
fn tab_title_font(temporary: bool) -> rune::TextStyle {
    let font = ui_label();
    if temporary { font.italic() } else { font }
}

/// Whether the path names a markdown file Rodtty can render as a preview.
fn is_markdown(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

fn editor_available(s: &State) -> bool {
    // No open editor tab and no Settings tab: the pane closes rather than
    // showing an empty placeholder, so the terminal keeps the workspace.
    s.workspace.layout.editor
        && (!s.workspace.tabs.is_empty() || s.settings_open || s.search_tab_open)
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
    let editor = editor_available(s) && (width >= 880.0 || !s.workspace.terminal_focus());
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
        body = body
            .child(explorer(s, a, workspace.layout.explorer_width))
            .child(
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
        let mut pane = col().key("terminal-pane").h_full().min_h(0.0).min_w(0.0);
        if s.terminal_search.open {
            pane = pane.child(terminal_search::bar(s, a));
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
        // Compact reveal: Files floats over the right of the workspace, above the
        // terminal and editor, so their layout (and the PTY's rows and columns)
        // stays exactly as it was while the overlay comes and goes.
        // The overlay slides in and out on its own transition, while what it
        // shows lands at once as the inline explorer's contents do.
        let overlay_width = workspace.layout.explorer_width.min(width - 48.0).max(160.0);
        body = body.child(
            el().key("explorer-overlay")
                .pin(None, Some(0.0), Some(0.0), Some(0.0))
                .enters_animated(a, Entrance::Drawer(16.0, 0.0))
                .child(
                    explorer(s, a, overlay_width)
                        .key("explorer")
                        .instant_changes()
                        .border(1.0, a.hover_bg())
                        .shadow(6.0, 20.0, a.scrim()),
                ),
        );
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
    let message = if matches!(prompt, FilePrompt::Conflict { .. }) {
        "Saving would replace changes made outside this editor. Reload discards your editor changes."
    } else {
        "Choose Save, Discard, or Cancel to continue."
    };
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
    Some(motion::modal(
        a,
        el().center().child(
            col()
                .key("file-dialog")
                .role("alertdialog")
                .label(title)
                .description(format!("{} {message}", tab.document.path.display()))
                .modal(false, |s: &mut State| s.cancel_file_action())
                .w(470.0)
                .px(24.0)
                .py(24.0)
                .gap(16.0)
                .radius(10.0)
                .fill(a.panel())
                .child(text(title).font(ui_style(17.0, 600.0)).color(a.ink()))
                .child(
                    text(tab.document.path.to_string_lossy())
                        .font(ui_label())
                        .color(a.muted())
                        .wrap_anywhere(),
                )
                .child(text(message).font(ui_label()).color(a.muted()).wrap())
                .child(buttons),
        ),
        Entrance::Dialog,
    ))
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
        save_token: 0,
        revision: 0,
        baseline: 0,
        after_save: None,
        autosave: Default::default(),
    });
    s.workspace.next_id = 2;
    s.workspace.set_terminal_focus(false);
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
    s.workspace.directory = Some(PathBuf::from("/workspace/rodtty"));
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
    s.workspace.tree_listed = s.workspace.layout.expanded_folders.clone();
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
        save_token: 0,
        revision: 0,
        baseline: 0,
        after_save: None,
        autosave: Default::default(),
    }];
    s.workspace.next_id = 2;
    s.workspace.set_terminal_focus(false);
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
    s.workspace.directory = Some(directory.clone());
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
    s.workspace.tree_listed = s.workspace.layout.expanded_folders.clone();
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
            save_token: 0,
            revision: u64::from(index == 0),
            baseline: 0,
            after_save: None,
            autosave: Default::default(),
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
    s.workspace.layout.hidden = s.config.explorer.show_hidden;
    s.workspace.layout.follow = s.config.explorer.follow_terminal;
    let listing = (|| -> anyhow::Result<_> {
        let repo = crate::git::inspect(&root)?;
        let ignored = if s.config.explorer.respect_ignore {
            repo.as_ref()
                .map(crate::git::ignored)
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let listing = crate::files::list_tree(
            &root,
            &s.workspace.layout.expanded_folders,
            s.config.explorer.show_hidden,
            &ignored,
            None,
        )?;
        Ok((listing, repo))
    })();
    match listing {
        Ok((listing, repo)) => {
            s.workspace.directory = Some(listing.root.clone());
            s.workspace.entries = listing.entries;
            s.workspace.tree_errors = listing.errors;
            s.workspace.tree_listed = listing.listed;
            s.workspace.repo = repo;
            s.rebuild_tree();
            s.workspace
                .tree
                .select(&root.join("src/desktop/workspace.rs").to_string_lossy());
        }
        Err(error) => s.workspace.explorer_error = Some(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests;
