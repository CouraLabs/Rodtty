use crate::{
    config::{self, Config},
    profile,
    session::Session,
};
use anyhow::{Context, Result};
use rune::{
    App, AppSpec, Color, Cx, El, Family, Key, KeyEvent, Phase, TextEdit, TextStyle, Tx, col, el,
    row, text, text_input,
};
use std::{path::PathBuf, sync::Arc};
mod actions;
mod appearance;
mod fonts;
mod palette;
use appearance::Appearance;
mod settings;
use fonts::{label as ui_label, ui as ui_style};
mod workspace;
use actions::AppAction;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    let decode = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Color {
        r: decode(r),
        g: decode(g),
        b: decode(b),
        a: 1.0,
    }
}
enum Pending {
    Tab(usize),
    Quit,
    File(workspace::FilePrompt),
}

struct State {
    workspace: workspace::Workspace,
    commands: Vec<actions::Command>,
    palette: Option<palette::Palette>,
    palette_query: TextEdit,
    handle: Option<rune::AppHandle<State>>,
    root: PathBuf,
    shell: Option<PathBuf>,
    config: Config,
    config_valid: bool,
    sessions: Vec<Session>,
    active: usize,
    directory: PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
    generations: Vec<u64>,
    settings: bool,
    settings_open: bool,
    settings_query: TextEdit,
    settings_error: Option<String>,
    settings_seen: Option<Vec<u8>>,
    settings_poll: std::time::Instant,
    last_valid_config: Config,
    search: bool,
    query: TextEdit,
    pending: Option<Pending>,
    selection: Option<((u16, u16), (u16, u16))>,
    message: String,
    preview: Option<vt100::Screen>,
    terminal_rect: Option<[f32; 4]>,
    terminal_metrics: fonts::Metrics,
}

impl State {
    fn init() -> Self {
        let root = config::app_root().unwrap_or_else(|_| std::env::temp_dir().join("rodtty"));
        Self::with_root(root)
    }

    fn with_root(root: PathBuf) -> Self {
        let loaded = Config::load(&root);
        let directory = config::initial_directory(
            std::env::args()
                .skip_while(|a| a != "--cwd")
                .nth(1)
                .map(PathBuf::from),
        )
        .unwrap_or_else(|_| root.clone());
        let message = loaded
            .as_ref()
            .err()
            .map(|error| format!("Could not load Settings JSON: {error:#}"))
            .unwrap_or_default();
        let config_valid = loaded.is_ok();
        let mut commands = Vec::new();
        actions::register(&mut commands);
        Self {
            commands,
            palette: None,
            palette_query: TextEdit::new(""),
            workspace: workspace::Workspace::default(),
            handle: None,
            settings_seen: std::fs::read(root.join("settings.json")).ok(),
            root,
            shell: config::shell_executable().ok(),
            last_valid_config: loaded.as_ref().cloned().unwrap_or_default(),
            config: loaded.unwrap_or_default(),
            config_valid,
            sessions: Vec::new(),
            active: 0,
            directory,
            wake: Arc::new(|| {}),
            generations: Vec::new(),
            settings: false,
            settings_open: false,
            settings_query: TextEdit::new(""),
            settings_error: (!config_valid).then(|| message.clone()),
            settings_poll: std::time::Instant::now(),
            search: false,
            query: TextEdit::new(""),
            pending: None,
            selection: None,
            message,
            preview: None,
            terminal_rect: None,
            terminal_metrics: Default::default(),
        }
    }

    fn active(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    fn new_tab(&mut self) {
        if !self.config_valid {
            return;
        }
        let cwd = self
            .active()
            .map(Session::cwd)
            .unwrap_or_else(|| self.directory.clone());
        let Some(shell) = &self.shell else {
            self.message =
                "Build rodtty-shell with cargo build --bins before starting Rodtty.".into();
            return;
        };
        match Session::spawn(
            shell,
            &self.root,
            &cwd,
            self.config.clone(),
            28,
            100,
            self.wake.clone(),
        ) {
            Ok(session) => {
                self.workspace.terminal_focus = true;
                self.sessions.push(session);
                self.active = self.sessions.len() - 1;
                self.selection = None;
                self.preview = None;
            }
            Err(error) => self.message = format!("Could not start session: {error:#}"),
        }
    }

    fn send(&mut self, bytes: impl Into<Vec<u8>>) {
        if let Some(session) = self.active() {
            session.scroll(isize::MIN);
            if let Err(error) = session.send(bytes) {
                self.message = error.to_string();
            }
        }
        self.selection = None;
    }

    fn paste(&mut self, content: &str) {
        if let Some(session) = self.active()
            && let Err(error) = session.paste(content)
        {
            self.message = error.to_string();
        }
    }

    fn with_screen<R>(&self, read: impl FnOnce(&vt100::Screen) -> R) -> R {
        if let Some(session) = self.active() {
            session.with_screen(read)
        } else if let Some(preview) = &self.preview {
            read(preview)
        } else {
            read(vt100::Parser::new(28, 100, 0).screen())
        }
    }

    fn save_config(&mut self, edit: impl FnOnce(&mut Config)) {
        // An invalid owner file may be in the middle of an external edit. Keep
        // it intact rather than treating a UI change as permission to replace it.
        match Config::load(&self.root) {
            Ok(current) => {
                self.last_valid_config = current.clone();
                self.config = current;
                edit(&mut self.config);
            }
            Err(error) => {
                self.config = self.last_valid_config.clone();
                let error = format!(
                    "Invalid Settings JSON; keeping last valid preferences. Repair the JSON before saving: {error:#}"
                );
                self.message = error.clone();
                self.settings_error = Some(error);
                return;
            }
        }
        self.message = match self.config.save(&self.root) {
            Ok(()) => {
                self.last_valid_config = self.config.clone();
                self.settings_seen = std::fs::read(self.root.join("settings.json")).ok();
                self.settings_error = None;
                self.config_valid = true;
                "Saved. Command, history, and scrollback settings apply to new sessions.".into()
            }
            Err(error) => {
                self.config = self.last_valid_config.clone();
                format!("Could not save settings: {error:#}")
            }
        };
    }

    fn copy(&self, tx: &mut Tx) {
        if let Some((a, b)) = self.selection {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            tx.copy(self.with_screen(|screen| screen.contents_between(a.0, a.1, b.0, b.1)));
        }
    }

    fn confirm(&mut self, tx: &mut Tx) {
        match self.pending.take() {
            Some(Pending::Quit) => {
                self.persist_workspace();
                self.sessions.clear();
                tx.quit();
            }
            Some(Pending::Tab(index)) if index < self.sessions.len() => {
                self.sessions.remove(index);
                self.active = self.active.min(self.sessions.len().saturating_sub(1));
                self.selection = None;
            }
            _ => {}
        }
    }
}

fn button(
    a: &Appearance,
    key: impl Into<String>,
    label: impl Into<String>,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let label = label.into();
    let key = key.into();
    let help = match key.as_str() {
        "explorer-hide" => "Hide file explorer",
        "explorer-up" => "Open parent folder",
        "explorer-refresh" => "Refresh files",
        "settings-close" => "Close Settings tab",
        "find-close" => "Close editor search",
        "diff-prev" => "Previous change",
        "diff-next" => "Next change",
        _ if key.starts_with("editor-close-") => "Close editor tab",
        _ => &label,
    };
    el().key(key)
        .label(help)
        .help(help)
        .focusable(true)
        .focus_ring(2.0, 1.0, a.accent())
        .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
        .px(10.0)
        .center()
        .radius(5.0)
        .fill(a.panel())
        .hover(a.hover_bg())
        .on_press_tx(move |state: &mut State, tx| {
            tx.blur();
            action(state, tx);
        })
        .child(text(label).font(ui_label()).color(a.ink()))
}

fn grid(state: &State, a: &Appearance) -> El {
    state.with_screen(|screen| grid_view(state, screen, a))
}

fn grid_view(state: &State, screen: &vt100::Screen, a: &Appearance) -> El {
    let metrics = fonts::Metrics::measure();
    let (cw, ch) = (metrics.width, metrics.height);
    let (rows, cols) = screen.size();
    let selection = state
        .selection
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    let mut grid = el()
        .key("terminal")
        .instant_changes()
        .role("terminal")
        .label("Terminal")
        .w_full()
        .grow(1.0)
        .clip_soft(0.0)
        .fill(a.bg());
    // Position runs on a fixed cell grid. Wide glyphs get their own run so
    // fallback-font advances cannot shift the cells following them.
    for r in 0..rows {
        let mut c = 0;
        while c < cols {
            let Some(cell) = screen.cell(r, c) else {
                c += 1;
                continue;
            };
            if cell.is_wide_continuation() {
                c += 1;
                continue;
            }
            let selected = |c| selection.is_some_and(|(a, b)| (r, c) >= a && (r, c) < b);
            let start = c;
            let mut content = String::new();
            let mut end = c;
            // Each terminal cell has an explicit origin. Font fallback, ligatures,
            // combining sequences and pixel rounding must never shift later cells.
            let next = screen.cell(r, end).unwrap();
            content.push_str(if next.contents().is_empty() {
                " "
            } else {
                next.contents()
            });
            end += if next.is_wide() { 2 } else { 1 };
            let mut fg = a.terminal(cell.fgcolor(), true);
            let mut background = a.terminal(cell.bgcolor(), false);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut background);
            }
            if selected(start) {
                background = a.bg().mix(a.accent(), 0.28);
            }
            let width = (end - start) as f32 * cw;
            if background != a.bg() {
                grid = grid.child(
                    el().absolute(start as f32 * cw, r as f32 * ch)
                        .size(width, ch)
                        .fill(background),
                );
            }
            if !content.trim().is_empty() {
                let mut style = metrics.style;
                if cell.bold() {
                    style.weight = 600.0;
                }
                style.italic = cell.italic();
                grid = grid.child(
                    text(content)
                        .key(format!("terminal-cell-{r}-{start}"))
                        .font(style)
                        .color(fg)
                        .absolute(start as f32 * cw, r as f32 * ch)
                        .size(width + 1.0, ch),
                );
            }
            if cell.underline() {
                grid = grid.child(
                    el().absolute(start as f32 * cw, r as f32 * ch + ch - 3.0)
                        .size(width, 1.0)
                        .fill(fg),
                );
            }
            c = end.max(c + 1);
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (r, c) = screen.cursor_position();
        grid = grid.child(
            el().key("terminal-caret")
                .absolute(c as f32 * cw, r as f32 * ch + ch - 3.0)
                .size(cw, 2.0)
                .fill(a.accent())
                .ime_cursor(),
        );
    }
    // A metric/session probe also reports when the viewport itself has not changed.
    // Rune's on_layout fires only for changed rectangles; a font-only update still
    // needs to notify the existing PTY before its next redraw.
    let session_key = state
        .active()
        .map(|s| Arc::as_ptr(&s.state) as usize)
        .unwrap_or(0);
    grid = grid.child(
        el().key(format!(
            "terminal-metrics-{session_key}-{}-{cw}-{ch}",
            rune::text::font_epoch()
        ))
        .absolute(0.0, 0.0)
        .size(cw, ch)
        .on_layout(move |s: &mut State, _| s.resize_terminal(metrics)),
    );
    grid.on_layout(move |s: &mut State, rect| {
        s.terminal_rect = Some(rect);
        s.resize_terminal(metrics);
    })
    .on_background_press_tx(|s: &mut State, tx| {
        s.workspace.terminal_focus = true;
        tx.blur();
    })
    .on_drag(move |s: &mut State, drag, tx| {
        s.workspace.terminal_focus = true;
        tx.blur();
        let (rows, cols) = s.with_screen(vt100::Screen::size);
        let point = |x: f32, y: f32| {
            (
                (y / ch).floor().clamp(0.0, (rows - 1) as f32) as u16,
                (x / cw).floor().clamp(0.0, cols as f32) as u16,
            )
        };
        if drag.cancelled() {
            s.selection = None;
            return;
        }
        if drag.phase == Phase::Down {
            s.selection = Some((point(drag.x, drag.y), point(drag.x, drag.y)));
        } else if let Some((start, _)) = s.selection {
            s.selection = Some((start, point(drag.x, drag.y)));
        }
    })
    .on_wheel(move |s: &mut State, event, _| {
        if let Some(session) = s.active() {
            session.scroll((-event.dy / ch).round() as isize);
        }
        true
    })
}

fn view(state: &State, cx: &mut Cx<State>) -> El {
    let _fonts = fonts::scope(&state.config.fonts, cx.env().scale);
    let a = &Appearance::resolve(
        state.config.theme_family,
        state.config.appearance,
        cx.system_dark(),
    );
    cx.foreground(a.ink());
    cx.focus_ring(2.0, 1.0, a.accent());
    cx.help_look(a.tooltip());
    cx.caption_color(a.ink());
    let mut tabs = row()
        .key("tabs")
        .w_full()
        .h(40.0)
        .px(16.0)
        .gap(6.0)
        .center_y()
        .fill(a.panel());
    for (i, session) in state.sessions.iter().enumerate() {
        let exited = session
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit
            .is_some();
        tabs = tabs.child(
            button(
                a,
                format!("tab-{i}"),
                format!("{} {}", if exited { "○" } else { "●" }, i + 1),
                move |s, tx| s.dispatch(AppAction::ActivateTerminal(i), tx),
            )
            .border(
                1.0,
                if i == state.active {
                    a.accent()
                } else {
                    a.panel()
                },
            ),
        );
    }
    if state.preview.is_some() {
        tabs = tabs.child(text("●  1   Preview").font(ui_label()).color(a.accent()));
    }
    tabs = tabs.child(button(a, "new-tab", "+  New tab", |s, tx| {
        s.dispatch(AppAction::NewTerminal, tx)
    }));
    let mut root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(a.bg())
        .child(
            row()
                .key("app-toolbar")
                .w_full()
                .h(52.0)
                .scroll_x()
                .px(16.0)
                .gap(10.0)
                .center_y()
                .child(text("RODTTY").font(ui_style(15.0, 600.0)).color(a.ink()))
                .child(
                    text("WORKSPACE")
                        .font(ui_style(10.0, 500.0))
                        .color(a.accent()),
                )
                .child(el().grow(1.0))
                .child(button(a, "files", "Files", |s, tx| {
                    s.dispatch(AppAction::ToggleExplorer, tx)
                }))
                .child(button(a, "toggle-terminal", "Terminal", |s, tx| {
                    s.dispatch(AppAction::ToggleTerminal, tx)
                }))
                .child(button(a, "toggle-editor", "Editor", |s, tx| {
                    s.dispatch(AppAction::ToggleEditor, tx)
                }))
                .child(button(a, "find", "Find", |s, tx| {
                    s.dispatch(AppAction::FindTerminal, tx)
                }))
                .child(button(a, "settings", "Settings", |s, tx| {
                    s.dispatch(AppAction::Settings, tx)
                }))
                .child(button(a, "close-tab", "End session", |s, tx| {
                    s.dispatch(AppAction::EndTerminal, tx)
                }))
                .child(button(a, "palette", "Actions", |s, tx| {
                    s.dispatch(AppAction::CommandPalette, tx)
                }))
                .child(button(a, "quit", "Quit", |s, tx| {
                    s.dispatch(AppAction::Quit, tx)
                })),
        )
        .child(tabs);
    if state.search {
        root = root.child(
            row()
                .px(16.0)
                .py(8.0)
                .gap(8.0)
                .fill(a.panel())
                .child(
                    text_input(&state.query, ui_label())
                        .key("search-query")
                        .w(320.0)
                        .h(30.0)
                        .color(a.ink())
                        .caret(a.accent(), a.selection())
                        .on_edit(|s: &mut State| &mut s.query)
                        .on_submit(|s: &mut State, tx| {
                            s.message = if s
                                .active()
                                .is_some_and(|session| session.find(&s.query.text))
                            {
                                "Match found".into()
                            } else {
                                "No match in retained scrollback".into()
                            };
                            tx.blur();
                        }),
                )
                .child(button(a, "close-search", "Close search", |s, _| {
                    s.search = false
                })),
        );
    }
    root = root.child(workspace::body(state, a));
    let cwd = if state.preview.is_some() {
        PathBuf::from("~/projects")
    } else {
        state
            .active()
            .map(Session::cwd)
            .unwrap_or_else(|| state.directory.clone())
    };
    root = root.child(
        row()
            .w_full()
            .h(30.0)
            .px(16.0)
            .gap(10.0)
            .center_y()
            .fill(a.panel())
            .child(
                text(cwd.to_string_lossy())
                    .font(ui_label())
                    .color(a.muted()),
            )
            .child(el().grow(1.0))
            .child(
                text(if state.message.is_empty() {
                    "Ctrl+Shift+P actions · Ctrl+Shift+T new tab · Ctrl+C interrupt"
                } else {
                    &state.message
                })
                .font(ui_label())
                .color(a.muted()),
            ),
    );
    if matches!(state.pending, Some(Pending::Tab(_) | Pending::Quit)) {
        let title = match state.pending {
            Some(Pending::Quit) => "Quit Rodtty and end all sessions?",
            _ => "End this terminal session?",
        };
        root = root.child(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .fill(a.scrim())
                .center()
                .child(
                    col()
                        .key("confirmation")
                        .modal(false, |s: &mut State| s.pending = None)
                        .w(410.0)
                        .px(24.0)
                        .py(24.0)
                        .gap(16.0)
                        .radius(10.0)
                        .fill(a.panel())
                        .child(text(title).font(ui_style(17.0, 600.0)).color(a.ink()))
                        .child(
                            text("Running commands in these sessions will stop.")
                                .font(ui_label())
                                .color(a.muted()),
                        )
                        .child(
                            row()
                                .gap(8.0)
                                .child(button(a, "cancel", "Cancel", |s, _| s.pending = None))
                                .child(button(a, "confirm", "End sessions", |s, tx| s.confirm(tx))),
                        ),
                ),
        );
    }
    if let Some(dialog) = workspace::file_dialog(state, a) {
        root = root.child(dialog);
    }
    if let Some(palette) = palette::view(state, a) {
        root = root.child(palette);
    }
    root.on_paste(|s: &mut State, content, _| {
        if s.pending.is_none() && s.palette.is_none() && s.workspace.terminal_focus {
            s.paste(content);
        }
    })
}

pub fn encode_key(event: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1
        + u8::from(event.mods.shift)
        + 2 * u8::from(event.mods.alt)
        + 4 * u8::from(event.mods.ctrl);
    let arrow = |end: char| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{end}")
        } else if application_cursor {
            format!("\x1bO{end}")
        } else {
            format!("\x1b[{end}")
        }
    };
    let sequence = match &event.key {
        Key::Text(value) => {
            if event.mods.alt {
                format!("\x1b{value}")
            } else {
                value.clone()
            }
        }
        Key::Command(c) if event.mods.ctrl && c.is_ascii() => return Some(vec![(*c as u8) & 0x1f]),
        Key::Enter => "\r".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::Tab => {
            if event.mods.shift {
                "\x1b[Z".into()
            } else {
                "\t".into()
            }
        }
        Key::Up => arrow('A'),
        Key::Down => arrow('B'),
        Key::Right => arrow('C'),
        Key::Left => arrow('D'),
        Key::Home => arrow('H'),
        Key::End => arrow('F'),
        Key::Insert => "\x1b[2~".into(),
        Key::Delete => "\x1b[3~".into(),
        Key::PageUp => "\x1b[5~".into(),
        Key::PageDown => "\x1b[6~".into(),
        Key::Function(n) if (1..=4).contains(n) => format!("\x1bO{}", char::from(b'P' + n - 1)),
        Key::Function(n) if (5..=12).contains(n) => format!(
            "\x1b[{}~",
            [15, 17, 18, 19, 20, 21, 23, 24][(*n - 5) as usize]
        ),
        _ => return None,
    };
    Some(sequence.into_bytes())
}

fn on_key(state: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    if state.pending.is_some() {
        if event.key == Key::Escape {
            state.cancel_file_action();
        }
        return true;
    }
    if state.palette.is_some() {
        return state.palette_key(event, tx);
    }
    if (if cfg!(target_os = "macos") {
        event.mods.cmd
    } else {
        event.mods.ctrl
    }) && event.mods.shift
        && event.key == Key::Command('p')
    {
        state.dispatch(AppAction::CommandPalette, tx);
        return true;
    }
    let app_modifier = event.mods.cmd && (cfg!(target_os = "macos") || event.mods.shift);
    if app_modifier {
        let action = match event.key {
            Key::Command('t') => AppAction::NewTerminal,
            Key::Command('w') => AppAction::EndTerminal,
            Key::Command('q') => AppAction::Quit,
            Key::Command('e') => AppAction::ToggleExplorer,
            Key::Command('c') => AppAction::CopyTerminal,
            Key::Command('v') => AppAction::PasteTerminal,
            Key::Command('f') => AppAction::FindTerminal,
            _ => return false,
        };
        state.dispatch(action, tx);
        return true;
    }
    if tx.focused().is_some() {
        return false;
    }
    if !state.workspace.terminal_focus
        && state.workspace.layout.editor
        && !state.workspace.tabs.is_empty()
    {
        return false;
    }
    if event.mods.shift && event.key == Key::PageUp {
        if let Some(session) = state.active() {
            session.scroll(20);
        }
        return true;
    }
    if event.mods.shift && event.key == Key::PageDown {
        if let Some(session) = state.active() {
            session.scroll(-20);
        }
        return true;
    }
    if let Some(bytes) = encode_key(event, state.with_screen(vt100::Screen::application_cursor)) {
        state.send(bytes);
        true
    } else {
        false
    }
}

fn setup(state: &mut State, wake: rune::Waker) {
    state.wake = wake;
    if rune::tooling::headless_run().is_none() {
        state.new_tab();
        state.workspace_setup();
    }
}
fn poll(state: &mut State, _: f64, tx: &mut Tx) -> bool {
    let generations: Vec<_> = state
        .sessions
        .iter()
        .map(|session| {
            session
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .generation
        })
        .collect();
    let changed = generations != state.generations;
    state.generations = generations;
    let settings_changed = state.poll_settings();
    let (sans, mono) = state.config.fonts.families();
    tx.set_font_families(sans, mono);
    state.workspace_poll(tx) || changed || settings_changed
}

fn preview(state: &mut State) {
    let mut parser = vt100::Parser::new(24, 112, 100);
    parser.process(b"\x1b[38;5;79mrodtty\x1b[0m:~/projects $ seq 1 10000 | wc -l\r\n10000\r\n\r\n");
    parser.process("\x1b[38;5;79mrodtty\x1b[0m:~/projects $ printf 'Hello, terminal 🌿\\n'\r\nHello, terminal 🌿\r\n\r\n".as_bytes());
    parser.process(b"\x1b[38;5;79mrodtty\x1b[0m:~/projects $ ");
    state.preview = Some(parser.screen().clone());
    state.message = "Renderer preview · sample content".into();
}
fn workspace_preview(state: &mut State) {
    preview(state);
    workspace::demo(state, false);
}
fn palette_preview(state: &mut State) {
    workspace_preview(state);
    state.open_palette(&mut Tx::default());
    state.palette_query.focused = true;
}
fn settings_preview(state: &mut State) {
    preview(state);
    state.open_settings(&mut Tx::default());
}
fn diff_preview(state: &mut State) {
    preview(state);
    workspace::demo(state, true);
}

pub fn run() -> Result<()> {
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
            ("palette", palette_preview),
            ("settings", settings_preview),
        ],
        fonts: fonts::BUNDLED,
        families: ("Geist", "Geist Mono"),
        min_size: Some((640.0, 360.0)),
        ..AppSpec::new("Rodtty", (1080.0, 720.0), State::init, view)
    };
    let backend = Arc::new(std::sync::Mutex::new(String::new()));
    let backend_slot = backend.clone();
    let mut app = App::new(spec)
        .keep_running()
        .on_close(State::on_window_close)
        .on_quit(|s, _| {
            s.persist_workspace();
            s.sessions.clear();
        })
        .on_window(move |window| {
            let raw = window
                .with_raw_handle(|handle| format!("{:?}", handle.as_raw()))
                .unwrap_or_default();
            *backend_slot.lock().unwrap_or_else(|e| e.into_inner()) =
                raw.split('(').next().unwrap_or("unknown").to_owned();
        })
        .on_ime(|s, event, _| {
            if s.pending.is_some() || s.palette.is_some() || !s.workspace.terminal_focus {
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
    let state_handle = handle.clone();
    app = app.init(move || {
        let mut state = State::init();
        state.handle = Some(state_handle.clone());
        state
    });
    let launch_handle = handle.clone();
    let _launch_watcher = instance
        .as_ref()
        .map(|instance| instance.watch(move || launch_handle.open()))
        .transpose()?;
    let native_report = std::env::args()
        .skip_while(|arg| {
            arg != "--native-check" && arg != "--workspace-check" && arg != "--font-check"
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
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                match line.trim() {
                    "open" => handle.open(),
                    "close" => handle.close(),
                    "quit" => handle.quit(),
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

fn native_check(
    handle: rune::AppHandle<State>,
    report: PathBuf,
    backend: Arc<std::sync::Mutex<String>>,
) {
    let with_fonts = std::env::args().any(|a| a == "--font-check");
    let with_workspace = with_fonts || std::env::args().any(|a| a == "--workspace-check");
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
            // Exercise live preferences against a real running terminal session.
            // Retain the owner's original choice when this developer harness finishes.
            let (choice_tx, choice_rx) = std::sync::mpsc::channel();
            handle.update(move |s, _| {
                let _ = choice_tx.send(s.config.appearance);
            });
            let original_appearance = choice_rx.recv_timeout(Duration::from_secs(10))?;
            for appearance in [
                config::Appearance::Light,
                config::Appearance::Dark,
                config::Appearance::System,
            ] {
                handle.update(move |s, _| s.save_config(|c| c.appearance = appearance));
                wait_app(&handle, move |s| s.config.appearance == appearance)?;
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
            handle.update(move |s, _| {
                s.save_config(|c| c.appearance = original_appearance);
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
                handle.close();
                wait_app(&handle, |s| {
                    matches!(
                        s.pending,
                        Some(Pending::File(workspace::FilePrompt::Unsaved { .. }))
                    )
                })
                .context("Show unsaved close prompt")?;
                wait_visible(true)?;
                handle.update(|s, _| {
                    s.cancel_file_action();
                    let id = s.workspace.tabs[0].id;
                    s.save_file(id, false, None);
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
                Some(fixture)
            } else {
                None
            };
            if with_fonts {
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
                handle.update(|s, _| {
                    let tab = &mut s.workspace.tabs[0];
                    tab.edit.insert("// font change preserves this buffer\n");
                    tab.revision += 1;
                    s.save_config(|c| {
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
                .context("live font metrics resize the existing full-screen PTY")?;
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
                "window_closed":true,"output_while_closed":true,"reopened_from_second_launch":true,"same_session":true,"appearance_changes_preserved_session":true,"output_after_appearance_change":true});
            if with_fonts {
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
                serde_json::json!({"passed":false,"error":format!("{error:#}"),"window_error":handle.window_error()})
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
        handle.quit();
    });
}

fn wait_app(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> rune::Runtime<State> {
        let mut state = State::init();
        state.workspace.layout.explorer = true;
        runtime_for(state)
    }

    pub(super) fn runtime_for(state: State) -> rune::Runtime<State> {
        let mut runtime =
            rune::Runtime::new(state, view, (1200.0, 700.0), rune::Env::new(true, 1.0));
        runtime.on_key = Some(on_key);
        runtime.frame(0.0);
        runtime
    }

    pub(super) fn press(runtime: &mut rune::Runtime<State>, key: Key, mods: rune::Mods) {
        runtime.key(KeyEvent { key, mods });
        runtime.frame(0.0);
    }

    pub(super) fn open_palette(runtime: &mut rune::Runtime<State>) {
        press(
            runtime,
            Key::Command('p'),
            rune::Mods {
                ctrl: !cfg!(target_os = "macos"),
                cmd: true,
                shift: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn palette_search_executes_the_same_explorer_action_as_the_toolbar() {
        let mut runtime = runtime();
        assert!(runtime.find("explorer").is_some());
        open_palette(&mut runtime);
        assert!(runtime.find("command-palette").is_some());
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("command-query") && e.focused)
        );
        press(
            &mut runtime,
            Key::Text("toggle explorer".into()),
            Default::default(),
        );
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("command-palette").is_none());
        assert!(runtime.find("explorer").is_none());
        let (x, y) = runtime.find("files").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.find("explorer").is_some());
    }

    #[test]
    fn palette_shows_shortcuts_and_keeps_unavailable_commands_open() {
        let mut runtime = runtime();
        open_palette(&mut runtime);
        press(
            &mut runtime,
            Key::Text("save file".into()),
            Default::default(),
        );
        let text = runtime
            .elements()
            .iter()
            .filter_map(|e| e.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains(if cfg!(target_os = "macos") {
            "Cmd+S"
        } else {
            "Ctrl+S"
        }));
        assert!(text.contains("No open editor"));
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("command-palette").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.find("command-palette").is_none());
    }

    #[test]
    fn palette_keyboard_selection_remains_visible_at_the_minimum_window_size() {
        let mut runtime = runtime();
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.frame(0.0);
        open_palette(&mut runtime);
        let palette = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("command-palette"))
            .unwrap();
        assert!(palette.rect[0] >= 0.0 && palette.rect[0] + palette.rect[2] <= 640.0);
        assert!(palette.rect[1] >= 0.0 && palette.rect[1] + palette.rect[3] <= 360.0);
        press(&mut runtime, Key::Up, Default::default());
        assert!(runtime.find("command-quit").is_some());
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("confirmation").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.find("confirmation").is_none());
    }

    #[test]
    fn terminal_controls_are_distinct_from_desktop_shortcuts() {
        let event = KeyEvent {
            key: Key::Command('c'),
            mods: rune::Mods {
                ctrl: true,
                cmd: true,
                ..Default::default()
            },
        };
        assert_eq!(encode_key(&event, false), Some(vec![3]));
        let arrow = KeyEvent {
            key: Key::Up,
            mods: Default::default(),
        };
        assert_eq!(encode_key(&arrow, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key(&arrow, true), Some(b"\x1bOA".to_vec()));
    }
}
