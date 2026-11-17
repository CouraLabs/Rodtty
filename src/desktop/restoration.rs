//! Cold startup reconstructs views and fresh shells; window reopen retains State.
use super::*;

impl State {
    pub(super) fn restore_startup(&mut self) {
        self.workspace_setup();
        let directories = self.workspace.layout.terminals.clone();
        let active = self.workspace.layout.active_terminal;
        if directories.is_empty() {
            self.start_tab(self.directory.clone());
        } else if self.config.behavior.terminal_startup
            == crate::config::TerminalStartup::SingleFresh
        {
            let directory = match self.config.behavior.new_terminal_directory {
                crate::config::NewTerminalDirectory::Active => directories.get(active).cloned(),
                crate::config::NewTerminalDirectory::Explorer => self.workspace.directory.clone(),
                crate::config::NewTerminalDirectory::Specified => {
                    Some(self.config.behavior.specified_directory.clone())
                }
            }
            .unwrap_or_else(|| self.directory.clone());
            self.start_restored_terminal(directory);
        } else {
            for directory in directories {
                self.start_restored_terminal(directory);
            }
            self.active = active.min(self.sessions.len().saturating_sub(1));
        }
        self.workspace.terminal_focus = self.workspace.layout.terminal_focus;
        if self.workspace_restoration_ready() && !self.workspace.restoration_warnings.is_empty() {
            self.message = self.workspace.restoration_warnings.join("\n");
        }
    }

    fn start_restored_terminal(&mut self, directory: PathBuf) {
        if directory.is_absolute() && directory.is_dir() {
            self.start_tab(directory);
        } else {
            self.workspace.restoration_warnings.push(format!(
                "Saved terminal folder is unavailable: {}. Opened the startup folder instead.",
                directory.display()
            ));
            self.start_tab(self.directory.clone());
        }
    }
}

/// Uses the real host, shells and window lifecycle. Two invocations distinguish
/// retained processes on window reopen from fresh processes after explicit Quit.
pub(super) fn native_check(
    handle: rune::AppHandle<State>,
    report: PathBuf,
    backend: Arc<std::sync::Mutex<String>>,
) {
    let previous = std::env::args()
        .skip_while(|a| a != "--restoration-cold")
        .nth(1)
        .map(PathBuf::from);
    std::thread::spawn(move || {
        let check = (|| -> Result<serde_json::Value> {
            wait_app(&handle, |s| {
                s.active()
                    .is_some_and(|t| t.screen().contents().contains("rodtty:"))
            })?;
            let before = if let Some(path) = previous {
                let previous: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
                let fresh = snapshot(&handle)?;
                anyhow::ensure!(
                    fresh["directories"] == previous["directories"],
                    "cold directory order changed: {fresh}"
                );
                anyhow::ensure!(
                    fresh["active"] == previous["active"],
                    "cold active terminal changed"
                );
                anyhow::ensure!(
                    !fresh["output"].to_string().contains("T22_OLD_OUTPUT"),
                    "old commands/output replayed"
                );
                probe_pids(&handle)?;
                let fresh = snapshot(&handle)?;
                let old_pids = previous["pids"]
                    .as_array()
                    .context("previous shell PIDs missing")?;
                let new_pids = fresh["pids"]
                    .as_array()
                    .context("fresh shell PIDs missing")?;
                anyhow::ensure!(
                    new_pids.len() == 2 && new_pids.iter().all(|pid| !old_pids.contains(pid)),
                    "cold startup retained old shell PIDs"
                );
                anyhow::ensure!(
                    !fresh["titles"].to_string().contains("T22 OLD TITLE"),
                    "cold startup restored stale title override"
                );
                let mut report = fresh;
                report["cold_fresh_processes"] = true.into();
                report["old_commands_not_replayed"] = true.into();
                report["previous_pids"] = previous["pids"].clone();
                report
            } else {
                handle.update(|s, _| {
                    let first = s.root.join("native-restoration/alpha");
                    let second = s.root.join("native-restoration/beta");
                    std::fs::create_dir_all(&first).unwrap();
                    std::fs::create_dir_all(&second).unwrap();
                    // The native fixture starts isolated fresh terminals through the
                    // same action boundary used by startup and new-tab policies.
                    s.start_tab(first);
                    s.start_tab(second);
                    s.sessions.remove(0);
                    s.active = 1;
                    s.move_tab(tabs::Target::Terminal(s.active().unwrap().id()), 0);
                });
                wait_app(&handle, |s| {
                    s.sessions.len() == 2
                        && s.sessions
                            .iter()
                            .all(|t| t.screen().contents().contains("rodtty:"))
                })?;
                probe_pids(&handle)?;
                handle.update(|s, _| {
                    s.save_config(|c| {
                        c.behavior.terminal_startup = crate::config::TerminalStartup::SingleFresh
                    })
                });
                handle.update(|s, _| s.send(b"printf '\\033]0;T22 OLD TITLE\\007'; sleep 1; printf 'T22_OLD_%s\\n' OUTPUT\r".to_vec()));
                handle.close();
                wait_visibility(&handle, false)?;
                wait_app(&handle, |s| {
                    s.active()
                        .is_some_and(|t| t.screen().contents().contains("T22_OLD_OUTPUT"))
                })?;
                let retained = snapshot(&handle)?;
                let code = std::process::Command::new(std::env::current_exe()?).status()?;
                anyhow::ensure!(code.success(), "relaunch did not attach");
                wait_visibility(&handle, true)?;
                let reopened = snapshot(&handle)?;
                anyhow::ensure!(
                    reopened == retained,
                    "reopen changed process/session/output/order: {reopened}"
                );
                let mut report = reopened;
                report["same_session_and_pid_on_reopen"] = true.into();
                report["recent_output_retained"] = true.into();
                report["single_fresh_preference_preserved_live_sessions"] = true.into();
                handle.update(|s, _| {
                    s.save_config(|c| {
                        c.behavior.terminal_startup =
                            crate::config::TerminalStartup::RestoreWorkspace
                    })
                });
                report
            };
            let mut result = before;
            result["passed"] = true.into();
            result["window_backend"] = backend
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .into();
            wait_app(&handle, |s| {
                s.sessions.iter().all(|t| !t.has_running_commands())
            })?;
            handle.update(|s, _| s.persist_workspace());
            Ok(result)
        })();
        let result =
            check.unwrap_or_else(|e| serde_json::json!({"passed":false,"error":format!("{e:#}")}));
        if let Some(parent) = report.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(report, serde_json::to_vec_pretty(&result).unwrap());
        eprintln!("rodtty restoration check: {result}");
        if result["passed"] != true {
            std::process::exit(1);
        }
        handle.update(|s, tx| s.dispatch(AppAction::Quit, tx));
    });
}

fn wait_visibility(handle: &rune::AppHandle<State>, visible: bool) -> Result<()> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while handle.is_visible() != visible && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    anyhow::ensure!(
        handle.is_visible() == visible,
        "window visibility did not become {visible}"
    );
    Ok(())
}

fn probe_pids(handle: &rune::AppHandle<State>) -> Result<()> {
    handle.update(|s, _| {
        for session in &s.sessions {
            let _ = session.send(b"printf 'T22_PID=%s\\n' \"$$\"\r".to_vec());
        }
    });
    wait_app(handle, |s| {
        s.sessions
            .iter()
            .all(|t| shell_pid(&t.screen().contents()).is_some())
    })
}

fn shell_pid(output: &str) -> Option<u32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("T22_PID=")?.trim().parse().ok())
}

fn snapshot(handle: &rune::AppHandle<State>) -> Result<serde_json::Value> {
    let (tx, rx) = std::sync::mpsc::channel();
    handle.update(move |s, _| {
        let outputs: Vec<_> = s.sessions.iter().map(|t| t.screen().contents()).collect();
        let _ = tx.send(serde_json::json!({
            "host_pid":std::process::id(),
            "session_ids":s.sessions.iter().map(Session::id).collect::<Vec<_>>(),
            "directories":s.sessions.iter().map(Session::cwd).collect::<Vec<_>>(),
            "active":s.active,
            "pids":outputs.iter().filter_map(|out| shell_pid(out)).collect::<Vec<_>>(),
            "output":outputs,
            "titles":s.sessions.iter().map(Session::tab_title).collect::<Vec<_>>(),
        }));
    });
    Ok(rx.recv_timeout(std::time::Duration::from_secs(10))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture_state(root: PathBuf) -> State {
        let mut state = State::with_root(root);
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state
    }

    #[test]
    fn cold_workspace_restores_reordered_terminals_in_last_directories_without_old_commands() {
        let fixture = tempfile::tempdir().unwrap();
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
            b"printf '\\033]0;OLD APPLICATION\\007'; printf 'OLD_WORKSPACE_%s\\n' OUTPUT\r"
                .to_vec(),
        );
        state
            .active()
            .unwrap()
            .wait_for("OLD_WORKSPACE_OUTPUT", Duration::from_secs(10))
            .unwrap();
        let mut runtime = super::super::tests::runtime_for(state);
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
            serde_json::from_slice(&std::fs::read(root.join("workspace-state.json")).unwrap())
                .unwrap();
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
        let runtime = super::super::tests::runtime_for(fresh);
        assert!(runtime.find("tab-0").is_some());
        assert!(runtime.find("tab-1").is_some());
    }

    #[test]
    fn regular_files_layout_and_terminal_focus_survive_missing_paths_without_drafts() {
        let fixture = tempfile::tempdir().unwrap();
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
        state.workspace.terminal_focus = true;
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
        assert!(fresh.workspace.terminal_focus);
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
        let runtime = super::super::tests::runtime_for(fresh);
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
        let fixture = tempfile::tempdir().unwrap();
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
        let mut runtime = super::super::tests::runtime_for(state);
        runtime.update(|s, tx| {
            s.open_settings(tx);
            s.settings_query.replace_text("startup");
        });
        runtime.frame(0.0);
        click(&mut runtime, "startup-single-fresh");
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
        let mut runtime = super::super::tests::runtime_for(fresh);
        runtime.update(|s, tx| {
            s.open_settings(tx);
            s.settings_query.replace_text("startup");
        });
        runtime.frame(0.0);
        click(&mut runtime, "reset-terminal-startup");
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

    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
    }
}
