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
mod autosave;
mod clipboard;
mod explorer_search;
mod file_icon_catalog;
mod file_icons;
mod fonts;
mod frame;
mod motion;
use motion::{Enters, Entrance};
mod palette;
mod tabs;
mod terminal_palette;
mod terminal_search;
mod terminal_view;
use appearance::Appearance;
use terminal_view::grid;
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
    /// Whether the editor pane's Search tab exists, and whether it shows.
    search_tab_open: bool,
    search_tab: bool,
    settings_query: TextEdit,
    /// The Settings page's selected tab.
    settings_tab: settings::Tab,
    /// The Settings dropdown whose list is open, by key.
    settings_select: Option<&'static str>,
    /// The Settings tab whose Restore Defaults is asking for confirmation.
    settings_reset_confirm: Option<settings::Tab>,
    shortcut_recording: Option<shortcuts::Recording>,
    behavior_directory: TextEdit,
    /// The Startup folder setting's Specified folder field.
    startup_directory: TextEdit,
    theme_family_picker: rune_kit::ComboboxState,
    settings_error: Option<String>,
    settings_seen: Option<Vec<u8>>,
    settings_poll: std::time::Instant,
    last_valid_config: Config,
    terminal_search: terminal_search::TerminalSearch,
    explorer_search: explorer_search::ExplorerSearch,
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
        let explicit = std::env::args()
            .skip_while(|a| a != "--cwd")
            .nth(1)
            .map(PathBuf::from);
        // `--cwd` wins; otherwise the Startup folder setting, then the home folder.
        let configured = explicit
            .is_none()
            .then(|| loaded.as_ref().ok())
            .flatten()
            .and_then(|c| config::configured_startup_directory(&c.behavior));
        let (directory, startup_warning) = match configured {
            Some(Ok(path)) => (path, None),
            other => (
                config::initial_directory(explicit).unwrap_or_else(|_| root.clone()),
                other.and_then(Result::err),
            ),
        };
        let message = loaded
            .as_ref()
            .err()
            .map(|error| format!("Could not load Settings JSON: {error:#}"))
            .or(startup_warning)
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
            startup_directory: TextEdit::new(
                loaded
                    .as_ref()
                    .ok()
                    .map(|c| {
                        c.behavior
                            .startup_specified_directory
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
            search_tab_open: false,
            search_tab: false,
            settings_query: TextEdit::new(""),
            settings_tab: Default::default(),
            settings_select: None,
            settings_reset_confirm: None,
            shortcut_recording: None,
            theme_family_picker: Default::default(),
            settings_error: (!config_valid).then(|| message.clone()),
            settings_poll: std::time::Instant::now(),
            terminal_search: Default::default(),
            explorer_search: Default::default(),
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
            || self.workspace.explorer_menu
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
                self.workspace.set_terminal_focus(true);
                self.sessions.push(session);
                self.active = self.sessions.len() - 1;
                self.workspace.note_session_switch();
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

    fn with_screen_mut<R>(&mut self, change: impl FnOnce(&mut vt100::Screen) -> R) -> R {
        if let Some(session) = self.active() {
            session.with_screen_mut(change)
        } else if let Some(preview) = &mut self.preview {
            change(preview)
        } else {
            change(vt100::Parser::new(28, 100, 0).screen_mut())
        }
    }

    /// Running Rodtty shells adopt prompt preferences at their next prompt.
    fn publish_prompt(&self) {
        for session in &self.sessions {
            // An ending session may already have removed its profile.
            let _ = session.publish_prompt(self.config.prompt);
        }
    }

    fn save_config(&mut self, edit: impl FnOnce(&mut Config)) {
        let prompt = self.config.prompt;
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
                if self.config.prompt != prompt {
                    self.publish_prompt();
                }
                "Saved. Command, history, and scrollback settings apply to new sessions.".into()
            }
            Err(error) => {
                self.config = self.last_valid_config.clone();
                format!("Could not save settings: {error:#}")
            }
        };
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
                        self.workspace.note_session_switch();
                    }
                }
            }
            _ => {}
        }
    }
}

thread_local! {
    /// Keys of the app's buttons, tabs and menu items, which Return presses
    /// as Space does. Rune follows AppKit, where Return goes on to the
    /// window's `on_key` rather than pressing the focused button; text
    /// fields never register, so Return keeps submitting them.
    static PRESSABLE: std::cell::RefCell<std::collections::HashSet<String>> =
        Default::default();
}

/// Let Return press the control keyed `key` while it has keyboard focus.
fn pressable(key: &str) {
    PRESSABLE.with(|keys| {
        if !keys.borrow().contains(key) {
            keys.borrow_mut().insert(key.to_owned());
        }
    });
}

fn presses_on_return(key: &str) -> bool {
    PRESSABLE.with(|keys| keys.borrow().contains(key))
}

/// An on/off control's state, for assistive technology (`checked`) and for
/// sight: an active control is tinted and outlined in the accent color, a
/// 1.5 pt ring that reads at 3:1 or more against the fills around it in
/// every appearance, so its state never rests on a subtle fill alone. An
/// inactive one keeps a clear ring of the same width, so toggling never
/// shifts its content.
trait ToggleState {
    fn toggle_state(self, a: &Appearance, on: bool) -> Self;
}
impl ToggleState for El {
    fn toggle_state(self, a: &Appearance, on: bool) -> Self {
        self.checked(on)
            .fill(if on {
                a.active_fill()
            } else {
                Color::TRANSPARENT
            })
            .border(
                TOGGLE_RING,
                if on { a.accent() } else { Color::TRANSPARENT },
            )
    }
}

/// The width of an active toggle's accent ring, in points.
const TOGGLE_RING: f32 = 1.5;

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
        "explorer-refresh" => "Refresh files",
        "settings-close" => "Close Settings tab",
        "find-close" => "Close editor search",
        "close-search" => "Close terminal search",
        "diff-prev" => "Previous change",
        "diff-next" => "Next change",
        _ if key.starts_with("editor-close-") => "Close editor tab",
        _ => &label,
    };
    pressable(&key);
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
        // A thin, dimmed outline marks a text button as pressable, so it
        // never reads as a plain label.
        .border(1.0, a.button_border())
        .hover(a.hover_bg())
        .press(a.pressed())
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
    pressable(&key);
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
        .press(a.pressed())
        .on_press_tx(move |state: &mut State, tx| {
            tx.blur();
            action(state, tx);
        })
        .child(rune::icon(glyph, 16.0).color(a.ink()))
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
        .role("tablist")
        .label("Terminal tabs")
        .shrink()
        .scroll_x()
        .gap(4.0)
        .center_y();
    let count = state.sessions.len();
    for (i, session) in state.sessions.iter().enumerate() {
        let label = session.tab_title();
        // The label names the program; the tooltip says where it runs.
        let location = tilde_path(&session.cwd());
        let id = tabs::session_id(session);
        let selected = i == state.active;
        list = list.child(
            row()
                .key(format!("terminal-group-{id}"))
                // The pill only groups its tab and close button for sight;
                // the tab list holds the tabs themselves.
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if selected { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .no_window_drag()
                .children(selected.then(|| tab_indicator(a, "indicator-terminal-tabs")))
                // The pill only draws; the label below carries the activation,
                // because interactive()'s drag and middle handlers make the label
                // its own press target and would otherwise swallow the press.
                .child(tabs::interactive(
                    terminal_tab(a, i, count, &label, selected)
                        .help(location)
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
                    // Each close button names the tab it ends, since every
                    // tab has one and a reader meets them out of the strip.
                    .label(format!(
                        "End terminal session: {label} (tab {} of {count})",
                        i + 1
                    ))
                    .description("")
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

/// A terminal tab within the strip's tab list: a focusable `tab` that
/// reports whether it is the active session's and where it sits among the
/// tabs, and that Return, Space or an assistive press activates.
fn terminal_tab(a: &Appearance, i: usize, count: usize, label: &str, selected: bool) -> El {
    let key = format!("tab-{i}");
    pressable(&key);
    tab_label(a, label, selected)
        .key(key)
        .role("tab")
        .label(label)
        .focusable(true)
        .focus_selected(selected)
        .position_in_set(i + 1, count)
        .focus_ring(2.0, 1.0, a.accent())
        .radius(6.0)
        .no_window_drag()
}

/// The active tab's accent bar along its pill's lower edge: the tab's
/// selection mark beyond its fill, at 3:1 or more against the tab fills.
/// One bar per tab strip (`strip`): when the active tab changes, Rune's
/// shared-element motion slides it from the old tab to the new one.
fn tab_indicator(a: &Appearance, strip: &'static str) -> El {
    el().key(strip)
        .pin(Some(8.0), None, Some(8.0), Some(0.0))
        .h(2.0)
        .radius(1.0)
        .fill(a.accent())
        .matched(strip)
        .motion(a.motion().spring())
        .enters_animated(a, Entrance::Fade)
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
        .px(6.0)
        .py(6.0)
        .gap(3.0)
        .radius(10.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg())
        .enters(a, Entrance::Menu(motion::FROM_TOP_TRAILING))
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
    menu.pin(None, Some(header_height()), Some(8.0), None)
}

/// The footer's status: the latest action's message, or the palette hint.
/// Each new message is its own element, so it enters as action feedback.
fn footer_message(state: &State, a: &Appearance) -> El {
    let message = if state.message.is_empty() {
        format!("{}  Actions", state.shortcut_label("command-palette"))
    } else {
        state.message.clone()
    };
    let generation = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        message.hash(&mut hasher);
        hasher.finish()
    };
    el().key(format!("footer-feedback-{generation:x}"))
        .min_w(0.0)
        .shrink()
        .enters(a, Entrance::Feedback)
        .child(
            text(message)
                .key("footer-message")
                .font(ui_label())
                .color(a.muted())
                .truncate()
                .min_w(0.0)
                .shrink(),
        )
}

/// The strip along the window's bottom: app identity, the active terminal's
/// folder and the latest action's message. The owner can hide it in Settings.
fn app_footer(state: &State, a: &Appearance, cwd: &std::path::Path) -> El {
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
                    text(tilde_path(cwd))
                        .key("footer-path")
                        .font(ui_label())
                        .color(a.muted())
                        .truncate()
                        .min_w(0.0)
                        .grow(1.0),
                )
                .child(footer_message(state, a)),
        )
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
    )
    .with_reduced_motion(cx.is_reduced_motion());
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
                .child(explorer_search::field(
                    state,
                    a,
                    if width < 880.0 { 140.0 } else { 220.0 },
                ))
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
                    .label("File explorer")
                    .toggle_state(a, show_explorer),
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
                    .label("Terminal pane")
                    .toggle_state(a, show_terminal),
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
                    .label("Editor pane")
                    .toggle_state(a, show_editor),
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
    if state.config.show_footer {
        root = root.child(app_footer(state, a, &cwd));
    }
    if matches!(state.pending, Some(Pending::Tab(_) | Pending::Quit)) {
        let (title, message, confirm) = match state.pending {
            Some(Pending::Quit) => (
                "Quit Rodtty and end all sessions?",
                "Running commands in these sessions will stop.",
                "End sessions",
            ),
            _ => (
                "End this terminal session?",
                "Running commands in this session will stop.",
                "End session",
            ),
        };
        root = root.child(motion::modal(
            a,
            el().center().child(
                col()
                    .key("confirmation")
                    .role("alertdialog")
                    .label(title)
                    .description(message)
                    .modal(false, |s: &mut State| s.pending = None)
                    .w(410.0)
                    .px(24.0)
                    .py(24.0)
                    .gap(16.0)
                    .radius(10.0)
                    .fill(a.panel())
                    .child(text(title).font(ui_style(17.0, 600.0)).color(a.ink()))
                    .child(text(message).font(ui_label()).color(a.muted()).wrap())
                    .child(
                        row()
                            .gap(8.0)
                            .child(button(a, "cancel", "Cancel", |s, _| s.pending = None))
                            .child(button(a, "confirm", confirm, |s, tx| s.confirm(tx))),
                    ),
            ),
            Entrance::Dialog,
        ));
    }
    if matches!(state.pending, Some(Pending::Window)) {
        root = root.child(motion::modal(
            a,
            el().center().child(
                col()
                    .key("window-close-choice")
                    .label("Close Rodtty window?")
                    .description("Keep terminal sessions running or quit Rodtty and end them.")
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
            Entrance::Dialog,
        ));
    }
    if let Some(dialog) = workspace::file_dialog(state, a) {
        root = root.child(dialog);
    }
    if state.workspace_menu {
        root = root.child(workspace_menu(state, a));
    }
    if let Some(menu) = workspace::explorer_menu(state, a) {
        root = root.child(menu);
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
    // No root `on_paste`: with one, Rune would take the primary paste chord
    // itself and hand over only text. Without it the chord reaches `on_key`,
    // whose shortcut dispatches `PasteTerminal` to the one terminal paste
    // handler; focused text fields still paste on their own.
    root.children(settings::overlays(state, a, width))
}

/// The control character Ctrl plus `c` types, as xterm sends it: Ctrl+A to
/// Ctrl+Z are 1 to 26, Ctrl+Space and Ctrl+@ are NUL, Ctrl+[ \ ] ^ _ are 27
/// to 31 and Ctrl+? is DEL.
fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
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
        // macOS names Ctrl+C by the key's own character, `Text("c")` with
        // Ctrl held, where other platforms send `Command('c')`: both are the
        // control character, or Ctrl+C would type "c" and never interrupt.
        Key::Text(value) if event.mods.ctrl && !event.mods.cmd => {
            let mut chars = value.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let byte = control_byte(c)?;
            return Some(if event.mods.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }
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
    // Return presses the focused button, tab or menu item as Space does. The
    // workspace menu dispatches its own items on Return.
    if event.key == Key::Enter
        && event.mods == rune::Mods::default()
        && !state.workspace_menu
        && tx.focused().is_some_and(presses_on_return)
    {
        if !tx.key_repeat() {
            tx.send_key(KeyEvent {
                key: Key::Text(" ".into()),
                mods: Default::default(),
            });
        }
        return true;
    }
    if state.removal_background_busy() {
        if state.dispatch_shortcut(event, tx) {
            return true;
        }
        if state.application_overlay_open()
            || !state.workspace.terminal_focus()
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
    if state.workspace.explorer_menu {
        let items = workspace::RootControl::ALL
            .map(|control| control.key().replace("explorer-", "explorer-menu-"));
        let index = items
            .iter()
            .position(|key| Some(key.as_str()) == tx.focused())
            .unwrap_or(0);
        match event.key {
            Key::Escape => {
                state.workspace.explorer_menu = false;
                tx.focus("explorer-more", true);
            }
            Key::Up | Key::Down => {
                let next = if event.key == Key::Down {
                    (index + 1) % items.len()
                } else {
                    (index + items.len() - 1) % items.len()
                };
                tx.focus(items[next].clone(), true);
            }
            _ => return false,
        }
        return true;
    }
    if event.key == Key::Escape && state.dismiss_explorer_overlay(tx) {
        return true;
    }
    if state.dispatch_shortcut(event, tx) {
        return true;
    }
    if tx.focused().is_some() {
        return false;
    }
    if !state.workspace.terminal_focus()
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
    let changed = state.observe_terminal_output();
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
    // Keep the owner's appearance: the reloaded settings come from this root.
    let _ = std::fs::copy(
        state.root.join("settings.json"),
        fixture.path().join("settings.json"),
    );
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
    state.workspace.set_terminal_focus(true);
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
    state.workspace.set_terminal_focus(false);
}
fn tabs_dark_editor_preview(state: &mut State) {
    tabs_dark_preview(state);
    state.workspace.set_terminal_focus(false);
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
    state.workspace.set_terminal_focus(false);
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
    state.settings_tab = settings::Tab::General;
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
/// Tooling-only: Search Files results for "main" in the --cwd folder.
fn search_files_preview(state: &mut State) {
    tree_preview(state);
    state.explorer_search.open = true;
    state.explorer_search.query = rune::TextEdit::new("main");
    state.show_search_tab();
    let started = std::time::Instant::now();
    while state.explorer_search.summary.is_none() && started.elapsed().as_secs() < 10 {
        state.poll_explorer_search(&mut Tx::default());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
/// Tooling-only: README.md from the --cwd folder opened as a temporary tab.
fn markdown_preview(state: &mut State) {
    tree_preview(state);
    state.workspace.worker = Some(crate::work::Worker::new(state.wake.clone()));
    let readme = state.directory.join("README.md");
    state.open_file_with_preview(readme, true);
    let started = std::time::Instant::now();
    while state.workspace.tabs.is_empty() && started.elapsed().as_secs() < 10 {
        state.workspace_poll(&mut Tx::default());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn terminal_search_preview(state: &mut State) {
    preview(state);
    state.workspace.set_terminal_focus(true);
    state.terminal_search.open = true;
}
fn diff_preview(state: &mut State) {
    preview(state);
    workspace::demo(state, true);
}

/// The app icon, rendered from `assets/app-icon/rodtty.svg`.
const APP_ICON: &[u8] = asset_bytes!("app-icon/rodtty-1024.png");

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
            s.persist_workspace();
            s.shutdown_saved = true;
            s.sessions.clear();
        })
        .on_ime(|s, event, _| {
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
                    s.workspace.set_terminal_focus(true);
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
mod tests;
