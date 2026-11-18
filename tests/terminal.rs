use rodtty::{
    config::Config,
    profile::PreparedProfile,
    prompt::{PromptPreferences, PromptStyle, PromptSymbol},
    session::Session,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;

mod common;

fn shell() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_rodtty-shell"))
}

fn execute(script: &str, config: Config) -> std::process::Output {
    let root = common::tempdir().unwrap();
    let profile = PreparedProfile::new(shell(), root.path(), &config).unwrap();
    let mut child = Command::new(shell())
        .args(["--no-config", "--noprofile", "--norc", "-c", script])
        .env("PATH", &profile.path)
        .env(rodtty::profile::DISABLED_ENV, &profile.disabled)
        .env_remove("BASH_ENV")
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if child
        .wait_timeout(Duration::from_secs(20))
        .unwrap()
        .is_none()
    {
        child.kill().unwrap();
        let output = child.wait_with_output().unwrap();
        panic!(
            "shell timed out: {script}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    child.wait_with_output().unwrap()
}

#[test]
fn bash_syntax_and_every_initial_utility_work_together() {
    let output = execute(
        "set -e; mkdir work && cd work; printf 'beta\\nalpha\\n' > source; cp source copy; mv copy moved; touch empty; cat moved | sort > sorted; head -n 1 sorted; tail -n 1 sorted; wc -l < sorted; seq 1 3; env --version; ls -1 empty; sleep 0; rm moved; test ! -f moved",
        Config::default(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.starts_with("alpha\nbeta\n2\n1\n2\n3\n"), "{out}");
    assert!(out.contains("empty"));
}

#[test]
fn large_pipeline_runs_concurrently_without_deadlock() {
    let output = execute("seq 1 100000 | cat | wc -l", Config::default());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "100000");
}

#[test]
fn bundled_commands_take_precedence_over_host_path() {
    let output = execute("ls --version", Config::default());
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("uutils")
    );
}

#[test]
fn disabled_names_do_not_fall_back_to_host_commands() {
    for script in [
        "cat",
        "command cat",
        "exec cat",
        "printf x | cat",
        "value=$(cat); exit $?",
        "hash cat; cat",
        "PATH=/usr/bin:/bin; cat",
    ] {
        let output = execute(
            script,
            Config {
                disabled: vec!["cat".into()],
                ..Config::default()
            },
        );
        assert_eq!(
            output.status.code(),
            Some(126),
            "{script}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("disabled for this session"),
            "{script}"
        );
    }
    let script = format!(
        "printf 'cat\\n' > disabled.sh; {} --no-config --noprofile --norc disabled.sh",
        rodtty::profile::quote(shell()).unwrap()
    );
    let output = execute(
        &script,
        Config {
            disabled: vec!["cat".into()],
            ..Config::default()
        },
    );
    assert_eq!(output.status.code(), Some(126));
    assert!(String::from_utf8_lossy(&output.stderr).contains("disabled for this session"));
}

#[cfg(unix)]
#[test]
fn explicit_host_path_remains_usable() {
    let output = execute(
        "printf host | /bin/cat",
        Config {
            disabled: vec!["cat".into()],
            ..Config::default()
        },
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"host");
}

fn session(root: &Path, config: Config) -> Session {
    let session = Session::spawn(shell(), root, root, config, 30, 110, Arc::new(|| {})).unwrap();
    session
        .wait_for("rodtty:", Duration::from_secs(15))
        .unwrap();
    session
}

fn run(session: &Session, script: &str, marker: &str) {
    session
        .send(format!("{script}; printf '\\n__DONE_%s__\\n' '{marker}'\r").into_bytes())
        .unwrap();
    session
        .wait_for(&format!("__DONE_{marker}__"), Duration::from_secs(15))
        .unwrap();
}

#[test]
fn sessions_keep_independent_directories_environments_and_command_snapshots() {
    let root = common::tempdir().unwrap();
    std::fs::create_dir(root.path().join("child")).unwrap();
    let first = session(root.path(), Config::default());
    let second = session(
        root.path(),
        Config {
            disabled: vec!["cat".into()],
            ..Config::default()
        },
    );
    run(
        &first,
        "export RODTTY_TEST=first; cd child; cat --version",
        "FIRST",
    );
    run(
        &second,
        "printf 'ENV:%s\\n' \"${RODTTY_TEST-unset}\"; cat",
        "SECOND",
    );
    assert!(first.screen().contents().to_lowercase().contains("uutils"));
    assert!(second.screen().contents().contains("ENV:unset"));
    assert!(
        second
            .screen()
            .contents()
            .contains("disabled for this session")
    );
    assert_eq!(first.cwd(), root.path().join("child"));
    assert_eq!(second.cwd(), root.path());
    Config {
        disabled: vec!["ls".into()],
        ..Config::default()
    }
    .save(root.path())
    .unwrap();
    run(&first, "ls --version", "UNCHANGED");
    assert!(!first.screen().contents().contains("ls: disabled"));
}

#[test]
fn owner_startup_path_edits_preserve_managed_commands() {
    let root = common::tempdir().unwrap();
    std::fs::write(
        root.path().join("startup.sh"),
        "export RODTTY_STARTUP=yes\nexport PATH=/usr/bin:/bin\n",
    )
    .unwrap();
    let terminal = session(root.path(), Config::default());
    run(
        &terminal,
        "printf 'START:%s\\n' \"$RODTTY_STARTUP\"; ls --version",
        "STARTUP",
    );
    assert!(terminal.screen().contents().contains("START:yes"));
    assert!(
        terminal
            .screen()
            .contents()
            .to_lowercase()
            .contains("uutils")
    );
}

#[test]
fn full_screen_app_resizes_and_returns_to_shell() {
    let root = common::tempdir().unwrap();
    let mut terminal = session(root.path(), Config::default());
    let fixture = rodtty::profile::quote(Path::new(env!("CARGO_BIN_EXE_rodtty-probe"))).unwrap();
    terminal
        .send(format!("{fixture} --resize-fixture\r").into_bytes())
        .unwrap();
    terminal
        .wait_for("FULL SCREEN FIXTURE", Duration::from_secs(15))
        .unwrap();
    assert!(terminal.screen().alternate_screen());
    assert!(terminal.screen().contents().contains("日本語"));
    terminal.resize(36, 120).unwrap();
    assert_eq!(terminal.screen().size(), (36, 120));
    terminal
        .wait_for("SIZE:120x36", Duration::from_secs(5))
        .unwrap();
    terminal.send(b"q".to_vec()).unwrap();
    let start = Instant::now();
    while terminal.screen().alternate_screen() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!terminal.screen().alternate_screen());
    run(&terminal, "printf returned", "RETURNED");
    assert!(terminal.screen().contents().contains("returned"));
}

#[test]
fn interrupt_stops_foreground_command_and_preserves_shell() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    terminal.send(b"sleep 20\r".to_vec()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send(vec![3]).unwrap();
    run(&terminal, "printf alive", "INTERRUPT");
    assert!(terminal.screen().contents().contains("alive"));
}

#[test]
fn terminal_output_and_scrollback_survive_without_a_view() {
    let root = common::tempdir().unwrap();
    let terminal = session(
        root.path(),
        Config {
            scrollback: 100,
            ..Config::default()
        },
    );
    // No frontend owns this session. Dropping/recreating a screen snapshot is
    // deliberately independent of the shell's process lifetime.
    let before = terminal.screen();
    run(&terminal, "seq 1 500", "SCROLLBACK");
    drop(before);
    assert!(terminal.find("500"));
    terminal.scroll(isize::MAX);
    assert!(terminal.screen().scrollback() <= 100);
}

#[test]
fn completion_and_unicode_paste_reach_the_shell() {
    let root = common::tempdir().unwrap();
    std::fs::write(
        root.path().join("unique_completion_file"),
        "COMPLETION_WORKED\n",
    )
    .unwrap();
    let terminal = session(root.path(), Config::default());
    terminal.send(b"cat unique_compl\t\r".to_vec()).unwrap();
    terminal
        .wait_for("COMPLETION_WORKED", Duration::from_secs(10))
        .unwrap();
    terminal
        .paste("printf 'PASTE:%s\\n' 'café 日本語'")
        .unwrap();
    terminal.send(b"\r".to_vec()).unwrap();
    terminal
        .wait_for("PASTE:café 日本語", Duration::from_secs(10))
        .unwrap();
}

#[test]
fn history_sync_does_not_print_errors_at_each_prompt() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    terminal
        .send(b"printf 'history_probe\\n'\r".to_vec())
        .unwrap();
    let start = Instant::now();
    while terminal.screen().contents().matches("rodtty:").count() < 2
        && start.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = terminal.screen().contents();
    assert!(output.matches("rodtty:").count() >= 2, "{output}");
    assert!(!output.contains("error: history:"), "{output}");
    assert!(!output.contains("not yet implemented"), "{output}");
}

#[test]
fn saved_history_is_shared_and_can_be_disabled() {
    let root = common::tempdir().unwrap();
    let first = session(root.path(), Config::default());
    run(&first, "printf history_marker", "HISTORY");
    first.wait_for("rodtty:", Duration::from_secs(5)).unwrap();
    let start = Instant::now();
    while !std::fs::read_to_string(root.path().join("history"))
        .unwrap_or_default()
        .contains("history_marker")
        && start.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        std::fs::read_to_string(root.path().join("history"))
            .unwrap()
            .contains("history_marker")
    );
    let second = session(root.path(), Config::default());
    second.send(b"\x1b[A\r".to_vec()).unwrap();
    second
        .wait_for("__DONE_HISTORY__", Duration::from_secs(10))
        .unwrap();
    let search = session(root.path(), Config::default());
    search.send(b"\x12history_marker\r\r".to_vec()).unwrap();
    search
        .wait_for("__DONE_HISTORY__", Duration::from_secs(10))
        .unwrap();
    let private_root = common::tempdir().unwrap();
    let private = session(
        private_root.path(),
        Config {
            history: false,
            ..Config::default()
        },
    );
    run(&private, "printf private_marker", "PRIVATE");
    assert!(!private_root.path().join("history").exists());
}

#[test]
fn ending_session_stops_a_normal_foreground_child() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    let fixture = rodtty::profile::quote(Path::new(env!("CARGO_BIN_EXE_rodtty-probe"))).unwrap();
    terminal
        .send(format!("{fixture} --linger child-status\r").into_bytes())
        .unwrap();
    let start = Instant::now();
    while !root.path().join("child-status").exists() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("child-status")).unwrap(),
        "started"
    );
    drop(terminal);
    std::thread::sleep(Duration::from_millis(750));
    assert_eq!(
        std::fs::read_to_string(root.path().join("child-status")).unwrap(),
        "started"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires host-installed vim; run explicitly for real-app validation"]
fn real_vim_accepts_keyboard_input_and_saves_a_file() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    terminal
        .send(b"vim -Nu NONE -n sample.txt\r".to_vec())
        .unwrap();
    let start = Instant::now();
    while !terminal.screen().alternate_screen() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(terminal.screen().alternate_screen());
    terminal
        .send("iHello from Rodtty — café\x1b:wq\r".as_bytes().to_vec())
        .unwrap();
    terminal
        .wait_for("rodtty:", Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("sample.txt")).unwrap(),
        "Hello from Rodtty — café\n"
    );
}

#[test]
fn migrated_json_preferences_keep_existing_session_snapshots_and_new_session_startup() {
    let legacy = common::tempdir().unwrap();
    let root = common::tempdir().unwrap();
    std::fs::write(
        legacy.path().join("config.toml"),
        "disabled = ['cat']\nhistory = true\n",
    )
    .unwrap();
    std::fs::write(
        legacy.path().join("startup.sh"),
        "export RODTTY_MIGRATED=original\n",
    )
    .unwrap();
    std::fs::write(
        legacy.path().join("history"),
        "echo retained-owner-history\n",
    )
    .unwrap();
    rodtty::config::migrate_storage(root.path(), legacy.path()).unwrap();
    let first = session(root.path(), Config::load(root.path()).unwrap());
    std::fs::write(
        root.path().join("settings.json"),
        r#"{"disabled":["ls"],"history":false,"scrollback":2000}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("startup.sh"),
        "export RODTTY_MIGRATED=changed\n",
    )
    .unwrap();
    let second = session(root.path(), Config::load(root.path()).unwrap());
    run(
        &first,
        "printf 'START:%s\\n' \"$RODTTY_MIGRATED\"; cat; ls --version; history",
        "OLD_JSON_SESSION",
    );
    run(
        &second,
        "printf 'START:%s\\n' \"$RODTTY_MIGRATED\"; test -z \"$HISTFILE\" && printf 'HISTORY_%s\\n' DISABLED; cat --version; ls",
        "NEW_JSON_SESSION",
    );
    let old = first.screen().contents();
    let new = second.screen().contents();
    assert!(old.contains("START:original"), "{old}");
    assert!(old.contains("cat: disabled for this session"), "{old}");
    assert!(old.contains("retained-owner-history"), "{old}");
    assert!(new.contains("START:changed"), "{new}");
    assert!(new.contains("HISTORY_DISABLED"), "{new}");
    assert!(new.contains("ls: disabled for this session"), "{new}");
    assert!(new.to_lowercase().contains("uutils"), "{new}");
    assert!(legacy.path().join("history").is_file());
}

#[test]
fn semicolon_rich_application_titles_remain_exact_through_the_pty() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    let first = "Agent; 日本語 — long application title;".repeat(20);
    let second = format!("{};;", "続き; application title;".repeat(20));
    for command in [0, 2] {
        for (ending, terminator) in [("BEL", "\\007"), ("ST", "\\033\\\\")] {
            let first = format!("{command} {ending};{first}");
            let title = format!("{first}{second}");
            run(
                &terminal,
                &format!(
                    "printf '\\033]{command};'; printf '%s' '{first}'; sleep 0.03; printf '%s' '{second}'; printf '{terminator}'"
                ),
                &format!("EXACT_TITLE_{command}_{ending}"),
            );
            assert_eq!(terminal.tab_title(), title, "OSC {command} with {ending}");
        }
    }
}

#[test]
fn semicolon_rich_working_directories_remain_exact_through_the_pty() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    run(&terminal, "PROMPT_COMMAND=''", "DIRECTORY_HOOK_DISABLED");
    for (ending, terminator) in [("BEL", "\\007"), ("ST", "\\033\\\\")] {
        let name = format!("{ending};{}日本語;;", "x;".repeat(40));
        let child = root.path().join(&name);
        std::fs::create_dir(&child).unwrap();
        run(
            &terminal,
            &format!(
                "cd {}; printf '\\033]7;file://localhost%s{terminator}' \"$PWD\"",
                rodtty::profile::quote(&child).unwrap()
            ),
            &format!("EXACT_DIRECTORY_{ending}"),
        );
        assert_eq!(terminal.cwd(), child, "OSC 7 with {ending}");
        assert_eq!(
            terminal.tab_title(),
            "Terminal",
            "the fallback label names no directory"
        );
    }
}

#[test]
fn application_titles_are_preserved_changed_and_cleared_through_the_pty() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    let title = format!("Agent; 日本語 — {};;", "long application title ".repeat(12));
    run(
        &terminal,
        &format!("printf '\\033]2;{}\\007'", title),
        "TITLE_SET",
    );
    assert_eq!(
        terminal.state.lock().unwrap().parser.callbacks().title,
        title
    );
    run(
        &terminal,
        "printf '\\033]0;Second; app 日本語; title\\033\\\\'",
        "TITLE_CHANGED",
    );
    assert_eq!(
        terminal.state.lock().unwrap().parser.callbacks().title,
        "Second; app 日本語; title"
    );
    let child = root.path().join("project; 日本語;");
    std::fs::create_dir(&child).unwrap();
    run(
        &terminal,
        &format!("cd {}", rodtty::profile::quote(&child).unwrap()),
        "TITLE_DIRECTORY_CHANGED",
    );
    assert_eq!(terminal.cwd(), child);
    assert_eq!(terminal.tab_title(), "Second; app 日本語; title");
    run(&terminal, "printf '\\033]2;\\007'", "TITLE_CLEARED");
    assert_eq!(terminal.state.lock().unwrap().parser.callbacks().title, "");
    assert_eq!(terminal.tab_title(), "Terminal");
}

fn wait_activity(session: &Session, running: bool) {
    let start = Instant::now();
    while session.has_running_commands() != running {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "activity did not become {running}: {}",
            session.screen().contents()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn command_confirmation_distinguishes_idle_builtin_foreground_background_interrupt_and_exit() {
    let root = common::tempdir().unwrap();
    std::fs::write(
        root.path().join("startup.sh"),
        "PS0='OWNER-PS0'; PROMPT_COMMAND=('printf OWNER-HOOK-ONE' 'printf OWNER-HOOK-TWO')\n",
    )
    .unwrap();
    let session = session(root.path(), Config::default());
    wait_activity(&session, false);
    assert!(
        session
            .screen()
            .contents()
            .contains("OWNER-HOOK-ONEOWNER-HOOK-TWO")
    );
    session.send(b"read answer\r".to_vec()).unwrap();
    wait_activity(&session, true);
    session.send(b"reply\r".to_vec()).unwrap();
    wait_activity(&session, false);
    assert!(session.screen().contents().contains("OWNER-PS0"));
    session.send(b"sleep 10\r".to_vec()).unwrap();
    wait_activity(&session, true);
    session.send(vec![3]).unwrap();
    wait_activity(&session, false);
    session.send(b"sleep 2 &\r".to_vec()).unwrap();
    wait_activity(&session, true);
    // A live job remains protected even when the shell has returned to its prompt.
    session.wait_for("[1]", Duration::from_secs(10)).unwrap();
    assert!(session.has_running_commands());
    session.send(b"wait\r".to_vec()).unwrap();
    wait_activity(&session, false);
    #[cfg(unix)]
    {
        session.send(b"/bin/sleep 10\r".to_vec()).unwrap();
        wait_activity(&session, true);
        session.send(vec![3]).unwrap();
        wait_activity(&session, false);
        session.send(b"/bin/sleep 1 &\r".to_vec()).unwrap();
        wait_activity(&session, true);
        // Brush can wrap even an external background command in an internal
        // async job without a representative PID. An authoritative wait/prompt
        // clears it; a PID-bearing job additionally gets a Unix liveness check.
        session.send(b"wait\r".to_vec()).unwrap();
        wait_activity(&session, false);
    }
    session.send(b"exit\r".to_vec()).unwrap();
    let start = Instant::now();
    while session.state.lock().unwrap().exit.is_none() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!session.has_running_commands());
}

#[test]
fn public_prompt_markers_cannot_hide_a_running_builtin() {
    let root = common::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    while terminal.has_running_commands() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !terminal.has_running_commands(),
        "initial prompt should be idle"
    );
    terminal.send(b"printf '\\033]133;A\\007'; printf 'REVIEW_%s\\n' BLOCKED; read pending; printf complete > completed\r".to_vec()).unwrap();
    terminal
        .wait_for("REVIEW_BLOCKED", Duration::from_secs(5))
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let running_while_blocked = terminal.has_running_commands();
    assert!(
        !root.path().join("completed").exists(),
        "read must still be blocked"
    );
    terminal.send(b"answer\r".to_vec()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.path().join("completed").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("completed")).unwrap(),
        "complete"
    );
    assert!(
        running_while_blocked,
        "Session::has_running_commands was false while builtin read blocked after a public OSC133 A"
    );
}

#[test]
fn other_sessions_activity_markers_cannot_hide_a_running_builtin() {
    let root = common::tempdir().unwrap();
    let other_root = common::tempdir().unwrap();
    let other = PreparedProfile::new(shell(), other_root.path(), &Config::default()).unwrap();
    let terminal = session(root.path(), Config::default());
    wait_activity(&terminal, false);
    terminal.send(format!(
        "printf '\\033]133;C\\007\\033]777;rodtty;{};C\\007\\033]777;rodtty;{};A\\033\\\\'; printf 'FOREIGN_%s\\n' BLOCKED; read answer; printf complete > completed\r",
        other.activity_tag, other.activity_tag
    ).into_bytes()).unwrap();
    terminal
        .wait_for("FOREIGN_BLOCKED", Duration::from_secs(5))
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let running = terminal.has_running_commands();
    assert!(!root.path().join("completed").exists());
    terminal.send(b"reply\r".to_vec()).unwrap();
    wait_activity(&terminal, false);
    assert_eq!(
        std::fs::read_to_string(root.path().join("completed")).unwrap(),
        "complete"
    );
    assert!(
        running,
        "another session's prompt marker must not clear this owning shell's command"
    );
}

fn wait_screen(session: &Session, what: &str, ready: impl Fn(&vt100::Screen) -> bool) {
    let start = Instant::now();
    while !session.with_screen(&ready) {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}; screen:\n{}",
            session.screen().contents()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The row holding the cursor, where the shell's current prompt ends.
fn prompt_row(screen: &vt100::Screen) -> String {
    let (row, _) = screen.cursor_position();
    screen
        .rows(0, screen.size().1)
        .nth(usize::from(row))
        .unwrap_or_default()
}

fn prefs(style: PromptStyle, symbol: PromptSymbol) -> PromptPreferences {
    PromptPreferences { style, symbol }
}

#[test]
fn prompt_preferences_apply_to_a_running_shell_at_its_next_prompt() {
    let root = common::tempdir().unwrap();
    let dir = root.path().display().to_string();
    let terminal = session(root.path(), Config::default());
    wait_screen(&terminal, "default classic prompt", |s| {
        prompt_row(s) == format!("rodtty:{dir}$ ")
    });
    let pid = "printf 'PID:%s\\n' \"$$\"";
    run(&terminal, pid, "PID_BEFORE");
    let shell_pid = terminal
        .screen()
        .contents()
        .lines()
        .find_map(|l| l.strip_prefix("PID:").map(str::to_owned))
        .unwrap();
    for (style, symbol, expected) in [
        (
            PromptStyle::Directory,
            PromptSymbol::Chevron,
            format!("{dir} › "),
        ),
        (
            PromptStyle::Rodtty,
            PromptSymbol::Triangle,
            "Rodtty ▶ ".into(),
        ),
        (
            PromptStyle::Powerline,
            PromptSymbol::DoubleChevron,
            format!(" Rodtty  {dir}  ⟫ "),
        ),
        (
            PromptStyle::Classic,
            PromptSymbol::Dollar,
            format!("rodtty:{dir}$ "),
        ),
    ] {
        terminal.publish_prompt(prefs(style, symbol)).unwrap();
        terminal.send(b"\r".to_vec()).unwrap();
        wait_screen(&terminal, &expected, |s| prompt_row(s) == expected);
        if style == PromptStyle::Powerline {
            let cell = terminal.with_screen(|s| {
                let (row, _) = s.cursor_position();
                s.cell(row, 1).unwrap().bgcolor()
            });
            assert_eq!(cell, vt100::Color::Idx(6), "indexed ANSI cyan segment");
        }
    }
    run(&terminal, pid, "PID_AFTER");
    assert_eq!(
        terminal
            .screen()
            .contents()
            .matches(&format!("PID:{shell_pid}"))
            .count(),
        2,
        "the same shell keeps running"
    );

    terminal
        .publish_prompt(prefs(PromptStyle::TwoLine, PromptSymbol::Dollar))
        .unwrap();
    let symbol_color = |failed: bool| {
        terminal
            .send(if failed { &b"false\r"[..] } else { b"true\r" }.to_vec())
            .unwrap();
        let color = vt100::Color::Idx(if failed { 1 } else { 2 });
        let what = format!("two-line prompt with symbol {color:?}");
        wait_screen(&terminal, &what, |s| {
            let (row, _) = s.cursor_position();
            let directory = s.rows(0, s.size().1).nth(usize::from(row) - 1);
            prompt_row(s) == "$ "
                && s.cell(row, 0).unwrap().fgcolor() == color
                && directory.is_some_and(|line| line.trim_end() == dir)
        });
    };
    symbol_color(true);
    symbol_color(false);
    symbol_color(true);
}

#[test]
fn owner_defined_ps1_from_startup_or_interactively_stays_unmanaged() {
    let root = common::tempdir().unwrap();
    std::fs::write(
        root.path().join("startup.sh"),
        "PS1='owner> '\nPROMPT_COMMAND=('printf \"[status=%s]\" $?')\n",
    )
    .unwrap();
    let owner = Session::spawn(
        shell(),
        root.path(),
        root.path(),
        Config::default(),
        30,
        110,
        Arc::new(|| {}),
    )
    .unwrap();
    wait_screen(&owner, "owner prompt", |s| {
        prompt_row(s) == "owner> " && s.contents().contains("[status=0]")
    });
    owner
        .publish_prompt(prefs(PromptStyle::Rodtty, PromptSymbol::Dollar))
        .unwrap();
    // Owner hooks still see the finished command's own exit status.
    owner.send(b"false\r".to_vec()).unwrap();
    wait_screen(&owner, "owner prompt after change", |s| {
        prompt_row(s) == "owner> " && s.contents().contains("[status=1]")
    });
    assert!(!owner.screen().contents().contains("Rodtty $"));

    let other = common::tempdir().unwrap();
    let interactive = session(other.path(), Config::default());
    interactive.send(b"PS1='x> '\r".to_vec()).unwrap();
    wait_screen(&interactive, "interactive prompt", |s| {
        prompt_row(s) == "x> "
    });
    interactive
        .publish_prompt(prefs(PromptStyle::TwoLine, PromptSymbol::Chevron))
        .unwrap();
    run(&interactive, "true", "INTERACTIVE");
    wait_screen(&interactive, "interactive prompt kept", |s| {
        prompt_row(s) == "x> "
    });
    assert!(!interactive.screen().contents().contains('›'));
}
