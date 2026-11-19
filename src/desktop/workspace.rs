use super::*;
use crate::{
    files::{Document, Entry, SaveFailure, TextFile},
    git::{Change, Comparison, Repository},
    work::{Event, Job, Layout, WindowGeometry, Worker},
};
use rune_kit::{
    TreeModel,
    diff::{DiffMode, DiffState},
};
use std::time::{Duration, Instant};

mod layout;
mod persistence;
mod poll;
pub(crate) use layout::*;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Action {
    Editor(u64),
    Window,
    KeepWindow,
    Quit,
}
pub(crate) enum FilePrompt {
    Unsaved { id: u64, action: Action },
    Conflict { id: u64, after: Option<Action> },
}
pub(crate) struct DeferredSaveConflict {
    pub(crate) id: u64,
    pub(crate) token: u64,
    pub(crate) revision: u64,
    pub(crate) baseline: u64,
    pub(crate) after: Option<Action>,
}
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum View {
    Edit,
    Diff(Comparison),
    /// Rendered markdown, offered for `.md` files.
    Preview,
}
pub(crate) struct Tab {
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

pub(crate) struct TreeEntry {
    pub(crate) entry: Option<Entry>,
    pub(crate) label: String,
    pub(crate) badge: String,
    /// How the file differs from HEAD, and its changed line counts.
    pub(crate) change: Option<Change>,
    pub(crate) lines: Option<(u32, u32)>,
    /// The synthetic workspace-root row: a folder that exists in the tree but
    /// not in `entries`, labelled with the current folder's own name.
    pub(crate) root: bool,
}

/// Where keyboard focus goes back to when the compact Files overlay closes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FocusReturn {
    Terminal,
    /// The editor pane: the text editor of this tab, or the pane itself
    /// (a diff, preview or Settings) when `None`.
    Editor(Option<u64>),
}

/// Where Search Files opens a content match: in the file's source text, never
/// a rendered view, with the match selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SourceLocation {
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
    pub(crate) fn range(self, text: &str) -> std::ops::Range<usize> {
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

pub(crate) struct RestoredPanes {
    pub(crate) editor_visible: bool,
    pub(crate) terminal_focus: bool,
    pub(crate) explorer_revealed: bool,
}

pub(crate) struct Workspace {
    pub autosave_preferences: config::AutosavePreferences,
    pub autosave_context: Option<u64>,
    pub(crate) deferred_save_conflicts: Vec<DeferredSaveConflict>,
    /// Private so every change goes through `set_terminal_focus`, which counts it.
    pub(crate) terminal_focus: bool,
    /// Counts terminal focus changes and active-session switches, so an
    /// asynchronous terminal paste can tell that its target moved meanwhile.
    pub(crate) focus_generation: u64,
    pub explorer_revealed: bool,
    /// The root row's overflow menu is open.
    pub(crate) explorer_menu: bool,
    /// Captured as the compact Files overlay opens.
    pub(crate) overlay_return: Option<FocusReturn>,
    pub show_empty_editor: bool,
    pub(crate) orphan_edit: TextEdit,
    pub layout: Layout,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub worker: Option<Worker>,
    pub entries: Vec<Entry>,
    pub(crate) tree: TreeModel<TreeEntry>,
    pub(crate) operation_busy: bool,
    pub(crate) removal: Option<super::removal::RemovalState>,
    pub(crate) operation_moving: bool,
    pub(crate) operation_result: Option<PathBuf>,
    pub(crate) operation_id: u64,
    pub(crate) operation_name: TextEdit,
    pub(crate) operation_destination: TextEdit,
    pub(crate) operation_error: Option<String>,
    pub(crate) operation_context: Option<(f32, f32)>,
    pub(crate) operation_viewport: (f32, f32),
    pub(crate) tree_errors: Vec<(PathBuf, String)>,
    /// Expanded folders the current `entries` came from reading successfully;
    /// only these can show **Empty folder**.
    pub(crate) tree_listed: Vec<PathBuf>,
    pub(crate) tree_dirty: bool,
    pub(crate) explorer_error: Option<String>,
    pub(crate) outside_root: Option<PathBuf>,
    pub(crate) pending_reveal: Option<PathBuf>,
    pub(crate) revealed_directory: Option<PathBuf>,
    pub(crate) explorer_preferences: crate::config::ExplorerPreferences,
    pub directory: Option<PathBuf>,
    pub repo: Option<Repository>,
    pub git_error: Option<String>,
    pub(crate) scan_token: u64,
    pub(crate) scan_busy: bool,
    pub(crate) last_scan: Instant,
    pub(crate) last_follow: Option<PathBuf>,
    pub(crate) next_id: u64,
    pub(crate) opening: Vec<(PathBuf, bool)>,
    /// A Search Files match to select once its file's tab shows.
    pub(crate) pending_location: Option<(PathBuf, SourceLocation)>,
    /// The match last revealed this way, while its selection stands.
    pub(crate) search_reveal: Option<(u64, std::ops::Range<usize>)>,
    pub(crate) latest_activation: Option<PathBuf>,
    pub(crate) restore_active: Option<PathBuf>,
    pub(crate) restore_panes: Option<RestoredPanes>,
    pub(crate) restoration_warnings: Vec<String>,
    pub(crate) last_persist: Instant,
    pub(crate) last_saved: Option<Vec<u8>>,
    pub diff: DiffState,
    pub diff_label: String,
    pub diff_error: Option<String>,
    pub diff_patch: String,
    pub(crate) diff_token: u64,
    pub(crate) diff_pending: Option<(u64, u64, u64, u64, Comparison)>,
    pub(crate) last_diff: Instant,
    pub diff_layout: u8,
    pub editor_width: f32,
    pub(crate) diff_viewport: (f32, f32),
    pub body_width: f32,
    pub find: bool,
    pub find_query: TextEdit,
    pub replacement: TextEdit,
    pub(crate) drag_width: f32,
    pub(crate) drag_share: f32,
}
impl Workspace {
    pub(crate) fn has_pending_opens(&self) -> bool {
        !self.opening.is_empty()
    }
    /// Whether keyboard input belongs to the terminal rather than the editor.
    pub(crate) fn terminal_focus(&self) -> bool {
        self.terminal_focus
    }
    pub(crate) fn set_terminal_focus(&mut self, focused: bool) {
        if self.terminal_focus != focused {
            self.terminal_focus = focused;
            self.focus_generation += 1;
        }
    }
    /// Record that the active terminal session changed, even if focus did not.
    pub(crate) fn note_session_switch(&mut self) {
        self.focus_generation += 1;
    }
    pub(crate) fn focus_generation(&self) -> u64 {
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
    pub(crate) const WORKSPACE_ROOT_EXPANDED: bool = true;
}

#[cfg(test)]
mod tests;
