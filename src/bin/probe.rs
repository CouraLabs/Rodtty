use std::{
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--linger") {
        let path = args
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("missing fixture path"))?;
        std::fs::write(path, "started")?;
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(path, "completed")?;
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--fixture") {
        return fixture();
    }
    let root = tempfile::tempdir()?;
    let shell = rodtty::config::shell_executable()?;
    let session = rodtty::session::Session::spawn(
        &shell,
        root.path(),
        &std::env::current_dir()?,
        Default::default(),
        28,
        100,
        Arc::new(|| {}),
    )?;
    session.wait_for("rodtty:", Duration::from_secs(15))?;
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "printf 'Rune + Brush + uutils\\n'; seq 1 10000 | wc -l".to_owned());
    session.send(format!("{command}; printf '\\n__RODTTY_%s__\\n' DONE\r").into_bytes())?;
    session.wait_for("__RODTTY_DONE__", Duration::from_secs(20))?;
    println!("{}", session.screen().contents());
    Ok(())
}

fn fixture() -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    print!(
        "\x1b[?1049h\x1b[2J\x1b[H\x1b[32mFULL SCREEN FIXTURE\x1b[0m\r\nUnicode: café 日本語 🌿\r\nPress q to exit\r\n"
    );
    std::io::stdout().flush()?;
    let result = (|| -> anyhow::Result<()> {
        for byte in std::io::stdin().lock().bytes() {
            if byte? == b'q' {
                break;
            }
        }
        Ok(())
    })();
    print!("\x1b[?1049l");
    std::io::stdout().flush()?;
    crossterm::terminal::disable_raw_mode()?;
    result
}
