use rodtty::{config::Config, profile::PreparedProfile, session::Session};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;

fn shell() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_rodtty-shell"))
}

fn execute(script: &str, config: Config) -> std::process::Output {
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
    let mut terminal = session(root.path(), Config::default());
    let fixture = rodtty::profile::quote(Path::new(env!("CARGO_BIN_EXE_rodtty-probe"))).unwrap();
    terminal
        .send(format!("{fixture} --fixture\r").into_bytes())
        .unwrap();
    terminal
        .wait_for("FULL SCREEN FIXTURE", Duration::from_secs(15))
        .unwrap();
    assert!(terminal.screen().alternate_screen());
    assert!(terminal.screen().contents().contains("日本語"));
    terminal.resize(36, 120).unwrap();
    assert_eq!(terminal.screen().size(), (36, 120));
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
    let root = tempfile::tempdir().unwrap();
    let terminal = session(root.path(), Config::default());
    terminal.send(b"sleep 20\r".to_vec()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    terminal.send(vec![3]).unwrap();
    run(&terminal, "printf alive", "INTERRUPT");
    assert!(terminal.screen().contents().contains("alive"));
}

#[test]
fn terminal_output_and_scrollback_survive_without_a_view() {
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let private_root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
    let root = tempfile::tempdir().unwrap();
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
