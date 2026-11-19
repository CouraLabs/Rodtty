use crate::desktop::*;

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
                s.workspace_restoration_ready()
                    && s.active()
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
                    fresh["regular_files"] == previous["regular_files"]
                        && fresh["editor_files"] == previous["regular_files"]
                        && fresh["panes"] == previous["panes"],
                    "cold editor/pane arrangement changed: {fresh}"
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
                report["regular_editor_and_panes_restored"] = true.into();
                report["preview_omitted_on_cold_start"] = true.into();
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
                handle.update(|s, _| {
                    let regular = s.root.join("native-restoration/alpha/retained.txt");
                    let preview = s.root.join("native-restoration/beta/preview.txt");
                    std::fs::write(&regular, "retained native file").unwrap();
                    std::fs::write(&preview, "temporary native file").unwrap();
                    s.open_file(regular);
                    s.open_file_with_preview(preview, true);
                });
                wait_app(&handle, |s| {
                    s.workspace_restoration_ready() && s.workspace.tabs.len() == 2
                })?;
                handle.update(|s, tx| {
                    s.dispatch(AppAction::ActivateEditor(0), tx);
                    s.workspace.layout.explorer = true;
                    s.workspace.layout.explorer_width = 310.0;
                    s.workspace.layout.editor_share = 0.61;
                    s.workspace.explorer_revealed = true;
                });
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

pub(crate) fn wait_visibility(handle: &rune::AppHandle<State>, visible: bool) -> Result<()> {
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

pub(crate) fn probe_pids(handle: &rune::AppHandle<State>) -> Result<()> {
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

pub(crate) fn shell_pid(output: &str) -> Option<u32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("T22_PID=")?.trim().parse().ok())
}

pub(crate) fn snapshot(handle: &rune::AppHandle<State>) -> Result<serde_json::Value> {
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
            "regular_files":s.workspace.tabs.iter().filter(|t| !t.preview).map(|t| &t.document.path).collect::<Vec<_>>(),
            "editor_files":s.workspace.tabs.iter().map(|t| &t.document.path).collect::<Vec<_>>(),
            "panes":{
                "editor_visible":s.workspace.layout.editor,
                "terminal_focus":s.workspace.terminal_focus(),
                "explorer_visible":s.workspace.layout.explorer,
                "explorer_revealed":s.workspace.explorer_revealed,
                "explorer_width":s.workspace.layout.explorer_width,
                "editor_share":s.workspace.layout.editor_share,
            },
        }));
    });
    Ok(rx.recv_timeout(std::time::Duration::from_secs(10))?)
}
