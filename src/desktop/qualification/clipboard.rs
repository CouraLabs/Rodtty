use crate::desktop::*;

/// Exercise the actual asynchronous host clipboard worker and a real terminal session.
/// The external native harness preserves arbitrary offered MIME data, verifies the copy,
/// and supplies only the public fixture below; owner clipboard contents never enter reports.
pub(crate) fn native_check(
    handle: &rune::AppHandle<State>,
    report: &std::path::Path,
) -> Result<serde_json::Value> {
    use std::time::{Duration, Instant};
    handle.update(|s, _| {
        s.active()
            .unwrap()
            .send("printf '\\033[2J\\033[HOlá 世界 🦀'\r".as_bytes().to_vec())
            .unwrap();
    });
    wait_app(handle, |s| {
        s.with_screen(vt100::Screen::contents)
            .starts_with("Olá 世界 🦀")
    })?;
    let (id_tx, id_rx) = std::sync::mpsc::channel();
    handle.update(move |s, tx| {
        let _ = id_tx.send(s.active().unwrap().id());
        s.selection = Some(((0, 0), (0, 11)));
        tx.blur();
    });
    let id = id_rx.recv_timeout(Duration::from_secs(10))?;
    std::fs::write(report.with_extension("ready.json"), b"{\"phase\":\"copy\"}")?;
    let copy = report.with_extension("copy.json");
    let start = Instant::now();
    while !copy.exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(25));
    }
    anyhow::ensure!(copy.exists(), "external native activation timed out");
    handle.update(|s, tx| s.dispatch(AppAction::CopyTerminal, tx));
    let input = report.with_extension("input.json");
    let start = Instant::now();
    while !input.exists() && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(25));
    }
    let external: serde_json::Value = serde_json::from_slice(
        &std::fs::read(input).context("external clipboard verification timed out")?,
    )?;
    anyhow::ensure!(
        external["copy_verified"] == true,
        "external copy verification failed"
    );
    handle.update(|s,_| {
        s.active().unwrap().send(b"stty raw -echo; printf '\\033[?1h'; head -c 16 | od -An -tx1 | tr -s ' '; stty sane; printf '\\033[?1l\\nCLIPBOARD_PASTE_DONE\\n'\r".to_vec()).unwrap();
    });
    wait_app(handle, |s| s.with_screen(vt100::Screen::application_cursor))?;
    handle.update(|s, tx| s.dispatch(AppAction::PasteTerminal, tx));
    wait_app(handle, |s| {
        let output = s.with_screen(vt100::Screen::contents);
        output.contains("4f 6c c3 a1 20 e4 b8 96 e7 95 8c 20 f0 9f 90 9a")
            && output.contains("CLIPBOARD_PASTE_DONE")
    })?;
    wait_app(handle, move |s| {
        s.active().unwrap().id() == id && s.workspace.terminal_focus()
    })?;
    Ok(
        serde_json::json!({"passed":true,"native_copy_exact_unicode":true,"native_paste_real_pty_exact_utf8":true,"same_session":true}),
    )
}
