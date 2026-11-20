use super::*;
use crate::automation::grants::{ClientGrant, Scope, Status};

fn state() -> (tempfile::TempDir, State) {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    state.shell = Some(crate::test_support::shell_binary().unwrap());
    (root, state)
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
    state.sessions[index].wait_for(needle, Duration::from_secs(15)).unwrap();
}

#[test]
fn agent_text_and_keys_reach_only_the_named_pty_and_owner_takeover_stops_them() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let a = state.auto_terminal_create(&g, &json!({"cwd": root.path()})).unwrap();
    let b = state.auto_terminal_create(&g, &json!({"cwd": root.path()})).unwrap();
    let (a, b) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());
    wait(&state, 0, "rodtty:");
    wait(&state, 1, "rodtty:");
    // The owner looks at B; the agent types into A.
    state.active = 1;
    state.auto_terminal_text(&g, &json!({"terminal": a, "text": "printf 'AGENT_%s\\n' ONLY"})).unwrap();
    state.auto_terminal_keys(&g, &json!({"terminal": a, "keys": ["Enter"]})).unwrap();
    wait(&state, 0, "AGENT_ONLY");
    assert!(!state.sessions[1].screen().contents().contains("AGENT"), "B got none of A's input");
    assert_eq!(state.automation.agent_focus.get(&a).map(String::as_str), Some("cl_test"));

    // The owner types into A while the agent drives it: the owner takes over.
    state.active = 0;
    state.automation.inputs.insert(("cl_test".into(), a.clone()), VirtualInput { buttons: vec!["left".into()], pointer: Some((1.0, 1.0)), ..Default::default() });
    let epoch = state.automation.shared.epoch(&a);
    state.send(b"printf 'OWNER_%s\\n' TYPED\r".to_vec());
    wait(&state, 0, "OWNER_TYPED");
    assert!(state.automation.takeovers.contains(&a));
    assert!(state.automation.inputs.is_empty(), "held virtual buttons were released");
    assert_ne!(state.automation.shared.epoch(&a), epoch, "queued agent input was cancelled");
    let refused = state.auto_terminal_text(&g, &json!({"terminal": a, "text": "x"})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::OwnerTakeover);
    assert_eq!(state.agent_char(&g, &a, 'x', epoch).unwrap_err().code, ErrorCode::Cancelled);
    // B is unaffected and still takes agent input.
    state.auto_terminal_text(&g, &json!({"terminal": b, "text": "echo B_OK\r"})).unwrap();
    wait(&state, 1, "B_OK");
    // Returning the target resumes agent input.
    state.automation_return_target();
    state.auto_terminal_text(&g, &json!({"terminal": a, "text": "echo BACK_TO_AGENT\r"})).unwrap();
    wait(&state, 0, "BACK_TO_AGENT");
}

#[test]
fn approval_for_one_workspace_does_not_expose_another() {
    let (root, mut state) = state();
    let other = crate::test_support::tempdir().unwrap();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let error = state.auto_terminal_create(&g, &json!({"cwd": other.path()})).unwrap_err();
    assert_eq!(error.code, ErrorCode::WorkspaceDenied);
    // The owner's own session in the other folder stays invisible.
    state.start_tab(other.path().into());
    let list = state.auto_terminal_list(&g).unwrap();
    assert_eq!(list["terminals"].as_array().unwrap().len(), 0);
    let owner = state.terminal_target(&state.sessions[0]);
    assert_eq!(state.auto_terminal_screen(&g, &json!({"terminal": owner})).unwrap_err().code, ErrorCode::WorkspaceDenied);
    let path = other.path().join("secret.txt");
    std::fs::write(&path, "x").unwrap();
    assert_eq!(state.authorize_path(&g, &path).unwrap_err().code, ErrorCode::WorkspaceDenied);
}

#[test]
fn command_results_record_success_failure_interrupts_and_builtins() {
    let (root, mut state) = state();
    let (workspace, _) = state.workspace_of(root.path());
    let g = grant(&[workspace]);
    let t = state.auto_terminal_create(&g, &json!({"cwd": root.path()})).unwrap()["id"].as_str().unwrap().to_owned();
    wait(&state, 0, "rodtty:");
    let run = |state: &mut State, line: &str, count: usize| {
        state.auto_terminal_text(&g, &json!({"terminal": t, "text": format!("{line}\r")})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while state.sessions[0].commands().iter().filter(|c| c.ended.is_some()).count() < count {
            assert!(Instant::now() < deadline, "command {line} never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    run(&mut state, "echo ok", 1);
    run(&mut state, "true | false", 2);
    run(&mut state, "cd /", 3);
    state.auto_terminal_text(&g, &json!({"terminal": t, "text": "sleep 30\r"})).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    state.auto_terminal_interrupt(&g, &json!({"terminal": t})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while state.sessions[0].commands().iter().filter(|c| c.ended.is_some()).count() < 4 {
        assert!(Instant::now() < deadline, "interrupt never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
    let commands = state.sessions[0].commands();
    use crate::session::CommandOutcome::*;
    let outcomes: Vec<_> = commands.iter().map(|c| c.outcome()).collect();
    assert_eq!(outcomes, [Succeeded, Failed, Succeeded, Interrupted], "{commands:?}");
    let (output, complete) = state.sessions[0].command_output(commands[0].id).unwrap();
    assert!(complete);
    assert_eq!(output.trim(), "ok");
    let screen = state.auto_terminal_screen(&g, &json!({"terminal": t})).unwrap();
    assert_eq!(screen["recent_commands"][0]["outcome"], "interrupted");
}
