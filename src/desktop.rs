use crate::{
    config::{self, Config},
    profile,
    session::Session,
    work::WindowGeometry,
};
use anyhow::{Context, Result};
use rune::{
    App, AppSpec, Color, Cx, El, Family, Key, KeyEvent, Phase, TextEdit, TextStyle, Tx, col, el,
    row, text, text_input,
};
use std::{path::PathBuf, sync::Arc};
#[cfg(test)]
mod accessibility;
mod actions;
mod appearance;
pub(crate) mod automation;
mod autosave;
#[cfg(feature = "browser")]
pub(crate) mod browser;
mod capture;
mod codex;
mod fonts;
mod motion;
mod search;
mod tasks;
use motion::{Enters, Entrance};
mod tabs;
use appearance::{Appearance, rgb};
use terminal::view::grid;
mod settings;
mod shortcuts;
use fonts::{label as ui_label, ui as ui_style};
mod editor;
mod explorer;
mod input;
mod qualification;
mod restoration;
mod state;
mod terminal;
mod ui;
mod window;
mod workspace;
use actions::AppAction;
use editor::*;
use explorer::*;
use input::*;
use qualification::*;
use state::*;
pub use terminal::encode_key;
use terminal::*;
use ui::*;
use window::*;
use workspace::{body, pane_visibility};

fn setup(state: &mut State, wake: rune::Waker) {
    state.wake = wake;
    state.font_catalog.start(state.wake.clone());
    if rune::tooling::headless_run().is_none() {
        state.restore_startup();
        if state.owner_instance {
            state.start_automation();
        }
    }
}
fn poll(state: &mut State, _: f64, tx: &mut Tx) -> bool {
    let maximized = state
        .window
        .as_ref()
        .and_then(|w| w.with_winit_window(|w| w.is_maximized()).ok())
        .unwrap_or(false);
    let frame_changed = state.window_maximized != maximized;
    state.window_maximized = maximized;
    let geometry_changed = state.capture_window_geometry();
    let changed = state.observe_terminal_output();
    let fonts_changed = state.font_catalog.poll();
    let settings_changed = state.poll_settings();
    let (sans, mono) = state.font_families();
    tx.set_font_families(sans, mono);
    let metrics_changed = state.refresh_terminal_metrics();
    let workspace_changed = state.workspace_poll(tx);
    let automation_changed = state.automation_poll(tx);
    if state.workspace.last_persist.elapsed() >= std::time::Duration::from_secs(1) {
        state.persist_workspace();
    }
    workspace_changed
        || automation_changed
        || changed
        || settings_changed
        || metrics_changed
        || frame_changed
        || geometry_changed
        || fonts_changed
}

/// The app icon, rendered from `assets/app-icon/rodtty.svg`.
const APP_ICON: &[u8] = asset_bytes!("app-icon/rodtty-1024.png");

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // `rodtty mcp` serves MCP on stdio for a running instance; it never
    // becomes the owner instance itself.
    if args.get(1).map(String::as_str) == Some("mcp") {
        let name = args
            .iter()
            .skip_while(|a| *a != "--client-name")
            .nth(1)
            .cloned();
        return crate::automation::mcp::serve(config::app_root()?, name);
    }
    if args.get(1).map(String::as_str) == Some("open") {
        std::process::exit(automation::open_cli(&config::app_root()?, &args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("automation") {
        return automation::cli(&config::app_root()?, &args[2..]);
    }
    // CEF needs its application class to be the shared NSApplication,
    // before winit or anything else creates one.
    #[cfg(all(feature = "browser", target_os = "macos"))]
    if browser::available() {
        browser::mac::install();
    }
    if std::env::args().any(|arg| arg == "--paths") {
        let root = config::app_root()?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"config":root.join("settings.json"),"workspace":root.join("workspace-state.json"),"startup":root.join("startup.sh"),"history":root.join("history")})
            )?
        );
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--list-utilities") {
        println!("{}", profile::utility_names().join("\n"));
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--cwd") {
        let path = std::env::args()
            .skip_while(|arg| arg != "--cwd")
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("--cwd requires a directory"))?;
        config::initial_directory(Some(PathBuf::from(path)))?;
    }
    let headless = std::env::args().any(|arg| {
        matches!(
            arg.as_str(),
            "--snap" | "--tree" | "--script" | "--list-scenes" | "--video" | "--strip"
        )
    });
    let instance = if headless {
        None
    } else {
        let Some(instance) = crate::instance::Instance::acquire_owner(
            &config::app_root()?,
            &config::legacy_root()?,
        )?
        else {
            return Ok(());
        };
        Some(instance)
    };
    if instance.is_some() {
        config::migrate_storage(&config::app_root()?, &config::legacy_root()?)?;
        profile::cleanup_stale(&config::app_root()?);
    }
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: &[
            ("preview", preview),
            ("workspace", workspace_preview),
            ("diff", diff_preview),
            ("real-diff", real_diff_preview),
            ("palette", palette_preview),
            ("file-collision", file_collision_preview),
            ("preview-tabs", preview_tabs_scene),
            ("tree", tree_preview),
            ("icons", icons_preview),
            ("tabs-light", tabs_light_preview),
            ("tabs-dark", tabs_dark_preview),
            ("tabs-light-editor", tabs_light_editor_preview),
            ("tabs-dark-editor", tabs_dark_editor_preview),
            ("settings", settings_preview),
            ("behavior", behavior_preview),
            ("terminal-search", terminal_search_preview),
            ("search-files", search_files_preview),
            ("markdown", markdown_preview),
        ],
        fonts: fonts::BUNDLED,
        families: ("Geist", "Geist Mono"),
        min_size: Some((640.0, 360.0)),
        ..AppSpec::new("Rodtty", (1080.0, 720.0), State::init, view)
    };
    // The Dock and app-switcher icon on macOS, and the window and taskbar icon
    // on Windows, for unbundled runs as well as packaged ones.
    if let Err(error) = rune::set_app_icon(APP_ICON) {
        eprintln!("rodtty: {error}");
    }
    let backend = Arc::new(std::sync::Mutex::new(String::new()));
    let backend_slot = backend.clone();
    let mut app = App::new(spec)
        .keep_running()
        .on_close(State::on_window_close)
        .on_quit(|s, _| {
            codex::shutdown();
            #[cfg(feature = "browser")]
            s.shutdown_browsers();
            s.persist_workspace();
            s.shutdown_saved = true;
            s.sessions.clear();
        })
        .on_ime(|s, event, _| {
            #[cfg(feature = "browser")]
            if !s.application_overlay_open()
                && let rune::Ime::Commit(content) = &event
                && s.browser_owner_text(content)
            {
                return true;
            }
            if s.application_overlay_open() || !s.workspace.terminal_focus() {
                return true;
            }
            if let rune::Ime::Commit(content) = event {
                s.send(content.as_bytes().to_vec());
                true
            } else {
                false
            }
        });
    let handle = app.handle();
    let native_state = handle.clone();
    app = app.window_options(window::frame::options());
    if let Some(titlebar) = window::frame::titlebar() {
        app = app.titlebar(titlebar);
    }
    app = app.on_window(move |window| {
        let raw = window
            .with_raw_handle(|handle| format!("{:?}", handle.as_raw()))
            .unwrap_or_default();
        *backend_slot.lock().unwrap_or_else(|e| e.into_inner()) =
            raw.split('(').next().unwrap_or("unknown").to_owned();
        let window = window.clone();
        native_state.update(move |s, _| s.window = Some(window));
    });
    let state_handle = handle.clone();
    let owner_instance = instance.is_some();
    app = app.init(move || {
        let mut state = State::init();
        state.handle = Some(state_handle.clone());
        state.owner_instance = owner_instance;
        state
    });
    let launch_handle = handle.clone();
    let _launch_watcher = instance
        .as_ref()
        .map(|instance| instance.watch(move || launch_handle.open()))
        .transpose()?;
    let native_report = std::env::args()
        .skip_while(|arg| {
            arg != "--native-check"
                && arg != "--workspace-check"
                && arg != "--font-check"
                && arg != "--frame-check"
                && arg != "--session-policy-check"
                && arg != "--clipboard-check"
                && arg != "--trash-check"
                && arg != "--restoration-check"
        })
        .nth(1)
        .map(PathBuf::from);
    if let Some(report) = native_report.clone() {
        native_check(handle.clone(), report, backend);
    }
    let reopen = handle.clone();
    let tray = rune::Tray::new(
        rune::TraySpec::new(
            "dev.couralabs.rodtty",
            "Rodtty",
            rune::TrayIcon::rgba(16, 16, [101, 218, 177, 255].repeat(256)),
        )
        .tooltip("Rodtty — terminal sessions keep running")
        .menu(vec![
            rune::TrayItem::action("open", "Open Rodtty"),
            rune::TrayItem::action("quit", "Quit Rodtty"),
        ]),
        move |event| match event {
            rune::TrayEvent::Activate { .. } => reopen.open(),
            rune::TrayEvent::Menu(id) if id == "open" => reopen.open(),
            rune::TrayEvent::Menu(id) if id == "quit" => {
                reopen.open();
                reopen.update(|s, tx| s.dispatch(AppAction::Quit, tx));
            }
            _ => {}
        },
    );
    if let Err(error) = &tray {
        eprintln!(
            "rodtty: tray unavailable: {error}; relaunch Rodtty to reopen, or use --control-stdin for developer controls"
        );
    }
    // A local developer control channel also makes native close/reopen checks
    // repeatable without depending on a desktop's tray implementation.
    if std::env::args().any(|a| a == "--control-stdin") {
        let control_report = native_report.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                match line.trim() {
                    "frame-status" => {
                        if let Some(report) = control_report.clone() {
                            handle.update(move |s, _| {
                                if let Some(window) = &s.window {
                                    let _ = window.with_winit_window(|w| {
                                        let size = w.inner_size();
                                        let _ = std::fs::write(report.with_extension("state.json"), serde_json::to_vec(&serde_json::json!({
                                            "position":w.inner_position().ok().map(|p| [p.x,p.y]),
                                            "decorated":w.is_decorated(),"maximized":w.is_maximized(),"minimized":w.is_minimized(),
                                            "size":[size.width,size.height],"scale":w.scale_factor(),
                                            "unsaved_prompt":matches!(s.pending, Some(Pending::File(workspace::FilePrompt::Unsaved { .. }))),
                                            "dirty":s.workspace.tabs.iter().any(|t| t.dirty())})).unwrap_or_default());
                                    });
                                }
                            });
                        }
                    }
                    "open" => handle.open(),
                    // The owner's own palette actions, for scripted native checks.
                    line if line.starts_with("action ") => {
                        let id = line["action ".len()..].trim().to_owned();
                        handle.update(move |s, tx| {
                            if let Some(action) =
                                s.commands.iter().find(|c| c.id == id).map(|c| c.action)
                            {
                                s.dispatch(action, tx);
                            }
                        });
                    }
                    "close" => handle.close(),
                    "quit" => {
                        handle.open();
                        handle.update(|s, tx| s.dispatch(AppAction::Quit, tx));
                    }
                    "status" => eprintln!(
                        "rodtty: visible={} error={:?}",
                        handle.is_visible(),
                        handle.window_error()
                    ),
                    _ => {}
                }
            }
        });
    }
    app.run();
    drop(tray);
    if let Some(report) = native_report {
        let result: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
        anyhow::ensure!(
            result["passed"] == true,
            "native lifecycle check failed: {result}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
