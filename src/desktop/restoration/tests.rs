use super::*;
use std::time::Duration;

fn fixture_state(root: PathBuf) -> State {
    let mut state = State::with_root(root);
    state.shell = crate::test_support::shell_binary();
    state
}

#[test]
fn cold_workspace_restores_reordered_terminals_in_last_directories_without_old_commands() {
    let fixture = crate::test_support::tempdir().unwrap();
    let root = fixture.path().join("profile");
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let last_directory = second.join("last-directory");
    std::fs::create_dir(&last_directory).unwrap();
    let mut state = fixture_state(root.clone());
    state.directory = first.clone();
    state.restore_startup();
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", Duration::from_secs(10))
        .unwrap();
    state.start_tab(second.clone());
    state
        .active()
        .unwrap()
        .wait_for("rodtty:", Duration::from_secs(10))
        .unwrap();
    state.send(b"cd last-directory\r".to_vec());
    state
        .active()
        .unwrap()
        .wait_for("last-directory$", Duration::from_secs(10))
        .unwrap();
    assert_eq!(state.active().unwrap().cwd(), last_directory);
    state.send(
        b"printf '\\033]0;OLD APPLICATION\\007'; printf 'OLD_WORKSPACE_%s\\n' OUTPUT\r".to_vec(),
    );
    state
        .active()
        .unwrap()
        .wait_for("OLD_WORKSPACE_OUTPUT", Duration::from_secs(10))
        .unwrap();
    let mut runtime = crate::desktop::tests::runtime_for(state);
    runtime.update(|s, _| {
        s.move_tab(tabs::Target::Terminal(s.active().unwrap().id()), 0);
        s.persist_workspace();
    });
    let raw = std::fs::read_to_string(root.join("workspace-state.json")).unwrap();
    assert!(!raw.contains("OLD APPLICATION"));
    assert!(!raw.contains("OLD_WORKSPACE_OUTPUT"));
    runtime.update(|s, tx| {
        s.dispatch(AppAction::Quit, tx);
        s.confirm(tx);
        // The native on_quit callback follows the confirmed Quit action.
        s.persist_workspace();
    });
    let after_quit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("workspace-state.json")).unwrap()).unwrap();
    assert_eq!(after_quit["terminals"].as_array().unwrap().len(), 2);
    drop(runtime);
    let mut fresh = fixture_state(root);
    fresh.directory = first.clone();
    fresh.restore_startup();
    assert_eq!(fresh.sessions.len(), 2);
    assert_eq!(fresh.sessions[0].cwd(), last_directory);
    assert_eq!(fresh.sessions[1].cwd(), first);
    assert_eq!(fresh.active, 0);
    for session in &fresh.sessions {
        session
            .wait_for("rodtty:", Duration::from_secs(10))
            .unwrap();
        assert!(!session.screen().contents().contains("OLD_WORKSPACE_OUTPUT"));
        assert!(!session.tab_title().contains("OLD APPLICATION"));
    }
    let runtime = crate::desktop::tests::runtime_for(fresh);
    assert!(runtime.find("tab-0").is_some());
    assert!(runtime.find("tab-1").is_some());
}

#[test]
fn regular_files_layout_and_terminal_focus_survive_missing_paths_without_drafts() {
    let fixture = crate::test_support::tempdir().unwrap();
    let root = fixture.path().join("profile");
    let folder = fixture.path().join("files");
    std::fs::create_dir_all(&folder).unwrap();
    let first = folder.join("first.txt");
    let last = folder.join("last.txt");
    let preview = folder.join("preview.txt");
    let missing_folder = fixture.path().join("removed-folder");
    std::fs::create_dir(&missing_folder).unwrap();
    for path in [&first, &last, &preview] {
        std::fs::write(path, "saved contents").unwrap();
    }
    let mut state = fixture_state(root.clone());
    state.directory = folder.clone();
    state.restore_startup();
    state.navigate(folder.clone());
    state.workspace.layout.explorer = true;
    state.workspace.layout.explorer_width = 310.0;
    state.workspace.layout.editor_share = 0.61;
    state.open_file(first.clone());
    state.open_file(last.clone());
    drain_files(&mut state);
    state.open_file_with_preview(preview.clone(), true);
    drain_files(&mut state);
    state.workspace.tabs[0].edit.insert("UNSAVED_DRAFT");
    state.start_tab(missing_folder.clone());
    state.navigate(missing_folder.clone());
    state.workspace.set_terminal_focus(true);
    state.persist_workspace();
    let settings_before = std::fs::read(root.join("settings.json")).ok();
    std::fs::remove_file(&last).unwrap();
    drop(state);
    std::fs::remove_dir(&missing_folder).unwrap();
    let mut fresh = fixture_state(root.clone());
    fresh.directory = folder.clone();
    fresh.restore_startup();
    drain_files(&mut fresh);
    assert_eq!(fresh.workspace.tabs.len(), 1);
    assert_eq!(fresh.workspace.tabs[0].document.path, first);
    assert_eq!(fresh.workspace.tabs[0].edit.text, "saved contents");
    assert!(fresh.workspace.terminal_focus());
    assert!(fresh.message.contains("last.txt"), "{}", fresh.message);
    assert!(
        fresh.message.contains("removed-folder"),
        "{}",
        fresh.message
    );
    assert_eq!(fresh.sessions.len(), 2);
    assert!(fresh.sessions.iter().all(|s| s.cwd() == folder));
    assert_eq!(fresh.workspace.directory, Some(folder));
    assert_eq!(fresh.workspace.layout.explorer_width, 310.0);
    assert_eq!(fresh.workspace.layout.editor_share, 0.61);
    assert_eq!(
        std::fs::read(root.join("settings.json")).ok(),
        settings_before
    );
    let mut runtime = crate::desktop::tests::runtime_for(fresh);
    assert!(
        runtime.find("file-list").is_none(),
        "the explorer starts hidden"
    );
    runtime.update(|s, tx| s.dispatch(AppAction::ToggleExplorer, tx));
    runtime.frame(0.0);
    assert!(runtime.find("file-list").is_some());
}

fn drain_files(state: &mut State) {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    // Observe actual worker results rather than assuming filesystem timing.
    while std::time::Instant::now() < until {
        state.workspace_poll(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(10));
        if state.workspace_restoration_ready() {
            return;
        }
    }
    panic!("file restoration timed out: {}", state.message);
}

#[test]
fn single_fresh_startup_is_live_persisted_resettable_and_uses_directory_policy() {
    let fixture = crate::test_support::tempdir().unwrap();
    let root = fixture.path().join("profile");
    let original = fixture.path().join("original");
    let specified = fixture.path().join("specified");
    for directory in [&original, &specified] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let mut state = fixture_state(root.clone());
    state.directory = original.clone();
    state.restore_startup();
    state.start_tab(specified.clone());
    state.navigate(original.clone());
    let retained = state.active().unwrap().id();
    let mut runtime = crate::desktop::tests::runtime_for(state);
    runtime.update(|s, tx| {
        s.open_settings(tx);
        s.settings_query.replace_text("startup");
    });
    runtime.frame(0.0);
    crate::desktop::settings::choose(&mut runtime, "terminal-startup", "One fresh terminal");
    assert_eq!(runtime.state.active().unwrap().id(), retained);
    assert_eq!(runtime.state.sessions.len(), 2);
    runtime.update(|s, _| {
        s.save_config(|c| {
            c.behavior.new_terminal_directory = crate::config::NewTerminalDirectory::Specified;
            c.behavior.specified_directory = specified.clone();
        });
        s.persist_workspace();
    });
    drop(runtime);
    let mut fresh = fixture_state(root.clone());
    fresh.directory = original.clone();
    fresh.restore_startup();
    assert_eq!(fresh.sessions.len(), 1);
    assert_eq!(fresh.active().unwrap().cwd(), specified);
    for (choice, expected) in [
        (
            crate::config::NewTerminalDirectory::Active,
            specified.clone(),
        ),
        (
            crate::config::NewTerminalDirectory::Explorer,
            original.clone(),
        ),
    ] {
        let mut preferences = fresh.config.clone();
        preferences.behavior.new_terminal_directory = choice;
        preferences.save(&root).unwrap();
        let mut next = fixture_state(root.clone());
        next.directory = original.clone();
        next.restore_startup();
        assert_eq!(next.sessions.len(), 1);
        assert_eq!(next.active().unwrap().cwd(), expected);
    }
    let mut runtime = crate::desktop::tests::runtime_for(fresh);
    runtime.update(|s, tx| {
        s.open_settings(tx);
        s.settings_query.replace_text("startup");
    });
    runtime.frame(0.0);
    crate::desktop::settings::choose(
        &mut runtime,
        "terminal-startup",
        "Restore terminal arrangement",
    );
    let preferences = std::fs::read_to_string(root.join("settings.json")).unwrap();
    assert!(preferences.contains("restore_workspace"));
    // First launch always uses its initial directory, even with Single Fresh + Specified.
    let virgin_root = fixture.path().join("first-launch");
    let mut virgin_preferences = runtime.state.config.clone();
    virgin_preferences.behavior.terminal_startup = crate::config::TerminalStartup::SingleFresh;
    virgin_preferences.save(&virgin_root).unwrap();
    let mut virgin = fixture_state(virgin_root);
    virgin.directory = original.clone();
    virgin.restore_startup();
    assert_eq!(virgin.active().unwrap().cwd(), original);
}

#[test]
fn cold_workspace_hides_the_editor_when_it_has_no_content() {
    // With no editor tab and Settings closed the editor pane no longer
    // reopens as an empty placeholder: the terminal keeps the workspace.
    let fixture = crate::test_support::tempdir().unwrap();
    let mut state = fixture_state(fixture.path().into());
    state.directory = fixture.path().into();
    state.restore_startup();
    let mut runtime = rune::Runtime::new(state, view, (640.0, 360.0), rune::Env::new(true, 1.0));
    runtime.frame(0.0);
    runtime.update(|s, tx| {
        s.dispatch(AppAction::ToggleEditor, tx);
        s.persist_workspace();
    });
    drop(runtime);
    let mut fresh = fixture_state(fixture.path().into());
    fresh.directory = fixture.path().into();
    fresh.restore_startup();
    let mut runtime = rune::Runtime::new(fresh, view, (640.0, 360.0), rune::Env::new(true, 1.0));
    runtime.frame(0.0);
    assert!(runtime.find("editor-pane").is_none());
    assert!(runtime.find("terminal").is_some());
}

#[test]
fn cold_start_opens_terminal_first_with_the_explorer_hidden() {
    // Rodtty is terminal first: an explorer the owner left open is hidden
    // on the next launch, while its root folder still restores.
    let fixture = crate::test_support::tempdir().unwrap();
    let mut state = fixture_state(fixture.path().into());
    state.directory = fixture.path().into();
    state.restore_startup();
    let mut runtime = rune::Runtime::new(state, view, (1200.0, 800.0), rune::Env::new(true, 1.0));
    runtime.settle_all();
    runtime.frame(0.0);
    runtime.update(|s, tx| {
        s.dispatch(AppAction::ToggleExplorer, tx);
        s.persist_workspace();
    });
    runtime.frame(0.0);
    assert!(runtime.state.workspace.layout.explorer);
    let root = runtime.state.workspace.directory.clone();
    drop(runtime);
    let mut fresh = fixture_state(fixture.path().into());
    fresh.directory = fixture.path().into();
    fresh.restore_startup();
    let mut runtime = rune::Runtime::new(fresh, view, (1200.0, 800.0), rune::Env::new(true, 1.0));
    runtime.frame(0.0);
    assert!(!runtime.state.workspace.layout.explorer);
    assert!(!runtime.state.workspace.explorer_revealed);
    assert!(runtime.find("file-list").is_none());
    assert!(runtime.find("terminal").is_some());
    assert_eq!(runtime.state.workspace.directory, root);
    runtime.update(|s, tx| s.dispatch(AppAction::ToggleExplorer, tx));
    runtime.frame(0.0);
    assert!(runtime.find("file-list").is_some(), "the owner opens it");
}
