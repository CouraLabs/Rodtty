use super::*;
use crate::desktop::tests::{open_palette, press, runtime_for};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn shell_state(root: &Path) -> State {
    let mut state = State::with_root(root.into());
    state.directory = root.into();
    state.shell = std::env::current_exe()
        .unwrap()
        .ancestors()
        .map(|p| p.join("rodtty-shell"))
        .find(|p| p.is_file());
    state
}

fn primary_shift(key: char) -> (Key, rune::Mods) {
    (
        Key::Command(key),
        rune::Mods {
            ctrl: !cfg!(target_os = "macos"),
            cmd: true,
            shift: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    )
}

/// A PNG header is all `ClipboardImage` checks; no pixels are decoded.
fn image() -> rune::ClipboardImage {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(1u32.to_be_bytes());
    png.extend(1u32.to_be_bytes());
    rune::ClipboardImage::from_png(png).unwrap()
}

/// Fixture paths that need quoting: spaces, single quotes, non-ASCII, and a folder.
fn fixtures(root: &Path) -> Vec<PathBuf> {
    let paths = vec![
        root.join("with space.txt"),
        root.join("it's here.txt"),
        root.join("Olá 世界 🦀.txt"),
        root.join("a folder"),
    ];
    for file in &paths[..3] {
        std::fs::write(file, b"x").unwrap();
    }
    std::fs::create_dir(&paths[3]).unwrap();
    paths
}

#[test]
fn paths_paste_as_single_bash_words_written_like_a_dropped_file() {
    let quote = |p: &str| quote_path(Path::new(p)).unwrap();
    assert_eq!(
        quote("/tmp/shot-1.png"),
        "/tmp/shot-1.png",
        "plain paths stay as is"
    );
    assert_eq!(quote("/tmp/with space"), r"/tmp/with\ space");
    assert_eq!(quote("/tmp/it's"), r"/tmp/it\'s");
    assert_eq!(quote("/tmp/Olá 世界 🦀"), r"/tmp/Olá\ 世界\ 🦀");
    assert_eq!(quote("/tmp/$(x)&;"), r"/tmp/\$\(x\)\&\;");
    assert_eq!(quote(r"C:\Users\o'n"), r"C:\\Users\\o\'n");
    assert_eq!(
        quote("/tmp/line\nbreak\t'\\\x1b"),
        r"$'/tmp/line\nbreak\t\'\\\x1b'",
        "control characters never reach the terminal raw"
    );
    let (text, pasted, skipped) = paste_paths(&[
        PathBuf::from("/a b"),
        PathBuf::from("relative"),
        PathBuf::from("/c"),
    ]);
    assert_eq!((text.as_str(), pasted, skipped), (r"/a\ b /c", 2, 1));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let invalid = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff"));
        assert_eq!(quote_path(invalid), None, "non-UTF-8 paths are skipped");
    }
}

/// File lists from each entry point reach a real PTY as one bracketed paste of
/// escaped absolute paths with nothing submitted; an image-only clipboard
/// sends a saved PNG's path; the words parse back to the same paths in the real shell.
#[test]
fn every_entry_point_pastes_file_lists_as_quoted_paths_without_submitting() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = shell_state(root.path());
    state.new_tab();
    let session = state.active().unwrap();
    session
        .wait_for("rodtty:", Duration::from_secs(15))
        .unwrap();
    let paths = fixtures(root.path());
    let words = paste_paths(&paths).0;
    let mut rt = runtime_for(state);
    let board = rune::clipboard::memory();
    rt.use_clipboard(&board);
    type Entry = (&'static str, fn(&mut rune::Runtime<State>));
    let entries: [Entry; 4] = [
        ("shortcut", |rt| {
            let (key, mods) = primary_shift('v');
            press(rt, key, mods);
        }),
        ("palette", |rt| {
            open_palette(rt);
            press(rt, Key::Text("Terminal: Paste".into()), Default::default());
            press(rt, Key::Enter, Default::default());
        }),
        ("action", |rt| {
            rt.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
            rt.frame(0.0);
        }),
        ("context-menu", |rt| {
            let (x, y) = rt.find("terminal").unwrap();
            rt.secondary_down(x, y);
            rt.secondary_up(x, y);
            rt.frame(0.0);
            rt.resolve_popup(Some("paste-terminal")).unwrap();
            rt.frame(0.0);
        }),
    ];
    let image_only = entries.len();
    // A pasted image arrives as the path of a saved PNG: `rodtty-paste-`,
    // six random characters, `.png`, in the system temporary folder.
    let image_prefix = quote_path(&std::env::temp_dir().join("rodtty-paste-")).unwrap();
    for (round, (name, paste)) in entries.iter().chain([&entries[0]]).enumerate() {
        let expected = if round == image_only {
            board.set_content(rune::ClipboardContent::default().with_image(image()));
            format!("\x1b[200~{image_prefix}XXXXXX.png\x1b[201~Z").into_bytes()
        } else {
            // Finder offers the bare names as text beside the files; files win.
            board.set_content(
                rune::ClipboardContent::new("with space.txt\rit's here.txt")
                    .with_files(paths.clone()),
            );
            format!("\x1b[200~{words}\x1b[201~Z").into_bytes()
        };
        let capture = root.path().join(format!("capture-{round}"));
        rt.settle_all();
        rt.frame(0.0);
        rt.state
            .active()
            .unwrap()
            .send(
                format!(
                    "stty raw -echo; printf '\\033[?2004hREADY_%s\\n' {round}; head -c {} > {}; printf '\\033[?2004l'; stty sane; printf 'CAPTURED_%s\\n' {round}\r",
                    expected.len(),
                    crate::profile::quote(&capture).unwrap(),
                )
                .into_bytes(),
            )
            .unwrap();
        rt.state
            .active()
            .unwrap()
            .wait_for(&format!("READY_{round}"), Duration::from_secs(15))
            .unwrap();
        assert!(rt.state.with_screen(vt100::Screen::bracketed_paste));
        rt.state.message.clear();
        paste(&mut rt);
        // The memory clipboard answers at the end of a frame; the next one
        // delivers the answer to the handler.
        rt.frame(0.0);
        rt.frame(0.0);
        // The sentinel follows the paste through the same ordered input queue,
        // so a submitted newline would take its place in the capture.
        rt.state.active().unwrap().send(b"Z".to_vec()).unwrap();
        rt.state
            .active()
            .unwrap()
            .wait_for(&format!("CAPTURED_{round}"), Duration::from_secs(15))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let captured = String::from_utf8_lossy(&std::fs::read(&capture).unwrap()).into_owned();
        if round == image_only {
            let path = captured
                .strip_prefix("\x1b[200~")
                .and_then(|rest| rest.strip_suffix("\x1b[201~Z"))
                .unwrap_or_else(|| panic!("image paste is one bracketed path: {captured:?}"));
            assert!(
                path.starts_with(&image_prefix) && path.ends_with(".png"),
                "{path:?}"
            );
            assert_eq!(path.len(), image_prefix.len() + 10);
            // The plain temporary path needs no escapes, so it names the file.
            assert_eq!(std::fs::read(path).unwrap(), image().png());
            std::fs::remove_file(path).unwrap();
        } else {
            assert_eq!(
                captured,
                String::from_utf8_lossy(&expected),
                "{name} (round {round})"
            );
        }
    }
    // The pasted words are the paths themselves to the real shell.
    let listing = root.path().join("listing");
    board.set_content(rune::ClipboardContent::default().with_files(paths.clone()));
    rt.state
        .active()
        .unwrap()
        .send(b"printf '<%s>\\n' ".to_vec())
        .unwrap();
    rt.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
    rt.frame(0.0);
    rt.frame(0.0);
    rt.state
        .active()
        .unwrap()
        .send(
            format!(
                " > {}; printf 'LISTED_%s\\n' OK\r",
                crate::profile::quote(&listing).unwrap()
            )
            .into_bytes(),
        )
        .unwrap();
    rt.state
        .active()
        .unwrap()
        .wait_for("LISTED_OK", Duration::from_secs(15))
        .unwrap();
    let expected: String = paths
        .iter()
        .map(|p| format!("<{}>\n", p.display()))
        .collect();
    assert_eq!(std::fs::read_to_string(listing).unwrap(), expected);
}

#[test]
fn a_pending_paste_is_dropped_when_its_session_or_focus_moves() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = shell_state(root.path());
    state.new_tab();
    state.new_tab();
    assert_eq!(state.sessions.len(), 2);
    let mut rt = runtime_for(state);
    let files = rune::ClipboardContent::default().with_files([PathBuf::from("/tmp/a b")]);
    let text = rune::ClipboardContent::new("echo hi");
    let target = |rt: &mut rune::Runtime<State>| {
        let mut target = None;
        rt.update(|s, _| target = s.terminal_paste_target());
        target.unwrap()
    };
    let finish = |rt: &mut rune::Runtime<State>, t, content: &rune::ClipboardContent| {
        let mut outcome = None;
        rt.update(|s, tx| outcome = Some(s.finish_terminal_paste(t, content, tx)));
        outcome.unwrap()
    };
    let activate = |rt: &mut rune::Runtime<State>, index| {
        rt.update(|s, tx| s.dispatch(AppAction::ActivateTerminal(index), tx));
        rt.frame(0.0);
    };
    // Unchanged target: files and text both arrive.
    let t = target(&mut rt);
    assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Files(1));
    assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Text);
    // Tab switch to another session.
    let t = target(&mut rt);
    activate(&mut rt, 0);
    assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Cancelled);
    // Away and back to the same session still drops it.
    let t = target(&mut rt);
    activate(&mut rt, 1);
    activate(&mut rt, 0);
    assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
    // Focus leaves the terminal...
    let t = target(&mut rt);
    rt.update(|s, _| s.workspace.set_terminal_focus(false));
    assert_eq!(finish(&mut rt, t, &files), PasteOutcome::Cancelled);
    // ...or leaves and returns before the read completes.
    rt.update(|s, _| s.workspace.set_terminal_focus(true));
    let t = target(&mut rt);
    rt.update(|s, _| {
        s.workspace.set_terminal_focus(false);
        s.workspace.set_terminal_focus(true);
    });
    assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
    // A text field took focus while the terminal kept its flag.
    let t = target(&mut rt);
    rt.update(|s, tx| s.dispatch(AppAction::FindTerminal, tx));
    rt.settle_all();
    rt.frame(0.0);
    assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
    // The session closed.
    rt.update(|s, tx| {
        s.terminal_search.open = false;
        tx.blur();
    });
    activate(&mut rt, 1);
    let t = target(&mut rt);
    rt.update(|s, _| {
        s.sessions.remove(1);
        s.active = 0;
    });
    assert_eq!(finish(&mut rt, t, &text), PasteOutcome::Cancelled);
    // An image-only clipboard pastes a saved file's path; an empty one does nothing.
    let t = target(&mut rt);
    assert_eq!(
        finish(
            &mut rt,
            t,
            &rune::ClipboardContent::default().with_image(image())
        ),
        PasteOutcome::Image
    );
    assert_eq!(
        finish(&mut rt, t, &rune::ClipboardContent::default()),
        PasteOutcome::Empty
    );
}
