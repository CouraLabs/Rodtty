// Release builds run as a Windows GUI app, so launching Rodtty opens no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    rodtty::desktop::run()
}
