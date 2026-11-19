use crate::desktop::*;

pub(crate) fn native_check(
    handle: rune::AppHandle<State>,
    report: PathBuf,
    backend: Arc<std::sync::Mutex<String>>,
) {
    let with_trash = std::env::args().any(|a| a == "--trash-check");
    if std::env::args().any(|a| a == "--restoration-check") {
        restoration::native_check(handle, report, backend);
        return;
    }
    let with_frame = std::env::args().any(|a| a == "--frame-check");
    let with_fonts = std::env::args().any(|a| a == "--font-check");
    let with_sessions = std::env::args().any(|a| a == "--session-policy-check");
    let with_workspace = with_frame
        || with_fonts
        || with_sessions
        || std::env::args().any(|a| a == "--workspace-check");
    std::thread::spawn(move || {
        use std::time::{Duration, Instant};
        let check = (|| -> Result<serde_json::Value> {
            let wait_visible = |visible: bool| -> Result<()> {
                let start = Instant::now();
                while handle.is_visible() != visible && start.elapsed() < Duration::from_secs(15) {
                    std::thread::sleep(Duration::from_millis(30));
                }
                anyhow::ensure!(
                    handle.is_visible() == visible,
                    "window visibility did not become {visible}; {:?}",
                    handle.window_error()
                );
                Ok(())
            };
            wait_visible(true)?;
            let (tx, rx) = std::sync::mpsc::channel();
            handle.update(move |state, _| {
                let _ = tx.send(state.active().map(|session| session.state.clone()));
            });
            let session = rx
                .recv_timeout(Duration::from_secs(15))?
                .ok_or_else(|| anyhow::anyhow!("no terminal session"))?;
            let wait_text = |needle: &str| -> Result<()> {
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(15) {
                    if session
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .parser
                        .screen()
                        .contents()
                        .contains(needle)
                    {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                anyhow::bail!("terminal did not produce {needle}");
            };
            wait_text("rodtty:")?;
            if std::env::args().any(|a| a == "--clipboard-check") {
                let mut result = terminal::clipboard::native_check(&handle, &report)?;
                result["window_backend"] = backend
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                    .into();
                return Ok(result);
            }
            let trash_evidence = if with_trash {
                Some(removal::native_check(&handle)?)
            } else {
                None
            };
            let frame_evidence = if with_frame {
                // Real editor work makes the native custom-close check exercise the
                // same dirty-file decision as the menu and OS close lifecycle.
                let fixture = tempfile::tempdir()?;
                let file = fixture.path().join("frame-close-check.txt");
                std::fs::write(&file, "saved frame fixture\n")?;
                handle.update(move |s, _| s.open_file(file));
                wait_app(&handle, |s| !s.workspace.tabs.is_empty())?;
                handle.update(|s, _| {
                    s.workspace.tabs[0].edit.insert("unsaved frame fixture\n");
                    s.workspace.tabs[0].revision += 1;
                });
                wait_app(&handle, |s| s.workspace.tabs[0].dirty())?;
                let (info_tx, info_rx) = std::sync::mpsc::channel();
                handle.update(move |s, _| {
                    let info = s.window.as_ref().and_then(|w| w.with_winit_window(|w| {
                        let size = w.inner_size();
                        serde_json::json!({"position":w.inner_position().ok().map(|p| [p.x,p.y]),"decorated":w.is_decorated(), "maximized":w.is_maximized(),
                            "minimized":w.is_minimized(),"size":[size.width,size.height],"scale":w.scale_factor()})
                    }).ok());
                    let _ = info_tx.send(info);
                });
                let info = info_rx
                    .recv_timeout(Duration::from_secs(10))?
                    .context("frame native window missing")?;
                // macOS keeps its native frame so the traffic lights survive; the
                // app-drawn frames are the other two, and they must be borderless.
                let expected_decorated = cfg!(target_os = "macos");
                anyhow::ensure!(
                    info["decorated"] == expected_decorated,
                    "expected decorated == {expected_decorated} on this platform: {info}"
                );
                std::fs::write(
                    report.with_extension("ready.json"),
                    serde_json::to_vec_pretty(&info)?,
                )?;
                let input = report.with_extension("input.json");
                let start = Instant::now();
                while !input.exists() && start.elapsed() < Duration::from_secs(120) {
                    std::thread::sleep(Duration::from_millis(30));
                }
                let evidence: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&input)
                        .context("external native frame input did not complete")?,
                )?;
                anyhow::ensure!(
                    evidence["passed"] == true,
                    "external frame input failed: {evidence}"
                );
                handle.update(|s, _| {
                    s.workspace.tabs.clear();
                    s.workspace.set_terminal_focus(true);
                });
                wait_app(&handle, |s| s.workspace.tabs.is_empty())?;
                Some(serde_json::json!({"initial_native_state":info,"compositor_input":evidence}))
            } else {
                None
            };
            // Exercise live preferences against a real running terminal session.
            // Retain the owner's original choice when this developer harness finishes.
            let (choice_tx, choice_rx) = std::sync::mpsc::channel();
            handle.update(move |s, _| {
                let _ = choice_tx.send((s.config.theme_family, s.config.appearance));
            });
            let (original_family, original_appearance) =
                choice_rx.recv_timeout(Duration::from_secs(10))?;
            for &family in config::ThemeFamily::ALL {
                for appearance in [
                    config::Appearance::Light,
                    config::Appearance::Dark,
                    config::Appearance::System,
                ] {
                    handle.update(move |s, _| {
                        s.save_config(|c| {
                            c.theme_family = family;
                            c.appearance = appearance;
                        })
                    });
                    wait_app(&handle, move |s| {
                        s.config.theme_family == family && s.config.appearance == appearance
                    })?;
                    let (identity_tx, identity_rx) = std::sync::mpsc::channel();
                    handle.update(move |s, _| {
                        let _ = identity_tx.send(s.active().map(|session| session.state.clone()));
                    });
                    let active = identity_rx
                        .recv_timeout(Duration::from_secs(10))?
                        .context("terminal missing after appearance change")?;
                    anyhow::ensure!(
                        Arc::ptr_eq(&session, &active),
                        "appearance change replaced the terminal session"
                    );
                }
            }
            handle.update(move |s, _| {
                s.save_config(|c| { c.theme_family = original_family; c.appearance = original_appearance; });
                s.send(b"printf '\\033[31mANSI \\033[38;5;196mEXTENDED \\033[38;2;12;34;56mTRUECOLOR\\033[0m APPEARANCE_%s\\n' RETAINED\r".to_vec());
            });
            wait_text("APPEARANCE_RETAINED")?;
            let _workspace_fixture = if with_workspace {
                let fixture = tempfile::tempdir()?;
                let file = fixture.path().join("workspace-check.rs");
                std::fs::write(&file, "fn main() {}\n")?;
                let opened = file.clone();
                handle.update(move |s, _| {
                    s.workspace.layout.explorer = true;
                    s.open_file(opened);
                });
                wait_app(&handle, |s| !s.workspace.tabs.is_empty())
                    .context("Open native editor fixture")?;
                handle.update(|s, _| {
                    let tab = &mut s.workspace.tabs[0];
                    tab.edit.insert("// unsaved native check\n");
                    tab.revision += 1;
                });
                wait_app(&handle, |s| {
                    s.workspace.tabs.first().is_some_and(|t| t.dirty())
                })
                .context("Apply native editor change")?;
                handle.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
                wait_app(&handle, |s| {
                    matches!(
                        s.pending,
                        Some(Pending::File(workspace::FilePrompt::Unsaved { .. }))
                    )
                })
                .context("Show unsaved close prompt")?;
                wait_visible(true)?;
                handle.update(move |s, _| {
                    let id = s.workspace.tabs[0].id;
                    if with_sessions {
                        // Choose Save in the actual dirty-close decision. Its
                        // continuation must close only after the protected save.
                        s.save_file(id, false, Some(workspace::Action::Window));
                    } else {
                        s.cancel_file_action();
                        s.save_file(id, false, None);
                    }
                });
                wait_app(&handle, |s| {
                    s.workspace
                        .tabs
                        .first()
                        .is_some_and(|t| !t.saving && !t.dirty())
                })
                .context("Save native editor fixture")?;
                anyhow::ensure!(
                    std::fs::read_to_string(&file)?.contains("unsaved native check"),
                    "editor save did not reach disk"
                );
                if with_sessions {
                    wait_visible(false)?;
                    let code = std::process::Command::new(std::env::current_exe()?).status()?;
                    anyhow::ensure!(code.success(), "Save/close relaunch failed");
                    wait_visible(true)?;
                }
                Some(fixture)
            } else {
                None
            };
            let mut font_inventory = None;
            if with_fonts {
                wait_app(&handle, |s| s.font_catalog.ready)?;
                let (tx, rx) = std::sync::mpsc::channel();
                handle.update(move |s, _| {
                    let mono = s
                        .font_catalog
                        .families
                        .iter()
                        .find(|f| !f.bundled && f.monospace)
                        .map(|f| f.name.clone());
                    let sans = s
                        .font_catalog
                        .families
                        .iter()
                        .find(|f| !f.bundled && !f.monospace)
                        .map(|f| f.name.clone());
                    let _ = tx.send((s.font_catalog.families.len(), mono, sans));
                });
                let (count, mono, sans) = rx.recv_timeout(Duration::from_secs(10))?;
                let mono = mono.context("installed monospace family")?;
                let sans = sans.context("installed UI family")?;
                font_inventory = Some(
                    serde_json::json!({"families":count,"terminal_family":mono,"ui_family":sans}),
                );
                let probe = std::env::current_exe()?
                    .with_file_name(format!("rodtty-probe{}", std::env::consts::EXE_SUFFIX));
                let command = format!("{} --resize-fixture\r", profile::quote(&probe)?);
                handle.update(move |s, _| s.send(command.into_bytes()));
                wait_text("FULL SCREEN FIXTURE")?;
                let before = session
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .parser
                    .screen()
                    .size();
                handle.update(move |s, _| {
                    let tab = &mut s.workspace.tabs[0];
                    tab.edit.insert("// font change preserves this buffer\n");
                    tab.revision += 1;
                    s.save_config(|c| {
                        c.fonts.ui_family = sans.clone();
                        c.fonts.terminal_family = mono.clone();
                        c.fonts.ui_size = 15.0;
                        c.fonts.terminal_size = 20.0;
                        c.fonts.editor_size = 18.0;
                    });
                });
                wait_app(&handle, move |s| {
                    s.with_screen(|screen| {
                        let (rows, cols) = screen.size();
                        rows < before.0
                            && cols < before.1
                            && screen.alternate_screen()
                            && screen.contents().contains(&format!("SIZE:{cols}x{rows}"))
                    })
                })
                .with_context(|| {
                    let terminal = session.lock().unwrap_or_else(|e| e.into_inner());
                    format!("live font metrics resize the existing full-screen PTY; before {before:?}; after {:?}; output {:?}", terminal.parser.screen().size(), terminal.parser.screen().contents())
                })?;
                wait_app(&handle, |s| {
                    s.workspace.tabs[0].dirty()
                        && s.workspace.tabs[0]
                            .edit
                            .text
                            .contains("font change preserves this buffer")
                        && Config::load(&s.root)
                            .is_ok_and(|c| c.fonts.ui_size == 15.0 && c.fonts.editor_size == 18.0)
                })?;
                handle.update(|s, _| {
                    s.send(b"q".to_vec());
                    // This isolated harness discards only its own fixture edit.
                    let tab = &mut s.workspace.tabs[0];
                    tab.edit.replace_text(&tab.document.saved.text);
                });
                wait_app(&handle, |s| {
                    s.with_screen(|screen| !screen.alternate_screen())
                })?;
            }
            let session_policies = if with_sessions {
                Some(native_session_policies(&handle, &wait_visible)?)
            } else {
                None
            };
            handle
                .update(|state, _| state.send(b"sleep 1; printf 'HIDDEN_%s\\n' OUTPUT\r".to_vec()));
            handle.close();
            wait_visible(false)?;
            wait_text("HIDDEN_OUTPUT")?;
            let code = std::process::Command::new(std::env::current_exe()?).status()?;
            anyhow::ensure!(code.success(), "second launch failed");
            wait_visible(true)?;
            let (tx, rx) = std::sync::mpsc::channel();
            handle.update(move |state, _| {
                let _ = tx.send(state.active().map(|session| session.state.clone()));
            });
            let reopened = rx
                .recv_timeout(Duration::from_secs(10))?
                .ok_or_else(|| anyhow::anyhow!("reopened session missing"))?;
            anyhow::ensure!(
                Arc::ptr_eq(&session, &reopened),
                "reopen created a different session"
            );
            let mut report = serde_json::json!({"passed":true,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
                "window_backend":backend.lock().unwrap_or_else(|e|e.into_inner()).clone(),
                "window_closed":true,"output_while_closed":true,"reopened_from_second_launch":true,"same_session":true,"appearance_changes_preserved_session":true,"theme_families_preserved_session":9,"family_mode_changes":27,"output_after_appearance_change":true});
            if let Some(trash) = trash_evidence {
                report["trash"] = trash;
            }
            if let Some(policies) = session_policies {
                report["session_policies"] = policies;
                wait_app(&handle, |s| {
                    s.sessions.len() == 1
                        && s.active()
                            .is_some_and(|session| !session.has_running_commands())
                })?;
            }
            if let Some(frame) = frame_evidence {
                report["custom_frame"] = frame;
            }
            if with_fonts {
                report["font_inventory"] = font_inventory.unwrap();
                report["live_font_families"] = true.into();
                report["live_font_sizes"] = true.into();
                report["fullscreen_pty_resize"] = true.into();
                report["unsaved_editor_preserved"] = true.into();
            }
            if with_workspace {
                report["unsaved_close_prompt"] = true.into();
                report["editor_saved"] = true.into();
            }
            Ok(report)
        })();
        let result = match check {
            Ok(result) => result,
            Err(error) => {
                serde_json::json!({"passed":false,"error":format!("{error:#}"),"window_backend":backend.lock().unwrap_or_else(|e|e.into_inner()).clone(),"window_error":handle.window_error()})
            }
        };
        if let Some(parent) = report.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(
            &report,
            serde_json::to_vec_pretty(&result).unwrap_or_default(),
        ) {
            eprintln!("rodtty: could not write native check: {error}");
        }
        eprintln!("rodtty native check: {result}");
        if result["passed"] != true {
            std::process::exit(1);
        }
        if with_sessions {
            handle.update(|s, tx| s.dispatch(AppAction::Quit, tx));
        } else {
            handle.quit();
        }
    });
}

/// Native lifecycle regression: actual hide/reopen notifications share the
/// same policy boundary as explicit app actions. Every fixture uses this host's
/// isolated owner root and the caller's per-ticket TMPDIR.
pub(crate) fn native_session_policies(
    handle: &rune::AppHandle<State>,
    wait_visible: &impl Fn(bool) -> Result<()>,
) -> Result<serde_json::Value> {
    use crate::config::{
        NewTerminalDirectory as Directory, TerminalClosePolicy as Tab, WindowClosePolicy as Window,
    };
    use std::time::Duration;
    wait_app(handle, |s| {
        s.active()
            .is_some_and(|session| !session.has_running_commands())
    })
    .context("initial idle shell")?;
    let (tx, rx) = std::sync::mpsc::channel();
    handle.update(move |s, _| {
        let first = s.active().unwrap();
        let _ = tx.send((first.id(), first.cwd(), first.config.clone()));
    });
    let (first, initial_cwd, initial_config) = rx.recv_timeout(Duration::from_secs(10))?;
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::AlwaysConfirm);
        s.dispatch(AppAction::EndTerminal, tx);
    });
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Tab(_))))
        .context("Always confirm idle tab")?;
    handle.update(|s, _| {
        s.cancel_file_action();
        s.save_config(|c| c.behavior.window_close = Window::AskEachTime);
    });
    wait_app(handle, |s| {
        s.pending.is_none() && s.config.behavior.window_close == Window::AskEachTime
    })
    .context("Ask policy applied before native close")?;
    handle.close();
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Window)))
        .context("Ask native close prompt")?;
    wait_visible(true)?;
    handle.update(|s, _| s.cancel_file_action());
    wait_app(handle, move |s| {
        s.pending.is_none() && s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("Ask cancellation preserves session")?;
    handle.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Window)))
        .context("Ask explicit close prompt")?;
    handle.update(|s, tx| {
        s.pending = None;
        s.request_action(workspace::Action::KeepWindow, tx);
    });
    wait_visible(false)?;
    let code = std::process::Command::new(std::env::current_exe()?).status()?;
    anyhow::ensure!(code.success(), "Ask/Keep relaunch failed");
    wait_visible(true)?;
    wait_app(handle, move |s| {
        s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("Ask Keep reopens same session")?;
    handle.update(|s, _| {
        s.save_config(|c| {
            c.behavior.terminal_tab_close = Tab::ConfirmRunning;
            c.behavior.window_close = Window::QuitRodtty;
        });
        s.send(b"printf '\\033]133;C\\007\\033]133;A\\007'; printf 'POLICY_%s\\n' BLOCKED; read session_policy_answer\r".to_vec());
    });
    wait_app(handle, |s| {
        s.active()
            .unwrap()
            .screen()
            .contents()
            .contains("POLICY_BLOCKED")
            && s.active().unwrap().has_running_commands()
    })
    .context("public shell integration leaves builtin activity protected")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Tab(_))))
        .context("confirm-running protects builtin after public prompt marker")?;
    handle.update(|s, tx| {
        s.cancel_file_action();
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::CloseImmediately);
        s.dispatch(AppAction::Quit, tx);
    });
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Quit)))
        .context("explicit Quit running confirmation")?;
    wait_visible(true)?;
    handle.update(|s, _| s.cancel_file_action());
    wait_app(handle, |s| {
        s.pending.is_none() && s.config.behavior.window_close == Window::QuitRodtty
    })
    .context("Quit cancellation applied before native close")?;
    handle.close();
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Quit)))
        .context("native Quit policy confirmation")?;
    wait_visible(true)?;
    handle.update(|s, _| {
        s.cancel_file_action();
        s.send(vec![3]);
    });
    wait_app(handle, |s| !s.active().unwrap().has_running_commands())
        .context("interrupt returns to idle")?;

    let fixture = tempfile::tempdir()?;
    let active = fixture.path().join("active");
    let explorer = fixture.path().join("explorer");
    let specified = fixture.path().join("specified");
    for path in [&active, &explorer, &specified] {
        std::fs::create_dir(path)?;
    }
    let command = format!("cd {}\r", profile::quote(&active)?);
    handle.update(move |s, _| s.send(command.into_bytes()));
    let expected = active.clone();
    wait_app(handle, move |s| {
        s.active().unwrap().cwd() == expected && !s.active().unwrap().has_running_commands()
    })
    .context("active session directory change")?;
    let root = explorer.clone();
    handle.update(move |s, tx| {
        s.navigate(root);
        s.save_config(|c| {
            c.behavior.new_terminal_directory = Directory::Active;
            c.history = false;
            c.disabled = vec!["cat".into()];
        });
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 2
            && s.active().unwrap().cwd() == active
            && !s.active().unwrap().has_running_commands()
    })
    .context("active directory new tab")?;
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.new_terminal_directory = Directory::Explorer);
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 3
            && s.active().unwrap().cwd() == explorer
            && !s.active().unwrap().has_running_commands()
    })
    .context("explorer directory new tab")?;
    let expected = specified.clone();
    handle.update(move |s, tx| {
        s.save_config(|c| {
            c.behavior.new_terminal_directory = Directory::Specified;
            c.behavior.specified_directory = specified;
        });
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 4
            && s.active().unwrap().cwd() == expected
            && !s.active().unwrap().has_running_commands()
    })
    .context("specified directory new tab")?;
    let snapshot = initial_config.clone();
    wait_app(handle, move |s| {
        s.sessions[0].id() == first
            && s.sessions[0].config.history == snapshot.history
            && s.sessions[0].config.disabled == snapshot.disabled
            && !s.active().unwrap().config.history
            && s.active().unwrap().config.disabled == ["cat"]
    })
    .context("old and new command snapshots")?;
    // Confirm-running closes an idle new shell directly.
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::ConfirmRunning);
        s.dispatch(AppAction::EndTerminal, tx);
    });
    wait_app(handle, |s| s.sessions.len() == 3 && s.pending.is_none())
        .context("confirm-running closes idle tab")?;
    // Close-immediately also applies to a busy shell without changing Quit's rule.
    handle.update(|s, _| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::CloseImmediately);
        s.send(b"read policy_close_answer\r".to_vec());
    });
    wait_app(handle, |s| s.active().unwrap().has_running_commands())
        .context("new builtin activity")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, |s| s.sessions.len() == 2 && s.pending.is_none())
        .context("immediate closes busy tab")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, move |s| {
        s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("remaining new tab close")?;
    let command = format!("cd {}\r", profile::quote(&initial_cwd)?);
    let expected = initial_cwd.clone();
    handle.update(move |s, _| {
        s.send(command.into_bytes());
        s.save_config(|c| {
            c.behavior = Default::default();
            c.history = initial_config.history;
            c.disabled = initial_config.disabled;
        });
        s.navigate(initial_cwd);
    });
    wait_app(handle, move |s| {
        s.active().unwrap().cwd() == expected && !s.active().unwrap().has_running_commands()
    })
    .context("original directory and idle restore")?;
    Ok(serde_json::json!({
        "ask_native_close_cancel":true, "ask_keep_same_session_relaunch":true,
        "quit_running_confirmation":true, "quit_policy_native_close_confirmation":true,
        "public_prompt_markers_preserve_running_confirmation":true,
        "confirm_running_busy_tab_after_public_marker":true,
        "always_confirm_idle_tab":true, "confirm_running_idle_tab_closes":true,
        "close_immediately_busy_tab":true, "new_tab_directory_choices":3,
        "running_session_configuration_snapshot":true, "cancel_preserved_session":true,
        "explicit_quit_after_check":true, "dirty_close_save_then_reopen":true
    }))
}

pub(crate) fn wait_app(
    handle: &rune::AppHandle<State>,
    condition: impl Fn(&State) -> bool + Send + Sync + 'static,
) -> Result<()> {
    let condition = Arc::new(condition);
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(15) {
        let (tx, rx) = std::sync::mpsc::channel();
        let condition = condition.clone();
        handle.update(move |s, _| {
            let _ = tx.send(condition(s));
        });
        if rx.recv_timeout(std::time::Duration::from_secs(5))? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    anyhow::bail!("workspace check timed out")
}
