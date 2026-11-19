use super::*;

impl State {
    pub(crate) fn open_settings(&mut self, tx: &mut Tx) {
        self.supersede_pending_preview();
        self.workspace.explorer_revealed = false;
        self.settings_open = true;
        self.settings = true;
        self.search_tab = false;
        self.workspace.layout.editor = true;
        self.workspace.set_terminal_focus(false);
        tx.focus_when_shown("settings-search", true);
    }

    pub(crate) fn open_settings_json(&mut self) {
        let path = self.root.join("settings.json");
        if !path.exists()
            && let Err(error) = self.config.save(&self.root)
        {
            self.message = format!("Could not create Settings JSON: {error:#}");
            return;
        }
        self.open_file(path);
    }

    pub(crate) fn poll_settings(&mut self) -> bool {
        if self.settings_poll.elapsed() < Duration::from_millis(500) {
            return false;
        }
        self.settings_poll = Instant::now();
        let next = match std::fs::read(self.root.join("settings.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                self.settings_error = Some(format!("Could not read Settings JSON: {error}"));
                return true;
            }
        };
        if next == self.settings_seen {
            return false;
        }
        self.settings_seen = next;
        match Config::load(&self.root) {
            Ok(config) => {
                self.behavior_directory
                    .replace_text(config.behavior.specified_directory.to_string_lossy());
                self.startup_directory.replace_text(
                    config
                        .behavior
                        .startup_specified_directory
                        .to_string_lossy(),
                );
                let prompt_changed = self.config.prompt != config.prompt;
                self.config = config.clone();
                self.last_valid_config = config;
                if prompt_changed {
                    self.publish_prompt();
                }
                self.config_valid = true;
                self.settings_error = None;
                self.message =
                    "Settings JSON updated. Terminal configuration applies to new sessions.".into();
            }
            Err(error) => {
                let error =
                    format!("Invalid Settings JSON; keeping last valid preferences: {error:#}");
                self.message = error.clone();
                self.settings_error = Some(error);
            }
        }
        true
    }
    /// Every preference shown on `tab`, back to its default.
    pub(crate) fn reset_settings_tab(&mut self, tab: Tab, tx: &mut Tx) {
        let defaults = Config::default();
        match tab {
            Tab::General => {
                self.behavior_directory.replace_text("");
                self.startup_directory.replace_text("");
                self.save_config(|c| {
                    let extra = std::mem::take(&mut c.behavior.extra);
                    c.behavior = defaults.behavior;
                    c.behavior.extra = extra;
                    c.terminal_clipboard = defaults.terminal_clipboard;
                    c.history = defaults.history;
                    c.scrollback = defaults.scrollback;
                    c.autosave = defaults.autosave;
                    c.single_click_keeps_open = defaults.single_click_keeps_open;
                    c.explorer = defaults.explorer;
                });
            }
            Tab::Appearance => {
                self.save_config(|c| {
                    c.theme_family = defaults.theme_family;
                    c.appearance = defaults.appearance;
                    c.fonts = defaults.fonts;
                    c.prompt = defaults.prompt;
                    c.show_footer = defaults.show_footer;
                });
                let (sans, mono) = self.font_families();
                tx.set_font_families(sans, mono);
            }
            Tab::Shortcuts => self.save_config(|c| c.shortcuts.clear()),
            Tab::Commands => self.save_config(|c| c.disabled.clear()),
        }
    }
}
