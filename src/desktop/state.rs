use super::*;

// One pending prompt at a time, held once in `State`: boxing the larger
// operation prompt would only add an allocation per prompt.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Pending {
    Tab(usize),
    Quit,
    Window,
    File(workspace::FilePrompt),
    Operation(explorer::operations::OperationPrompt),
    Removal(explorer::removal::RemovalPrompt),
}

pub(crate) struct State {
    pub(crate) workspace: workspace::Workspace,
    pub(crate) tab_strips: tabs::Tabs,
    pub(crate) commands: Vec<actions::Command>,
    pub(crate) palette: Option<window::palette::Palette>,
    pub(crate) palette_query: TextEdit,
    pub(crate) workspace_menu: bool,
    pub(crate) handle: Option<rune::AppHandle<State>>,
    pub(crate) window: Option<rune::WindowHandle>,
    pub(crate) window_maximized: bool,
    /// The geometry last written to the saved layout, and when. A drag or
    /// resize moves the window many times a second; compare against this and
    /// write once it settles rather than on every host event.
    pub(crate) window_saved: Option<WindowGeometry>,
    pub(crate) window_geometry_poll: Option<std::time::Instant>,
    pub(crate) window_close_approved: bool,
    pub(crate) shutdown_saved: bool,
    pub(crate) root: PathBuf,
    pub(crate) shell: Option<PathBuf>,
    pub(crate) config: Config,
    pub(crate) config_valid: bool,
    pub(crate) sessions: Vec<Session>,
    pub(crate) active: usize,
    pub(crate) directory: PathBuf,
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    pub(crate) generations: Vec<u64>,
    pub(crate) settings: bool,
    pub(crate) settings_open: bool,
    /// Whether the editor pane's Search tab exists, and whether it shows.
    pub(crate) search_tab_open: bool,
    pub(crate) search_tab: bool,
    pub(crate) settings_query: TextEdit,
    /// The Settings page's selected tab.
    pub(crate) settings_tab: settings::Tab,
    /// The Settings dropdown whose list is open, by key.
    pub(crate) settings_select: Option<&'static str>,
    /// The Settings tab whose Restore Defaults is asking for confirmation.
    pub(crate) settings_reset_confirm: Option<settings::Tab>,
    pub(crate) shortcut_recording: Option<shortcuts::Recording>,
    pub(crate) behavior_directory: TextEdit,
    /// The Startup folder setting's Specified folder field.
    pub(crate) startup_directory: TextEdit,
    pub(crate) theme_family_picker: rune_kit::ComboboxState,
    pub(crate) settings_error: Option<String>,
    pub(crate) settings_seen: Option<Vec<u8>>,
    pub(crate) settings_poll: std::time::Instant,
    pub(crate) last_valid_config: Config,
    pub(crate) terminal_search: terminal::search::TerminalSearch,
    pub(crate) explorer_search: search::ExplorerSearch,
    pub(crate) pending: Option<Pending>,
    pub(crate) selection: Option<((u16, u16), (u16, u16))>,
    /// Where the current terminal press started, before it has travelled far
    /// enough to be a selection. `None` between gestures.
    pub(crate) drag_origin: Option<(f32, f32)>,
    pub(crate) terminal_context_open: bool,
    pub(crate) message: String,
    pub(crate) preview: Option<vt100::Screen>,
    pub(crate) terminal_rect: Option<[f32; 4]>,
    pub(crate) terminal_metrics: fonts::Metrics,
    pub(crate) font_catalog: fonts::Catalog,
    pub(crate) font_ui: rune_kit::ComboboxState,
    pub(crate) font_terminal: rune_kit::ComboboxState,
}

impl State {
    pub(crate) fn init() -> Self {
        let root = config::app_root().unwrap_or_else(|_| std::env::temp_dir().join("rodtty"));
        Self::with_root(root)
    }

    pub(crate) fn with_root(root: PathBuf) -> Self {
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
            || self.pending.as_ref().is_some_and(|p| {
                !matches!(p, Pending::Removal(explorer::removal::RemovalPrompt::Busy))
            })
            || self.palette.is_some()
            || self.workspace_menu
            || self.workspace.operation_context.is_some()
            || self.workspace.explorer_menu
            || self.tab_strips.context_open
    }

    pub(crate) fn active(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    pub(crate) fn new_tab(&mut self) {
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

    pub(crate) fn start_tab(&mut self, cwd: PathBuf) {
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

    pub(crate) fn send(&mut self, bytes: impl Into<Vec<u8>>) {
        if let Some(session) = self.active() {
            session.scroll(isize::MIN);
            if let Err(error) = session.send(bytes) {
                self.message = error.to_string();
            }
        }
        self.selection = None;
    }

    pub(crate) fn paste(&mut self, content: &str) {
        if let Some(session) = self.active()
            && let Err(error) = session.paste(content)
        {
            self.message = error.to_string();
        }
    }

    pub(crate) fn with_screen<R>(&self, read: impl FnOnce(&vt100::Screen) -> R) -> R {
        if let Some(session) = self.active() {
            session.with_screen(read)
        } else if let Some(preview) = &self.preview {
            read(preview)
        } else {
            read(vt100::Parser::new(28, 100, 0).screen())
        }
    }

    pub(crate) fn with_screen_mut<R>(&mut self, change: impl FnOnce(&mut vt100::Screen) -> R) -> R {
        if let Some(session) = self.active() {
            session.with_screen_mut(change)
        } else if let Some(preview) = &mut self.preview {
            change(preview)
        } else {
            change(vt100::Parser::new(28, 100, 0).screen_mut())
        }
    }

    /// Running Rodtty shells adopt prompt preferences at their next prompt.
    pub(crate) fn publish_prompt(&self) {
        for session in &self.sessions {
            // An ending session may already have removed its profile.
            let _ = session.publish_prompt(self.config.prompt);
        }
    }

    pub(crate) fn save_config(&mut self, edit: impl FnOnce(&mut Config)) {
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

    pub(crate) fn confirm(&mut self, tx: &mut Tx) {
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
