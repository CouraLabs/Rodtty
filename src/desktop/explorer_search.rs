//! Search Files in the explorer: one query field, debounced searches on their
//! own thread, and grouped result rows that open through the editor's preview
//! flow. The search itself is [`crate::files::search`].
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

/// The editor highlight a Search Files match scrolls to.
pub(super) fn search_match_key(id: u64) -> String {
    format!("search-match-{id}")
}

/// How long typing must pause before a search starts.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(200);
pub(super) const QUERY_KEY: &str = "explorer-search-query";
const RESULTS_KEY: &str = "explorer-search-results";

/// What a search ran over; a change to any part searches again.
#[derive(Clone, Debug, PartialEq)]
struct Request {
    root: PathBuf,
    text: String,
    hidden: bool,
    respect_ignore: bool,
}

enum Message {
    File(u64, FileMatch),
    Done(u64, Summary),
}

/// A selectable result row: a file-name match or a content match.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    Name(usize),
    Line(usize, usize),
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

    #[cfg(test)]
    pub fn searching(&self) -> bool {
        self.active() && self.summary.is_none()
    }

    /// The selectable rows in display order.
    pub fn hits(&self) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (f, file) in self.results.iter().enumerate() {
            if file.name.is_some() {
                hits.push(Hit::Name(f));
            }
            hits.extend((0..file.lines.len()).map(|l| Hit::Line(f, l)));
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

    /// Show the explorer if needed and focus its Search Files field.
    pub(super) fn open_explorer_search(&mut self, tx: &mut Tx) {
        self.workspace.explorer_menu = false;
        let (shown, _, _) = workspace::pane_visibility(self, self.workspace.body_width);
        if !shown {
            self.toggle_files_with_focus(tx);
        }
        self.explorer_search.open = true;
        self.workspace.set_terminal_focus(false);
        tx.focus_when_shown(QUERY_KEY, true);
    }

    /// Escape in the field: clear the query and results and return to the tree.
    pub(super) fn close_explorer_search(&mut self, tx: &mut Tx) {
        let search = &mut self.explorer_search;
        search.reset();
        search.query = TextEdit::new("");
        search.open = false;
        self.workspace.set_terminal_focus(false);
        tx.focus("file-list", true);
    }

    fn explorer_search_edited(&mut self) {
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
            root: self.workspace.directory.clone()?,
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
                search::ignored_paths(&request.root).unwrap_or_default()
            } else {
                Vec::new()
            };
            let query = search::Query {
                root: request.root,
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
        }
    }
}

/// The Search Files field, shown above the tree or results.
pub(super) fn field(s: &State, a: &Appearance) -> El {
    row()
        .key("explorer-search-bar")
        .w_full()
        .gap(4.0)
        .center_y()
        .child(
            text_input(&s.explorer_search.query, ui_label())
                .key(QUERY_KEY)
                .label("Search Files")
                .placeholder("Search files…", a.muted())
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .px(8.0)
                .py(4.0)
                .color(a.ink())
                .border(1.0, a.kit().c.border_base)
                .radius(6.0)
                .caret(a.accent(), a.selection())
                .on_edit(|s: &mut State| &mut s.explorer_search.query)
                .on_input_change(|s: &mut State, _| s.explorer_search_edited())
                .on_input_key(|s: &mut State, event, tx| s.explorer_search_key(event, tx)),
        )
        .child(
            icon_button(
                a,
                "explorer-search-close",
                &rune_kit::glyphs::CLOSE,
                "Close search",
                |s, tx| s.close_explorer_search(tx),
            )
            .fill(Color::TRANSPARENT),
        )
}

/// The result rows grouped under path headers, with the search's state.
pub(super) fn results(s: &State, a: &Appearance) -> El {
    let search = &s.explorer_search;
    let note = |message: String| {
        text(message)
            .key("explorer-search-status")
            .font(ui_label())
            .color(a.muted())
            .wrap()
    };
    let mut list = col()
        .key(RESULTS_KEY)
        .role("listbox")
        .label("Search results")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(1.0)
        .pb(6.0)
        .preserve_focus_on_press(true)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .scroll_margin(8.0);
    let row_height = 24.0_f32.max(ui_label().size * 1.5 + 4.0);
    let mark = a.accent().alpha(0.32);
    let mut index = 0;
    for (f, file) in search.results.iter().enumerate() {
        let path = file.path.clone();
        let (folder, name) = match file.relative.rsplit_once('/') {
            Some((folder, name)) => (Some(folder), name),
            None => (None, file.relative.as_str()),
        };
        let mut header = row()
            .key(format!("explorer-search-file-{f}"))
            .help(file.relative.clone())
            .w_full()
            .min_h(row_height)
            .px(4.0)
            .gap(6.0)
            .center_y()
            .radius(5.0)
            .hover(a.hover_bg())
            .on_press(move |s: &mut State| {
                let preview = !s.config.single_click_keeps_open;
                s.open_file_with_preview(path.clone(), preview);
            })
            .child(rune::icon(
                file_icons::resolve(&file.path, false, false, a.dark),
                16.0,
            ))
            .child(
                text(name)
                    .font(ui_label())
                    .color(a.ink())
                    .min_w(0.0)
                    .truncate(),
            );
        if let Some(folder) = folder {
            header = header.child(
                text(folder)
                    .font(ui_label())
                    .color(a.muted())
                    .min_w(0.0)
                    .shrink()
                    .truncate(),
            );
        }
        list = list.child(header);
        let mut hit_row = |label: String, lead: El, body: El| {
            let i = index;
            index += 1;
            let selected = i == search.selected;
            row()
                .key(hit_key(i))
                .role("option")
                .label(label)
                .w_full()
                .min_h(row_height)
                .pl(26.0)
                .pr(4.0)
                .gap(8.0)
                .center_y()
                .radius(5.0)
                .fill(if selected {
                    a.hover_bg()
                } else {
                    Color::TRANSPARENT
                })
                .hover(a.hover_bg())
                .on_press(move |s: &mut State| s.open_search_hit(i))
                .child(lead)
                .child(body)
        };
        if let Some(range) = file.name.clone() {
            let file_name = file.path.file_name().unwrap_or_default().to_string_lossy();
            list = list.child(hit_row(
                format!("File name: {file_name}"),
                text("Name").font(ui_label()).color(a.muted()),
                text(file_name.into_owned())
                    .key("name")
                    .font(ui_label())
                    .color(a.ink())
                    .min_w(0.0)
                    .truncate()
                    .text_highlight(rune::TextHighlight::new(range, mark)),
            ));
        }
        for line in &file.lines {
            list = list.child(hit_row(
                format!("Line {}: {}", line.line, line.snippet.text),
                text(line.line.to_string())
                    .key("line")
                    .font(ui_label())
                    .color(a.muted())
                    .min_w(28.0),
                text(line.snippet.text.clone())
                    .key("snippet")
                    .font(fonts::editor())
                    .color(a.ink())
                    .min_w(0.0)
                    .shrink()
                    .truncate()
                    .text_highlight(rune::TextHighlight::new(
                        line.snippet.highlight.clone(),
                        mark,
                    )),
            ));
        }
    }
    match &search.summary {
        None => list = list.child(note("Searching…".into())),
        Some(summary) if summary.rows == 0 => list = list.child(note("No results".into())),
        Some(summary) if summary.truncated => {
            list = list.child(note(format!(
                "Showing first {} results. Refine your search.",
                search::RESULT_LIMIT
            )))
        }
        Some(_) => {}
    }
    a.kit().scroll_bar(list)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, runtime_for};
    use super::*;
    use crate::files::operations::{FileOperation, OperationRequest};

    fn state(root: &std::path::Path) -> State {
        let mut state = State::with_root(root.join(".profile"));
        state.directory = root.to_owned();
        state.workspace_setup();
        state.toggle_files();
        // Wide enough for the root row's controls to show inline.
        state.workspace.layout.explorer_width = 300.0;
        state
    }

    fn settle(runtime: &mut rune::Runtime<State>) {
        runtime.settle_all();
        runtime.frame(1.0);
        runtime.settle_all();
        runtime.frame(1.0);
    }

    /// Poll as the app does until `condition` holds.
    fn wait(runtime: &mut rune::Runtime<State>, condition: impl Fn(&State) -> bool) {
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
        panic!("timed out: {}", runtime.state.message);
    }

    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        runtime.frame(1.0);
        let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0);
    }

    fn focused(runtime: &rune::Runtime<State>, key: &str) -> bool {
        runtime
            .elements()
            .iter()
            .any(|e| e.key.as_deref() == Some(key) && e.focused)
    }

    fn type_text(runtime: &mut rune::Runtime<State>, value: &str) {
        press(runtime, Key::Text(value.into()), Default::default());
    }

    /// A runtime with the explorer showing `root` and the Search Files field
    /// focused through its root-row control.
    fn searching(root: &std::path::Path) -> rune::Runtime<State> {
        let mut runtime = runtime_for(state(root));
        wait(&mut runtime, |s| !s.workspace.entries.is_empty());
        settle(&mut runtime);
        click(&mut runtime, "explorer-search-files");
        settle(&mut runtime);
        assert!(runtime.state.explorer_search.open);
        assert!(focused(&runtime, QUERY_KEY), "the field takes the keyboard");
        runtime
    }

    fn finished(s: &State) -> bool {
        s.explorer_search.summary.is_some()
    }

    #[test]
    fn typing_waits_for_a_pause_then_groups_rows_with_line_numbers_and_highlights() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src/main.rs"),
            "fn main() {\n    let needle = 1;\n}\n",
        )
        .unwrap();
        std::fs::write(root.path().join("Needle.txt"), "none here\nA NEEDLE again").unwrap();
        std::fs::write(root.path().join(".hidden"), "needle").unwrap();
        let mut runtime = searching(root.path());
        let started = Instant::now();
        type_text(&mut runtime, "need");
        type_text(&mut runtime, "le");
        runtime.update(|s, tx| {
            s.workspace_poll(tx);
        });
        if started.elapsed() < DEBOUNCE {
            assert!(
                runtime.state.explorer_search.cancel_flag().is_none(),
                "no search starts while typing"
            );
            assert!(runtime.state.explorer_search.searching());
        }
        settle(&mut runtime);
        assert!(
            runtime.find("file-list").is_none(),
            "results replace the tree"
        );
        wait(&mut runtime, finished);
        assert!(started.elapsed() >= DEBOUNCE);
        settle(&mut runtime);
        let search = &runtime.state.explorer_search;
        let relatives: Vec<_> = search.results.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(
            relatives,
            ["src/main.rs", "Needle.txt"],
            "hidden files stay out"
        );
        assert_eq!(
            search.hits(),
            [Hit::Line(0, 0), Hit::Name(1), Hit::Line(1, 0)]
        );
        let snippet = &search.results[0].lines[0].snippet;
        assert_eq!(&snippet.text[snippet.highlight.clone()], "needle");
        let element = |key: &str| {
            runtime
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        // Each file's header, then its rows: line number and trimmed snippet.
        assert!(element("explorer-search-file-0").rect[1] < element(&hit_key(0)).rect[1]);
        assert!(element(&hit_key(0)).rect[1] < element("explorer-search-file-1").rect[1]);
        assert_eq!(
            element(&hit_key(0)).label.as_deref(),
            Some("Line 2: let needle = 1;")
        );
        assert_eq!(
            element(&hit_key(1)).label.as_deref(),
            Some("File name: Needle.txt")
        );
        assert_eq!(
            element(&hit_key(2)).label.as_deref(),
            Some("Line 2: A NEEDLE again")
        );
        let texts: Vec<_> = runtime
            .elements()
            .into_iter()
            .filter(|e| e.path.contains(&hit_key(0)))
            .filter_map(|e| e.text)
            .collect();
        assert!(texts.iter().any(|t| t == "2") && texts.iter().any(|t| t == "let needle = 1;"));
        let names = runtime
            .state
            .explorer_search
            .results
            .iter()
            .map(|f| f.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [None, Some(0..6)],
            "the highlighted part of the name"
        );

        // Showing hidden files searches again, now including them.
        runtime.update(|s, tx| s.dispatch(AppAction::ToggleHidden, tx));
        wait(&mut runtime, |s| {
            finished(s)
                && s.explorer_search
                    .results
                    .iter()
                    .any(|f| f.relative == ".hidden")
        });

        // Nothing matching says so.
        type_text(&mut runtime, "zzz");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        assert_eq!(
            runtime
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("explorer-search-status"))
                .and_then(|e| e.text)
                .as_deref(),
            Some("No results")
        );
    }

    #[test]
    fn more_than_two_hundred_rows_end_with_the_limit_message() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("many.txt"), "hit\n".repeat(250)).unwrap();
        let mut runtime = searching(root.path());
        type_text(&mut runtime, "hit");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        assert_eq!(runtime.state.explorer_search.hits().len(), 200);
        assert!(runtime.find(&hit_key(199)).is_some());
        assert!(runtime.find(&hit_key(200)).is_none());
        assert!(runtime.elements().iter().any(|e| {
            e.text.as_deref() == Some("Showing first 200 results. Refine your search.")
        }));
    }

    #[test]
    fn arrows_and_enter_open_a_name_as_a_preview_and_a_line_in_markdown_source() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("alpha.txt"), "nothing").unwrap();
        let mut doc = "filler line\n".repeat(200);
        doc.push_str("    see the Alpha section\n");
        doc.push_str(&"more filler\n".repeat(100));
        let doc_path = root.path().join("doc.md");
        std::fs::write(&doc_path, &doc).unwrap();
        let mut runtime = searching(root.path());
        type_text(&mut runtime, "alpha");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        assert_eq!(
            runtime.state.explorer_search.hits(),
            [Hit::Name(0), Hit::Line(1, 0)]
        );
        // Enter on a name opens a preview tab, as a single click in the tree
        // does, and the field keeps the keyboard for browsing on.
        press(&mut runtime, Key::Enter, Default::default());
        wait(&mut runtime, |s| s.workspace.tabs.len() == 1);
        settle(&mut runtime);
        let tab = &runtime.state.workspace.tabs[0];
        assert_eq!(tab.document.path, root.path().join("alpha.txt"));
        assert!(tab.preview);
        assert!(focused(&runtime, QUERY_KEY));
        // The Markdown file already shows rendered; a content match still
        // opens its source at the match.
        runtime.update(|s, _| s.open_file(doc_path.clone()));
        wait(&mut runtime, |s| {
            s.workspace.tabs.iter().any(|t| t.document.path == doc_path)
        });
        runtime.update(|s, tx| {
            let tab = s
                .workspace
                .tabs
                .iter_mut()
                .find(|t| t.document.path == doc_path)
                .unwrap();
            tab.mode = workspace::View::Preview;
            tab.edit.set_caret(0);
            tx.focus(QUERY_KEY, true);
        });
        settle(&mut runtime);
        press(&mut runtime, Key::Down, Default::default());
        assert_eq!(runtime.state.explorer_search.selected, 1);
        press(&mut runtime, Key::Down, Default::default());
        assert_eq!(
            runtime.state.explorer_search.selected, 1,
            "stops at the end"
        );
        press(&mut runtime, Key::Enter, Default::default());
        wait(&mut runtime, |s| s.workspace.search_reveal.is_some());
        settle(&mut runtime);
        let workspace = &runtime.state.workspace;
        let tab = &workspace.tabs[workspace.active];
        assert_eq!(tab.document.path, doc_path);
        assert!(tab.mode == workspace::View::Edit, "source, not rendered");
        let (start, end) = tab.edit.range();
        assert_eq!(&tab.edit.text[start..end], "Alpha");
        assert_eq!(tab.edit.text[..start].matches('\n').count() + 1, 201);
        // The match is scrolled into the editor's view.
        let id = tab.id;
        let viewport = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(&format!("editor-scroll-{id}")))
            .unwrap()
            .rect;
        let (_, y) = runtime
            .find(&search_match_key(id))
            .expect("the match is marked");
        assert!(
            y > viewport[1] && y < viewport[1] + viewport[3],
            "match at {y} within {viewport:?}"
        );
        // Up goes back to the name.
        press(&mut runtime, Key::Up, Default::default());
        assert_eq!(runtime.state.explorer_search.selected, 0);
    }

    #[test]
    fn escape_clears_the_query_and_results_and_returns_to_the_tree() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("notes.txt"), "needle").unwrap();
        let mut runtime = searching(root.path());
        type_text(&mut runtime, "needle");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        assert!(runtime.find(&hit_key(0)).is_some());
        press(&mut runtime, Key::Escape, Default::default());
        settle(&mut runtime);
        let search = &runtime.state.explorer_search;
        assert!(!search.open && search.query.text.is_empty() && search.results.is_empty());
        assert!(runtime.find(QUERY_KEY).is_none());
        assert!(runtime.find(&hit_key(0)).is_none());
        assert!(focused(&runtime, "file-list"), "the tree has the keyboard");
        assert!(
            runtime
                .find(root.path().join("notes.txt").to_str().unwrap())
                .is_some()
        );
        // The palette command and the overflow menu reach the same field.
        let mut commands = Vec::new();
        super::super::actions::register(&mut commands);
        assert!(
            commands
                .iter()
                .any(|c| c.action == AppAction::SearchFiles && c.title == "Explorer: Search Files")
        );
        runtime.update(|s, _| s.workspace.layout.explorer_width = 200.0);
        settle(&mut runtime);
        assert!(runtime.find("explorer-search-files").is_none());
        click(&mut runtime, "explorer-more");
        settle(&mut runtime);
        click(&mut runtime, "explorer-menu-search-files");
        settle(&mut runtime);
        assert!(runtime.state.explorer_search.open);
        assert!(focused(&runtime, QUERY_KEY));
    }

    #[test]
    fn a_new_query_cancels_the_running_search_and_its_rows_never_show() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "aa\naab").unwrap();
        let mut runtime = searching(root.path());
        let hold = Arc::new(AtomicBool::new(true));
        runtime.state.explorer_search.hold = Some(hold.clone());
        type_text(&mut runtime, "aa");
        wait(&mut runtime, |s| s.explorer_search.cancel_flag().is_some());
        let first = runtime.state.explorer_search.cancel_flag().unwrap();
        assert!(!first.load(Ordering::Relaxed));
        // The next keystroke stops it at once, before the pause ends.
        type_text(&mut runtime, "b");
        assert!(first.load(Ordering::Relaxed), "superseded immediately");
        hold.store(false, Ordering::Relaxed);
        wait(&mut runtime, finished);
        let lines: Vec<_> = runtime.state.explorer_search.results[0]
            .lines
            .iter()
            .map(|l| l.line)
            .collect();
        assert_eq!(lines, [2], "only the current query's rows");
        // Clearing the field cancels too, and shows the tree again.
        runtime.state.explorer_search.hold = Some(hold.clone());
        hold.store(true, Ordering::Relaxed);
        type_text(&mut runtime, "c");
        wait(&mut runtime, |s| s.explorer_search.cancel_flag().is_some());
        let running = runtime.state.explorer_search.cancel_flag().unwrap();
        runtime.update(|s, _| {
            s.explorer_search.query = TextEdit::new("");
            s.explorer_search_edited();
        });
        assert!(running.load(Ordering::Relaxed));
        settle(&mut runtime);
        assert!(runtime.find("file-list").is_some());
    }

    #[test]
    fn saves_and_file_operations_finish_while_a_search_runs() {
        let root = crate::test_support::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        std::fs::write(&path, "before").unwrap();
        let mut runtime = searching(root.path());
        runtime.update(|s, _| s.open_file(path.clone()));
        wait(&mut runtime, |s| !s.workspace.tabs.is_empty());
        let hold = Arc::new(AtomicBool::new(true));
        runtime.state.explorer_search.hold = Some(hold.clone());
        runtime.update(|_, tx| tx.focus(QUERY_KEY, true));
        type_text(&mut runtime, "before");
        wait(&mut runtime, |s| s.explorer_search.cancel_flag().is_some());
        let id = runtime.state.workspace.tabs[0].id;
        runtime.update(|s, tx| {
            let tab = &mut s.workspace.tabs[0];
            tab.edit.replace_text("after".to_owned());
            tab.revision += 1;
            s.dispatch(AppAction::SaveEditor(Some(id)), tx);
            s.start_file_operation(
                OperationRequest {
                    operation: FileOperation::CreateFile,
                    target: root.path().join("created.txt"),
                },
                None,
                tx,
            );
        });
        wait(&mut runtime, |s| {
            !s.workspace.tabs[0].saving && !s.workspace.operation_busy
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after");
        assert!(root.path().join("created.txt").is_file());
        assert!(
            runtime.state.explorer_search.searching(),
            "the search was still running throughout"
        );
        hold.store(false, Ordering::Relaxed);
        wait(&mut runtime, finished);
    }

    #[test]
    fn opening_a_result_from_the_compact_overlay_closes_it_and_focuses_the_editor() {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("notes.txt"), "one\nthe needle").unwrap();
        let mut state = State::with_root(root.path().join(".profile"));
        state.directory = root.path().to_owned();
        state.workspace_setup();
        state.navigate(root.path().to_owned());
        let mut runtime = runtime_for(state);
        runtime.set_viewport((640.0, 360.0), 1.0);
        settle(&mut runtime);
        runtime.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
        settle(&mut runtime);
        assert!(runtime.state.explorer_overlay_open());
        assert!(focused(&runtime, QUERY_KEY));
        type_text(&mut runtime, "needle");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        // Escape first returns to the tree inside the overlay.
        press(&mut runtime, Key::Escape, Default::default());
        settle(&mut runtime);
        assert!(runtime.state.explorer_overlay_open());
        assert!(focused(&runtime, "file-list"));
        runtime.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
        settle(&mut runtime);
        type_text(&mut runtime, "needle");
        wait(&mut runtime, finished);
        settle(&mut runtime);
        click(&mut runtime, &hit_key(0));
        wait(&mut runtime, |s| s.workspace.search_reveal.is_some());
        settle(&mut runtime);
        assert!(!runtime.state.explorer_overlay_open());
        let tab = &runtime.state.workspace.tabs[0];
        assert!(tab.edit.focused);
        let (start, end) = tab.edit.range();
        assert_eq!(&tab.edit.text[start..end], "needle");
        assert!(focused(&runtime, &format!("file-editor-{}", tab.id)));
    }
}
