use crate::desktop::*;

pub(crate) fn preview(state: &mut State) {
    let mut parser = vt100::Parser::new(24, 112, 100);
    parser.process(b"\x1b[38;5;79mrodtty\x1b[0m:~/projects $ seq 1 10000 | wc -l\r\n10000\r\n\r\n");
    parser.process("\x1b[38;5;79mrodtty\x1b[0m:~/projects $ printf 'Hello, terminal 🌿\\n'\r\nHello, terminal 🌿\r\n\r\n".as_bytes());
    parser.process(b"\x1b[38;5;79mrodtty\x1b[0m:~/projects $ ");
    state.preview = Some(parser.screen().clone());
    state.message = "Renderer preview · sample content".into();
}
pub(crate) fn workspace_preview(state: &mut State) {
    preview(state);
    demo(state, false);
}
pub(crate) fn tabs_preview(state: &mut State) {
    demo(state, false);
    state.preview = None;
    let fixture = tempfile::tempdir().expect("tab preview directory");
    let Some(shell) = state.shell.clone() else {
        return;
    };
    // Keep the owner's appearance: the reloaded settings come from this root.
    let _ = std::fs::copy(
        state.root.join("settings.json"),
        fixture.path().join("settings.json"),
    );
    state.root = fixture.path().to_owned();
    state.directory = state.root.clone();
    let explorer_root = state.root.join("Rodtty");
    for index in 0..7 {
        let cwd = explorer_root.join(if index % 2 == 0 { "client" } else { "host" });
        std::fs::create_dir_all(&cwd).unwrap();
        let session = Session::spawn(
            &shell,
            &state.root,
            &cwd,
            Config::default(),
            28,
            100,
            Arc::new(|| {}),
        )
        .unwrap();
        session
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        if index == 0 {
            session.send(format!("printf '\\033]2;{}\\007\\033[2J\\033[H'; printf 'TITLE_READY\\n'\r",
                "Agent 日本語 — preserving the complete application title while the label visually ellipsizes ".repeat(3)).into_bytes()).unwrap();
            session
                .wait_for("TITLE_READY", std::time::Duration::from_secs(15))
                .unwrap();
        }
        state.sessions.push(session);
    }
    state.workspace.tabs.clear();
    state.workspace.worker = Some(crate::work::Worker::new(state.wake.clone()));
    for (index, folder) in ["client", "host", "shared", "packages", "tests"]
        .iter()
        .enumerate()
    {
        let path = explorer_root.join(folder).join("config.rs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "// Tab preview\nfn main() {}\n").unwrap();
        state.open_file_with_preview(path, index == 4);
        let started = std::time::Instant::now();
        while state.workspace.tabs.len() <= index {
            state.workspace_poll(&mut Tx::default());
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    state.navigate(explorer_root.clone());
    let started = std::time::Instant::now();
    while !state
        .workspace
        .entries
        .iter()
        .any(|entry| entry.path.starts_with(&explorer_root))
    {
        state.workspace_poll(&mut Tx::default());
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    state.message = "Tab strip preview · running PTYs and real file buffers".into();
    state.workspace.tabs[0].edit.insert("// Unsaved changes\n");
    state.workspace.active = 0;
    state.workspace.set_terminal_focus(true);
    state.active = 0;
    state.tab_strips.preview_root = Some(fixture);
}
pub(crate) fn tabs_light_preview(state: &mut State) {
    tabs_preview(state);
    state.config.appearance = config::Appearance::Light;
}
pub(crate) fn tabs_dark_preview(state: &mut State) {
    tabs_preview(state);
    state.config.appearance = config::Appearance::Dark;
}
pub(crate) fn tabs_light_editor_preview(state: &mut State) {
    tabs_light_preview(state);
    state.workspace.set_terminal_focus(false);
}
pub(crate) fn tabs_dark_editor_preview(state: &mut State) {
    tabs_dark_preview(state);
    state.workspace.set_terminal_focus(false);
}
pub(crate) fn preview_tabs_scene(state: &mut State) {
    tabs_preview(state);
    let first = state.workspace.tabs.first().map(|tab| tab.id);
    state
        .workspace
        .tabs
        .retain(|tab| Some(tab.id) == first || tab.preview);
    state.workspace.active = state
        .workspace
        .tabs
        .iter()
        .position(|tab| tab.preview)
        .unwrap_or(0);
    state.workspace.set_terminal_focus(false);
    state.message = "Regular dirty file and temporary preview · real file buffers".into();
}
pub(crate) fn tree_preview(state: &mut State) {
    preview(state);
    host_tree_demo(state);
}
pub(crate) fn icons_preview(state: &mut State) {
    preview(state);
    icons_demo(state);
}
pub(crate) fn palette_preview(state: &mut State) {
    workspace_preview(state);
    state.open_palette(&mut Tx::default());
    state.palette_query.focused = true;
}
pub(crate) fn settings_preview(state: &mut State) {
    preview(state);
    state.open_settings(&mut Tx::default());
}
pub(crate) fn behavior_preview(state: &mut State) {
    settings_preview(state);
    state.settings_tab = settings::Tab::General;
}
pub(crate) fn real_diff_preview(state: &mut State) {
    preview(state);
    if let Err(error) = real_diff_demo(state) {
        state.message = format!("Could not render real Git fixture: {error:#}");
    }
}
pub(crate) fn file_collision_preview(state: &mut State) {
    workspace_preview(state);
    file_operations::collision_demo(state);
}
/// Tooling-only: Search Files results for "main" in the --cwd folder.
pub(crate) fn search_files_preview(state: &mut State) {
    tree_preview(state);
    state.explorer_search.open = true;
    state.explorer_search.query = rune::TextEdit::new("main");
    state.show_search_tab();
    let started = std::time::Instant::now();
    while state.explorer_search.summary.is_none() && started.elapsed().as_secs() < 10 {
        state.poll_explorer_search(&mut Tx::default());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
/// Tooling-only: README.md from the --cwd folder opened as a temporary tab.
pub(crate) fn markdown_preview(state: &mut State) {
    tree_preview(state);
    state.workspace.worker = Some(crate::work::Worker::new(state.wake.clone()));
    let readme = state.directory.join("README.md");
    state.open_file_with_preview(readme, true);
    let started = std::time::Instant::now();
    while state.workspace.tabs.is_empty() && started.elapsed().as_secs() < 10 {
        state.workspace_poll(&mut Tx::default());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
pub(crate) fn terminal_search_preview(state: &mut State) {
    preview(state);
    state.workspace.set_terminal_focus(true);
    state.terminal_search.open = true;
}
pub(crate) fn diff_preview(state: &mut State) {
    preview(state);
    demo(state, true);
}
