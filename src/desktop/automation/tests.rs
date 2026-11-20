use super::*;
use crate::automation::grants::{ClientGrant, Scope, Status};

fn state() -> (tempfile::TempDir, State) {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.shell = Some(crate::test_support::shell_binary().unwrap());
    (root, state)
}

pub(super) fn tests_state_for_restore() -> (tempfile::TempDir, State) {
    state()
}

fn grant(workspaces: &[String]) -> ClientGrant {
    ClientGrant {
        id: "cl_test".into(),
        name: "test".into(),
        status: Status::Approved,
        scopes: Scope::ALL.to_vec(),
        workspaces: workspaces.to_vec(),
        requested_at: 0,
        updated_at: 0,
    }
}

fn wait(state: &State, index: usize, needle: &str) {
    state.sessions[index]
        .wait_for(needle, Duration::from_secs(15))
        .unwrap();
}

#[test]
fn agent_text_and_keys_reach_only_the_named_pty_and_owner_takeover_stops_them() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let a = state
        .auto_terminal_create(&g, &json!({"cwd": root.path()}))
        .unwrap();
    let b = state
        .auto_terminal_create(&g, &json!({"cwd": root.path()}))
        .unwrap();
    let (a, b) = (
        a["id"].as_str().unwrap().to_owned(),
        b["id"].as_str().unwrap().to_owned(),
    );
    wait(&state, 0, "rodtty:");
    wait(&state, 1, "rodtty:");
    // The owner looks at B; the agent types into A.
    state.active = 1;
    state
        .auto_terminal_text(
            &g,
            &json!({"terminal": a, "text": "printf 'AGENT_%s\\n' ONLY"}),
        )
        .unwrap();
    state
        .auto_terminal_keys(&g, &json!({"terminal": a, "keys": ["Enter"]}))
        .unwrap();
    wait(&state, 0, "AGENT_ONLY");
    assert!(
        !state.sessions[1].screen().contents().contains("AGENT"),
        "B got none of A's input"
    );
    assert_eq!(
        state.automation.agent_focus.get(&a).map(String::as_str),
        Some("cl_test")
    );

    // The owner types into A while the agent drives it: the owner takes over.
    state.active = 0;
    state.automation.inputs.insert(
        ("cl_test".into(), a.clone()),
        VirtualInput {
            buttons: vec!["left".into()],
            pointer: Some((1.0, 1.0)),
            ..Default::default()
        },
    );
    let epoch = state.automation.shared.epoch(&a);
    state.send(b"printf 'OWNER_%s\\n' TYPED\r".to_vec());
    wait(&state, 0, "OWNER_TYPED");
    assert!(state.automation.takeovers.contains(&a));
    assert!(
        state.automation.inputs.is_empty(),
        "held virtual buttons were released"
    );
    assert_ne!(
        state.automation.shared.epoch(&a),
        epoch,
        "queued agent input was cancelled"
    );
    let refused = state
        .auto_terminal_text(&g, &json!({"terminal": a, "text": "x"}))
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::OwnerTakeover);
    assert_eq!(
        state.agent_char(&g, &a, 'x', epoch).unwrap_err().code,
        ErrorCode::Cancelled
    );
    // B is unaffected and still takes agent input.
    state
        .auto_terminal_text(&g, &json!({"terminal": b, "text": "echo B_OK\r"}))
        .unwrap();
    wait(&state, 1, "B_OK");
    // Returning the target resumes agent input.
    state.automation_return_target();
    state
        .auto_terminal_text(&g, &json!({"terminal": a, "text": "echo BACK_TO_AGENT\r"}))
        .unwrap();
    wait(&state, 0, "BACK_TO_AGENT");
}

#[test]
fn approval_for_one_workspace_does_not_expose_another() {
    let (root, mut state) = state();
    let other = crate::test_support::tempdir().unwrap();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let error = state
        .auto_terminal_create(&g, &json!({"cwd": other.path()}))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::WorkspaceDenied);
    // The owner's own session in the other folder stays invisible.
    state.start_tab(other.path().into());
    let list = state.auto_terminal_list(&g).unwrap();
    assert_eq!(list["terminals"].as_array().unwrap().len(), 0);
    let owner = state.terminal_target(&state.sessions[0]);
    assert_eq!(
        state
            .auto_terminal_screen(&g, &json!({"terminal": owner}))
            .unwrap_err()
            .code,
        ErrorCode::WorkspaceDenied
    );
    let path = other.path().join("secret.txt");
    std::fs::write(&path, "x").unwrap();
    assert_eq!(
        state.authorize_path(&g, &path).unwrap_err().code,
        ErrorCode::WorkspaceDenied
    );
}

#[test]
fn command_results_record_success_failure_interrupts_and_builtins() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let t = state
        .auto_terminal_create(&g, &json!({"cwd": root.path()}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    wait(&state, 0, "rodtty:");
    let run = |state: &mut State, line: &str, count: usize| {
        state
            .auto_terminal_text(&g, &json!({"terminal": t, "text": format!("{line}\r")}))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while state.sessions[0]
            .commands()
            .iter()
            .filter(|c| c.ended.is_some())
            .count()
            < count
        {
            assert!(Instant::now() < deadline, "command {line} never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    run(&mut state, "echo ok", 1);
    run(&mut state, "true | false", 2);
    run(&mut state, "cd /", 3);
    state
        .auto_terminal_text(&g, &json!({"terminal": t, "text": "sleep 30\r"}))
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    state
        .auto_terminal_interrupt(&g, &json!({"terminal": t}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while state.sessions[0]
        .commands()
        .iter()
        .filter(|c| c.ended.is_some())
        .count()
        < 4
    {
        assert!(Instant::now() < deadline, "interrupt never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
    let commands = state.sessions[0].commands();
    use crate::session::CommandOutcome::*;
    let outcomes: Vec<_> = commands.iter().map(|c| c.outcome()).collect();
    assert_eq!(
        outcomes,
        [Succeeded, Failed, Succeeded, Interrupted],
        "{commands:?}"
    );
    let (output, complete) = state.sessions[0].command_output(commands[0].id).unwrap();
    assert!(complete);
    assert_eq!(output.trim(), "ok");
    let screen = state
        .auto_terminal_screen(&g, &json!({"terminal": t}))
        .unwrap();
    assert_eq!(screen["recent_commands"][0]["outcome"], "interrupted");
}

#[test]
fn multiline_paste_waits_for_confirmation_and_never_reaches_another_session() {
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    wait(&state, 1, "rodtty:");
    state.save_config(|c| c.behavior.confirm_multiline_paste = true);
    state.active = 0;
    // An app with bracketed paste reads exactly what arrives.
    state.sessions[0]
        .send(b"stty raw -echo; printf '\\033[?2004hREADY\\n'; head -c 40 | od -c | head -3; stty sane\r".to_vec())
        .unwrap();
    wait(&state, 0, "READY");
    let before = state.sessions[0].state.lock().unwrap().output.end();
    state.paste("one\r\ntwo\nthree\n");
    std::thread::sleep(Duration::from_millis(300));
    assert!(matches!(state.pending, Some(Pending::Paste { .. })));
    assert_eq!(
        state.sessions[0].state.lock().unwrap().output.end(),
        before,
        "no bytes before confirmation"
    );
    // Cancel delivers nothing.
    state.pending = None;
    // Approval delivers one paste, bracketed, to the intended session.
    state.paste("one\r\ntwo\nthree\n");
    state.confirm_paste();
    state.sessions[0].send(b"Z".repeat(40)).unwrap();
    wait(&state, 0, "0000040");
    let screen = state.sessions[0].screen().contents();
    assert!(
        screen.contains("[   2   0   0   ~"),
        "bracketed paste start arrived once: {screen}"
    );
    // A focus change before confirmation discards the text.
    state.paste("x\ny\n");
    state.active = 1;
    let other = state.sessions[1].state.lock().unwrap().output.end();
    state.confirm_paste();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        state.sessions[1].state.lock().unwrap().output.end(),
        other,
        "the other session got nothing"
    );
    assert!(state.message.contains("discarded"));
    // Single lines are never held.
    state.paste("echo single");
    assert!(state.pending.is_none());
}

#[test]
fn two_runs_compare_with_their_exit_statuses() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let t = state
        .auto_terminal_create(&g, &json!({"cwd": root.path()}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    wait(&state, 0, "rodtty:");
    std::fs::write(root.path().join("state"), "1").unwrap();
    let build = "if [ \"$(cat state)\" = 1 ]; then printf '\\033[31mFAIL\\033[0m step b\\n'; false; else printf 'ok step b\\n'; fi";
    for (n, _) in [(1, ()), (2, ())] {
        state
            .auto_terminal_text(
                &g,
                &json!({"terminal": t, "text": format!("printf 'ok step a\\n'; {build}\r")}),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while state.sessions[0]
            .commands()
            .iter()
            .filter(|c| c.ended.is_some())
            .count()
            < n
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::write(root.path().join("state"), "2").unwrap();
    }
    let c = state
        .auto_terminal_compare(&g, &json!({"terminal": t}))
        .unwrap();
    assert_eq!(c["before"]["exit_status"], 1);
    assert_eq!(c["after"]["exit_status"], 0);
    assert_eq!(
        (c["added"].as_u64(), c["removed"].as_u64()),
        (Some(1), Some(1)),
        "{c}"
    );
    assert!(c["unified"].as_str().unwrap().contains("-FAIL step b"));
}

#[test]
fn the_same_relative_reference_printed_in_two_directories_opens_each_ones_file() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    for name in ["one", "two"] {
        std::fs::create_dir_all(root.path().join(name).join("src")).unwrap();
        std::fs::write(
            root.path().join(name).join("src/a.rs"),
            format!("{name}\nsecond line\n"),
        )
        .unwrap();
    }
    let t = state
        .auto_terminal_create(&g, &json!({"cwd": root.path()}))
        .unwrap();
    let t = t["id"].as_str().unwrap().to_owned();
    wait(&state, 0, "rodtty:");
    state.active = 0;
    let run = |state: &mut State, line: &str, marker: &str| {
        state
            .auto_terminal_text(&g, &json!({"terminal": t, "text": line}))
            .unwrap();
        state
            .auto_terminal_keys(&g, &json!({"terminal": t, "keys": ["Enter"]}))
            .unwrap();
        wait(state, 0, marker);
        std::thread::sleep(Duration::from_millis(300));
    };
    run(&mut state, "cd one; echo IN_ONE", "\nIN_ONE");
    run(
        &mut state,
        "printf 'src/a.rs:2:3\\n'; echo ONE_DONE",
        "\nONE_DONE",
    );
    run(&mut state, "cd ../two; echo IN_TWO", "\nIN_TWO");
    run(
        &mut state,
        "printf 'src/a.rs:2:3\\n'; echo TWO_DONE",
        "\nTWO_DONE",
    );
    run(&mut state, "cd ..; echo BACK", "\nBACK");
    let rows: Vec<u16> = state.sessions[0].with_screen(|screen| {
        screen
            .rows(0, screen.size().1)
            .enumerate()
            .filter(|(_, r)| r.trim_end() == "src/a.rs:2:3")
            .map(|(i, _)| i as u16)
            .collect()
    });
    assert_eq!(rows.len(), 2, "{}", state.sessions[0].screen().contents());
    for (row, name) in rows.iter().zip(["one", "two"]) {
        assert!(state.open_terminal_reference(*row, 4, &mut Tx::default()));
        let opened = state
            .workspace
            .latest_activation
            .clone()
            .expect(&state.message);
        assert_eq!(
            dunce::canonicalize(&opened).unwrap(),
            dunce::canonicalize(root.path().join(name).join("src/a.rs")).unwrap(),
            "{}",
            state.message
        );
    }
    // A prompt or blank cell is no reference.
    assert!(!state.open_terminal_reference(rows[0], 30, &mut Tx::default()));
}

#[test]
fn previous_and_next_command_scroll_to_each_prompt_and_background_long_commands_notify_once() {
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    let finished = |state: &State| {
        state.sessions[0]
            .commands()
            .iter()
            .filter(|c| c.ended.is_some())
            .count()
    };
    let run = |state: &mut State, line: &str| {
        let before = finished(state);
        state.sessions[0]
            .send(format!("{line}\r").into_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while finished(state) == before {
            assert!(Instant::now() < deadline, "{line} never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    run(&mut state, "echo FIRST_RUN");
    // Enough output to push the first prompt off the visible screen.
    run(&mut state, "seq 1 80");
    run(&mut state, "false");
    let commands = state.sessions[0].commands();
    assert!(
        commands.iter().all(|c| c.prompt_line.is_some()),
        "{commands:?}"
    );
    state.step_command(true);
    assert!(
        state.message.contains("Command 3 of 3: failed (exit 1)"),
        "{}",
        state.message
    );
    state.step_command(true);
    state.step_command(true);
    assert!(
        state.message.contains("Command 1 of 3: succeeded"),
        "{}",
        state.message
    );
    let top = state.sessions[0].with_screen(|s| s.rows(0, s.size().1).next().unwrap());
    assert!(
        top.contains("echo FIRST_RUN"),
        "the first prompt is at the top: {top}"
    );
    state.step_command(false);
    state.step_command(false);
    state.step_command(false);
    assert!(state.message.contains("live screen"), "{}", state.message);
    assert_eq!(state.sessions[0].with_screen(vt100::Screen::scrollback), 0);

    // Notifications: off by default, then only for long, backgrounded work.
    state.config.behavior.command_notification_seconds = 1;
    state.window_focused = false;
    run(&mut state, "sleep 1.2");
    state.poll_command_notifications();
    assert_eq!(state.notifications_sent, 0, "disabled by default");
    state.config.behavior.command_notifications = true;
    run(&mut state, "sleep 1.2");
    state.poll_command_notifications();
    state.poll_command_notifications();
    assert_eq!(state.notifications_sent, 1, "one notification per command");
    run(&mut state, "true");
    state.poll_command_notifications();
    assert_eq!(state.notifications_sent, 1, "a short command is quiet");
    state.window_focused = true;
    run(&mut state, "sleep 1.2");
    state.poll_command_notifications();
    assert_eq!(state.notifications_sent, 1, "foreground work is quiet");
}

#[test]
fn filtering_a_live_session_lists_incoming_matches_jumps_and_restores_the_terminal() {
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    state.sessions[0]
        .send(b"seq 1 200 | sed 's/^/req=/'; echo SEQ_DONE\r".to_vec())
        .unwrap();
    wait(&state, 0, "\nSEQ_DONE");
    let before = state.sessions[0].screen().contents();
    state.open_log_filter(&mut Tx::default());
    state.log_filter.query = TextEdit::new("req=7");
    state.poll_log_filter();
    // req=7 and req=70..79.
    assert_eq!(state.log_filter.total, 11, "{:?}", state.log_filter.shown);
    state.log_filter.regex = true;
    state.log_filter.query = TextEdit::new(r"^req=1\d7$");
    state.log_filter.refresh();
    assert_eq!(state.log_filter.total, 10);
    state.log_filter.query = TextEdit::new("req=(");
    state.log_filter.refresh();
    assert!(state.log_filter.error.is_some());
    // New output appends; nonmatching lines stay out.
    state.log_filter.regex = false;
    state.log_filter.query = TextEdit::new("LATE_");
    state.sessions[0]
        .send(b"echo LATE_ONE; echo other; echo LATE_TWO\r".to_vec())
        .unwrap();
    wait(&state, 0, "\nLATE_TWO");
    std::thread::sleep(Duration::from_millis(100));
    state.poll_log_filter();
    let lines: Vec<&str> = state
        .log_filter
        .shown
        .iter()
        .map(|(l, _)| l.as_str())
        .collect();
    assert!(lines.ends_with(&["LATE_ONE", "LATE_TWO"]), "{lines:?}");
    // Jump to an early line: the view closes and the terminal scrolls to it.
    state.log_filter.query = TextEdit::new("req=3");
    state.poll_log_filter();
    let (line, after) = state
        .log_filter
        .shown
        .iter()
        .find(|(l, _)| l == "req=3")
        .cloned()
        .unwrap();
    state.jump_to_filtered_line(&line, after, &mut Tx::default());
    assert!(!state.log_filter.open);
    let shown = state.sessions[0].screen().contents();
    assert!(shown.lines().any(|l| l == "req=3"), "{shown}");
    // Open and close again: the scroll position the terminal had returns.
    let scrolled = state.sessions[0].with_screen(vt100::Screen::scrollback);
    assert!(scrolled > 0);
    state.open_log_filter(&mut Tx::default());
    state.close_log_filter(&mut Tx::default());
    assert_eq!(
        state.sessions[0].with_screen(vt100::Screen::scrollback),
        scrolled
    );
    state.sessions[0].with_screen_mut(|s| s.set_scrollback(0));
    assert!(
        state.sessions[0]
            .screen()
            .contents()
            .contains(before.lines().last().unwrap())
    );
}

#[test]
fn a_pinned_failing_run_compares_with_its_rerun_and_clearing_leaves_the_terminal_intact() {
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    let finished = |state: &State| {
        state.sessions[0]
            .commands()
            .iter()
            .filter(|c| c.ended.is_some())
            .count()
    };
    let run = |state: &mut State, line: &str| {
        let before = finished(state);
        state.sessions[0]
            .send(format!("{line}\r").into_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while finished(state) == before {
            assert!(Instant::now() < deadline, "{line} never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // A failing build with colors and a line wider than the terminal.
    let wide = "w".repeat(300);
    run(
        &mut state,
        &format!("printf '\\033[32mok\\033[0m a\\nFAIL b\\n{wide}\\n'; false"),
    );
    state.pin_last_run();
    assert!(
        state.message.contains("failed (exit 1)"),
        "{}",
        state.message
    );
    run(&mut state, &format!("printf 'ok a\\nok b\\n{wide}\\n'"));
    state.compare_with_pinned();
    let c = state.run_comparison.as_ref().expect(&state.message);
    assert_eq!(
        (c.added, c.removed),
        (1, 1),
        "colors and wraps make no changes"
    );
    assert!(c.before.contains("failed (exit 1)") && c.after.contains("succeeded"));
    assert!(!c.before.contains("incomplete"));
    let hunk = &state.run_diff.files()[0].hunks[0];
    assert!(hunk.lines.iter().any(|l| l.text == "FAIL b"));
    // A run longer than retention is labeled incomplete.
    run(&mut state, "seq 1 250000");
    state.compare_with_pinned();
    let c = state.run_comparison.as_ref().unwrap();
    assert!(c.after.contains("incomplete"), "{}", c.after);
    let before = state.sessions[0].screen().contents();
    let mut runtime = crate::desktop::tests::runtime_for(state);
    runtime.frame(1.0);
    assert!(runtime.find("run-diff").is_some());
    runtime.update(|s, _| s.clear_pinned_run());
    runtime.frame(1.0);
    assert!(runtime.find("run-comparison").is_none());
    assert!(runtime.state.pinned_run.is_none());
    assert_eq!(runtime.state.sessions[0].screen().contents(), before);
}

#[test]
fn split_panes_keep_their_own_sizes_input_close_policy_and_restore_both_orientations() {
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    state.split_active(true);
    assert_eq!(state.sessions.len(), 2);
    wait(&state, 1, "rodtty:");
    let (first, second) = (state.sessions[0].id(), state.sessions[1].id());
    assert_eq!(state.active, 1, "the new pane has focus");
    let mut runtime = crate::desktop::tests::runtime_for(state);
    for _ in 0..4 {
        runtime.frame(1.0);
    }
    let size = |rt: &rune::Runtime<State>, i: usize| rt.state.sessions[i].size();
    let (a, b) = (size(&runtime, 0), size(&runtime, 1));
    assert!(
        a.1 < 100 && b.1 < 100 && a.1 > 20,
        "side-by-side panes are each narrower: {a:?} {b:?}"
    );
    // Drag the divider: the first pane grows, the second shrinks.
    runtime.update(|s, _| s.set_split_share(first, 0.7));
    for _ in 0..4 {
        runtime.frame(1.0);
    }
    let (a2, b2) = (size(&runtime, 0), size(&runtime, 1));
    assert!(a2.1 > a.1 && b2.1 < b.1, "{a2:?} {b2:?}");
    // Each app sees its own size; typing reaches only the focused pane.
    runtime.update(|s, _| {
        s.sessions[1]
            .send(b"stty size; echo ONLY_SECOND\r".to_vec())
            .unwrap();
    });
    runtime.state.sessions[1]
        .wait_for("ONLY_SECOND", Duration::from_secs(10))
        .unwrap();
    assert!(
        runtime.state.sessions[1]
            .screen()
            .contents()
            .contains(&format!("{} {}", b2.0, b2.1))
    );
    assert!(
        !runtime.state.sessions[0]
            .screen()
            .contents()
            .contains("ONLY_SECOND")
    );
    runtime.update(|s, _| s.paste("pasted-here"));
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        runtime.state.sessions[1]
            .screen()
            .contents()
            .contains("pasted-here")
    );
    assert!(
        !runtime.state.sessions[0]
            .screen()
            .contents()
            .contains("pasted-here")
    );
    runtime.update(|s, _| s.focus_other_pane());
    assert_eq!(runtime.state.active, 0);
    // The tab strip shows one tab for both panes.
    runtime.frame(1.0);
    assert!(runtime.find("tab-0").is_some() && runtime.find("tab-1").is_none());
    // Saved layouts keep the orientation, share and positions.
    let saved = runtime.state.saved_splits();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].side_by_side && (saved[0].share - 0.7).abs() < 1e-3);
    // Cold restoration rebuilds the split over fresh shells.
    let (root2, mut cold) = tests_state_for_restore();
    cold.start_tab(root2.path().into());
    cold.start_tab(root2.path().into());
    let down = crate::work::SavedSplit {
        first: 0,
        second: 1,
        side_by_side: false,
        share: 0.4,
    };
    cold.restore_splits(&[down], 2);
    assert!(
        cold.split_of(cold.sessions[0].id())
            .is_some_and(|s| !s.side_by_side)
    );
    cold.restore_splits(&[down], 3);
    // A running command in the focused pane: cancel keeps both; confirm keeps the other.
    runtime.update(|s, _| {
        s.sessions[0].send(b"sleep 30\r".to_vec()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(600));
    runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    assert!(matches!(runtime.state.pending, Some(Pending::Tab(id)) if id == first));
    runtime.update(|s, _| s.pending = None);
    assert_eq!(
        runtime.state.sessions.len(),
        2,
        "cancel keeps both sessions"
    );
    runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    runtime.update(|s, tx| s.confirm(tx));
    assert_eq!(runtime.state.sessions.len(), 1);
    assert_eq!(
        runtime.state.sessions[0].id(),
        second,
        "the other pane keeps running"
    );
    assert!(runtime.state.splits.is_empty(), "and fills the tab");
    // Compact windows: both panes stay reachable through the switcher.
    runtime.update(|s, _| {
        s.split_active(false);
    });
    runtime.state.sessions[1]
        .wait_for("rodtty:", Duration::from_secs(10))
        .unwrap();
    let mut compact = rune::Runtime::new(
        std::mem::replace(&mut runtime.state, State::with_root(root.path().into())),
        view,
        (480.0, 600.0),
        rune::Env::new(true, 1.0),
    );
    compact.frame(0.0);
    compact.frame(1.0);
    assert!(compact.find("split-switch").is_some());
    let before = compact.state.active;
    compact.update(|s, _| s.focus_other_pane());
    assert_ne!(compact.state.active, before);
}

#[test]
fn remote_devices_pair_attach_read_type_resize_follow_control_and_lose_access_when_revoked_or_quit()
{
    use std::io::{Read, Write};
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    state.sessions[0]
        .send(b"for i in 1 2 3; do echo WATCH_$i; done\r".to_vec())
        .unwrap();
    wait(&state, 0, "WATCH_3");
    let (dispatch, rx) = crate::remote::channel_dispatch();
    let server = crate::remote::start("127.0.0.1:0".parse().unwrap(), dispatch).unwrap();
    let addr = server.addr;
    state.remote.server = Some(server);
    let req = |state: &mut State,
               method: &str,
               path: &str,
               token: Option<&str>,
               body: serde_json::Value|
     -> (u16, serde_json::Value) {
        let (method, path, token, body) = (
            method.to_owned(),
            path.to_owned(),
            token.map(str::to_owned),
            body.to_string(),
        );
        let client = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).ok()?;
            let auth = token
                .map(|t| format!("Authorization: Bearer {t}\r\n"))
                .unwrap_or_default();
            write!(
                s,
                "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .ok()?;
            let mut out = String::new();
            s.read_to_string(&mut out).ok()?;
            let status = out.split_whitespace().nth(1)?.parse().ok()?;
            let json = out.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
            Some((
                status,
                serde_json::from_str(json).unwrap_or(serde_json::Value::Null),
            ))
        });
        loop {
            while let Ok((call, reply)) = rx.recv_timeout(Duration::from_millis(10)) {
                let _ = reply.send(state.remote_call(call));
            }
            if client.is_finished() {
                return client
                    .join()
                    .unwrap()
                    .unwrap_or((0, serde_json::Value::Null));
            }
        }
    };
    let none = serde_json::Value::Null;
    // Unauthenticated: no list, output or input.
    assert_eq!(
        req(&mut state, "GET", "/api/sessions", None, none.clone()).0,
        401
    );
    assert_eq!(
        req(
            &mut state,
            "GET",
            "/api/sessions",
            Some("forged"),
            none.clone()
        )
        .0,
        401
    );
    let id = state.sessions[0].id();
    assert_eq!(
        req(
            &mut state,
            "POST",
            &format!("/api/sessions/{id}/input"),
            Some("forged"),
            json!({"text": "x"})
        )
        .0,
        401
    );
    // Pair.
    let code = state.remote.pairing.new_code().unwrap();
    let (status, paired) = req(
        &mut state,
        "POST",
        "/api/pair",
        None,
        json!({"code": code, "name": "phone"}),
    );
    assert_eq!(status, 200, "{paired}");
    let token = paired["token"].as_str().unwrap().to_owned();
    let t = Some(token.as_str());
    let (_, list) = req(&mut state, "GET", "/api/sessions", t, none.clone());
    assert_eq!(list["sessions"][0]["id"], id);
    // Attach: recent output arrives.
    let (_, read) = req(
        &mut state,
        "GET",
        &format!("/api/sessions/{id}/output?cursor=0"),
        t,
        none.clone(),
    );
    let decode = |b64: &str| {
        let t = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = 0u32;
        let mut n = 0;
        let mut out = Vec::new();
        for c in b64.bytes().filter(|c| *c != b'=') {
            bits = (bits << 6) | t.iter().position(|x| *x == c).unwrap() as u32;
            n += 6;
            if n >= 8 {
                n -= 8;
                out.push((bits >> n) as u8);
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    assert!(decode(read["data"].as_str().unwrap()).contains("WATCH_3"));
    let cursor = read["next"].as_u64().unwrap();
    // Typing needs control; the device takes it.
    let input = format!("/api/sessions/{id}/input");
    assert_eq!(
        req(
            &mut state,
            "POST",
            &input,
            t,
            json!({"text": "echo REMOTE_TYPED\r"})
        )
        .0,
        409
    );
    assert_eq!(
        req(
            &mut state,
            "POST",
            &format!("/api/sessions/{id}/control"),
            t,
            json!({})
        )
        .0,
        200
    );
    assert_eq!(
        req(
            &mut state,
            "POST",
            &input,
            t,
            json!({"text": "echo REMOTE_TYPED\r"})
        )
        .0,
        200
    );
    wait(&state, 0, "\nREMOTE_TYPED");
    // Disconnect and reconnect from the cursor: continued output.
    let (_, more) = req(
        &mut state,
        "GET",
        &format!("/api/sessions/{id}/output?cursor={cursor}"),
        t,
        none.clone(),
    );
    assert!(decode(more["data"].as_str().unwrap()).contains("REMOTE_TYPED"));
    assert!(state.remote_indicator().unwrap().contains("1 device"));
    // Resize from the device.
    assert_eq!(
        req(
            &mut state,
            "POST",
            &format!("/api/sessions/{id}/resize"),
            t,
            json!({"rows": 20, "cols": 60})
        )
        .0,
        200
    );
    assert_eq!(state.sessions[0].size(), (20, 60));
    // The owner types locally: the desktop takes control back.
    state.active = 0;
    state.send(b"echo LOCAL\r".to_vec());
    assert_eq!(
        req(&mut state, "POST", &input, t, json!({"text": "x"})).0,
        409
    );
    // Revoked: nothing.
    state.revoke_remote_devices();
    assert_eq!(
        req(&mut state, "GET", "/api/sessions", t, none.clone()).0,
        401
    );
    assert_eq!(
        req(
            &mut state,
            "GET",
            &format!("/api/sessions/{id}/output?cursor=0"),
            t,
            none
        )
        .0,
        401
    );
    // Quit ends the sessions and closes the listener.
    state.pending = Some(Pending::Quit);
    state.confirm(&mut Tx::default());
    assert!(state.sessions.is_empty() && state.remote.server.is_none());
    std::thread::sleep(Duration::from_millis(300));
    let refused = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500))
        .map(|mut s| {
            let _ = write!(s, "GET / HTTP/1.1\r\n\r\n");
            let mut out = String::new();
            s.read_to_string(&mut out)
                .map(|_| out.is_empty())
                .unwrap_or(true)
        })
        .unwrap_or(true);
    assert!(refused, "the port no longer serves");
}

#[test]
fn agent_status_follows_its_own_session_rejects_bad_reports_expires_and_leaves_close_protection_alone()
 {
    use crate::session::AgentState::*;
    let (root, mut state) = state();
    state.start_tab(root.path().into());
    state.start_tab(root.path().into());
    wait(&state, 0, "rodtty:");
    wait(&state, 1, "rodtty:");
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/agent-status/picobu-sample.sh");
    state.sessions[0]
        .send(format!("PICOBU_STEP=1 sh '{}'\r", sample.display()).into_bytes())
        .unwrap();
    let until = |state: &State, i: usize, want: Option<crate::session::AgentState>| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while state.sessions[i].agent_status() != want {
            assert!(
                Instant::now() < deadline,
                "{want:?}: {:?}",
                state.sessions[i].agent_status()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    until(&state, 0, Some(Working));
    until(&state, 0, Some(Waiting));
    assert_eq!(
        state.sessions[1].agent_status(),
        None,
        "the other terminal keeps its own status"
    );
    // Visible on the tab with an accessible label.
    let mut runtime = crate::desktop::tests::runtime_for(state);
    runtime.frame(1.0);
    let label = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("tab-0"))
        .and_then(|e| e.label)
        .unwrap_or_default();
    assert!(label.contains("Agent waiting for input"), "{label}");
    let mut state = std::mem::replace(&mut runtime.state, State::with_root(root.path().into()));
    // Session close protection is the shell's, unchanged by the report.
    let id = state.sessions[0].id();
    state.dispatch(AppAction::CloseTerminal(id), &mut Tx::default());
    assert!(matches!(state.pending, Some(Pending::Tab(_))));
    state.pending = None;
    state.sessions[0].send(b"y\r".to_vec()).unwrap();
    until(&state, 0, Some(Working));
    until(&state, 0, Some(Finished));
    until(&state, 0, None);
    // Bad reports from the other session change nothing.
    let token = |i: usize| {
        state.sessions[i]
            .send(b"echo TOKEN=$RODTTY_AGENT_STATUS_TOKEN\r".to_vec())
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        state.sessions[i]
            .screen()
            .contents()
            .lines()
            .filter_map(|l| l.strip_prefix("TOKEN="))
            .last()
            .unwrap()
            .to_owned()
    };
    let (a, b) = (token(0), token(1));
    assert_ne!(a, b);
    let rejected = state.sessions[1].rejected_status_updates();
    let send = |i: usize, body: &str| {
        state.sessions[i]
            .send(format!("printf '\\033]777;rodtty-status;{body}\\007'\r").into_bytes())
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
    };
    send(1, &format!("1;{a};1;working"));
    send(1, &format!("2;{b};1;working"));
    send(1, &format!("1;{b};1;dancing"));
    send(1, &format!("1;{b};1;working;99999"));
    send(1, &format!("1;{b};1;working;{}", "x".repeat(300)));
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.sessions[1].rejected_status_updates() < rejected + 5 {
        assert!(
            Instant::now() < deadline,
            "{}",
            state.sessions[1].rejected_status_updates()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(state.sessions[1].agent_status(), None);
    // Out of order within one command, then expiry to Unknown.
    state.sessions[1]
        .send(format!("printf '\\033]777;rodtty-status;1;{b};5;waiting;1\\007\\033]777;rodtty-status;1;{b};4;working\\007'; sleep 3\r").into_bytes())
        .unwrap();
    until(&state, 1, Some(Waiting));
    until(&state, 1, Some(Unknown));
    assert_eq!(state.sessions[0].agent_status(), None);
}
