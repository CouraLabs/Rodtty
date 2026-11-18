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
mod actions;
mod appearance;
mod autosave;
mod clipboard;
mod file_icon_catalog;
mod file_icons;
mod fonts;
mod frame;
mod palette;
mod tabs;
use appearance::Appearance;
mod file_operations;
mod removal;
mod settings;
mod shortcuts;
use fonts::{label as ui_label, ui as ui_style};
mod restoration;
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
    Window,
    File(workspace::FilePrompt),
    Operation(file_operations::OperationPrompt),
    Removal(removal::RemovalPrompt),
}

struct State {
    workspace: workspace::Workspace,
    tab_strips: tabs::Tabs,
    commands: Vec<actions::Command>,
    palette: Option<palette::Palette>,
    palette_query: TextEdit,
    workspace_menu: bool,
    handle: Option<rune::AppHandle<State>>,
    window: Option<rune::WindowHandle>,
    window_maximized: bool,
    /// The geometry last written to the saved layout, and when. A drag or
    /// resize moves the window many times a second; compare against this and
    /// write once it settles rather than on every host event.
    window_saved: Option<WindowGeometry>,
    window_geometry_poll: Option<std::time::Instant>,
    window_close_approved: bool,
    shutdown_saved: bool,
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
    shortcut_recording: Option<shortcuts::Recording>,
    behavior_directory: TextEdit,
    theme_family_picker: rune_kit::ComboboxState,
    settings_error: Option<String>,
    settings_seen: Option<Vec<u8>>,
    settings_poll: std::time::Instant,
    last_valid_config: Config,
    search: bool,
    query: TextEdit,
    pending: Option<Pending>,
    selection: Option<((u16, u16), (u16, u16))>,
    /// Where the current terminal press started, before it has travelled far
    /// enough to be a selection. `None` between gestures.
    drag_origin: Option<(f32, f32)>,
    terminal_context_open: bool,
    message: String,
    preview: Option<vt100::Screen>,
    terminal_rect: Option<[f32; 4]>,
    terminal_metrics: fonts::Metrics,
    font_catalog: fonts::Catalog,
    font_ui: rune_kit::ComboboxState,
    font_terminal: rune_kit::ComboboxState,
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
            tab_strips: Default::default(),
            commands,
            palette: None,
            palette_query: TextEdit::new(""),
            workspace_menu: false,
            workspace: workspace::Workspace::default(),
            handle: None,
            window: None,
            window_maximized: false,
            window_saved: None,
            window_geometry_poll: None,
            drag_origin: None,
            window_close_approved: false,
            shutdown_saved: false,
            settings_seen: std::fs::read(root.join("settings.json")).ok(),
            root,
            shell: config::shell_executable().ok(),
            behavior_directory: TextEdit::new(
                loaded
                    .as_ref()
                    .ok()
                    .map(|c| {
                        c.behavior
                            .specified_directory
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default(),
            ),
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
            shortcut_recording: None,
            theme_family_picker: Default::default(),
            settings_error: (!config_valid).then(|| message.clone()),
            settings_poll: std::time::Instant::now(),
            search: false,
            query: TextEdit::new(""),
            pending: None,
            selection: None,
            terminal_context_open: false,
            message,
            preview: None,
            terminal_rect: None,
            terminal_metrics: Default::default(),
            font_catalog: fonts::Catalog::default(),
            font_ui: Default::default(),
            font_terminal: Default::default(),
        }
    }

    pub(super) fn application_overlay_open(&self) -> bool {
        self.terminal_context_open
            || self.shortcut_recording.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|p| !matches!(p, Pending::Removal(removal::RemovalPrompt::Busy)))
            || self.palette.is_some()
            || self.workspace_menu
            || self.workspace.operation_context.is_some()
            || self.tab_strips.context_open
    }

    fn active(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    fn new_tab(&mut self) {
        let cwd = match self.config.behavior.new_terminal_directory {
            crate::config::NewTerminalDirectory::Active => self
                .active()
                .map(Session::cwd)
                .unwrap_or_else(|| self.directory.clone()),
            crate::config::NewTerminalDirectory::Explorer => self
                .workspace
                .directory
                .clone()
                .unwrap_or_else(|| self.directory.clone()),
            crate::config::NewTerminalDirectory::Specified => {
                let path = &self.config.behavior.specified_directory;
                if !path.is_absolute() {
                    self.message =
                        "Choose an absolute new-terminal directory in Behavior settings.".into();
                    return;
                }
                path.clone()
            }
        };
        self.start_tab(cwd);
    }

    fn start_tab(&mut self, cwd: PathBuf) {
        if !self.config_valid {
            return;
        }
        let cwd = match cwd.canonicalize() {
            Ok(cwd) if cwd.is_dir() => cwd,
            _ => {
                self.message = format!("Could not open terminal directory: {}", cwd.display());
                return;
            }
        };
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
            if a != b {
                let text = self.with_screen(|screen| screen.contents_between(a.0, a.1, b.0, b.1));
                if !text.is_empty() {
                    tx.copy(text);
                }
            }
        }
    }

    fn confirm(&mut self, tx: &mut Tx) {
        match self.pending.take() {
            Some(Pending::Quit) => {
                self.persist_workspace();
                self.shutdown_saved = true;
                self.sessions.clear();
                tx.quit();
            }
            Some(Pending::Tab(id)) => {
                if let Some(index) = self.sessions.iter().position(|s| tabs::session_id(s) == id) {
                    let active = self.active().map(tabs::session_id);
                    self.sessions.remove(index);
                    if index < self.generations.len() {
                        self.generations.remove(index);
                    }
                    self.active = self
                        .sessions
                        .iter()
                        .position(|s| Some(tabs::session_id(s)) == active)
                        .unwrap_or_else(|| index.min(self.sessions.len().saturating_sub(1)));
                    if active == Some(id) {
                        self.selection = None;
                    }
                }
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
        "settings" => "Open Settings",
        "workspace-menu" => "Workspace menu",
        "return-terminal" => "Return to terminal",
        "new-tab" => "New terminal tab",
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
        .no_window_drag()
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
        .child(
            text(label)
                .font(ui_label())
                .color(a.ink())
                .min_w(0.0)
                .truncate(),
        )
}

fn icon_button(
    a: &Appearance,
    key: impl Into<String>,
    glyph: &'static rune::Icon,
    tooltip: impl Into<String>,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let key = key.into();
    let tooltip = tooltip.into();
    el().key(key)
        .label(tooltip.clone())
        .help(tooltip)
        .focusable(true)
        .no_window_drag()
        .focus_ring(2.0, 1.0, a.accent())
        .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
        .w(30.0)
        .px(0.0)
        .center()
        .radius(5.0)
        .fill(a.panel())
        .hover(a.hover_bg())
        .on_press_tx(move |state: &mut State, tx| {
            tx.blur();
            action(state, tx);
        })
        .child(rune::icon(glyph, 16.0).color(a.ink()))
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
        s.supersede_pending_preview();
        s.workspace.terminal_focus = true;
        // The selection is the drag's to keep or drop: a pointer press reaches
        // the drag handler first, whose `Down` drops any earlier selection, and
        // this press also follows the release of a drag that just selected, so
        // clearing here would erase every selection the moment it was made.
        tx.blur();
    })
    .on_secondary_press(clipboard::terminal_menu)
    .on_drag(move |s: &mut State, drag, tx| {
        s.supersede_pending_preview();
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
            s.drag_origin = None;
            return;
        }
        match drag.phase {
            Phase::Down => {
                // Record the press but select nothing yet: a click that drifts
                // a few points on a trackpad would otherwise highlight a cell
                // the owner never meant to touch, and a click is not a select.
                s.selection = None;
                s.drag_origin = Some((drag.x, drag.y));
            }
            Phase::Move => {
                let Some((sx, sy)) = s.drag_origin else {
                    return;
                };
                if s.selection.is_none() {
                    let travel = (drag.x - sx).hypot(drag.y - sy);
                    if travel < SELECTION_THRESHOLD {
                        return;
                    }
                    s.selection = Some((point(sx, sy), point(drag.x, drag.y)));
                } else {
                    let (start, _) = s.selection.unwrap_or((point(sx, sy), point(sx, sy)));
                    s.selection = Some((start, point(drag.x, drag.y)));
                }
            }
            Phase::Up => {
                let extended = s.selection.is_some();
                s.drag_origin = None;
                if extended && s.config.terminal_clipboard.copy_on_selection {
                    s.copy(tx);
                }
                // A press that never became a selection leaves nothing behind.
                if !extended {
                    s.selection = None;
                }
            }
        }
    })
    .on_wheel(move |s: &mut State, event, _| {
        if let Some(session) = s.active() {
            session.scroll((-event.dy / ch).round() as isize);
        }
        true
    })
}

fn header_height() -> f32 {
    44.0_f32.max(ui_label().size * 1.5 + 18.0)
}

/// A display path with the owner's home folder as `~`, so `/Users/rodcoura/…`
/// and `/home/…/…` both read as `~/…`. The owner's actual home folder, whatever
/// the host calls it, comes from the same source the app root does.
fn tilde_path(path: &std::path::Path) -> String {
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()) else {
        return path.display().to_string();
    };
    if path == home {
        return "~".into();
    }
    if let Some(rest) = path
        .strip_prefix(&home)
        .ok()
        .filter(|rest| !rest.as_os_str().is_empty())
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// How far the pointer travels before a terminal press becomes a selection, in
/// points. Below this it is a click: a trackpad's contact wobble and a slow
/// click both stay under it, so neither highlights a cell the owner never
/// meant to select and a click never copies.
pub(super) const SELECTION_THRESHOLD: f32 = 4.0;

/// A titlebar tab's height, in points. Shorter than the band so the pills keep
/// a margin above and below at any UI zoom.
const HEADER_TAB_HEIGHT: f32 = 26.0;

/// The terminal tabs, laid out as the titlebar's own strip: each tab a pill,
/// the active one filled, its close button beside the label, and the new-tab
/// button after the last tab. The strip takes the width it is given and
/// scrolls rather than pushing the header's actions off the edge.
fn terminal_tabs(state: &State, a: &Appearance) -> El {
    let mut tabs = row()
        .key("tabs")
        .h(HEADER_TAB_HEIGHT)
        .gap(4.0)
        .min_w(0.0)
        .center_y();
    // Sized to its tabs so the new-tab button follows the last one, but free
    // to shrink below them so a crowded strip scrolls inside the header.
    let mut list = row()
        .key("terminal-tabs")
        .shrink()
        .scroll_x()
        .gap(4.0)
        .center_y();
    for (i, session) in state.sessions.iter().enumerate() {
        let label = session.tab_title();
        let id = tabs::session_id(session);
        let selected = i == state.active;
        list = list.child(
            row()
                .key(format!("terminal-group-{id}"))
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if selected { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .no_window_drag()
                // The pill only draws; the label below carries the activation,
                // because interactive()'s drag and middle handlers make the label
                // its own press target and would otherwise swallow the press.
                .child(tabs::interactive(
                    tab_label(a, &label, selected)
                        .key(format!("tab-{i}"))
                        .help(&label)
                        .on_press_tx(move |s: &mut State, tx| {
                            tx.blur();
                            s.dispatch(AppAction::ActivateTerminal(i), tx);
                        }),
                    tabs::Target::Terminal(id),
                ))
                .child(
                    icon_button(
                        a,
                        format!("terminal-close-{i}"),
                        &rune_kit::glyphs::CLOSE,
                        "End terminal session",
                        move |s, tx| s.dispatch(AppAction::CloseTerminal(id), tx),
                    )
                    .w(22.0)
                    .min_h(22.0)
                    .fill(if selected { a.hover_bg() } else { a.panel() }),
                ),
        );
    }
    if state.preview.is_some() {
        list = list.child(
            icon_button(
                a,
                "preview-tab",
                &rune_kit::glyphs::TERMINAL,
                "Show terminal",
                |s, tx| s.dispatch(AppAction::ShowTerminal, tx),
            )
            .fill(a.hover_bg()),
        );
    }
    tabs = tabs
        .child(tabs::scroll_strip(list, state, true))
        .child(icon_button(
            a,
            "new-tab",
            &rune_kit::glyphs::PLUS,
            "New terminal tab",
            |s, tx| s.dispatch(AppAction::NewTerminal, tx),
        ));
    tabs
}

/// A tab's label: the text, tinted for whether the tab is the active one and
/// sized to sit in the strip.
fn tab_label(a: &Appearance, label: &str, selected: bool) -> El {
    el().px(9.0).h(HEADER_TAB_HEIGHT).center().child(
        text(label)
            .font(ui_label())
            .color(if selected { a.ink() } else { a.muted() })
            .max_w(150.0)
            .truncate(),
    )
}

fn workspace_menu_items() -> [(&'static str, &'static str, AppAction); 6] {
    [
        ("palette", "Command Palette…", AppAction::CommandPalette),
        ("menu-settings", "Settings", AppAction::Settings),
        ("find", "Find in Terminal…", AppAction::FindTerminal),
        ("close-tab", "End Terminal Session…", AppAction::EndTerminal),
        ("menu-close-window", "Close Window", AppAction::CloseWindow),
        ("quit", "Quit Rodtty…", AppAction::Quit),
    ]
}

fn workspace_menu(state: &State, a: &Appearance) -> El {
    let mut menu = col()
        .key("workspace-menu-popover")
        .w(276.0)
        .self_start()
        .px(6.0)
        .py(6.0)
        .gap(3.0)
        .radius(10.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg())
        .modal_tx(true, |s: &mut State, tx| {
            s.workspace_menu = false;
            tx.focus("workspace-menu", true);
        });
    for (key, title, action) in workspace_menu_items() {
        menu = menu.child(
            button(a, key, title, move |s, tx| s.dispatch(action, tx))
                .help(state.action_help(action, title))
                .w_full()
                .justify_start()
                .px(12.0),
        );
    }
    el().absolute(0.0, header_height())
        .w_full()
        .h_full()
        .child(row().w_full().px(8.0).child(el().grow(1.0)).child(menu))
}

fn view(state: &State, cx: &mut Cx<State>) -> El {
    view_with_frame(state, cx, frame::placement())
}

fn view_with_frame(state: &State, cx: &mut Cx<State>, placement: frame::Placement) -> El {
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
    let width = cx
        .layout_size("root")
        .map(|r| r[0])
        .unwrap_or(state.workspace.body_width);
    let (show_explorer, show_terminal, show_editor) = workspace::pane_visibility(state, width);
    let mut root = col()
        .key("root")
        .pointer_buttons([rune::PointerButton::Back, rune::PointerButton::Forward])
        .on_background_press_tx(|s: &mut State, tx| {
            s.dispatch_mouse_shortcut(tx.button(), tx);
        })
        .on_middle_press(|s: &mut State, tx| {
            s.dispatch_mouse_shortcut(rune::PointerButton::Middle, tx);
        })
        .on_layout(|s: &mut State, r| s.workspace.operation_viewport = (r[2], r[3]))
        .w_full()
        .h_full()
        .fill(a.bg())
        .padding(if frame::borderless() {
            frame::BORDER
        } else {
            0.0
        })
        .border(if frame::borderless() { 1.0 } else { 0.0 }, a.hover_bg())
        .child(
            row()
                .key("workspace-header")
                .window_drag()
                .instant_changes()
                .w_full()
                .h(header_height())
                // A hosted titlebar draws the window's own controls over this
                // band: keep the header's content clear of them.
                .pl(if placement == frame::Placement::Hosted {
                    cx.chrome_insets().leading
                } else {
                    0.0
                } + if width < 880.0 { 4.0 } else { 12.0 })
                .pr(if width < 880.0 { 4.0 } else { 12.0 })
                .gap(if width < 880.0 { 3.0 } else { 6.0 })
                .center_y()
                .fill(a.panel())
                .child(terminal_tabs(state, a).grow(1.0).min_w(0.0))
                .child(el().grow(0.0))
                .child(
                    icon_button(
                        a,
                        "files",
                        &rune_kit::glyphs::SIDEBAR,
                        state.shortcut_help(
                            "toggle-explorer",
                            if show_explorer {
                                "Hide file explorer"
                            } else {
                                "Show file explorer"
                            },
                        ),
                        |s, tx| s.dispatch(AppAction::ToggleExplorer, tx),
                    )
                    .fill(if show_explorer {
                        a.hover_bg()
                    } else {
                        a.panel()
                    }),
                )
                .child(
                    icon_button(
                        a,
                        "toggle-terminal",
                        &rune_kit::glyphs::TERMINAL,
                        if width < 880.0 {
                            "Show terminal"
                        } else if show_terminal {
                            "Hide terminal pane"
                        } else {
                            "Show terminal pane"
                        },
                        |s, tx| {
                            if s.workspace.body_width < 880.0 {
                                s.dispatch(AppAction::ShowTerminal, tx)
                            } else {
                                s.dispatch(AppAction::ToggleTerminal, tx)
                            }
                        },
                    )
                    .fill(if show_terminal {
                        a.hover_bg()
                    } else {
                        a.panel()
                    }),
                )
                .child(
                    icon_button(
                        a,
                        "toggle-editor",
                        &rune_kit::glyphs::PANE_EDIT,
                        if width < 880.0 {
                            "Show editor"
                        } else if show_editor {
                            "Hide editor pane"
                        } else {
                            "Show editor pane"
                        },
                        |s, tx| s.dispatch(AppAction::ToggleEditor, tx),
                    )
                    .fill(if show_editor { a.hover_bg() } else { a.panel() }),
                )
                .child(icon_button(
                    a,
                    "workspace-menu",
                    &rune_kit::glyphs::ELLIPSIS,
                    "Workspace menu",
                    |s, tx| {
                        s.workspace_menu = !s.workspace_menu;
                        if s.workspace_menu {
                            tx.focus_when_shown("palette", true);
                        }
                    },
                ))
                .child(frame::controls(state, a, placement)),
        );
    root = root.child(workspace::body(state, a, width));

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
            .key("app-footer")
            .w_full()
            // A one-point frame on every side keeps the footer off the window edge.
            .padding(1.0)
            .fill(a.bg())
            .child(
                row()
                    .key("app-footer-content")
                    .w_full()
                    .h(25.0_f32.max(ui_label().size * 1.5 + 4.0))
                    .px(10.0)
                    .gap(10.0)
                    .center_y()
                    .fill(a.panel())
                    .child(
                        text(concat!("Rodtty ", env!("CARGO_PKG_VERSION")))
                            .key("footer-app-name")
                            .font(ui_style(13.0, 700.0))
                            .color(a.ink())
                            .shrink(),
                    )
                    .child(
                        text(tilde_path(&cwd))
                            .key("footer-path")
                            .font(ui_label())
                            .color(a.muted())
                            .truncate()
                            .min_w(0.0)
                            .grow(1.0),
                    )
                    .child(
                        text(if state.message.is_empty() {
                            format!("{}  Actions", state.shortcut_label("command-palette"))
                        } else {
                            state.message.clone()
                        })
                        .key("footer-message")
                        .font(ui_label())
                        .color(a.muted())
                        .truncate()
                        .min_w(0.0)
                        .shrink(),
                    ),
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
    if matches!(state.pending, Some(Pending::Window)) {
        root = root.child(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .fill(a.scrim())
                .center()
                .child(
                    col()
                        .key("window-close-choice")
                        .modal(false, |s: &mut State| s.pending = None)
                        .w(410.0)
                        .px(24.0)
                        .py(24.0)
                        .gap(16.0)
                        .radius(10.0)
                        .fill(a.panel())
                        .child(
                            text("Close Rodtty window?")
                                .font(ui_style(17.0, 600.0))
                                .color(a.ink()),
                        )
                        .child(
                            text("Keep terminal sessions running or quit Rodtty and end them.")
                                .font(ui_label())
                                .color(a.muted())
                                .wrap(),
                        )
                        .child(button(
                            a,
                            "keep-sessions",
                            "Keep sessions running",
                            |s, tx| {
                                s.pending = None;
                                s.request_action(workspace::Action::KeepWindow, tx);
                            },
                        ))
                        .child(button(a, "quit-sessions", "Quit Rodtty", |s, tx| {
                            s.pending = None;
                            s.request_action(workspace::Action::Quit, tx);
                        }))
                        .child(button(a, "window-close-cancel", "Cancel", |s, _| {
                            s.pending = None
                        })),
                ),
        );
    }
    if let Some(dialog) = workspace::file_dialog(state, a) {
        root = root.child(dialog);
    }
    if state.workspace_menu {
        root = root.child(workspace_menu(state, a));
    }
    if let Some(dialog) = removal::view(state, a) {
        root = root.child(dialog);
    }
    if let Some(dialog) = file_operations::view(state, a) {
        root = root.child(dialog);
    }
    if let Some(palette) = palette::view(state, a) {
        root = root.child(palette);
    }
    if let Some(recorder) = shortcuts::recorder(state, a) {
        root = root.child(recorder);
    }
    root = root.children(fonts::overlays(state, a, width));
    root = root.children(settings::overlays(state, a, width));
    root.on_paste(|s: &mut State, content, _| {
        if !s.application_overlay_open() && s.workspace.terminal_focus {
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
    if state.shortcut_recording.is_some() {
        return state.record_shortcut(event, tx);
    }
    if state.terminal_context_open || state.tab_strips.context_open {
        return true;
    }
    if state.removal_background_busy() {
        if state.dispatch_shortcut(event, tx) {
            return true;
        }
        if state.application_overlay_open()
            || !state.workspace.terminal_focus
            || !state.workspace.layout.terminal
            || tx.focused().is_some()
        {
            return true;
        }
        if let Some(bytes) = encode_key(event, state.with_screen(vt100::Screen::application_cursor))
        {
            state.send(bytes);
        }
        return true;
    }
    if state.pending.is_some() {
        if event.key == Key::Escape {
            if state.workspace.removal.is_some() {
                state.cancel_removal(tx);
            } else {
                state.cancel_file_action();
            }
            return true;
        }
        return !matches!(
            state.pending,
            Some(Pending::Operation(_) | Pending::Removal(removal::RemovalPrompt::SaveAs(_)))
        );
    }
    if state.workspace.operation_context.is_some() {
        if event.key == Key::Escape {
            state.workspace.operation_context = None;
            return true;
        }
        return false;
    }
    if state.palette.is_some() {
        return state.palette_key(event, tx);
    }
    if state.workspace_menu {
        let items = workspace_menu_items();
        let index = items
            .iter()
            .position(|(key, _, _)| Some(*key) == tx.focused())
            .unwrap_or(0);
        match event.key {
            Key::Escape => {
                state.workspace_menu = false;
                tx.focus("workspace-menu", true);
            }
            Key::Up | Key::Down => {
                let next = if event.key == Key::Down {
                    (index + 1) % items.len()
                } else {
                    (index + items.len() - 1) % items.len()
                };
                tx.focus(items[next].0, true);
            }
            Key::Enter => state.dispatch(items[index].2, tx),
            _ => return false,
        }
        return true;
    }
    if state.dispatch_shortcut(event, tx) {
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
    state.font_catalog.start(state.wake.clone());
    if rune::tooling::headless_run().is_none() {
        state.restore_startup();
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
    if changed {
        // New output moves the cells a highlight was drawn over. Keep it only
        // while a gesture is still extending it, where the drag re-derives the
        // range on its next move anyway.
        if state.drag_origin.is_none() {
            state.selection = None;
        }
    }
    let fonts_changed = state.font_catalog.poll();
    let settings_changed = state.poll_settings();
    let (sans, mono) = state.font_families();
    tx.set_font_families(sans, mono);
    let metrics_changed = state.refresh_terminal_metrics();
    let workspace_changed = state.workspace_poll(tx);
    if state.workspace.last_persist.elapsed() >= std::time::Duration::from_secs(1) {
        state.persist_workspace();
    }
    workspace_changed
        || changed
        || settings_changed
        || metrics_changed
        || frame_changed
        || geometry_changed
        || fonts_changed
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
fn tabs_preview(state: &mut State) {
    workspace::demo(state, false);
    state.preview = None;
    let fixture = tempfile::tempdir().expect("tab preview directory");
    let Some(shell) = state.shell.clone() else {
        return;
    };
    state.root = fixture.path().to_owned();
    state.directory = state.root.clone();
    let explorer_root = state.root.join("Rodtty");
    for index in 0..7 {
        let cwd = explorer_root.join(if index % 2 == 0 { "client" } else { "host" });
        std::fs::create_dir_all(&cwd).unwrap();
        let session = Session::spawn(
            &shell,
            &state.root,
            &cwd,
            Config::default(),
            28,
            100,
            Arc::new(|| {}),
        )
        .unwrap();
        session
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        if index == 0 {
            session.send(format!("printf '\\033]2;{}\\007\\033[2J\\033[H'; printf 'TITLE_READY\\n'\r",
                "Agent 日本語 — preserving the complete application title while the label visually ellipsizes ".repeat(3)).into_bytes()).unwrap();
            session
                .wait_for("TITLE_READY", std::time::Duration::from_secs(15))
                .unwrap();
        }
        state.sessions.push(session);
    }
    state.workspace.tabs.clear();
    state.workspace.worker = Some(crate::work::Worker::new(state.wake.clone()));
    for (index, folder) in ["client", "host", "shared", "packages", "tests"]
        .iter()
        .enumerate()
    {
        let path = explorer_root.join(folder).join("config.rs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "// Tab preview\nfn main() {}\n").unwrap();
        state.open_file_with_preview(path, index == 4);
        let started = std::time::Instant::now();
        while state.workspace.tabs.len() <= index {
            state.workspace_poll(&mut Tx::default());
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    state.navigate(explorer_root.clone());
    let started = std::time::Instant::now();
    while !state
        .workspace
        .entries
        .iter()
        .any(|entry| entry.path.starts_with(&explorer_root))
    {
        state.workspace_poll(&mut Tx::default());
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    state.message = "Tab strip preview · running PTYs and real file buffers".into();
    state.workspace.tabs[0].edit.insert("// Unsaved changes\n");
    state.workspace.active = 0;
    state.workspace.terminal_focus = true;
    state.active = 0;
    state.tab_strips.preview_root = Some(fixture);
}
fn tabs_light_preview(state: &mut State) {
    tabs_preview(state);
    state.config.appearance = config::Appearance::Light;
}
fn tabs_dark_preview(state: &mut State) {
    tabs_preview(state);
    state.config.appearance = config::Appearance::Dark;
}
fn tabs_light_editor_preview(state: &mut State) {
    tabs_light_preview(state);
    state.workspace.terminal_focus = false;
}
fn tabs_dark_editor_preview(state: &mut State) {
    tabs_dark_preview(state);
    state.workspace.terminal_focus = false;
}
fn preview_tabs_scene(state: &mut State) {
    tabs_preview(state);
    let first = state.workspace.tabs.first().map(|tab| tab.id);
    state
        .workspace
        .tabs
        .retain(|tab| Some(tab.id) == first || tab.preview);
    state.workspace.active = state
        .workspace
        .tabs
        .iter()
        .position(|tab| tab.preview)
        .unwrap_or(0);
    state.workspace.terminal_focus = false;
    state.message = "Regular dirty file and temporary preview · real file buffers".into();
}
fn tree_preview(state: &mut State) {
    preview(state);
    workspace::host_tree_demo(state);
}
fn icons_preview(state: &mut State) {
    preview(state);
    workspace::icons_demo(state);
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
fn behavior_preview(state: &mut State) {
    settings_preview(state);
    state.settings_query.replace_text("Behavior");
}
fn real_diff_preview(state: &mut State) {
    preview(state);
    if let Err(error) = workspace::real_diff_demo(state) {
        state.message = format!("Could not render real Git fixture: {error:#}");
    }
}
fn file_collision_preview(state: &mut State) {
    workspace_preview(state);
    file_operations::collision_demo(state);
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
            s.shutdown_saved = true;
            s.sessions.clear();
        })
        .on_ime(|s, event, _| {
            if s.application_overlay_open() || !s.workspace.terminal_focus {
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
    app = app.window_options(frame::options());
    if let Some(titlebar) = frame::titlebar() {
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

fn native_check(
    handle: rune::AppHandle<State>,
    report: PathBuf,
    backend: Arc<std::sync::Mutex<String>>,
) {
    let with_trash = std::env::args().any(|a| a == "--trash-check");
    if std::env::args().any(|a| a == "--restoration-check") {
        restoration::native_check(handle, report, backend);
        return;
    }
    let with_frame = std::env::args().any(|a| a == "--frame-check");
    let with_fonts = std::env::args().any(|a| a == "--font-check");
    let with_sessions = std::env::args().any(|a| a == "--session-policy-check");
    let with_workspace = with_frame
        || with_fonts
        || with_sessions
        || std::env::args().any(|a| a == "--workspace-check");
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
            if std::env::args().any(|a| a == "--clipboard-check") {
                let mut result = clipboard::native_check(&handle, &report)?;
                result["window_backend"] = backend
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                    .into();
                return Ok(result);
            }
            let trash_evidence = if with_trash {
                Some(removal::native_check(&handle)?)
            } else {
                None
            };
            let frame_evidence = if with_frame {
                // Real editor work makes the native custom-close check exercise the
                // same dirty-file decision as the menu and OS close lifecycle.
                let fixture = tempfile::tempdir()?;
                let file = fixture.path().join("frame-close-check.txt");
                std::fs::write(&file, "saved frame fixture\n")?;
                handle.update(move |s, _| s.open_file(file));
                wait_app(&handle, |s| !s.workspace.tabs.is_empty())?;
                handle.update(|s, _| {
                    s.workspace.tabs[0].edit.insert("unsaved frame fixture\n");
                    s.workspace.tabs[0].revision += 1;
                });
                wait_app(&handle, |s| s.workspace.tabs[0].dirty())?;
                let (info_tx, info_rx) = std::sync::mpsc::channel();
                handle.update(move |s, _| {
                    let info = s.window.as_ref().and_then(|w| w.with_winit_window(|w| {
                        let size = w.inner_size();
                        serde_json::json!({"position":w.inner_position().ok().map(|p| [p.x,p.y]),"decorated":w.is_decorated(), "maximized":w.is_maximized(),
                            "minimized":w.is_minimized(),"size":[size.width,size.height],"scale":w.scale_factor()})
                    }).ok());
                    let _ = info_tx.send(info);
                });
                let info = info_rx
                    .recv_timeout(Duration::from_secs(10))?
                    .context("frame native window missing")?;
                // macOS keeps its native frame so the traffic lights survive; the
                // app-drawn frames are the other two, and they must be borderless.
                let expected_decorated = cfg!(target_os = "macos");
                anyhow::ensure!(
                    info["decorated"] == expected_decorated,
                    "expected decorated == {expected_decorated} on this platform: {info}"
                );
                std::fs::write(
                    report.with_extension("ready.json"),
                    serde_json::to_vec_pretty(&info)?,
                )?;
                let input = report.with_extension("input.json");
                let start = Instant::now();
                while !input.exists() && start.elapsed() < Duration::from_secs(120) {
                    std::thread::sleep(Duration::from_millis(30));
                }
                let evidence: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&input)
                        .context("external native frame input did not complete")?,
                )?;
                anyhow::ensure!(
                    evidence["passed"] == true,
                    "external frame input failed: {evidence}"
                );
                handle.update(|s, _| {
                    s.workspace.tabs.clear();
                    s.workspace.terminal_focus = true;
                });
                wait_app(&handle, |s| s.workspace.tabs.is_empty())?;
                Some(serde_json::json!({"initial_native_state":info,"compositor_input":evidence}))
            } else {
                None
            };
            // Exercise live preferences against a real running terminal session.
            // Retain the owner's original choice when this developer harness finishes.
            let (choice_tx, choice_rx) = std::sync::mpsc::channel();
            handle.update(move |s, _| {
                let _ = choice_tx.send((s.config.theme_family, s.config.appearance));
            });
            let (original_family, original_appearance) =
                choice_rx.recv_timeout(Duration::from_secs(10))?;
            for &family in config::ThemeFamily::ALL {
                for appearance in [
                    config::Appearance::Light,
                    config::Appearance::Dark,
                    config::Appearance::System,
                ] {
                    handle.update(move |s, _| {
                        s.save_config(|c| {
                            c.theme_family = family;
                            c.appearance = appearance;
                        })
                    });
                    wait_app(&handle, move |s| {
                        s.config.theme_family == family && s.config.appearance == appearance
                    })?;
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
            }
            handle.update(move |s, _| {
                s.save_config(|c| { c.theme_family = original_family; c.appearance = original_appearance; });
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
                handle.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
                wait_app(&handle, |s| {
                    matches!(
                        s.pending,
                        Some(Pending::File(workspace::FilePrompt::Unsaved { .. }))
                    )
                })
                .context("Show unsaved close prompt")?;
                wait_visible(true)?;
                handle.update(move |s, _| {
                    let id = s.workspace.tabs[0].id;
                    if with_sessions {
                        // Choose Save in the actual dirty-close decision. Its
                        // continuation must close only after the protected save.
                        s.save_file(id, false, Some(workspace::Action::Window));
                    } else {
                        s.cancel_file_action();
                        s.save_file(id, false, None);
                    }
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
                if with_sessions {
                    wait_visible(false)?;
                    let code = std::process::Command::new(std::env::current_exe()?).status()?;
                    anyhow::ensure!(code.success(), "Save/close relaunch failed");
                    wait_visible(true)?;
                }
                Some(fixture)
            } else {
                None
            };
            let mut font_inventory = None;
            if with_fonts {
                wait_app(&handle, |s| s.font_catalog.ready)?;
                let (tx, rx) = std::sync::mpsc::channel();
                handle.update(move |s, _| {
                    let mono = s
                        .font_catalog
                        .families
                        .iter()
                        .find(|f| !f.bundled && f.monospace)
                        .map(|f| f.name.clone());
                    let sans = s
                        .font_catalog
                        .families
                        .iter()
                        .find(|f| !f.bundled && !f.monospace)
                        .map(|f| f.name.clone());
                    let _ = tx.send((s.font_catalog.families.len(), mono, sans));
                });
                let (count, mono, sans) = rx.recv_timeout(Duration::from_secs(10))?;
                let mono = mono.context("installed monospace family")?;
                let sans = sans.context("installed UI family")?;
                font_inventory = Some(
                    serde_json::json!({"families":count,"terminal_family":mono,"ui_family":sans}),
                );
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
                handle.update(move |s, _| {
                    let tab = &mut s.workspace.tabs[0];
                    tab.edit.insert("// font change preserves this buffer\n");
                    tab.revision += 1;
                    s.save_config(|c| {
                        c.fonts.ui_family = sans.clone();
                        c.fonts.terminal_family = mono.clone();
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
                .with_context(|| {
                    let terminal = session.lock().unwrap_or_else(|e| e.into_inner());
                    format!("live font metrics resize the existing full-screen PTY; before {before:?}; after {:?}; output {:?}", terminal.parser.screen().size(), terminal.parser.screen().contents())
                })?;
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
            let session_policies = if with_sessions {
                Some(native_session_policies(&handle, &wait_visible)?)
            } else {
                None
            };
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
                "window_closed":true,"output_while_closed":true,"reopened_from_second_launch":true,"same_session":true,"appearance_changes_preserved_session":true,"theme_families_preserved_session":9,"family_mode_changes":27,"output_after_appearance_change":true});
            if let Some(trash) = trash_evidence {
                report["trash"] = trash;
            }
            if let Some(policies) = session_policies {
                report["session_policies"] = policies;
                wait_app(&handle, |s| {
                    s.sessions.len() == 1
                        && s.active()
                            .is_some_and(|session| !session.has_running_commands())
                })?;
            }
            if let Some(frame) = frame_evidence {
                report["custom_frame"] = frame;
            }
            if with_fonts {
                report["font_inventory"] = font_inventory.unwrap();
                report["live_font_families"] = true.into();
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
                serde_json::json!({"passed":false,"error":format!("{error:#}"),"window_backend":backend.lock().unwrap_or_else(|e|e.into_inner()).clone(),"window_error":handle.window_error()})
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
        if with_sessions {
            handle.update(|s, tx| s.dispatch(AppAction::Quit, tx));
        } else {
            handle.quit();
        }
    });
}

/// Native lifecycle regression: actual hide/reopen notifications share the
/// same policy boundary as explicit app actions. Every fixture uses this host's
/// isolated owner root and the caller's per-ticket TMPDIR.
fn native_session_policies(
    handle: &rune::AppHandle<State>,
    wait_visible: &impl Fn(bool) -> Result<()>,
) -> Result<serde_json::Value> {
    use crate::config::{
        NewTerminalDirectory as Directory, TerminalClosePolicy as Tab, WindowClosePolicy as Window,
    };
    use std::time::Duration;
    wait_app(handle, |s| {
        s.active()
            .is_some_and(|session| !session.has_running_commands())
    })
    .context("initial idle shell")?;
    let (tx, rx) = std::sync::mpsc::channel();
    handle.update(move |s, _| {
        let first = s.active().unwrap();
        let _ = tx.send((first.id(), first.cwd(), first.config.clone()));
    });
    let (first, initial_cwd, initial_config) = rx.recv_timeout(Duration::from_secs(10))?;
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::AlwaysConfirm);
        s.dispatch(AppAction::EndTerminal, tx);
    });
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Tab(_))))
        .context("Always confirm idle tab")?;
    handle.update(|s, _| {
        s.cancel_file_action();
        s.save_config(|c| c.behavior.window_close = Window::AskEachTime);
    });
    wait_app(handle, |s| {
        s.pending.is_none() && s.config.behavior.window_close == Window::AskEachTime
    })
    .context("Ask policy applied before native close")?;
    handle.close();
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Window)))
        .context("Ask native close prompt")?;
    wait_visible(true)?;
    handle.update(|s, _| s.cancel_file_action());
    wait_app(handle, move |s| {
        s.pending.is_none() && s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("Ask cancellation preserves session")?;
    handle.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Window)))
        .context("Ask explicit close prompt")?;
    handle.update(|s, tx| {
        s.pending = None;
        s.request_action(workspace::Action::KeepWindow, tx);
    });
    wait_visible(false)?;
    let code = std::process::Command::new(std::env::current_exe()?).status()?;
    anyhow::ensure!(code.success(), "Ask/Keep relaunch failed");
    wait_visible(true)?;
    wait_app(handle, move |s| {
        s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("Ask Keep reopens same session")?;
    handle.update(|s, _| {
        s.save_config(|c| {
            c.behavior.terminal_tab_close = Tab::ConfirmRunning;
            c.behavior.window_close = Window::QuitRodtty;
        });
        s.send(b"printf '\\033]133;C\\007\\033]133;A\\007'; printf 'POLICY_%s\\n' BLOCKED; read session_policy_answer\r".to_vec());
    });
    wait_app(handle, |s| {
        s.active()
            .unwrap()
            .screen()
            .contents()
            .contains("POLICY_BLOCKED")
            && s.active().unwrap().has_running_commands()
    })
    .context("public shell integration leaves builtin activity protected")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Tab(_))))
        .context("confirm-running protects builtin after public prompt marker")?;
    handle.update(|s, tx| {
        s.cancel_file_action();
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::CloseImmediately);
        s.dispatch(AppAction::Quit, tx);
    });
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Quit)))
        .context("explicit Quit running confirmation")?;
    wait_visible(true)?;
    handle.update(|s, _| s.cancel_file_action());
    wait_app(handle, |s| {
        s.pending.is_none() && s.config.behavior.window_close == Window::QuitRodtty
    })
    .context("Quit cancellation applied before native close")?;
    handle.close();
    wait_app(handle, |s| matches!(s.pending, Some(Pending::Quit)))
        .context("native Quit policy confirmation")?;
    wait_visible(true)?;
    handle.update(|s, _| {
        s.cancel_file_action();
        s.send(vec![3]);
    });
    wait_app(handle, |s| !s.active().unwrap().has_running_commands())
        .context("interrupt returns to idle")?;

    let fixture = tempfile::tempdir()?;
    let active = fixture.path().join("active");
    let explorer = fixture.path().join("explorer");
    let specified = fixture.path().join("specified");
    for path in [&active, &explorer, &specified] {
        std::fs::create_dir(path)?;
    }
    let command = format!("cd {}\r", profile::quote(&active)?);
    handle.update(move |s, _| s.send(command.into_bytes()));
    let expected = active.clone();
    wait_app(handle, move |s| {
        s.active().unwrap().cwd() == expected && !s.active().unwrap().has_running_commands()
    })
    .context("active session directory change")?;
    let root = explorer.clone();
    handle.update(move |s, tx| {
        s.navigate(root);
        s.save_config(|c| {
            c.behavior.new_terminal_directory = Directory::Active;
            c.history = false;
            c.disabled = vec!["cat".into()];
        });
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 2
            && s.active().unwrap().cwd() == active
            && !s.active().unwrap().has_running_commands()
    })
    .context("active directory new tab")?;
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.new_terminal_directory = Directory::Explorer);
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 3
            && s.active().unwrap().cwd() == explorer
            && !s.active().unwrap().has_running_commands()
    })
    .context("explorer directory new tab")?;
    let expected = specified.clone();
    handle.update(move |s, tx| {
        s.save_config(|c| {
            c.behavior.new_terminal_directory = Directory::Specified;
            c.behavior.specified_directory = specified;
        });
        s.dispatch(AppAction::NewTerminal, tx);
    });
    wait_app(handle, move |s| {
        s.sessions.len() == 4
            && s.active().unwrap().cwd() == expected
            && !s.active().unwrap().has_running_commands()
    })
    .context("specified directory new tab")?;
    let snapshot = initial_config.clone();
    wait_app(handle, move |s| {
        s.sessions[0].id() == first
            && s.sessions[0].config.history == snapshot.history
            && s.sessions[0].config.disabled == snapshot.disabled
            && !s.active().unwrap().config.history
            && s.active().unwrap().config.disabled == ["cat"]
    })
    .context("old and new command snapshots")?;
    // Confirm-running closes an idle new shell directly.
    handle.update(|s, tx| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::ConfirmRunning);
        s.dispatch(AppAction::EndTerminal, tx);
    });
    wait_app(handle, |s| s.sessions.len() == 3 && s.pending.is_none())
        .context("confirm-running closes idle tab")?;
    // Close-immediately also applies to a busy shell without changing Quit's rule.
    handle.update(|s, _| {
        s.save_config(|c| c.behavior.terminal_tab_close = Tab::CloseImmediately);
        s.send(b"read policy_close_answer\r".to_vec());
    });
    wait_app(handle, |s| s.active().unwrap().has_running_commands())
        .context("new builtin activity")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, |s| s.sessions.len() == 2 && s.pending.is_none())
        .context("immediate closes busy tab")?;
    handle.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
    wait_app(handle, move |s| {
        s.sessions.len() == 1 && s.active().unwrap().id() == first
    })
    .context("remaining new tab close")?;
    let command = format!("cd {}\r", profile::quote(&initial_cwd)?);
    let expected = initial_cwd.clone();
    handle.update(move |s, _| {
        s.send(command.into_bytes());
        s.save_config(|c| {
            c.behavior = Default::default();
            c.history = initial_config.history;
            c.disabled = initial_config.disabled;
        });
        s.navigate(initial_cwd);
    });
    wait_app(handle, move |s| {
        s.active().unwrap().cwd() == expected && !s.active().unwrap().has_running_commands()
    })
    .context("original directory and idle restore")?;
    Ok(serde_json::json!({
        "ask_native_close_cancel":true, "ask_keep_same_session_relaunch":true,
        "quit_running_confirmation":true, "quit_policy_native_close_confirmation":true,
        "public_prompt_markers_preserve_running_confirmation":true,
        "confirm_running_busy_tab_after_public_marker":true,
        "always_confirm_idle_tab":true, "confirm_running_idle_tab_closes":true,
        "close_immediately_busy_tab":true, "new_tab_directory_choices":3,
        "running_session_configuration_snapshot":true, "cancel_preserved_session":true,
        "explicit_quit_after_check":true, "dirty_close_save_then_reopen":true
    }))
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

    /// A runtime pinned to one platform's window chrome, for the tests that
    /// exercise app-drawn or host-drawn controls regardless of the host they
    /// run on. `Runtime::new` takes a plain function pointer, so the
    /// placement is selected here rather than captured.
    pub(super) fn runtime_for_frame(
        state: State,
        placement: frame::Placement,
    ) -> rune::Runtime<State> {
        fn hosted(state: &State, cx: &mut Cx<State>) -> El {
            view_with_frame(state, cx, frame::Placement::Hosted)
        }
        fn trailing(state: &State, cx: &mut Cx<State>) -> El {
            view_with_frame(state, cx, frame::Placement::Trailing)
        }
        fn native(state: &State, cx: &mut Cx<State>) -> El {
            view_with_frame(state, cx, frame::Placement::Native)
        }
        let view = match placement {
            frame::Placement::Hosted => hosted as fn(&State, &mut Cx<State>) -> El,
            frame::Placement::Trailing => trailing,
            frame::Placement::Native => native,
        };
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

    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("missing {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
    }

    fn close_terminal_from(runtime: &mut rune::Runtime<State>, entry: &str) {
        runtime.settle_all();
        runtime.frame(0.0);
        match entry {
            "button" => click(runtime, "terminal-close-0"),
            "middle" => {
                let (x, y) = runtime.find("tab-0").unwrap();
                runtime.button_down(x, y, rune::PointerButton::Middle);
                runtime.button_up(x, y, rune::PointerButton::Middle);
                runtime.frame(0.0);
            }
            "context" => {
                let (x, y) = runtime.find("tab-0").unwrap();
                runtime.secondary_down(x, y);
                runtime.secondary_up(x, y);
                runtime.frame(0.0);
                runtime.resolve_popup(Some("close")).unwrap();
                runtime.frame(0.0);
            }
            "shortcut" => press(
                runtime,
                Key::Command('w'),
                rune::Mods {
                    ctrl: !cfg!(target_os = "macos"),
                    cmd: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            "remapped" => press(runtime, Key::Function(9), Default::default()),
            "menu" => {
                click(runtime, "workspace-menu");
                click(runtime, "close-tab");
            }
            "palette" => {
                open_palette(runtime);
                press(runtime, Key::Text("End Session".into()), Default::default());
                assert!(
                    runtime
                        .elements()
                        .iter()
                        .any(|e| e.text.as_deref() == Some("F9"))
                );
                press(runtime, Key::Enter, Default::default());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn every_terminal_close_entry_point_honors_live_idle_and_running_policy_matrix() {
        use crate::config::TerminalClosePolicy as Policy;
        let shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file())
            .unwrap();
        for policy in [
            Policy::ConfirmRunning,
            Policy::AlwaysConfirm,
            Policy::CloseImmediately,
        ] {
            for running in [false, true] {
                for entry in [
                    "button", "middle", "context", "shortcut", "remapped", "menu", "palette",
                ] {
                    let root = crate::test_support::tempdir().unwrap();
                    let mut state = State::with_root(root.path().into());
                    state.shell = Some(shell.clone());
                    state.start_tab(root.path().into());
                    state
                        .active()
                        .unwrap()
                        .wait_for("rodtty:", std::time::Duration::from_secs(15))
                        .unwrap();
                    state.save_config(|c| {
                        c.behavior.terminal_tab_close = policy;
                        if matches!(entry, "remapped" | "palette") {
                            c.shortcuts.insert("end-terminal".into(), "F9".into());
                        }
                    });
                    if running {
                        state.send(b"read answer\r".to_vec());
                        let start = std::time::Instant::now();
                        while !state.active().unwrap().has_running_commands() {
                            assert!(start.elapsed().as_secs() < 10);
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                    let id = state.active().unwrap().id();
                    let mut rt = runtime_for(state);
                    close_terminal_from(&mut rt, entry);
                    let confirms = policy == Policy::AlwaysConfirm
                        || (policy == Policy::ConfirmRunning && running);
                    if confirms {
                        assert!(
                            rt.find("confirmation").is_some(),
                            "{policy:?} running={running} {entry}"
                        );
                        press(&mut rt, Key::Escape, Default::default());
                        assert_eq!(rt.state.active().unwrap().id(), id);
                        close_terminal_from(&mut rt, entry);
                        click(&mut rt, "confirm");
                    }
                    assert!(
                        rt.state.sessions.is_empty(),
                        "{policy:?} running={running} {entry}"
                    );
                }
            }
        }
    }

    #[test]
    fn window_quit_and_explicit_quit_keep_running_confirmation_even_with_immediate_tab_close() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state.start_tab(root.path().into());
        state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        state.save_config(|c| {
            c.behavior.terminal_tab_close = crate::config::TerminalClosePolicy::CloseImmediately;
            c.behavior.window_close = crate::config::WindowClosePolicy::QuitRodtty;
        });
        state.send(b"read answer\r".to_vec());
        let start = std::time::Instant::now();
        while !state.active().unwrap().has_running_commands() {
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let id = state.active().unwrap().id();
        let mut rt = runtime_for(state);
        for action in [AppAction::Quit, AppAction::CloseWindow] {
            rt.update(|s, tx| s.dispatch(action, tx));
            rt.frame(0.0);
            assert!(rt.find("confirmation").is_some());
            assert!(rt.shell_request.is_none());
            press(&mut rt, Key::Escape, Default::default());
            assert_eq!(rt.state.active().unwrap().id(), id);
        }
        // An OS close callback uses the same configured quit flow.
        rt.update(State::on_window_close);
        rt.frame(0.0);
        assert!(rt.find("confirmation").is_some());
        press(&mut rt, Key::Escape, Default::default());
        assert_eq!(rt.state.active().unwrap().id(), id);
    }

    #[test]
    fn dirty_editor_decision_precedes_each_window_policy_and_quit() {
        use crate::config::WindowClosePolicy as Policy;
        for policy in [
            Policy::KeepSessionsRunning,
            Policy::AskEachTime,
            Policy::QuitRodtty,
        ] {
            for action in [AppAction::CloseWindow, AppAction::Quit] {
                let root = crate::test_support::tempdir().unwrap();
                let mut state = State::with_root(root.path().into());
                workspace::demo(&mut state, false);
                state.workspace.tabs[0].edit.insert("draft remains ");
                let draft = state.workspace.tabs[0].edit.text.clone();
                state.save_config(|c| c.behavior.window_close = policy);
                let mut rt = runtime_for(state);
                rt.update(|s, tx| s.dispatch(action, tx));
                rt.frame(0.0);
                assert!(rt.find("file-save-confirm").is_some());
                assert!(rt.shell_request.is_none());
                click(&mut rt, "file-cancel");
                assert_eq!(rt.state.workspace.tabs[0].edit.text, draft);
                assert!(rt.state.workspace.tabs[0].dirty());
                rt.update(|s, tx| s.dispatch(action, tx));
                rt.frame(0.0);
                click(&mut rt, "file-discard");
                assert!(!rt.state.workspace.tabs[0].dirty());
                if matches!(action, AppAction::CloseWindow) && policy == Policy::AskEachTime {
                    assert!(rt.find("window-close-choice").is_some());
                } else {
                    assert_eq!(
                        rt.shell_request.take(),
                        Some(
                            if matches!(action, AppAction::Quit) || policy == Policy::QuitRodtty {
                                rune::Shell::Quit
                            } else {
                                rune::Shell::Close
                            }
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn new_terminal_directories_follow_live_choices_without_changing_startup_or_session_snapshots()
    {
        use crate::config::NewTerminalDirectory;
        let root = crate::test_support::tempdir().unwrap();
        let active = root.path().join("active");
        let explorer = root.path().join("explorer");
        let specified = root.path().join("specified");
        for dir in [&active, &explorer, &specified] {
            std::fs::create_dir(dir).unwrap();
        }
        let mut state = State::with_root(root.path().into());
        state.directory = active.clone();
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state.save_config(|c| {
            c.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
            c.behavior.specified_directory = specified.clone();
        });
        setup(&mut state, Arc::new(|| {}));
        state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        assert_eq!(
            state.active().unwrap().cwd(),
            active,
            "first launch still uses the startup directory"
        );
        state.navigate(explorer.clone());
        let first = state.active().unwrap().id();
        let mut rt = runtime_for(state);
        rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
        assert_eq!(rt.state.active().unwrap().cwd(), specified);
        rt.update(|s, _| {
            s.save_config(|c| {
                c.behavior.new_terminal_directory = NewTerminalDirectory::Explorer;
                c.history = false;
                c.disabled = vec!["cat".into()];
            })
        });
        rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
        let newest = rt.state.active().unwrap();
        assert_eq!(newest.cwd(), explorer);
        assert!(!newest.config.history);
        assert_eq!(newest.config.disabled, ["cat"]);
        assert_eq!(rt.state.sessions[0].id(), first);
        assert!(rt.state.sessions[0].config.history);
        assert!(rt.state.sessions[0].config.disabled.is_empty());
        rt.update(|s, _| {
            s.save_config(|c| c.behavior.new_terminal_directory = NewTerminalDirectory::Active)
        });
        rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
        assert_eq!(rt.state.active().unwrap().cwd(), explorer);
        let count = rt.state.sessions.len();
        rt.update(|s, _| {
            s.save_config(|c| {
                c.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
                c.behavior.specified_directory = PathBuf::from("/missing-rodtty-folder");
            })
        });
        rt.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
        assert_eq!(rt.state.sessions.len(), count);
        assert!(rt.state.message.contains("directory"));
    }

    #[test]
    fn window_close_policy_applies_live_and_ask_can_cancel_keep_or_quit() {
        use crate::config::WindowClosePolicy;
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime_for(State::with_root(root.path().into()));
        runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Close));
        runtime.update(|s, _| {
            s.save_config(|c| c.behavior.window_close = WindowClosePolicy::AskEachTime)
        });
        runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        runtime.frame(0.0);
        assert!(runtime.find("window-close-choice").is_some());
        assert!(runtime.shell_request.is_none());
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.state.pending.is_none());
        runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find("keep-sessions").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Close));
        runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find("quit-sessions").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
        runtime.update(|s, _| {
            s.save_config(|c| c.behavior.window_close = WindowClosePolicy::QuitRodtty)
        });
        runtime.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
    }

    #[test]
    fn terminal_ansi_colors_follow_the_selected_theme_family_live() {
        // A preview screen with a bright-blue word and default-ink text: the
        // fills of the glyphs the terminal draws must come from the selected
        // family's palette, the way a themed prompt restyles the terminal.
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        let mut parser = vt100::Parser::new(24, 112, 100);
        parser.process(b"\x1b[38;5;12mthemed\x1b[0m: prompt $ ");
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        let mut draw_in_family = |family: crate::config::ThemeFamily| {
            rt.update(|s, _| {
                s.save_config(|c| c.theme_family = family);
                s.config.appearance = crate::config::Appearance::Dark;
            });
            // Theme colors animate to their new values; let the flight land first.
            for _ in 0..60 {
                rt.frame(1.0 / 60.0);
            }
            let a = Appearance::resolve(family, crate::config::Appearance::Dark, true);
            let blue = a.terminal(vt100::Color::Idx(12), true);
            let ink = a.terminal(vt100::Color::Default, true);
            // The terminal's own cells report their element rects; find the glyph
            // instances drawn inside them and compare their fills.
            let cells: Vec<[f32; 4]> = rt
                .elements()
                .into_iter()
                .filter(|e| {
                    e.key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("terminal-cell-"))
                })
                .map(|e| e.rect)
                .collect();
            let fills: Vec<[f32; 4]> = rt
                .instances
                .iter()
                .filter(|i| i.params[1] == rune::render::KIND_GLYPH)
                .filter(|i| {
                    cells.iter().any(|c| {
                        i.rect[0] >= c[0]
                            && i.rect[1] >= c[1]
                            && i.rect[0] + i.rect[2] <= c[0] + c[2] + 1.0
                            && i.rect[1] + i.rect[3] <= c[1] + c[3] + 1.0
                    })
                })
                .map(|i| i.fill)
                .collect();
            let close = |fill: [f32; 4], expected: rune::Color| {
                (fill[0] - expected.r).abs() < 0.001
                    && (fill[1] - expected.g).abs() < 0.001
                    && (fill[2] - expected.b).abs() < 0.001
            };
            let has_blue = fills.iter().any(|f| close(*f, blue));
            let has_ink = fills.iter().any(|f| close(*f, ink));
            has_blue && has_ink
        };
        assert!(
            draw_in_family(crate::config::ThemeFamily::TokyoNight),
            "Tokyo Night palette missing from the terminal"
        );
        assert!(
            draw_in_family(crate::config::ThemeFamily::Everforest),
            "Everforest palette missing from the terminal"
        );
    }

    #[test]
    fn footer_shows_bold_app_identity_then_tilde_home_path_with_a_point_frame() {
        let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
        // The owner's actual home folder collapses to `~`, whatever the host
        // calls it, and never collapses folders that merely sit beside it.
        assert_eq!(tilde_path(&home), "~");
        let project = home.join("Projects/CouraLabs/Rodtty");
        assert_eq!(tilde_path(&project), "~/Projects/CouraLabs/Rodtty");
        let elsewhere = home.parent().unwrap().join("shared-not-home/readme");
        assert_eq!(tilde_path(&elsewhere), elsewhere.display().to_string());

        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime_for(State::with_root(root.path().to_owned()));
        runtime.frame(0.0);
        let identity = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("footer-app-name"))
            .expect("footer identity");
        assert_eq!(
            identity.text.as_deref(),
            Some(concat!("Rodtty ", env!("CARGO_PKG_VERSION")))
        );
        // One point of breathing room around the footer strip's content.
        let rect = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("app-footer"))
            .unwrap()
            .rect;
        let content = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("app-footer-content"))
            .unwrap()
            .rect;
        assert!((content[0] - rect[0] - 1.0).abs() < 0.001);
        assert!((content[1] - rect[1] - 1.0).abs() < 0.001);
        assert!((rect[0] + rect[2] - 1.0 - content[0] - content[2]).abs() < 0.001);
        assert!((rect[1] + rect[3] - 1.0 - content[1] - content[3]).abs() < 0.001);
    }

    #[test]
    fn clicking_a_terminal_tab_switches_the_active_session_both_ways() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        let shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file())
            .unwrap();
        state.shell = Some(shell);
        for _ in 0..2 {
            state.start_tab(root.path().into());
        }
        let mut rt = runtime_for(state);
        rt.state.sessions.iter().for_each(|s| {
            s.wait_for("rodtty:", std::time::Duration::from_secs(15))
                .unwrap()
        });
        assert_eq!(rt.state.active, 1);
        let (x, y) = rt.find("tab-0").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        assert_eq!(
            rt.state.active, 0,
            "click on tab-0 switches the active session"
        );
        let (x, y) = rt.find("tab-1").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        assert_eq!(rt.state.active, 1, "click on tab-1 switches back");
    }

    #[test]
    fn idle_terminal_closes_immediately_but_live_commands_offer_cancel() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state.new_tab();
        state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        let mut runtime = runtime_for(state);
        runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
        assert!(
            runtime.state.sessions.is_empty(),
            "an idle shell needs no confirmation"
        );
        runtime.update(|s, tx| s.dispatch(AppAction::NewTerminal, tx));
        runtime
            .state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        runtime
            .state
            .active()
            .unwrap()
            .send(b"read pending\r".to_vec())
            .unwrap();
        let started = std::time::Instant::now();
        while !runtime.state.active().unwrap().has_running_commands() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let id = runtime.state.active().unwrap().id();
        runtime.update(|s, tx| s.dispatch(AppAction::EndTerminal, tx));
        runtime.frame(0.0);
        assert!(runtime.find("confirmation").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        assert_eq!(runtime.state.active().unwrap().id(), id);
        runtime.update(|s, tx| s.dispatch(AppAction::Quit, tx));
        runtime.frame(0.0);
        assert!(runtime.find("confirmation").is_some());
        press(&mut runtime, Key::Escape, Default::default());
        assert_eq!(runtime.state.active().unwrap().id(), id);
    }

    #[test]
    fn mac_hosted_titlebar_keeps_traffic_lights_and_header_clicks() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        workspace::demo(&mut state, false);
        let mut runtime = rune::Runtime::new(
            state,
            |state, cx| view_with_frame(state, cx, frame::Placement::Hosted),
            (640.0, 360.0),
            rune::Env::new(true, 2.0),
        );
        runtime.frame(0.0);
        // macOS keeps the host's traffic lights: the app draws no window controls,
        // and nothing in the header sits where the lights are.
        assert!(runtime.find("window-close").is_none(), "no app-drawn close");
        assert!(
            runtime.find("window-minimize").is_none(),
            "no app-drawn minimize"
        );
        assert!(
            runtime.find("window-maximize").is_none(),
            "no app-drawn maximize"
        );
        assert!(
            runtime
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("toggle-terminal"))
                .unwrap()
                .rect[0]
                > 0.0,
            "header controls sit past the traffic lights, not under them"
        );
        // Header controls keep their presses and fit the minimum-size frame.
        for key in ["toggle-terminal", "workspace-menu", "files"] {
            let rect = runtime
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap()
                .rect;
            assert!(
                rect[0] >= 0.0
                    && rect[0] + rect[2] <= 640.0
                    && rect[1] >= 0.0
                    && rect[1] + rect[3] <= 54.0,
                "{key} fits inside the minimum-size frame: {rect:?}"
            );
        }
        let pane = runtime.find("toggle-terminal").unwrap();
        runtime.pointer_down(pane.0, pane.1);
        assert_eq!(runtime.take_titlebar_action(), None);
        runtime.pointer_up(pane.0, pane.1);
        runtime.frame(0.3);
        // The header band still moves the window, and a double press zooms it.
        runtime.set_viewport((1200.0, 700.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.5);
        let header = runtime.find("workspace-header").unwrap();
        runtime.pointer_down(header.0, header.1);
        assert_eq!(
            runtime.take_titlebar_action(),
            Some(rune::TitlebarAction::Drag)
        );
        runtime.pointer_up(header.0, header.1);
        runtime.pointer_down(header.0, header.1);
        assert_eq!(
            runtime.take_titlebar_action(),
            Some(rune::TitlebarAction::DoubleClick)
        );
        runtime.pointer_up(header.0, header.1);
    }

    #[test]
    fn right_frame_places_controls_right_and_header_controls_keep_their_clicks() {
        let mut runtime = rune::Runtime::new(
            State::init(),
            |state, cx| view_with_frame(state, cx, frame::Placement::Trailing),
            (1200.0, 700.0),
            rune::Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        let minimize = runtime
            .find("window-minimize")
            .expect("custom minimize control");
        let maximize = runtime
            .find("window-maximize")
            .expect("custom maximize control");
        let close = runtime.find("window-close").expect("custom close control");
        assert!(minimize.0 < maximize.0 && maximize.0 < close.0);
        assert!(close.0 > 1100.0);
        runtime.pointer_down(240.0, 20.0);
        assert_eq!(
            runtime.take_titlebar_action(),
            Some(rune::TitlebarAction::Drag)
        );
        runtime.pointer_up(240.0, 20.0);
        runtime.frame(0.1);
        runtime.pointer_down(240.0, 20.0);
        assert_eq!(
            runtime.take_titlebar_action(),
            Some(rune::TitlebarAction::DoubleClick)
        );
        runtime.pointer_up(240.0, 20.0);
        let (x, y) = runtime.find("toggle-terminal").unwrap();
        runtime.pointer_down(x, y);
        assert_eq!(runtime.take_titlebar_action(), None);
        runtime.pointer_up(x, y);
        runtime.frame(0.2);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.3);
        for key in [
            "window-minimize",
            "window-maximize",
            "window-close",
            "toggle-terminal",
            "workspace-menu",
        ] {
            let rect = runtime
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap()
                .rect;
            assert!(
                rect[0] >= 0.0
                    && rect[0] + rect[2] <= 640.0
                    && rect[1] >= 0.0
                    && rect[1] + rect[3] <= 60.0,
                "{key} must fit narrow header: {rect:?}"
            );
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn custom_close_keeps_unsaved_editor_until_owner_decides() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        workspace::demo(&mut state, false);
        state.workspace.tabs[0].edit.insert("draft remains ");
        let draft = state.workspace.tabs[0].edit.text.clone();
        let mut runtime = runtime_for_frame(state, frame::Placement::Trailing);
        let (x, y) = runtime.find("window-close").unwrap();
        runtime.pointer_down(x, y);
        assert_eq!(runtime.take_titlebar_action(), None);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.05);
        assert!(runtime.find("file-save-confirm").is_some());
        assert!(runtime.find("file-discard").is_some());
        let (x, y) = runtime.find("file-cancel").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.1);
        assert!(runtime.state.pending.is_none());
        assert_eq!(runtime.state.workspace.tabs[0].edit.text, draft);
        assert!(runtime.state.workspace.tabs[0].dirty());
    }

    #[test]
    fn terminal_tabs_show_application_title_verbatim_then_directory_fallback() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        let shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file())
            .unwrap();
        let session = Session::spawn(
            &shell,
            root.path(),
            root.path(),
            Config::default(),
            28,
            100,
            Arc::new(|| {}),
        )
        .unwrap();
        session
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        session
            .send(b"printf '\\033]2;Agent title\\007'\r".to_vec())
            .unwrap();
        let start = std::time::Instant::now();
        while session.state.lock().unwrap().parser.callbacks().title != "Agent title" {
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        state.sessions.push(session);
        let mut runtime = runtime_for(state);
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("Agent title"))
        );
        assert!(
            !runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref().is_some_and(|t| t.contains("Terminal 1")))
        );
        assert!(runtime.find("terminal-close-0").is_some());
        let child = root.path().join("new-folder");
        std::fs::create_dir(&child).unwrap();
        runtime.state.sessions[0]
            .send(format!("cd {}\r", profile::quote(&child).unwrap()).into_bytes())
            .unwrap();
        let start = std::time::Instant::now();
        while runtime.state.sessions[0].cwd() != child {
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        runtime.frame(0.0);
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("Agent title"))
        );
        runtime.state.sessions[0]
            .send(b"printf '\\033]2;\\007'\r".to_vec())
            .unwrap();
        let start = std::time::Instant::now();
        while !runtime.state.sessions[0]
            .state
            .lock()
            .unwrap()
            .parser
            .callbacks()
            .title
            .is_empty()
        {
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("›_  Term new-folder"))
        );
        let full = format!(
            "Application; title 日本語 with {};;",
            "preserved content ".repeat(8)
        );
        runtime.state.sessions[0]
            .send(format!("printf '\\033]2;{}\\007'\r", full).into_bytes())
            .unwrap();
        let start = std::time::Instant::now();
        while runtime.state.sessions[0].tab_title() != full {
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        runtime.update(|_, _| {});
        runtime.frame(0.0);
        let tab = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("tab-0") && !e.exiting)
            .unwrap();
        assert_eq!(tab.help.as_deref(), Some(full.as_str()));
        assert!(tab.rect[2] <= 180.0);
    }

    #[test]
    fn dragging_and_closing_earlier_terminal_tabs_preserves_active_session() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        state.config.behavior.terminal_tab_close =
            crate::config::TerminalClosePolicy::AlwaysConfirm;
        let shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file())
            .unwrap();
        for _ in 0..3 {
            state.sessions.push(
                Session::spawn(
                    &shell,
                    root.path(),
                    root.path(),
                    Config::default(),
                    28,
                    100,
                    Arc::new(|| {}),
                )
                .unwrap(),
            );
        }
        for session in &state.sessions {
            session
                .wait_for("rodtty:", std::time::Duration::from_secs(15))
                .unwrap();
        }
        let observed: std::collections::HashMap<_, _> = state
            .sessions
            .iter()
            .map(|session| (session.id(), session.state.lock().unwrap().generation))
            .collect();
        state.generations = state.sessions.iter().map(|s| observed[&s.id()]).collect();
        state.active = 2;
        let active = state.sessions[2].state.clone();
        let first = state.sessions[0].state.clone();
        state.selection = Some(((0, 0), (0, 2)));
        let mut runtime = runtime_for(state);
        let (x, y) = runtime.find("tab-0").unwrap();
        let (end, _) = runtime.find("tab-2").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_move(end + 30.0, y);
        runtime.pointer_up(end + 30.0, y);
        runtime.frame(0.0);
        assert!(Arc::ptr_eq(&runtime.state.sessions[2].state, &first));
        assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
        assert!(runtime.state.selection.is_some());
        assert_eq!(
            runtime.state.generations,
            runtime
                .state
                .sessions
                .iter()
                .map(|s| observed[&s.id()])
                .collect::<Vec<_>>()
        );

        let (x, y) = runtime.find("tab-0").unwrap();
        runtime.button_down(x, y, rune::PointerButton::Middle);
        runtime.button_up(x, y, rune::PointerButton::Middle);
        runtime.frame(0.0);
        assert!(runtime.find("confirmation").is_some());
        let (x, y) = runtime.find("confirm").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
        assert_eq!(
            runtime.state.generations,
            runtime
                .state
                .sessions
                .iter()
                .map(|s| observed[&s.id()])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn terminal_overflow_scrolls_and_context_close_targets_its_session() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        state.config.behavior.terminal_tab_close =
            crate::config::TerminalClosePolicy::AlwaysConfirm;
        let shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file())
            .unwrap();
        state.shell = Some(shell.clone());
        for _ in 0..5 {
            state.sessions.push(
                Session::spawn(
                    &shell,
                    root.path(),
                    root.path(),
                    Config::default(),
                    28,
                    100,
                    Arc::new(|| {}),
                )
                .unwrap(),
            );
        }
        state.active = 2;
        let active = state.sessions[2].state.clone();
        let closing = tabs::session_id(&state.sessions[4]);
        let mut runtime = runtime_for(state);
        runtime.set_viewport((640.0, 360.0), 1.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find("tab-0").unwrap();
        runtime.wheel_xy(x, y, 0.0, -5000.0);
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.0);
        let last = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("tab-4") && !e.exiting)
            .unwrap();
        assert!(last.visible[2] > 0.0);
        let (x, y) = runtime.find("tab-4").unwrap();
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(0.0);
        assert!(runtime.popup_menu().is_some());
        assert!(runtime.state.application_overlay_open());
        runtime
            .state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        runtime.paste("PASTE_BEHIND_TAB_MENU");
        runtime.frame(0.0);
        press(
            &mut runtime,
            Key::Text("KEY_BEHIND_TAB_MENU".into()),
            Default::default(),
        );
        assert!(
            runtime
                .state
                .active()
                .unwrap()
                .wait_for(
                    "PASTE_BEHIND_TAB_MENU",
                    std::time::Duration::from_millis(200)
                )
                .is_err()
        );
        runtime.resolve_popup(None).unwrap();
        runtime.frame(0.0);
        assert!(!runtime.state.application_overlay_open());
        runtime.paste("PASTE_AFTER_TAB_MENU");
        runtime.frame(0.0);
        runtime
            .state
            .active()
            .unwrap()
            .wait_for("PASTE_AFTER_TAB_MENU", std::time::Duration::from_secs(10))
            .unwrap();
        let contents = runtime.state.active().unwrap().screen().contents();
        assert!(!contents.contains("PASTE_BEHIND_TAB_MENU"));
        assert!(!contents.contains("KEY_BEHIND_TAB_MENU"));
        runtime.secondary_down(x, y);
        runtime.secondary_up(x, y);
        runtime.frame(0.0);
        runtime.resolve_popup(Some("close")).unwrap();
        runtime.frame(0.0);
        assert!(matches!(runtime.state.pending, Some(Pending::Tab(id)) if id == closing));
        assert!(Arc::ptr_eq(&runtime.state.active().unwrap().state, &active));
        runtime.settle_all();
        runtime.frame(0.0);
        let (x, y) = runtime.find("cancel").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        runtime.settle_all();
        runtime.frame(0.0);
        assert!(runtime.state.pending.is_none());
        let (x, y) = runtime.find("new-tab").unwrap();
        // The tabs share the header band now: the button sits in it, after
        // the last tab, and pressing it must not start a window drag.
        assert!(x > last.rect[0] && x < 640.0 && y < header_height());
        runtime.pointer_down(x, y);
        assert_eq!(runtime.take_titlebar_action(), None);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert_eq!(runtime.state.sessions.len(), 6);
        assert_eq!(runtime.state.active, 5);
        assert_eq!(runtime.state.active().unwrap().cwd(), root.path());
    }

    #[test]
    fn compact_workspace_menu_supports_keyboard_settings_and_palette_entry() {
        let mut runtime = runtime();
        assert!(runtime.find("quit").is_none());
        assert!(runtime.find("close-tab").is_none());
        let (x, y) = runtime.find("workspace-menu").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        let popover = runtime
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("workspace-menu-popover") && !e.exiting)
            .unwrap();
        assert!(
            popover.rect[3] <= 260.0,
            "a short action menu must fit its content"
        );
        press(&mut runtime, Key::Down, Default::default());
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("workspace-menu-popover").is_none());
        assert!(runtime.find("settings-search").is_some());
        let (x, y) = runtime.find("workspace-menu").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        press(&mut runtime, Key::Escape, Default::default());
        assert!(runtime.find("workspace-menu-popover").is_none());
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("workspace-menu") && e.focused)
        );
        let (x, y) = runtime.find("workspace-menu").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.settle_all();
        runtime.frame(0.0);
        press(&mut runtime, Key::Enter, Default::default());
        assert!(runtime.find("command-palette").is_some());
        assert!(runtime.find("workspace-menu-popover").is_none());
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
        assert_eq!(runtime.shell_request.take(), Some(rune::Shell::Quit));
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
