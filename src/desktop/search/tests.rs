use super::*;
use crate::desktop::tests::{press, runtime_for};
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
        root.path().join("src").join("main.rs"),
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
    assert!(runtime.state.search_tab, "results show in the Search tab");
    assert!(runtime.find("search-content").is_some());
    assert!(runtime.find("file-list").is_some(), "the tree stays");
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
    let status = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("explorer-search-status"))
        .and_then(|e| e.text)
        .unwrap();
    assert_eq!(
        status,
        "200 results · 1 file · First 200 file results shown, refine to see more"
    );
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
    assert!(
        runtime.find(QUERY_KEY).is_some(),
        "the header keeps its field"
    );
    assert!(runtime.find(&hit_key(0)).is_none());
    assert!(!runtime.state.search_tab_open, "the Search tab closes");
    assert!(runtime.find("search-tab").is_none());
    assert!(
        runtime
            .find(root.path().join("notes.txt").to_str().unwrap())
            .is_some()
    );
    // The palette command and the overflow menu reach the same field.
    let mut commands = Vec::new();
    super::super::actions::register(&mut commands);
    assert!(commands.iter().any(
        |c| c.action == AppAction::SearchFiles && c.title == "Search: Files and Terminals"
    ));
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
fn a_compact_window_searches_from_the_header_and_opens_a_result_in_the_editor() {
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
    assert!(
        !runtime.state.explorer_overlay_open(),
        "the field is in the header"
    );
    assert!(focused(&runtime, QUERY_KEY));
    type_text(&mut runtime, "needle");
    wait(&mut runtime, finished);
    settle(&mut runtime);
    assert!(runtime.find("search-content").is_some());
    // Escape clears the search and closes its tab.
    press(&mut runtime, Key::Escape, Default::default());
    settle(&mut runtime);
    assert!(runtime.find("search-content").is_none());
    runtime.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
    settle(&mut runtime);
    type_text(&mut runtime, "needle");
    wait(&mut runtime, finished);
    settle(&mut runtime);
    click(&mut runtime, &hit_key(0));
    wait(&mut runtime, |s| s.workspace.search_reveal.is_some());
    settle(&mut runtime);
    assert!(!runtime.state.search_tab, "the file's tab shows");
    assert!(runtime.state.search_tab_open, "the Search tab stays open");
    let tab = &runtime.state.workspace.tabs[0];
    let (start, end) = tab.edit.range();
    assert_eq!(&tab.edit.text[start..end], "needle");
    // The Search tab shows its results again.
    click(&mut runtime, "search-tab");
    settle(&mut runtime);
    assert!(runtime.state.search_tab);
    assert!(runtime.find(&hit_key(0)).is_some());
    click(&mut runtime, "search-close");
    settle(&mut runtime);
    assert!(!runtime.state.search_tab_open);
    assert!(runtime.state.explorer_search.query.text.is_empty());
    assert!(
        runtime
            .find(&format!(
                "editor-tab-{}",
                runtime.state.workspace.tabs[0].id
            ))
            .is_some()
    );
}

#[test]
fn terminal_text_is_searched_with_files_and_a_row_selects_the_match() {
    let root = crate::test_support::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "nothing").unwrap();
    let shell = crate::test_support::shell_binary().unwrap();
    let session = Session::spawn(
        &shell,
        root.path(),
        root.path(),
        Config::default(),
        28,
        100,
        Arc::new(|| {}),
    )
    .unwrap();
    session
        .wait_for("rodtty:", Duration::from_secs(15))
        .unwrap();
    session
        .send(b"printf '  Build FAILED: wanted\\n'\r".to_vec())
        .unwrap();
    let start = Instant::now();
    while session.with_screen_mut(|screen| terminal::search::find_all(screen, "FAILED:").len()) < 2
    {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the shell printed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut state = state(root.path());
    state.sessions.push(session);
    let mut runtime = runtime_for(state);
    wait(&mut runtime, |s| !s.workspace.entries.is_empty());
    settle(&mut runtime);
    runtime.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
    settle(&mut runtime);
    assert!(runtime.state.active().is_some(), "a terminal session runs");
    type_text(&mut runtime, "failed");
    wait(&mut runtime, finished);
    settle(&mut runtime);
    let search = &runtime.state.explorer_search;
    assert_eq!(search.terminal.len(), 1);
    // The echoed command line, then its output, leading spaces trimmed.
    let found = &search.terminal[0].found;
    let output = found.last().unwrap();
    assert_eq!(output.text, "Build FAILED: wanted");
    assert_eq!(&output.text[output.highlight.clone()], "FAILED");
    assert_eq!(
        search.hits().last(),
        Some(&Hit::Terminal(0, found.len() - 1))
    );
    assert!(runtime.find("explorer-search-terminal-0").is_some());
    let last = search.hits().len() - 1;
    click(&mut runtime, &hit_key(last));
    settle(&mut runtime);
    let find = &runtime.state.terminal_search;
    assert!(find.open, "the terminal's find bar shows the match");
    let output = runtime.state.explorer_search.terminal[0]
        .found
        .last()
        .unwrap()
        .at;
    assert_eq!(find.current(), Some(output));
    assert!(runtime.state.workspace.layout.terminal);
}

fn status(runtime: &rune::Runtime<State>) -> String {
    runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("explorer-search-status") && !e.exiting)
        .and_then(|e| e.text)
        .unwrap_or_default()
}

#[test]
fn files_are_searched_with_the_explorer_closed_and_the_footer_reports_progress_and_counts() {
    let root = crate::test_support::tempdir().unwrap();
    std::fs::write(root.path().join("one.txt"), "needle\nand needle").unwrap();
    std::fs::write(root.path().join("needle.md"), "none").unwrap();
    let mut state = State::with_root(root.path().join(".profile"));
    state.directory = root.path().to_owned();
    state.workspace_setup();
    let mut runtime = runtime_for(state);
    settle(&mut runtime);
    assert!(
        runtime.find("file-list").is_none(),
        "the explorer is closed"
    );
    let hold = Arc::new(AtomicBool::new(true));
    runtime.state.explorer_search.hold = Some(hold.clone());
    runtime.update(|s, tx| s.dispatch(AppAction::SearchFiles, tx));
    settle(&mut runtime);
    type_text(&mut runtime, "needle");
    wait(&mut runtime, |s| s.explorer_search.cancel_flag().is_some());
    settle(&mut runtime);
    // While the file search runs, the footer says so.
    assert!(runtime.find("explorer-search-searching").is_some());
    assert_eq!(status(&runtime), "0 results so far · 0 files");
    hold.store(false, Ordering::Relaxed);
    wait(&mut runtime, finished);
    settle(&mut runtime);
    assert!(runtime.find("explorer-search-searching").is_none());
    assert_eq!(status(&runtime), "3 results · 2 files");

    // A group's header hides and shows its rows.
    let count = runtime.state.explorer_search.hits().len();
    assert_eq!(count, 3);
    let one = runtime
        .state
        .explorer_search
        .results
        .iter()
        .position(|f| f.relative == "one.txt")
        .unwrap();
    click(&mut runtime, &format!("explorer-search-file-{one}"));
    settle(&mut runtime);
    assert_eq!(runtime.state.explorer_search.hits().len(), 1);
    assert!(runtime.find(&hit_key(1)).is_none());
    assert_eq!(
        status(&runtime),
        "3 results · 2 files",
        "counts include hidden rows"
    );
    click(&mut runtime, &format!("explorer-search-file-{one}"));
    settle(&mut runtime);
    assert_eq!(runtime.state.explorer_search.hits().len(), 3);
    assert!(runtime.find(&hit_key(2)).is_some());
}
