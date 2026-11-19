//! Search: the header's one query field searches file names, file contents
//! and every terminal's retained text at once. Results show in a Search tab in
//! the editor pane, grouped by file and by terminal; a file row opens through
//! the editor's preview flow and a terminal row selects the match in its
//! terminal. The file search itself is [`crate::files::search`]; the terminal
//! search is [`terminal::search::find_lines`].
//!
//! Each search runs on a fresh thread with its own cancel flag. Editing the
//! query sets the in-flight search's flag at once, and the next search starts
//! [`DEBOUNCE`] after the last edit. Results carry the search's generation so
//! a superseded search's late rows are dropped. The thread shares nothing with
//! the file worker, so saves and file operations never wait for it.
use super::*;
use crate::files::search::{self, FileMatch, Summary};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod view;
pub(crate) use view::*;

/// The editor highlight a Search Files match scrolls to.
pub(super) fn search_match_key(id: u64) -> String {
    format!("search-match-{id}")
}

/// How long typing must pause before a search starts.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(200);
pub(super) const QUERY_KEY: &str = "explorer-search-query";
/// The most terminal matches listed, over all terminals.
const TERMINAL_LIMIT: usize = 200;
const RESULTS_KEY: &str = "explorer-search-results";

/// What a search ran over; a change to any part searches again.
#[derive(Clone, Debug, PartialEq)]
struct Request {
    /// The explorer's folder, or the terminal's while the explorer has none.
    root: PathBuf,
    text: String,
    hidden: bool,
    respect_ignore: bool,
}

enum Message {
    File(u64, FileMatch),
    Done(u64, Summary),
}

/// A selectable result row: a file-name match, a content match, or a match
/// in a terminal's text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    Name(usize),
    Line(usize, usize),
    Terminal(usize, usize),
}

/// A collapsible result group: a file by its index, or a terminal by its
/// session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Group {
    File(usize),
    Terminal(usize),
}

/// One terminal's matches, found when the search started.
#[derive(Clone, Debug)]
pub(super) struct TerminalMatches {
    pub session: usize,
    pub title: String,
    pub found: Vec<terminal::search::LineMatch>,
}

pub(super) struct ExplorerSearch {
    pub open: bool,
    pub query: TextEdit,
    edited_at: Option<Instant>,
    issued: Option<Request>,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    sender: mpsc::Sender<Message>,
    receiver: mpsc::Receiver<Message>,
    pub results: Vec<FileMatch>,
    pub terminal: Vec<TerminalMatches>,
    /// Groups whose rows are hidden; a new search shows every group again.
    pub collapsed: std::collections::HashSet<Group>,
    /// The finished search's summary; `None` while one is pending or running.
    pub summary: Option<Summary>,
    /// Index into [`ExplorerSearch::hits`].
    pub selected: usize,
    /// Holds the next searches before their first file while set, so a test
    /// can observe other work completing during a search.
    #[cfg(test)]
    pub hold: Option<Arc<AtomicBool>>,
}

impl Default for ExplorerSearch {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            open: false,
            query: TextEdit::new(""),
            edited_at: None,
            issued: None,
            generation: 0,
            cancel: None,
            sender,
            receiver,
            results: Vec::new(),
            terminal: Vec::new(),
            collapsed: Default::default(),
            summary: None,
            selected: 0,
            #[cfg(test)]
            hold: None,
        }
    }
}

impl ExplorerSearch {
    /// Whether results, or a search for them, take the tree's place.
    pub fn active(&self) -> bool {
        self.open && !self.query.text.is_empty()
    }

    /// Every result row, collapsed or not: file names, file lines, terminal
    /// lines.
    pub fn counts(&self) -> (usize, usize, usize) {
        let files: usize = self
            .results
            .iter()
            .map(|f| usize::from(f.name.is_some()) + f.lines.len())
            .sum();
        let terminal = self.terminal.iter().map(|t| t.found.len()).sum();
        (files, self.results.len(), terminal)
    }

    /// Show or hide a group's rows, keeping the selection on a shown row.
    pub fn toggle(&mut self, group: Group) {
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
        self.selected = self.selected.min(self.hits().len().saturating_sub(1));
    }

    #[cfg(test)]
    pub fn searching(&self) -> bool {
        self.active() && self.summary.is_none()
    }

    /// The selectable rows in display order; a collapsed group's are left out.
    pub fn hits(&self) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (f, file) in self.results.iter().enumerate() {
            if self.collapsed.contains(&Group::File(f)) {
                continue;
            }
            if file.name.is_some() {
                hits.push(Hit::Name(f));
            }
            hits.extend((0..file.lines.len()).map(|l| Hit::Line(f, l)));
        }
        for (t, terminal) in self.terminal.iter().enumerate() {
            if self.collapsed.contains(&Group::Terminal(terminal.session)) {
                continue;
            }
            hits.extend((0..terminal.found.len()).map(|l| Hit::Terminal(t, l)));
        }
        hits
    }

    /// The in-flight search's cancel flag, for observing that it was set.
    #[cfg(test)]
    pub fn cancel_flag(&self) -> Option<Arc<AtomicBool>> {
        self.cancel.clone()
    }

    fn cancel(&mut self) {
        if let Some(flag) = self.cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Forget the query's searches and results.
    fn reset(&mut self) {
        self.cancel();
        self.generation += 1;
        self.issued = None;
        self.edited_at = None;
        self.results.clear();
        self.terminal.clear();
        self.collapsed.clear();
        self.summary = None;
        self.selected = 0;
    }
}

fn hit_key(index: usize) -> String {
    format!("explorer-search-hit-{index}")
}

impl State {
    /// Open `path` as [`Self::open_file_with_preview`] does, then select
    /// `location` in its source. The tab is switched to source text even when
    /// the file would otherwise show a rendered view (Markdown preview).
    pub(super) fn open_file_at(
        &mut self,
        path: PathBuf,
        preview: bool,
        location: workspace::SourceLocation,
    ) {
        self.open_file_with_preview(path.clone(), preview);
        if self.workspace.latest_activation.as_ref() == Some(&path) {
            self.workspace.pending_location = Some((path, location));
        }
    }

    /// Select a pending Search Files location once its tab exists, and scroll
    /// it into view.
    pub(super) fn apply_pending_location(&mut self, tx: &mut Tx) -> bool {
        let Some((path, location)) = self.workspace.pending_location.clone() else {
            return false;
        };
        let Some(tab) = self
            .workspace
            .tabs
            .iter_mut()
            .find(|tab| tab.document.path == path)
        else {
            return false;
        };
        self.workspace.pending_location = None;
        tab.mode = workspace::View::Edit;
        let range = location.range(&tab.edit.text);
        tab.edit.set_selection(range.start, range.end);
        tab.edit.break_history_group();
        let id = tab.id;
        self.workspace.search_reveal = Some((id, range));
        tx.scroll_center(search_match_key(id));
        true
    }

    /// Focus the header's search field, showing the Search tab when it
    /// already has a query.
    pub(super) fn open_explorer_search(&mut self, tx: &mut Tx) {
        self.workspace.explorer_menu = false;
        self.explorer_search.open = true;
        if !self.explorer_search.query.text.is_empty() {
            self.show_search_tab();
        }
        tx.focus_when_shown(QUERY_KEY, true);
    }

    /// Show the editor pane's Search tab, opening it if needed.
    pub(super) fn show_search_tab(&mut self) {
        // Closing the compact explorer overlay first, as opening a file does.
        if self.explorer_overlay_open() {
            self.workspace.overlay_return = None;
            self.workspace.explorer_menu = false;
        }
        self.supersede_pending_preview();
        self.workspace.explorer_revealed = false;
        self.search_tab_open = true;
        self.search_tab = true;
        self.settings = false;
        self.workspace.layout.editor = true;
        self.workspace.set_terminal_focus(false);
    }

    /// Escape in the field, or closing the Search tab: clear the query and
    /// results and close the tab.
    pub(super) fn close_explorer_search(&mut self, tx: &mut Tx) {
        let search = &mut self.explorer_search;
        search.reset();
        search.query = TextEdit::new("");
        search.open = false;
        let shown = self.search_tab;
        self.search_tab_open = false;
        self.search_tab = false;
        if !shown {
            return;
        }
        // The pane shows what it showed before: a file, Settings, or nothing.
        if !self.workspace.tabs.is_empty() {
            self.workspace.set_terminal_focus(false);
            tx.blur();
        } else if self.settings_open {
            self.settings = true;
        } else {
            self.workspace.set_terminal_focus(true);
            tx.blur();
        }
    }

    fn explorer_search_edited(&mut self) {
        self.explorer_search.open = true;
        if !self.explorer_search.query.text.is_empty() {
            self.show_search_tab();
        }
        let search = &mut self.explorer_search;
        // The running search no longer answers this query: stop it now, not
        // when the debounce ends.
        search.cancel();
        search.summary = None;
        search.edited_at = Some(Instant::now());
        if search.query.text.is_empty() {
            search.reset();
        }
    }

    fn explorer_search_request(&self) -> Option<Request> {
        if !self.explorer_search.active() {
            return None;
        }
        Some(Request {
            // Files are searched whether or not the explorer has opened: its
            // folder when it has one, else where the terminal is.
            root: self.workspace.directory.clone().unwrap_or_else(|| {
                self.active()
                    .map(Session::cwd)
                    .unwrap_or_else(|| self.directory.clone())
            }),
            text: self.explorer_search.query.text.clone(),
            hidden: self.config.explorer.show_hidden,
            respect_ignore: self.config.explorer.respect_ignore,
        })
    }

    /// Drain results and start a due search. Returns whether the view changed.
    pub(super) fn poll_explorer_search(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        let search = &mut self.explorer_search;
        for message in search.receiver.try_iter() {
            match message {
                Message::File(generation, file) if generation == search.generation => {
                    search.results.push(file);
                    changed = true;
                }
                Message::Done(generation, summary) if generation == search.generation => {
                    search.summary = Some(summary);
                    search.cancel = None;
                    changed = true;
                }
                _ => {}
            }
        }
        let request = self.explorer_search_request();
        let search = &mut self.explorer_search;
        if request == search.issued {
            return changed;
        }
        let Some(request) = request else {
            search.reset();
            return true;
        };
        let waited = search.edited_at.map_or(DEBOUNCE, |at| at.elapsed());
        if waited < DEBOUNCE {
            tx.wake_after(DEBOUNCE - waited);
            return changed;
        }
        search.reset();
        search.issued = Some(request.clone());
        // The terminals' text is on this thread already: search it now.
        let mut remaining = TERMINAL_LIMIT;
        let mut terminal = Vec::new();
        for session in &self.sessions {
            let found = session.with_screen_mut(|screen| {
                terminal::search::find_lines(screen, &request.text, remaining)
            });
            remaining -= found.len();
            if !found.is_empty() {
                terminal.push(TerminalMatches {
                    session: session.id(),
                    title: session.tab_title(),
                    found,
                });
            }
        }
        let search = &mut self.explorer_search;
        search.terminal = terminal;
        let root = request.root.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        search.cancel = Some(cancel.clone());
        let generation = search.generation;
        let sender = search.sender.clone();
        let wake = self.wake.clone();
        #[cfg(test)]
        let hold = search.hold.clone();
        std::thread::spawn(move || {
            let send = |message| {
                if sender.send(message).is_ok() {
                    wake();
                }
            };
            #[cfg(test)]
            while hold.as_ref().is_some_and(|h| h.load(Ordering::Relaxed))
                && !cancel.load(Ordering::Relaxed)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let ignored = if request.respect_ignore {
                // Without Git (or outside a repository) nothing is ignored,
                // as in the tree.
                search::ignored_paths(&root).unwrap_or_default()
            } else {
                Vec::new()
            };
            let query = search::Query {
                root,
                text: request.text,
                hidden: request.hidden,
                ignored,
                limit: search::RESULT_LIMIT,
            };
            let summary = search::search(
                &query,
                || cancel.load(Ordering::Relaxed),
                |file| send(Message::File(generation, file)),
            );
            if !summary.cancelled {
                send(Message::Done(generation, summary));
            }
        });
        true
    }

    fn explorer_search_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        let count = self.explorer_search.hits().len();
        match event.key {
            Key::Down | Key::Up if count > 0 => {
                let selected = self.explorer_search.selected.min(count - 1);
                let next = if event.key == Key::Down {
                    (selected + 1).min(count - 1)
                } else {
                    selected.saturating_sub(1)
                };
                self.explorer_search.selected = next;
                tx.scroll_into_view_in(RESULTS_KEY, hit_key(next));
                true
            }
            // Also inside the compact overlay, which a second Escape closes.
            Key::Escape => {
                self.close_explorer_search(tx);
                true
            }
            Key::Enter => {
                if count > 0 {
                    self.open_search_hit(self.explorer_search.selected.min(count - 1));
                }
                true
            }
            _ => false,
        }
    }

    /// Open a result the way the tree's single click opens a file: a file
    /// name in a preview editor tab, a line at its match in the file's source.
    pub(super) fn open_search_hit(&mut self, index: usize) {
        let Some(hit) = self.explorer_search.hits().get(index).copied() else {
            return;
        };
        self.explorer_search.selected = index;
        let preview = !self.config.single_click_keeps_open;
        match hit {
            Hit::Name(f) => {
                let path = self.explorer_search.results[f].path.clone();
                self.open_file_with_preview(path, preview);
            }
            Hit::Line(f, l) => {
                let file = &self.explorer_search.results[f];
                let line = &file.lines[l];
                let location = workspace::SourceLocation {
                    line: line.line,
                    column: line.column,
                    length: line.length,
                };
                let path = file.path.clone();
                self.open_file_at(path, preview, location);
            }
            Hit::Terminal(t, l) => {
                let terminal = &self.explorer_search.terminal[t];
                let session = terminal.session;
                let at = terminal.found[l].at;
                let Some(index) = self.sessions.iter().position(|s| s.id() == session) else {
                    self.message = "That terminal session has ended".into();
                    return;
                };
                if index != self.active {
                    self.workspace.note_session_switch();
                }
                self.active = index;
                self.workspace.layout.terminal = true;
                self.workspace.explorer_revealed = false;
                self.selection = None;
                let query = self.explorer_search.query.text.clone();
                let mut find = std::mem::take(&mut self.terminal_search);
                find.open = true;
                let found = self.sessions[index]
                    .with_screen_mut(|screen| find.select(screen, session, &query, at));
                self.terminal_search = find;
                if !found {
                    self.message = "The match has scrolled out of the terminal's history".into();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
