//! Owner preferences shown as a dedicated editor tab.
use super::*;
use std::time::{Duration, Instant};

impl State {
    pub(super) fn open_settings(&mut self, tx: &mut Tx) {
        self.supersede_pending_preview();
        self.workspace.explorer_revealed = false;
        self.settings_open = true;
        self.settings = true;
        self.search_tab = false;
        self.workspace.layout.editor = true;
        self.workspace.set_terminal_focus(false);
        tx.focus_when_shown("settings-search", true);
    }

    pub(super) fn open_settings_json(&mut self) {
        let path = self.root.join("settings.json");
        if !path.exists()
            && let Err(error) = self.config.save(&self.root)
        {
            self.message = format!("Could not create Settings JSON: {error:#}");
            return;
        }
        self.open_file(path);
    }

    pub(super) fn poll_settings(&mut self) -> bool {
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
}

/// The Settings page's tabs. Appearance holds everything that changes how
/// Rodtty looks; General holds the rest, grouped into fieldsets. A search
/// looks through every tab at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    General,
    Appearance,
    Shortcuts,
    Commands,
}

impl Tab {
    pub(super) const ALL: [Tab; 4] = [Tab::General, Tab::Appearance, Tab::Shortcuts, Tab::Commands];
    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Shortcuts => "Shortcuts",
            Tab::Commands => "Commands",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::Appearance => "appearance",
            Tab::Shortcuts => "shortcuts",
            Tab::Commands => "commands",
        }
    }
    /// Other words a search finds the tab by, such as the sections' former names.
    fn aliases(self) -> &'static str {
        match self {
            Tab::General => "Behavior",
            Tab::Appearance => "Theme",
            Tab::Shortcuts => "Keyboard Shortcuts",
            Tab::Commands => "Bundled Commands",
        }
    }
}

fn query(s: &State) -> String {
    s.settings_query.text.trim().to_lowercase()
}

/// Whether the search query occurs in the tab's names followed by `words`.
fn matches(s: &State, tab: Tab, words: &str) -> bool {
    format!("{} {} {words}", tab.label(), tab.aliases())
        .to_lowercase()
        .contains(&query(s))
}

type Build = fn(&State, &Appearance, &mut Vec<El>) -> El;

/// One setting: its name and explanation on the left, its control on the
/// right, and optionally content under both (a preview or a follow-up field).
struct Setting {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    /// What a search matches besides the tab's names, fieldset and label.
    words: &'static str,
    control: Build,
    below: Option<Build>,
}

/// A titled group of settings within a tab.
struct Fieldset {
    tab: Tab,
    key: &'static str,
    title: &'static str,
    settings: &'static [Setting],
    /// Rows built from data, already filtered by the search, for the lists
    /// of shortcuts and bundled commands.
    rows: Option<fn(&State, &Appearance) -> Vec<El>>,
}

impl Fieldset {
    fn shown_settings(&self, s: &State) -> Vec<&Setting> {
        self.settings
            .iter()
            .filter(|setting| {
                if query(s).is_empty() {
                    s.settings_tab == self.tab
                } else {
                    matches(
                        s,
                        self.tab,
                        &format!("{} {} {}", self.title, setting.label, setting.words),
                    )
                }
            })
            .collect()
    }
    fn shown_rows(&self, s: &State, a: &Appearance) -> Vec<El> {
        match self.rows {
            Some(rows) if !query(s).is_empty() || s.settings_tab == self.tab => rows(s, a),
            _ => Vec::new(),
        }
    }
}

const fn setting(key: &'static str, label: &'static str, control: Build) -> Setting {
    Setting {
        key,
        label,
        description: "",
        words: "",
        control,
        below: None,
    }
}

const FIELDSETS: &[Fieldset] = &[
    Fieldset {
        tab: Tab::General,
        key: "window",
        title: "Window",
        settings: &[
            Setting {
                words: "window close keep sessions running ask each time quit Rodtty",
                ..setting("window-close", "When the window closes", window_close)
            },
            Setting {
                description: "Applies after Quit or restart; reopening a running host keeps its sessions. The file explorer always starts hidden.",
                words: "startup launch restore workspace one single fresh terminal",
                ..setting("terminal-startup", "On launch", terminal_startup)
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::General,
        key: "terminal",
        title: "Terminal",
        settings: &[
            Setting {
                description: "Where the first terminal opens when no saved terminals are restored, and where missing saved folders fall back. Applies at the next launch.",
                words: "startup starting default folder directory home specified launch first terminal",
                below: Some(startup_specified_directory),
                ..setting("startup-directory", "Startup folder", startup_directory)
            },
            Setting {
                description: "The first terminal opens in the startup folder.",
                words: "new terminal directory active explorer specified folder",
                below: Some(specified_directory),
                ..setting(
                    "new-terminal-directory",
                    "New terminal directory",
                    new_terminal_directory,
                )
            },
            Setting {
                words: "terminal tab close confirm running commands always confirm close immediately",
                ..setting(
                    "terminal-tab-close",
                    "Closing a terminal tab",
                    terminal_tab_close,
                )
            },
            Setting {
                words: "clipboard right click context menu paste",
                ..setting("terminal-right-click", "Right-click", right_click)
            },
            Setting {
                description: "A finished selection is copied to the clipboard.",
                words: "clipboard copy on selection",
                ..setting("copy-on-selection", "Copy on selection", copy_on_selection)
            },
            Setting {
                description: "Applies to new terminal sessions.",
                words: "history save shell commands",
                ..setting("history", "Save command history", history)
            },
            Setting {
                description: "Lines kept per session; applies to new terminal sessions.",
                words: "scrollback retained lines",
                ..setting("scrollback", "Scrollback", scrollback)
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::General,
        key: "editor",
        title: "Editor",
        settings: &[
            Setting {
                words: "autosave explicit save after delay editor focus loss",
                ..setting("autosave", "Autosave", autosave)
            },
            Setting {
                description: "How long After Delay waits once you stop typing.",
                words: "autosave delay milliseconds",
                ..setting("autosave-delay", "Autosave delay", autosave_delay)
            },
            Setting {
                description: "Otherwise single click previews a file; double click or editing keeps it open.",
                words: "single click keeps open file preview editor tabs",
                ..setting(
                    "single-click-keeps-open",
                    "Single click keeps files open",
                    single_click,
                )
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::General,
        key: "files",
        title: "Files",
        settings: &[
            Setting {
                words: "explorer confirm deletion move trash remove files",
                ..setting(
                    "settings-confirm-deletion",
                    "Confirm moving files to Trash",
                    |s, a, _| file_switch(s, a, AppAction::ToggleDeletionConfirmation),
                )
            },
            Setting {
                description: "Expands the explorer to the active terminal's folder.",
                words: "explorer",
                ..setting("settings-explorer-follow", "Follow Terminal", |s, a, _| {
                    file_switch(s, a, AppAction::ToggleFollow)
                })
            },
            Setting {
                words: "explorer",
                ..setting(
                    "settings-explorer-hidden",
                    "Show hidden files",
                    |s, a, _| file_switch(s, a, AppAction::ToggleHidden),
                )
            },
            Setting {
                words: "explorer",
                ..setting(
                    "settings-explorer-ignore",
                    "Respect Git ignore rules",
                    |s, a, _| file_switch(s, a, AppAction::ToggleIgnore),
                )
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::Appearance,
        key: "theme",
        title: "Theme",
        settings: &[
            Setting {
                words: THEME_SEARCH,
                ..setting("theme-family", "Theme family", |s, a, layers| {
                    family_picker(s, a, layers)
                })
            },
            Setting {
                description: "System follows the host's light or dark mode.",
                words: THEME_SEARCH,
                below: Some(theme_previews),
                ..setting("appearance", "Appearance", appearance)
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::Appearance,
        key: "interface",
        title: "Interface",
        settings: &[
            Setting {
                words: "ui installed preview",
                below: Some(|s, a, _| fonts::family_notes(s, a, false)),
                ..setting("ui-font-family", "Font family", |s, a, layers| {
                    fonts::family_picker(s, a, false, layers)
                })
            },
            Setting {
                words: "ui text",
                ..setting("ui-font-size", "Font size", |s, a, _| font_size(s, a, "ui"))
            },
            Setting {
                description: "The strip along the bottom of the window with the app name, the terminal's folder and the latest message.",
                words: "show hide footer status bar app name folder path messages",
                ..setting("show-footer", "Show footer", show_footer)
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::Appearance,
        key: "terminal-text",
        title: "Terminal",
        settings: &[
            Setting {
                description: "The editor uses the same family.",
                words: "terminal monospace installed preview",
                below: Some(|s, a, _| fonts::family_notes(s, a, true)),
                ..setting("terminal-font-family", "Font family", |s, a, layers| {
                    fonts::family_picker(s, a, true, layers)
                })
            },
            Setting {
                words: "terminal text",
                ..setting("terminal-font-size", "Font size", |s, a, _| {
                    font_size(s, a, "terminal")
                })
            },
            Setting {
                description: "Row height as a multiple of the font size.",
                words: "row",
                ..setting("terminal-line-height", "Line height", line_height)
            },
            Setting {
                description: "Points added to each character cell.",
                words: "character width tracking",
                ..setting("terminal-letter-spacing", "Letter spacing", letter_spacing)
            },
            Setting {
                description: "Applies to running Rodtty shells at their next prompt, unless the shell sets its own PS1.",
                words: "classic directory rodtty powerline two-line ps1",
                below: Some(prompt_style_preview),
                ..setting("prompt-style", "Prompt style", prompt_style)
            },
            Setting {
                words: "dollar chevron triangle ps1",
                below: Some(prompt_symbol_preview),
                ..setting("prompt-symbol", "Prompt symbol", prompt_symbol)
            },
        ],
        rows: None,
    },
    Fieldset {
        tab: Tab::Appearance,
        key: "editor-text",
        title: "Editor",
        settings: &[Setting {
            words: "editor text",
            ..setting("editor-font-size", "Font size", |s, a, _| {
                font_size(s, a, "editor")
            })
        }],
        rows: None,
    },
    Fieldset {
        tab: Tab::Shortcuts,
        key: "keyboard-shortcuts",
        title: "Keyboard shortcuts",
        settings: &[],
        rows: Some(shortcuts::rows),
    },
    Fieldset {
        tab: Tab::Commands,
        key: "bundled-commands",
        title: "Bundled commands",
        settings: &[],
        rows: Some(bundled_commands),
    },
];

const THEME_SEARCH: &str = "theme family Catppuccin Latte Mocha Dracula Alucard Tokyo Night Nord Gruvbox medium One Dark Pro Rodtty adaptation Light Owl Ayu Solarized Rosé Pine Dawn Moon Kanagawa Lotus Wave Everforest Light Dark System palette";

pub(super) fn content(s: &State, a: &Appearance) -> El {
    page(s, a, &mut Vec::new())
}

fn page(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    let searching = !query(s).is_empty();
    let mut page = col()
        .key("settings-page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .child(
            text("Settings")
                .role("heading")
                .level(1)
                .font(ui_style(18.0, 600.0))
                .color(a.ink()),
        )
        .child(
            row()
                .key("settings-toolbar")
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text_input(&s.settings_query, ui_label())
                        .caret(a.accent(), a.selection())
                        .key("settings-search")
                        .label("Search settings")
                        .single_line()
                        .w_full()
                        .min_w(0.0)
                        .grow(1.0)
                        .px(8.0)
                        .py(5.0)
                        .color(a.ink())
                        .placeholder("Search settings", a.muted())
                        // A thin border marks the field's bounds against the page.
                        .border(1.0, a.kit().c.border_base)
                        .radius(6.0)
                        .on_edit(|s: &mut State| &mut s.settings_query),
                )
                .child(
                    button(a, "open-settings-json", "Open Settings JSON", |s, tx| {
                        s.dispatch(AppAction::OpenSettingsJson, tx)
                    })
                    .shrink(),
                ),
        )
        .child(tab_strip(s, a, searching));
    if let Some(error) = &s.settings_error {
        page = page.child(
            text(error)
                .font(ui_label())
                .color(a.danger())
                .wrap_anywhere(),
        );
    }
    let mut list = col()
        .key("settings-list")
        .instant_changes()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(16.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    let mut count = 0;
    for tab in Tab::ALL {
        let mut fieldsets = Vec::new();
        for fieldset in FIELDSETS.iter().filter(|f| f.tab == tab) {
            let mut rows: Vec<El> = fieldset
                .shown_settings(s)
                .into_iter()
                .map(|setting| setting_row(s, a, setting, layers))
                .collect();
            rows.extend(fieldset.shown_rows(s, a));
            if !rows.is_empty() {
                fieldsets.push(fieldset_box(a, fieldset, rows));
            }
        }
        if fieldsets.is_empty() {
            continue;
        }
        count += 1;
        let mut section = col()
            .key(format!("settings-section-{}", tab.label()))
            .role("group")
            .label(tab.label())
            .w_full()
            .gap(16.0);
        // The selected tab names its own page; results from a search are
        // headed by the tab they belong to.
        if searching {
            section = section.child(
                text(tab.label())
                    .role("heading")
                    .level(2)
                    .font(ui_style(13.0, 600.0))
                    .color(a.accent()),
            );
        }
        if !searching {
            section = section.child(restore_defaults(s, a, tab));
        }
        section = section.children(fieldsets);
        list = list.child(section);
    }
    if count == 0 {
        list = list.child(
            text("No settings match your search.")
                .font(ui_label())
                .color(a.muted()),
        );
    }
    page.child(list)
}

/// The row of tabs under the search field. While a search shows results from
/// every tab, none is selected; choosing one clears the search.
fn tab_strip(s: &State, a: &Appearance, searching: bool) -> El {
    let mut strip = row()
        .key("settings-tabs")
        .role("tablist")
        .label("Settings sections")
        .w_full()
        .gap(4.0)
        .padding(3.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.kit().c.border_base)
        // Wrap onto a second line in a narrow pane so no tab is out of view.
        .flex_wrap();
    for (position, tab) in Tab::ALL.into_iter().enumerate() {
        let selected = !searching && s.settings_tab == tab;
        let key = format!("settings-page-tab-{}", tab.key());
        pressable(&key);
        strip = strip.child(
            el().key(key)
                .role("tab")
                .label(tab.label())
                .focus_selected(selected)
                .position_in_set(position + 1, Tab::ALL.len())
                .focusable(true)
                .no_window_drag()
                .focus_ring(2.0, 1.0, a.accent())
                .h(26.0_f32.max(ui_label().size * 1.5 + 6.0))
                .px(12.0)
                .center()
                .radius(6.0)
                .fill(if selected {
                    a.hover_bg()
                } else {
                    Color::TRANSPARENT
                })
                .border(
                    1.0,
                    if selected {
                        a.button_border()
                    } else {
                        Color::TRANSPARENT
                    },
                )
                .hover(a.hover_bg())
                .press(a.pressed())
                .on_press_tx(move |s: &mut State, tx| {
                    tx.blur();
                    s.settings_tab = tab;
                    s.settings_select = None;
                    s.settings_reset_confirm = None;
                    s.settings_query.replace_text("");
                })
                .child(
                    text(tab.label())
                        .font(if selected {
                            ui_style(ui_label().size, 600.0)
                        } else {
                            ui_label()
                        })
                        .color(if selected { a.ink() } else { a.muted() })
                        .a11y_hidden(),
                ),
        );
    }
    strip
}

/// A fieldset: its title above a bordered box of rows divided by thin rules.
fn fieldset_box(a: &Appearance, fieldset: &Fieldset, rows: Vec<El>) -> El {
    let divider = || {
        el().w_full()
            .h(1.0)
            .fill(a.kit().c.border_base.mix(a.panel(), 0.5))
            .a11y_hidden()
    };
    let mut body = col()
        .w_full()
        .px(14.0)
        .py(2.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.kit().c.border_base);
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(divider());
        }
        body = body.child(row);
    }
    col()
        .key(format!("settings-group-{}", fieldset.key))
        .role("group")
        .label(fieldset.title)
        .w_full()
        .gap(6.0)
        .child(
            text(fieldset.title)
                .role("heading")
                .level(3)
                .font(ui_style(12.0, 600.0))
                .color(a.muted())
                .pl(2.0),
        )
        .child(body)
}

/// A setting's line: label and description on the left, its control on the
/// right, wrapping below them when the pane is too narrow for both.
fn setting_row(s: &State, a: &Appearance, setting: &Setting, layers: &mut Vec<El>) -> El {
    let mut name = col()
        .grow(1.0)
        .shrink()
        .min_w(160.0)
        .gap(2.0)
        .child(text(setting.label).font(ui_label()).color(a.ink()).wrap());
    if !setting.description.is_empty() {
        name = name.child(
            text(setting.description)
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap(),
        );
    }
    let mut line = col()
        .key(format!("{}-setting", setting.key))
        .w_full()
        .py(10.0)
        .gap(8.0)
        .child(
            row()
                .key(format!("{}-row", setting.key))
                .w_full()
                .gap(12.0)
                .flex_wrap()
                .center_y()
                .child(name)
                .child((setting.control)(s, a, layers)),
        );
    if let Some(below) = setting.below {
        line = line.child(below(s, a, layers));
    }
    line
}

/// A row for data-driven lists, such as one bundled command.
fn plain_row(a: &Appearance, key: String, label: &str, control: El) -> El {
    row()
        .key(key)
        .w_full()
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(
            text(label)
                .font(ui_label())
                .color(a.ink())
                .grow(1.0)
                .shrink()
                .min_w(0.0),
        )
        .child(control)
}

/// The tab's one reset, above its fieldsets: a button that asks before
/// restoring every setting on the tab to its default.
fn restore_defaults(s: &State, a: &Appearance, tab: Tab) -> El {
    let mut line = row()
        .key(format!("restore-defaults-{}", tab.key()))
        .w_full()
        .gap(8.0)
        .flex_wrap()
        .center_y();
    if s.settings_reset_confirm == Some(tab) {
        line = line
            .child(
                text(format!(
                    "Restore every {} setting to its default?",
                    tab.label()
                ))
                .font(ui_label())
                .color(a.ink())
                .wrap()
                .shrink()
                .min_w(0.0),
            )
            .child(
                button(a, "confirm-reset-tab", "Restore Defaults", move |s, tx| {
                    s.settings_reset_confirm = None;
                    s.reset_settings_tab(tab, tx);
                })
                .label(format!("Restore {} defaults", tab.label())),
            )
            .child(button(a, "cancel-reset-tab", "Cancel", |s, _| {
                s.settings_reset_confirm = None;
            }));
    } else {
        line = line.child(el().grow(1.0)).child(
            button(
                a,
                format!("reset-tab-{}", tab.key()),
                "Restore Defaults",
                move |s, _| s.settings_reset_confirm = Some(tab),
            )
            .label(format!("Restore {} defaults", tab.label()))
            .help(format!(
                "Restore every {} setting to its default",
                tab.label()
            )),
        );
    }
    line
}

impl State {
    /// Every preference shown on `tab`, back to its default.
    fn reset_settings_tab(&mut self, tab: Tab, tx: &mut Tx) {
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

/// An on/off setting drawn as a switch, named for assistive technology.
fn toggle(
    a: &Appearance,
    key: impl Into<String>,
    name: &str,
    on: bool,
    flip: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let key = key.into();
    pressable(&key);
    rune_kit::Switch::new(on)
        .label(name)
        .hide_label()
        .on_toggle_tx(move |s: &mut State, tx: &mut Tx| {
            tx.blur();
            flip(s, tx);
        })
        .build(a.kit())
        .key(key)
        .no_window_drag()
        .help(name)
}

/// A dropdown choosing one of `options`, saved through `set` as it is chosen.
/// One dropdown is open at a time; its list is drawn as an overlay layer.
#[allow(clippy::too_many_arguments)]
fn choice<T: Copy + PartialEq + 'static>(
    s: &State,
    a: &Appearance,
    layers: &mut Vec<El>,
    key: &'static str,
    name: &str,
    options: Vec<(String, T)>,
    current: T,
    set: fn(&mut Config, T),
) -> El {
    let labels: Vec<&str> = options.iter().map(|(label, _)| label.as_str()).collect();
    let shown = options
        .iter()
        .find(|(_, value)| *value == current)
        .map(|(label, _)| label.clone());
    // Return opens the dropdown and chooses its focused option, as Space does.
    pressable(&format!("{key}-trigger"));
    for i in 0..options.len() {
        pressable(&format!("option{i}"));
    }
    let select = rune_kit::Select::new(key, &labels)
        .label(name)
        .current(shown.as_deref())
        .appearance(rune_kit::SelectAppearance::Field)
        .width(240.0)
        .open(s.settings_select == Some(key))
        .on_toggle(move |s: &mut State| {
            s.theme_family_picker.close();
            s.font_ui.close();
            s.font_terminal.close();
            s.settings_select = if s.settings_select == Some(key) {
                None
            } else {
                Some(key)
            };
        })
        .on_close(move |s: &mut State| {
            if s.settings_select == Some(key) {
                s.settings_select = None;
            }
        })
        .on_choose(move |s: &mut State, label: &str, _: &mut Tx| {
            s.settings_select = None;
            if let Some(&(_, value)) = options.iter().find(|(l, _)| l == label) {
                s.save_config(|c| set(c, value));
            }
        });
    select.build(a.kit(), layers).shrink().min_w(0.0)
}

fn labels<T: Copy>(options: &[(&str, T)]) -> Vec<(String, T)> {
    options.iter().map(|&(l, v)| (l.to_owned(), v)).collect()
}

/// A value between decrease and increase buttons keyed `less-{key}` and
/// `more-{key}`. `change` gets -1 or 1.
fn stepper(
    a: &Appearance,
    key: &str,
    subject: &str,
    shown: String,
    change: fn(&mut Config, f32),
) -> El {
    let step_button = |action: &str, title: &'static str, name: &str, sign: f32| {
        let name = format!("{name} {subject}");
        button(a, format!("{action}-{key}"), title, move |s, _| {
            s.save_config(|c| change(c, sign));
        })
        .w(32.0)
        .px(0.0)
        .label(name.clone())
        .help(name)
    };
    row()
        .gap(6.0)
        .center_y()
        .child(step_button("less", "−", "Decrease", -1.0))
        .child(
            text(shown)
                .key(format!("{key}-value"))
                .font(ui_label())
                .color(a.ink())
                .min_w(72.0),
        )
        .child(step_button("more", "+", "Increase", 1.0))
}

/// `value` moved by `delta`, kept to one decimal place and inside `range`.
fn step(value: f32, delta: f32, (min, max): (f32, f32)) -> f32 {
    (((value + delta) * 10.0).round() / 10.0).clamp(min, max)
}

fn window_close(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::WindowClosePolicy as Window;
    choice(
        s,
        a,
        layers,
        "window-close",
        "Window close",
        labels(&[
            ("Keep sessions running", Window::KeepSessionsRunning),
            ("Ask each time", Window::AskEachTime),
            ("Quit Rodtty", Window::QuitRodtty),
        ]),
        s.config.behavior.window_close,
        |c, v| c.behavior.window_close = v,
    )
}

fn terminal_startup(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::TerminalStartup;
    choice(
        s,
        a,
        layers,
        "terminal-startup",
        "Terminal startup",
        labels(&[
            (
                "Restore terminal arrangement",
                TerminalStartup::RestoreWorkspace,
            ),
            ("One fresh terminal", TerminalStartup::SingleFresh),
        ]),
        s.config.behavior.terminal_startup,
        |c, v| c.behavior.terminal_startup = v,
    )
}

fn new_terminal_directory(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::NewTerminalDirectory as Directory;
    choice(
        s,
        a,
        layers,
        "new-terminal-directory",
        "New terminal directory",
        labels(&[
            ("Active terminal directory", Directory::Active),
            ("Explorer folder", Directory::Explorer),
            ("Specified folder", Directory::Specified),
        ]),
        s.config.behavior.new_terminal_directory,
        |c, v| c.behavior.new_terminal_directory = v,
    )
}

/// The folder for Specified folder, shown only while that choice is made.
fn specified_directory(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    if s.config.behavior.new_terminal_directory != crate::config::NewTerminalDirectory::Specified {
        return el();
    }
    text_input(&s.behavior_directory, ui_label())
        .caret(a.accent(), a.selection())
        .key("specified-directory")
        .label("Specified terminal folder")
        .single_line()
        .w_full()
        .px(8.0)
        .py(5.0)
        .color(a.ink())
        .placeholder("Absolute folder path", a.muted())
        .border(1.0, a.kit().c.border_base)
        .radius(6.0)
        .on_edit(|s: &mut State| &mut s.behavior_directory)
        .on_input_change(|s: &mut State, _| {
            let path = PathBuf::from(&s.behavior_directory.text);
            s.save_config(|c| c.behavior.specified_directory = path);
        })
}

fn startup_directory(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::StartupDirectory as Directory;
    choice(
        s,
        a,
        layers,
        "startup-directory",
        "Startup folder",
        labels(&[
            ("Home folder", Directory::Home),
            ("Specified folder", Directory::Specified),
        ]),
        s.config.behavior.startup_directory,
        |c, v| c.behavior.startup_directory = v,
    )
}

/// The folder for the Startup folder's Specified folder, shown only while chosen.
fn startup_specified_directory(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    if s.config.behavior.startup_directory != crate::config::StartupDirectory::Specified {
        return el();
    }
    text_input(&s.startup_directory, ui_label())
        .caret(a.accent(), a.selection())
        .key("startup-specified-directory")
        .label("Specified startup folder")
        .single_line()
        .w_full()
        .px(8.0)
        .py(5.0)
        .color(a.ink())
        .placeholder("Absolute folder path", a.muted())
        .border(1.0, a.kit().c.border_base)
        .radius(6.0)
        .on_edit(|s: &mut State| &mut s.startup_directory)
        .on_input_change(|s: &mut State, _| {
            let path = PathBuf::from(&s.startup_directory.text);
            s.save_config(|c| c.behavior.startup_specified_directory = path);
        })
}

fn terminal_tab_close(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::TerminalClosePolicy as Terminal;
    choice(
        s,
        a,
        layers,
        "terminal-tab-close",
        "Terminal tab close",
        labels(&[
            (
                "Confirm when commands are running",
                Terminal::ConfirmRunning,
            ),
            ("Always confirm", Terminal::AlwaysConfirm),
            ("Close immediately", Terminal::CloseImmediately),
        ]),
        s.config.behavior.terminal_tab_close,
        |c, v| c.behavior.terminal_tab_close = v,
    )
}

fn right_click(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::TerminalRightClick;
    choice(
        s,
        a,
        layers,
        "terminal-right-click",
        "Terminal right-click",
        labels(&[
            ("Context menu", TerminalRightClick::ContextMenu),
            ("Paste", TerminalRightClick::Paste),
        ]),
        s.config.terminal_clipboard.right_click,
        |c, v| c.terminal_clipboard.right_click = v,
    )
}

fn copy_on_selection(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "copy-on-selection",
        "Copy on selection",
        s.config.terminal_clipboard.copy_on_selection,
        |s, _| {
            s.save_config(|c| {
                c.terminal_clipboard.copy_on_selection = !c.terminal_clipboard.copy_on_selection
            })
        },
    )
}

fn history(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "history",
        "Save command history",
        s.config.history,
        |s, _| s.save_config(|c| c.history = !c.history),
    )
}

fn scrollback(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "scrollback",
        "scrollback by 1000 lines",
        format!("{} lines", s.config.scrollback),
        |c, sign| {
            c.scrollback = if sign < 0.0 {
                c.scrollback.saturating_sub(1000).max(100)
            } else {
                (c.scrollback + 1000).min(100_000)
            }
        },
    )
}

fn autosave(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use config::AutosavePolicy;
    choice(
        s,
        a,
        layers,
        "autosave",
        "Autosave",
        labels(&[
            ("Explicit Save", AutosavePolicy::Explicit),
            ("After Delay", AutosavePolicy::AfterDelay),
            ("Editor Focus Loss", AutosavePolicy::EditorFocusLoss),
        ]),
        s.config.autosave.policy,
        |c, v| c.autosave.policy = v,
    )
}

fn autosave_delay(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "autosave-delay",
        "autosave delay by 100 ms",
        format!("{} ms", s.config.autosave.delay_ms),
        |c, sign| {
            c.autosave.delay_ms = if sign < 0.0 {
                c.autosave.delay_ms.saturating_sub(100).max(100)
            } else {
                (c.autosave.delay_ms + 100).min(60_000)
            }
        },
    )
}

fn single_click(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "single-click-keeps-open",
        "Single click keeps files open",
        s.config.single_click_keeps_open,
        |s, _| s.save_config(|c| c.single_click_keeps_open = !c.single_click_keeps_open),
    )
}

/// A file explorer preference's switch; its action keeps the explorer in step.
fn file_switch(s: &State, a: &Appearance, action: AppAction) -> El {
    let e = &s.config.explorer;
    let (key, name, on) = match action {
        AppAction::ToggleDeletionConfirmation => (
            "settings-confirm-deletion",
            "Confirm moving files to Trash",
            e.confirm_deletion,
        ),
        AppAction::ToggleFollow => (
            "settings-explorer-follow",
            "Follow Terminal",
            e.follow_terminal,
        ),
        AppAction::ToggleHidden => (
            "settings-explorer-hidden",
            "Show hidden files",
            e.show_hidden,
        ),
        _ => (
            "settings-explorer-ignore",
            "Respect Git ignore rules",
            e.respect_ignore,
        ),
    };
    toggle(a, key, name, on, move |s, tx| s.dispatch(action, tx))
}

fn appearance(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::Appearance as Mode;
    choice(
        s,
        a,
        layers,
        "appearance",
        "Appearance",
        labels(&[
            ("Light", Mode::Light),
            ("Dark", Mode::Dark),
            ("System", Mode::System),
        ]),
        s.config.appearance,
        |c, v| c.appearance = v,
    )
}

fn theme_previews(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    use crate::config::Appearance as Mode;
    col()
        .w_full()
        .gap(8.0)
        .child(
            text(format!("{} · applies immediately", a.name))
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap(),
        )
        .child(palette_preview(s.config.theme_family, Mode::Light))
        .child(palette_preview(s.config.theme_family, Mode::Dark))
        .child(
            text(a.notes)
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap_anywhere(),
        )
}

fn show_footer(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "show-footer",
        "Show footer",
        s.config.show_footer,
        |s, _| s.save_config(|c| c.show_footer = !c.show_footer),
    )
}

/// The UI, terminal or editor font size.
fn font_size(s: &State, a: &Appearance, target: &'static str) -> El {
    let fonts = &s.config.fonts;
    let (size, subject, change): (f32, &str, fn(&mut Config, f32)) = match target {
        "ui" => (fonts.ui_size, "UI font size", |c, d| {
            c.fonts.ui_size = (c.fonts.ui_size + d).clamp(8.0, 32.0)
        }),
        "terminal" => (fonts.terminal_size, "terminal font size", |c, d| {
            c.fonts.terminal_size = (c.fonts.terminal_size + d).clamp(8.0, 32.0)
        }),
        _ => (fonts.editor_size, "editor font size", |c, d| {
            c.fonts.editor_size = (c.fonts.editor_size + d).clamp(8.0, 32.0)
        }),
    };
    stepper(
        a,
        &format!("{target}-font-size"),
        subject,
        format!("{size}px"),
        change,
    )
}

fn line_height(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "terminal-line-height",
        "terminal line height",
        format!("{:.1}×", s.config.fonts.terminal_line_height),
        |c, sign| {
            let f = &mut c.fonts;
            f.terminal_line_height = step(
                f.terminal_line_height,
                0.1 * sign,
                config::TERMINAL_LINE_HEIGHT,
            );
        },
    )
}

fn letter_spacing(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "terminal-letter-spacing",
        "terminal letter spacing",
        format!("{:+.1}px", s.config.fonts.terminal_letter_spacing),
        |c, sign| {
            let f = &mut c.fonts;
            f.terminal_letter_spacing = step(
                f.terminal_letter_spacing,
                0.5 * sign,
                config::TERMINAL_LETTER_SPACING,
            );
        },
    )
}

fn bundled_commands(s: &State, a: &Appearance) -> Vec<El> {
    profile::utility_names()
        .into_iter()
        .filter(|name| {
            if query(s).is_empty() {
                s.settings_tab == Tab::Commands
            } else {
                matches(s, Tab::Commands, name)
            }
        })
        .map(|name| {
            let enabled = !s.config.disabled.contains(&name);
            let key = format!("utility-{name}");
            let flip = name.clone();
            plain_row(
                a,
                format!("{key}-row"),
                &name,
                toggle(a, key, &name, enabled, move |s, _| {
                    let name = flip.clone();
                    s.save_config(move |config| {
                        if config.disabled.contains(&name) {
                            config.disabled.retain(|n| n != &name);
                        } else {
                            config.disabled.push(name);
                        }
                    });
                }),
            )
        })
        .collect()
}

fn family_picker(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::ThemeFamily;
    let options: Vec<_> = ThemeFamily::ALL
        .iter()
        .map(|family| rune_kit::ComboboxOption {
            id: family.label().into(),
            label: family.label().into(),
            detail: match family {
                ThemeFamily::OneDarkPro => "Rodtty light adaptation / Dark",
                ThemeFamily::Gruvbox => "Medium contrast Light / Dark",
                ThemeFamily::Dracula => "Alucard / Dracula",
                ThemeFamily::NightOwl => "Light Owl / Night Owl",
                ThemeFamily::Catppuccin => "Latte / Mocha",
                ThemeFamily::TokyoNight => "Light / Night",
                ThemeFamily::RosePine => "Dawn / Moon",
                ThemeFamily::Kanagawa => "Lotus / Wave",
                ThemeFamily::Everforest => "Light / Dark Green",
                _ => "Light / Dark",
            }
            .into(),
            disabled: false,
        })
        .collect();
    pressable("theme-family-value-trigger");
    rune_kit::Combobox::new(
        "theme-family",
        &s.theme_family_picker,
        &options,
        |s: &mut State| {
            s.font_ui.close();
            s.font_terminal.close();
            s.settings_select = None;
            &mut s.theme_family_picker
        },
    )
    .label("Theme family")
    .current(Some(s.config.theme_family.label()))
    .search_placeholder("Search theme families")
    .width(240.0)
    .max_height(256.0)
    .on_choose(|s, label, _| {
        if let Some(&family) = ThemeFamily::ALL.iter().find(|f| f.label() == label) {
            s.save_config(|c| c.theme_family = family);
        }
    })
    .build(a.kit(), layers)
    .shrink()
    .min_w(0.0)
}

/// The open dropdown's or picker's list, drawn above the page.
pub(super) fn overlays(s: &State, a: &Appearance, width: f32) -> Vec<El> {
    let mut layers = Vec::new();
    if s.settings
        && !s.application_overlay_open()
        && pane_visibility(s, width).2
        && (s.settings_select.is_some()
            || s.theme_family_picker.is_open()
            || s.font_ui.is_open()
            || s.font_terminal.is_open())
    {
        let _ = page(s, a, &mut layers);
    }
    layers
}

fn prompt_style(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::prompt::PromptStyle;
    let options = PromptStyle::ALL
        .iter()
        .map(|&style| (style.label().to_owned(), style))
        .collect();
    choice(
        s,
        a,
        layers,
        "prompt-style",
        "Prompt style",
        options,
        s.config.prompt.style,
        |c, v| c.prompt.style = v,
    )
}

fn prompt_style_preview(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    let mut preview = col()
        .key("prompt-style-preview")
        .gap(4.0)
        .child(prompt_preview(s.config.prompt, false, a));
    if s.config.prompt.style.reports_status() {
        preview = preview
            .child(
                text("After a failed command:")
                    .font(ui_style(12.0, 400.0))
                    .color(a.muted()),
            )
            .child(prompt_preview(s.config.prompt, true, a));
    }
    preview
}

fn prompt_symbol(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::prompt::PromptSymbol;
    let options = PromptSymbol::ALL
        .iter()
        .map(|&symbol| (symbol_name(symbol), symbol))
        .collect();
    choice(
        s,
        a,
        layers,
        "prompt-symbol",
        "Prompt symbol",
        options,
        s.config.prompt.symbol,
        |c, v| c.prompt.symbol = v,
    )
}

/// The line holding the symbol, as drawn after success and, when the style
/// reports it, after failure.
fn prompt_symbol_preview(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    let prefs = s.config.prompt;
    let last_line = |failed| {
        let lines = prefs.preview(failed, PREVIEW_DIRECTORY);
        prompt_line(lines.last().map(Vec::as_slice).unwrap_or_default(), a)
    };
    let mut preview = row()
        .key("prompt-symbol-preview")
        .gap(12.0)
        .scroll_x()
        .child(last_line(false));
    if prefs.style.reports_status() {
        preview = preview.child(last_line(true));
    }
    preview
}

/// A prompt symbol's spoken name: a screen reader may skip or misread the
/// glyph alone.
fn symbol_name(symbol: crate::prompt::PromptSymbol) -> String {
    use crate::prompt::PromptSymbol;
    let word = match symbol {
        PromptSymbol::Dollar => "Dollar sign",
        PromptSymbol::Chevron => "Chevron",
        PromptSymbol::Triangle => "Triangle",
        PromptSymbol::DoubleChevron => "Double chevron",
    };
    format!("{word} {}", symbol.glyph())
}

const PREVIEW_DIRECTORY: &str = "~/project";

/// The prompt drawn with the active theme's ANSI palette, as the terminal would.
fn prompt_preview(prefs: crate::prompt::PromptPreferences, failed: bool, a: &Appearance) -> El {
    let mut lines = col()
        .px(8.0)
        .py(6.0)
        .radius(6.0)
        .fill(a.bg())
        .label(prefs.preview_text(failed, PREVIEW_DIRECTORY));
    for line in prefs.preview(failed, PREVIEW_DIRECTORY) {
        lines = lines.child(prompt_line(&line, a));
    }
    lines
}

fn prompt_line(line: &[(String, crate::prompt::Segment)], a: &Appearance) -> El {
    let font = ui_label().family(Family::Mono);
    let mut spans = row().label(line.iter().map(|(t, _)| t.as_str()).collect::<String>());
    for (content, segment) in line {
        let color = segment
            .fg
            .map_or(a.ink(), |i| a.terminal(vt100::Color::Idx(i), true));
        let mut font = font;
        if segment.bold {
            font.weight = 700.0;
        }
        let mut span = row().child(text(content).font(font).color(color));
        if let Some(bg) = segment.bg {
            span = span.fill(a.terminal(vt100::Color::Idx(bg), false));
        }
        spans = spans.child(span);
    }
    spans
}

fn palette_preview(family: crate::config::ThemeFamily, mode: crate::config::Appearance) -> El {
    let p = Appearance::resolve(family, mode, false);
    let mut swatches = row().gap(3.0);
    for i in 0..8 {
        swatches = swatches.child(
            el().size(16.0, 10.0)
                .radius(2.0)
                .fill(p.terminal(vt100::Color::Idx(i + 1), true)),
        );
    }
    col()
        .key(format!(
            "palette-preview-{}",
            if p.dark { "dark" } else { "light" }
        ))
        .w_full()
        .min_w(0.0)
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .radius(6.0)
        .fill(p.bg())
        .border(1.0, p.kit().c.border_base)
        .child(text(p.name).font(ui_label()).color(p.ink()).wrap())
        .child(
            text("rodtty:~/projects $ cargo build")
                .font(ui_label())
                .color(p.accent())
                .wrap(),
        )
        .child(swatches)
}

/// Tests: restores every setting on the tab keyed `tab` (`general`,
/// `appearance`, …) through its Restore Defaults button and confirmation.
#[cfg(test)]
pub(super) fn restore_tab_defaults(rt: &mut rune::Runtime<State>, tab: &str) {
    for key in [
        format!("settings-page-tab-{tab}"),
        format!("reset-tab-{tab}"),
        "confirm-reset-tab".to_owned(),
    ] {
        let (x, y) = rt
            .find(&key)
            .unwrap_or_else(|| panic!("missing control {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
    }
}

/// Tests: opens the dropdown keyed `key` and chooses its option `label`, as a
/// pointer would.
#[cfg(test)]
pub(super) fn choose(rt: &mut rune::Runtime<State>, key: &str, label: &str) {
    let press = |rt: &mut rune::Runtime<State>, (x, y): (f32, f32)| {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
    };
    let trigger = rt
        .find(&format!("{key}-trigger"))
        .unwrap_or_else(|| panic!("missing dropdown {key}"));
    press(rt, trigger);
    let option = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "option" && e.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("dropdown {key} has no option {label}"))
        .rect;
    press(
        rt,
        (option[0] + option[2] * 0.5, option[1] + option[3] * 0.5),
    );
}

#[cfg(test)]
mod tests;
