//! Owner preferences shown as a dedicated editor tab.
use super::*;
use std::time::{Duration, Instant};

impl State {
    pub(super) fn open_settings(&mut self, tx: &mut Tx) {
        self.supersede_pending_preview();
        self.workspace.explorer_revealed = false;
        self.settings_open = true;
        self.settings = true;
        self.workspace.layout.editor = true;
        self.workspace.terminal_focus = false;
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
                self.config = config.clone();
                self.last_valid_config = config;
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

fn matches(s: &State, section: &str, name: &str) -> bool {
    let query = s.settings_query.text.trim().to_lowercase();
    format!("{section} {name}").to_lowercase().contains(&query)
}

pub(super) fn content(s: &State, a: &Appearance) -> El {
    let mut page = col()
        .key("settings-page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(12.0)
        .py(12.0)
        .gap(10.0)
        .child(text("Settings").font(ui_style(18.0, 600.0)).color(a.ink()))
        .child(
            text_input(&s.settings_query, ui_label())
                .caret(a.accent(), a.selection())
                .key("settings-search")
                .label("Search settings")
                .w_full()
                .h(30.0)
                .color(a.ink())
                .placeholder("Search settings", a.muted())
                .on_edit(|s: &mut State| &mut s.settings_query),
        )
        .child(button(
            a,
            "open-settings-json",
            "Open Settings JSON",
            |s, tx| s.dispatch(AppAction::OpenSettingsJson, tx),
        ));
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
        .gap(12.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    let mut count = 0;
    for section in [
        "Appearance",
        "Behavior",
        "Keyboard Shortcuts",
        "Terminal",
        "Editor",
        "Files",
        "Bundled Commands",
    ] {
        let visible = if section == "Appearance" {
            matches(s, section, THEME_SEARCH)
                || matches(
                    s,
                    section,
                    "font size ui font family terminal font family installed monospace preview",
                )
        } else if section == "Keyboard Shortcuts" {
            s.commands.iter().any(|c| {
                matches(
                    s,
                    section,
                    &format!("{} {} {}", c.id, c.title, s.shortcut_label(c.id)),
                )
            })
        } else if section == "Behavior" {
            matches(
                s,
                section,
                "single click keeps open file preview editor tabs",
            ) || matches(
                s,
                section,
                "window close keep sessions running ask each time quit Rodtty",
            ) || matches(
                s,
                section,
                "terminal tab close confirm running commands always confirm close immediately",
            ) || matches(
                s,
                section,
                "new terminal directory active explorer specified folder",
            ) || matches(
                s,
                section,
                "startup restore workspace one single fresh terminal",
            )
        } else if section == "Terminal" {
            matches(s, section, "history save shell commands")
                || matches(s, section, "scrollback retained lines")
                || matches(s, section, "font size")
        } else if section == "Editor" {
            matches(s, section, "font size")
                || matches(
                    s,
                    section,
                    "autosave explicit save after delay editor focus loss milliseconds",
                )
        } else if section == "Files" {
            matches(
                s,
                section,
                "explorer follow terminal show hidden files respect ignore rules confirm deletion trash",
            )
        } else if section == "Bundled Commands" {
            profile::utility_names()
                .iter()
                .any(|name| matches(s, section, name))
        } else {
            matches(s, section, "")
        };
        if !visible {
            continue;
        }
        count += 1;
        let mut group = col()
            .key(format!("settings-section-{section}"))
            .w_full()
            .gap(6.0)
            .child(text(section).font(ui_style(13.0, 600.0)).color(a.accent()));
        let target = match section {
            "Appearance" => Some("ui"),
            "Terminal" => Some("terminal"),
            "Editor" => Some("editor"),
            _ => None,
        };
        if let Some(target) = target.filter(|_| matches(s, section, "font size")) {
            let size = match target {
                "ui" => s.config.fonts.ui_size,
                "terminal" => s.config.fonts.terminal_size,
                _ => s.config.fonts.editor_size,
            };
            let sizes = col().w_full().gap(6.0).child(
                text(format!(
                    "{} font size: {size}px",
                    if target == "ui" { "UI" } else { target }
                ))
                .font(ui_label())
                .color(a.ink()),
            );
            let mut buttons = row().gap(6.0).scroll_x();
            for (action, title, delta) in [
                ("less", "−", -1.0),
                ("more", "+", 1.0),
                ("reset", "Reset", 0.0),
            ] {
                buttons = buttons.child(button(
                    a,
                    format!("{action}-{target}-font-size"),
                    title,
                    move |s, _| {
                        s.save_config(|c| {
                            let defaults = config::FontPreferences::default();
                            let (size, default) = match target {
                                "ui" => (&mut c.fonts.ui_size, defaults.ui_size),
                                "terminal" => (&mut c.fonts.terminal_size, defaults.terminal_size),
                                _ => (&mut c.fonts.editor_size, defaults.editor_size),
                            };
                            *size = if delta == 0.0 {
                                default
                            } else {
                                (*size + delta).clamp(8.0, 32.0)
                            };
                        });
                    },
                ));
            }
            group = group.child(sizes.child(buttons));
        }
        match section {
            "Editor"
                if matches(
                    s,
                    section,
                    "autosave explicit save after delay editor focus loss milliseconds",
                ) =>
            {
                use config::AutosavePolicy;
                let mut choices = row().gap(6.0).scroll_x();
                for (id, title, policy) in [
                    ("explicit", "Explicit Save", AutosavePolicy::Explicit),
                    ("after-delay", "After Delay", AutosavePolicy::AfterDelay),
                    (
                        "editor-focus-loss",
                        "Editor Focus Loss",
                        AutosavePolicy::EditorFocusLoss,
                    ),
                ] {
                    choices = choices.child(button(
                        a,
                        format!("autosave-{id}"),
                        format!(
                            "{}{}",
                            if s.config.autosave.policy == policy {
                                "✓ "
                            } else {
                                ""
                            },
                            title
                        ),
                        move |s, _| {
                            s.save_config(|c| c.autosave.policy = policy);
                        },
                    ));
                }
                group = group
                    .child(text("Autosave").font(ui_label()).color(a.ink()))
                    .child(choices)
                    .child(
                        text(format!("Autosave delay: {} ms", s.config.autosave.delay_ms))
                            .font(ui_label())
                            .color(a.ink()),
                    )
                    .child(
                        row()
                            .gap(6.0)
                            .child(button(a, "less-autosave-delay", "− 100 ms", |s, _| {
                                s.save_config(|c| {
                                    c.autosave.delay_ms =
                                        c.autosave.delay_ms.saturating_sub(100).max(100)
                                });
                            }))
                            .child(button(a, "more-autosave-delay", "+ 100 ms", |s, _| {
                                s.save_config(|c| {
                                    c.autosave.delay_ms = (c.autosave.delay_ms + 100).min(60_000)
                                });
                            }))
                            .child(button(a, "reset-autosave", "Reset Autosave", |s, _| {
                                s.save_config(|c| c.autosave = Default::default());
                            })),
                    );
            }
            "Keyboard Shortcuts" => {
                group = group.child(shortcuts::list(s, a));
            }
            "Behavior" => {
                if matches(
                    s,
                    section,
                    "startup restore workspace one single fresh terminal",
                ) {
                    use crate::config::TerminalStartup;
                    group = group.child(text("Terminal startup").font(ui_label()).color(a.muted()));
                    for (key, title, choice) in [
                        (
                            "startup-restore",
                            "Restore terminal arrangement",
                            TerminalStartup::RestoreWorkspace,
                        ),
                        (
                            "startup-single-fresh",
                            "One fresh terminal",
                            TerminalStartup::SingleFresh,
                        ),
                    ] {
                        group = group.child(
                            button(
                                a,
                                key,
                                format!(
                                    "{} {title}",
                                    if s.config.behavior.terminal_startup == choice {
                                        "●"
                                    } else {
                                        "○"
                                    }
                                ),
                                move |s, _| {
                                    s.save_config(|c| c.behavior.terminal_startup = choice);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(button(a, "reset-terminal-startup", "Reset terminal startup", |s, _| {
                        s.save_config(|c| c.behavior.terminal_startup = TerminalStartup::default());
                    })).child(text("Applies after Quit or restart. Reopening a running host keeps its sessions. One fresh terminal uses the new-terminal directory choice; first launch keeps the startup directory.").font(ui_label()).color(a.muted()).wrap());
                }
                if matches(
                    s,
                    section,
                    "single click keeps open file preview editor tabs",
                ) {
                    group = group
                    .child(button(
                        a,
                        "single-click-keeps-open",
                        format!(
                            "Single click keeps files open: {}",
                            if s.config.single_click_keeps_open { "on" } else { "off" }
                        ),
                        |s, _| {
                            s.save_config(|c| c.single_click_keeps_open = !c.single_click_keeps_open);
                        },
                    ))
                    .child(button(a, "reset-single-click-keeps-open", "Reset", |s, _| {
                        s.save_config(|c| {
                            c.single_click_keeps_open = Config::default().single_click_keeps_open;
                        });
                    }))
                    .child(
                        text("By default, single click previews a file; double click or editing keeps it open.")
                            .font(ui_label())
                            .color(a.muted())
                            .wrap(),
                    );
                }
                use crate::config::WindowClosePolicy as Window;
                if matches(
                    s,
                    section,
                    "window close keep sessions running ask each time quit Rodtty",
                ) {
                    group = group.child(text("Window close").font(ui_label()).color(a.muted()));
                    for (key, label, policy) in [
                        (
                            "window-close-keep",
                            "Keep sessions running",
                            Window::KeepSessionsRunning,
                        ),
                        ("window-close-ask", "Ask each time", Window::AskEachTime),
                        ("window-close-quit", "Quit Rodtty", Window::QuitRodtty),
                    ] {
                        group = group.child(
                            button(
                                a,
                                key,
                                format!(
                                    "{} {label}",
                                    if s.config.behavior.window_close == policy {
                                        "●"
                                    } else {
                                        "○"
                                    }
                                ),
                                move |s, _| {
                                    s.save_config(|c| c.behavior.window_close = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(button(
                        a,
                        "reset-window-close",
                        "Reset window close",
                        |s, _| {
                            s.save_config(|c| c.behavior.window_close = Window::default());
                        },
                    ));
                }
                use crate::config::{
                    NewTerminalDirectory as Directory, TerminalClosePolicy as Terminal,
                };
                if matches(
                    s,
                    section,
                    "terminal tab close confirm running commands always confirm close immediately",
                ) {
                    group =
                        group.child(text("Terminal tab close").font(ui_label()).color(a.muted()));
                    for (key, label, policy) in [
                        (
                            "terminal-close-running",
                            "Confirm when commands are running",
                            Terminal::ConfirmRunning,
                        ),
                        (
                            "terminal-close-always",
                            "Always confirm",
                            Terminal::AlwaysConfirm,
                        ),
                        (
                            "terminal-close-immediate",
                            "Close immediately",
                            Terminal::CloseImmediately,
                        ),
                    ] {
                        group = group.child(
                            button(
                                a,
                                key,
                                format!(
                                    "{} {label}",
                                    if s.config.behavior.terminal_tab_close == policy {
                                        "●"
                                    } else {
                                        "○"
                                    }
                                ),
                                move |s, _| {
                                    s.save_config(|c| c.behavior.terminal_tab_close = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(button(
                        a,
                        "reset-terminal-close",
                        "Reset terminal tab close",
                        |s, _| {
                            s.save_config(|c| c.behavior.terminal_tab_close = Terminal::default());
                        },
                    ));
                }
                if matches(
                    s,
                    section,
                    "new terminal directory active explorer specified folder",
                ) {
                    group = group.child(
                        text("New terminal directory")
                            .font(ui_label())
                            .color(a.muted()),
                    );
                    for (key, label, policy) in [
                        (
                            "new-directory-active",
                            "Active terminal directory",
                            Directory::Active,
                        ),
                        (
                            "new-directory-explorer",
                            "Explorer folder",
                            Directory::Explorer,
                        ),
                        (
                            "new-directory-specified",
                            "Specified folder",
                            Directory::Specified,
                        ),
                    ] {
                        group = group.child(
                            button(
                                a,
                                key,
                                format!(
                                    "{} {label}",
                                    if s.config.behavior.new_terminal_directory == policy {
                                        "●"
                                    } else {
                                        "○"
                                    }
                                ),
                                move |s, _| {
                                    s.save_config(|c| c.behavior.new_terminal_directory = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group
                        .child(button(a, "reset-new-directory", "Reset new terminal directory", |s, _| {
                            s.save_config(|c| c.behavior.new_terminal_directory = Directory::default());
                        }))
                        .child(text_input(&s.behavior_directory, ui_label())
                            .caret(a.accent(), a.selection()).key("specified-directory")
                            .label("Specified terminal folder").single_line().w_full().h(30.0).color(a.ink())
                            .placeholder("Absolute folder path", a.muted())
                            .on_edit(|s: &mut State| &mut s.behavior_directory)
                            .on_input_change(|s: &mut State, _| {
                                let path = PathBuf::from(&s.behavior_directory.text);
                                s.save_config(|c| c.behavior.specified_directory = path);
                            }))
                        .child(button(a, "reset-specified-directory", "Reset specified folder", |s, _| {
                            s.behavior_directory.replace_text("");
                            s.save_config(|c| c.behavior.specified_directory = PathBuf::new());
                        }))
                        .child(text("The first terminal keeps the startup directory. These choices apply to new tabs.").font(ui_label()).color(a.muted()).wrap());
                }
            }
            "Appearance" if matches(s, section, THEME_SEARCH) => {
                use crate::config::Appearance as Mode;
                group = group
                    .child(text("Theme family").font(ui_label()).color(a.muted()))
                    .child(family_picker(s, a, &mut Vec::new()))
                    .child(button(a, "reset-theme-family", "Reset family", |s, _| {
                        s.save_config(|c| c.theme_family = Config::default().theme_family);
                    }));
                let mut choices = row().gap(6.0).scroll_x();
                for (key, label, mode) in [
                    ("light", "Light", Mode::Light),
                    ("dark", "Dark", Mode::Dark),
                    ("system", "System", Mode::System),
                ] {
                    choices = choices.child(
                        button(a, format!("appearance-{key}"), label, move |s, _| {
                            s.save_config(|c| c.appearance = mode)
                        })
                        .role("radio")
                        .checked(s.config.appearance == mode)
                        .border(
                            1.0,
                            if s.config.appearance == mode {
                                a.accent()
                            } else {
                                a.panel()
                            },
                        ),
                    );
                }
                group = group
                    .child(choices)
                    .child(
                        text(format!("{} · applies immediately", a.name))
                            .font(ui_label())
                            .color(a.muted())
                            .wrap(),
                    )
                    .child(palette_preview(s.config.theme_family, Mode::Light))
                    .child(palette_preview(s.config.theme_family, Mode::Dark))
                    .child(button(a, "reset-appearance", "Reset appearance", |s, _| {
                        s.save_config(|c| c.appearance = Config::default().appearance);
                    }))
                    .child(
                        text(a.notes)
                            .font(ui_label())
                            .color(a.muted())
                            .wrap_anywhere(),
                    );
            }
            "Appearance" | "Editor" => {}
            "Terminal" => {
                if matches(s, section, "history save shell commands") {
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(button(
                                a,
                                "history",
                                format!("History: {}", if s.config.history { "on" } else { "off" }),
                                |s, _| {
                                    s.save_config(|config| config.history = !config.history);
                                },
                            ))
                            .child(button(a, "reset-history", "Reset", |s, _| {
                                s.save_config(|config| config.history = Config::default().history);
                            })),
                    );
                }
                if matches(s, section, "scrollback retained lines") {
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .scroll_x()
                            .child(
                                text(format!("Scrollback: {} lines", s.config.scrollback))
                                    .font(ui_label())
                                    .color(a.ink()),
                            )
                            .child(button(a, "less-scrollback", "− 1000", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback =
                                        config.scrollback.saturating_sub(1000).max(100)
                                });
                            }))
                            .child(button(a, "more-scrollback", "+ 1000", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback = (config.scrollback + 1000).min(100_000)
                                });
                            }))
                            .child(button(a, "reset-scrollback", "Reset", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback = Config::default().scrollback
                                });
                            })),
                    );
                }
                group = group.child(
                    text(
                        "Command, history, and scrollback choices apply to new terminal sessions.",
                    )
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
                );
            }
            "Files" => {
                for (key, title, enabled, action) in [
                    (
                        "settings-confirm-deletion",
                        "Confirm moving files to Trash",
                        s.config.explorer.confirm_deletion,
                        AppAction::ToggleDeletionConfirmation,
                    ),
                    (
                        "settings-explorer-follow",
                        "Follow Terminal",
                        s.config.explorer.follow_terminal,
                        AppAction::ToggleFollow,
                    ),
                    (
                        "settings-explorer-hidden",
                        "Show hidden files",
                        s.config.explorer.show_hidden,
                        AppAction::ToggleHidden,
                    ),
                    (
                        "settings-explorer-ignore",
                        "Respect Git ignore rules",
                        s.config.explorer.respect_ignore,
                        AppAction::ToggleIgnore,
                    ),
                ] {
                    if matches(s, section, title)
                        || (key == "settings-confirm-deletion"
                            && matches(s, section, "confirm deletion move trash remove files"))
                    {
                        group = group.child(
                            row()
                                .gap(6.0)
                                .center_y()
                                .child(button(
                                    a,
                                    key,
                                    format!("{title}: {}", if enabled { "on" } else { "off" }),
                                    move |s, tx| s.dispatch(action, tx),
                                ))
                                .child(button(a, format!("reset-{key}"), "Reset", move |s, _| {
                                    s.save_config(|config| {
                                        let defaults = Config::default().explorer;
                                        match action {
                                            AppAction::ToggleDeletionConfirmation => {
                                                config.explorer.confirm_deletion =
                                                    defaults.confirm_deletion
                                            }
                                            AppAction::ToggleFollow => {
                                                config.explorer.follow_terminal =
                                                    defaults.follow_terminal
                                            }
                                            AppAction::ToggleHidden => {
                                                config.explorer.show_hidden = defaults.show_hidden
                                            }
                                            AppAction::ToggleIgnore => {
                                                config.explorer.respect_ignore =
                                                    defaults.respect_ignore
                                            }
                                            _ => unreachable!(),
                                        }
                                    });
                                })),
                        );
                    }
                }
                group = group.child(button(
                    a,
                    "reset-explorer-preferences",
                    "Reset file preferences",
                    |s, _| {
                        s.save_config(|config| config.explorer = Default::default());
                    },
                ));
            }
            "Bundled Commands" => {
                for name in profile::utility_names() {
                    if !matches(s, section, &name) {
                        continue;
                    }
                    let enabled = !s.config.disabled.contains(&name);
                    let reset = name.clone();
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(button(
                                a,
                                format!("utility-{name}"),
                                format!("{} {name}", if enabled { "✓" } else { "−" }),
                                move |s, _| {
                                    s.save_config(|config| {
                                        if config.disabled.contains(&name) {
                                            config.disabled.retain(|n| n != &name);
                                        } else {
                                            config.disabled.push(name.clone());
                                        }
                                    });
                                },
                            ))
                            .child(button(
                                a,
                                format!("reset-utility-{reset}"),
                                "Reset",
                                move |s, _| {
                                    s.save_config(|config| config.disabled.retain(|n| n != &reset));
                                },
                            )),
                    );
                }
            }
            _ => {
                group = group.child(
                    text("No configurable preferences in this section yet.")
                        .font(ui_label())
                        .color(a.muted())
                        .wrap(),
                );
            }
        }
        if section == "Appearance"
            && matches(
                s,
                section,
                "ui font family terminal font family installed monospace preview",
            )
        {
            group = group.child(fonts::selectors(s, a));
        }
        list = list.child(group);
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

const THEME_SEARCH: &str = "theme family Catppuccin Latte Mocha Dracula Alucard Tokyo Night Nord Gruvbox medium One Dark Pro Rodtty adaptation Light Owl Ayu Solarized Light Dark System palette";

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
                _ => "Light / Dark",
            }
            .into(),
            disabled: false,
        })
        .collect();
    rune_kit::Combobox::new(
        "theme-family",
        &s.theme_family_picker,
        &options,
        |s: &mut State| {
            s.font_ui.close();
            s.font_terminal.close();
            &mut s.theme_family_picker
        },
    )
    .label("Theme family")
    .current(Some(s.config.theme_family.label()))
    .search_placeholder("Search nine theme families")
    .width(280.0)
    .max_height(256.0)
    .on_choose(|s, label, _| {
        if let Some(&family) = ThemeFamily::ALL.iter().find(|f| f.label() == label) {
            s.save_config(|c| c.theme_family = family);
        }
    })
    .build(a.kit(), layers)
    .w(280.0)
}

pub(super) fn overlays(s: &State, a: &Appearance, width: f32) -> Vec<El> {
    let mut layers = Vec::new();
    if s.settings
        && !s.application_overlay_open()
        && workspace::pane_visibility(s, width).2
        && matches(s, "Appearance", THEME_SEARCH)
    {
        let _ = family_picker(s, a, &mut layers);
    }
    layers
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

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime(root: &std::path::Path) -> rune::Runtime<State> {
        let mut state = State::with_root(root.to_owned());
        state.workspace_setup();
        let mut runtime = rune::Runtime::new(
            state,
            view,
            (1200.0, 800.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        runtime.frame(0.0);
        runtime
    }
    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        let (x, y) = runtime
            .find(key)
            .unwrap_or_else(|| panic!("missing control {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
    }
    fn assert_canvas(runtime: &rune::Runtime<State>, hex: &str) {
        let c = rune::theme::hex(hex).unwrap();
        let actual = runtime.clear_color();
        assert!(
            actual
                .iter()
                .zip([c.r, c.g, c.b, c.a])
                .all(|(a, b)| (a - b).abs() < 0.0001),
            "canvas {actual:?}, expected {hex}"
        );
    }
    #[test]
    fn file_preferences_reset_individually_without_changing_other_choices() {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.explorer.confirm_deletion = false;
        config.explorer.follow_terminal = true;
        config.explorer.show_hidden = true;
        config.explorer.respect_ignore = false;
        config.save(root.path()).unwrap();
        let mut rt = runtime(root.path());
        click(&mut rt, "settings");
        for (query, control, expected) in [
            (
                "Trash",
                "reset-settings-confirm-deletion",
                [true, true, true, false],
            ),
            (
                "Follow Terminal",
                "reset-settings-explorer-follow",
                [true, false, true, false],
            ),
            (
                "hidden files",
                "reset-settings-explorer-hidden",
                [true, false, false, false],
            ),
            (
                "ignore rules",
                "reset-settings-explorer-ignore",
                [true, false, false, true],
            ),
        ] {
            rt.update(|s, _| s.settings_query.replace_text(query));
            rt.frame(0.0);
            click(&mut rt, control);
            rt.update(|s, tx| {
                s.workspace_poll(tx);
            });
            let saved = Config::load(root.path()).unwrap().explorer;
            assert_eq!(
                [
                    saved.confirm_deletion,
                    saved.follow_terminal,
                    saved.show_hidden,
                    saved.respect_ignore
                ],
                expected,
            );
            assert_eq!(rt.state.workspace.layout.follow, expected[1]);
            assert_eq!(rt.state.workspace.layout.hidden, expected[2]);
        }
    }

    #[test]
    fn behavior_settings_save_and_reset_terminal_close_and_directory_choices() {
        use crate::config::{NewTerminalDirectory, TerminalClosePolicy};
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("terminal tab close"));
        rt.frame(0.0);
        click(&mut rt, "terminal-close-always");
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .terminal_tab_close,
            TerminalClosePolicy::AlwaysConfirm
        );
        click(&mut rt, "terminal-close-immediate");
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .terminal_tab_close,
            TerminalClosePolicy::CloseImmediately
        );
        click(&mut rt, "reset-terminal-close");
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .terminal_tab_close,
            TerminalClosePolicy::ConfirmRunning
        );
        rt.update(|s, _| s.settings_query.replace_text("new terminal directory"));
        rt.frame(0.0);
        click(&mut rt, "new-directory-explorer");
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .new_terminal_directory,
            NewTerminalDirectory::Explorer
        );
        click(&mut rt, "new-directory-specified");
        click(&mut rt, "specified-directory");
        rt.key(KeyEvent {
            key: Key::Text(root.path().display().to_string()),
            mods: Default::default(),
        });
        rt.frame(0.0);
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .specified_directory,
            root.path()
        );
        click(&mut rt, "reset-new-directory");
        assert_eq!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .new_terminal_directory,
            NewTerminalDirectory::Active
        );
        click(&mut rt, "reset-specified-directory");
        assert!(
            Config::load(root.path())
                .unwrap()
                .behavior
                .specified_directory
                .as_os_str()
                .is_empty()
        );
        let mut external = Config::load(root.path()).unwrap();
        external.behavior.new_terminal_directory = NewTerminalDirectory::Specified;
        external.behavior.specified_directory = root.path().into();
        external.behavior.terminal_tab_close = TerminalClosePolicy::AlwaysConfirm;
        external.save(root.path()).unwrap();
        rt.update(|s, _| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            s.poll_settings();
        });
        assert_eq!(
            rt.state.config.behavior.terminal_tab_close,
            TerminalClosePolicy::AlwaysConfirm
        );
    }

    #[test]
    fn behavior_settings_search_apply_live_persist_and_reset_window_policy() {
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("window close"));
        rt.frame(0.0);
        click(&mut rt, "window-close-ask");
        assert_eq!(
            Config::load(root.path()).unwrap().behavior.window_close,
            crate::config::WindowClosePolicy::AskEachTime
        );
        rt.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        rt.frame(0.0);
        rt.settle_all();
        rt.frame(0.0);
        assert!(rt.find("window-close-choice").is_some());
        click(&mut rt, "window-close-cancel");
        click(&mut rt, "reset-window-close");
        assert_eq!(
            Config::load(root.path()).unwrap().behavior.window_close,
            crate::config::WindowClosePolicy::KeepSessionsRunning
        );
        rt.update(|s, tx| s.dispatch(AppAction::CloseWindow, tx));
        assert_eq!(rt.shell_request.take(), Some(rune::Shell::Close));
    }

    #[test]
    fn owner_selects_dracula_family_and_keeps_both_appearances_on_reopen() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        runtime.update(|s, _| s.settings_query.replace_text("theme"));
        runtime.frame(0.0);
        click(&mut runtime, "theme-family-trigger");
        click(&mut runtime, "theme-family-option-Dracula");
        assert_canvas(&runtime, "#282a36");
        click(&mut runtime, "appearance-light");
        assert_canvas(&runtime, "#fffbeb");
        let reopened = self::runtime(root.path());
        assert_canvas(&reopened, "#fffbeb");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["theme_family"], "dracula");
        assert_eq!(saved["appearance"], "light");
    }

    #[test]
    fn all_nine_families_apply_both_previews_live_system_json_and_independent_resets() {
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("theme"));
        rt.frame(0.0);
        // Independently specified upstream canvas values, not derived from the resolver.
        let families = [
            (
                "Catppuccin",
                "catppuccin",
                "#eff1f5",
                "#1e1e2e",
                "Catppuccin Latte",
                "Catppuccin Mocha",
            ),
            (
                "Dracula",
                "dracula",
                "#fffbeb",
                "#282a36",
                "Dracula Alucard",
                "Dracula",
            ),
            (
                "Tokyo Night",
                "tokyo_night",
                "#e6e7ed",
                "#1a1b26",
                "Tokyo Night Light",
                "Tokyo Night Night",
            ),
            (
                "Nord",
                "nord",
                "#eceff4",
                "#2e3440",
                "Nord Light",
                "Nord Dark",
            ),
            (
                "Gruvbox",
                "gruvbox",
                "#fbf1c7",
                "#282828",
                "Gruvbox Light (medium)",
                "Gruvbox Dark (medium)",
            ),
            (
                "One Dark Pro",
                "one_dark_pro",
                "#fafafa",
                "#282c34",
                "One Dark Pro Light (Rodtty adaptation)",
                "One Dark Pro Dark",
            ),
            (
                "Night Owl",
                "night_owl",
                "#fbfbfb",
                "#011627",
                "Light Owl",
                "Night Owl",
            ),
            ("Ayu", "ayu", "#fafafa", "#0a0e14", "Ayu Light", "Ayu Dark"),
            (
                "Solarized",
                "solarized",
                "#fdf6e3",
                "#002b36",
                "Solarized Light",
                "Solarized Dark",
            ),
        ];
        assert_eq!(crate::config::ThemeFamily::ALL.len(), 9);
        for (label, json_name, light, dark, light_name, dark_name) in families {
            click(&mut rt, "theme-family-trigger");
            rt.paste(label);
            for _ in 0..45 {
                rt.frame(1.0 / 60.0);
            }
            click(&mut rt, &format!("theme-family-option-{label}"));
            for name in [light_name, dark_name] {
                assert!(
                    rt.elements()
                        .iter()
                        .any(|e| e.text.as_deref() == Some(name)),
                    "missing preview {name}"
                );
            }
            for (control, value, canvas) in [
                ("appearance-light", "light", light),
                ("appearance-dark", "dark", dark),
            ] {
                click(&mut rt, control);
                assert_canvas(&rt, canvas);
                let saved: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(root.path().join("settings.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(saved["theme_family"], json_name);
                assert_eq!(saved["appearance"], value);
                assert_canvas(&runtime(root.path()), canvas);
            }
            click(&mut rt, "appearance-system");
            for size in [(680.0, 800.0), (1400.0, 1000.0)] {
                rt.set_viewport(size, 1.0);
                for (host_dark, canvas) in [(false, light), (true, dark)] {
                    rt.set_system_dark(host_dark);
                    for _ in 0..45 {
                        rt.frame(1.0 / 60.0);
                    }
                    assert_canvas(&rt, canvas);
                    assert!(rt.find("theme-family-trigger").is_some());
                }
            }
            // External JSON changes enter through the actual file polling boundary.
            let mut external = Config::load(root.path()).unwrap();
            external.appearance = crate::config::Appearance::Light;
            external.save(root.path()).unwrap();
            rt.update(|s, _| {
                s.settings_poll = Instant::now() - Duration::from_secs(1);
                s.poll_settings();
            });
            for _ in 0..45 {
                rt.frame(1.0 / 60.0);
            }
            assert_canvas(&rt, light);
            click(&mut rt, "reset-appearance");
            assert_canvas(&rt, dark);
            assert_eq!(
                Config::load(root.path()).unwrap().theme_family.label(),
                label
            );
            click(&mut rt, "reset-theme-family");
            assert_canvas(&rt, "#1e1e2e");
            assert_eq!(
                Config::load(root.path()).unwrap().appearance,
                crate::config::Appearance::System
            );
        }
    }

    #[test]
    fn every_family_recolors_rendered_ansi_without_rewriting_program_rgb_or_terminal_selection() {
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        rt.update(|s, _| {
            let mut parser = vt100::Parser::new(8, 30, 0);
            parser.process(
                b"\x1b[41m \x1b[101m \x1b[48;5;196m \x1b[48;2;12;34;56m \x1b[0m output stays",
            );
            s.preview = Some(parser.screen().clone());
            s.selection = Some(((1, 0), (1, 5)));
        });
        // Normal / bright red literals from the pinned primary palettes; custom light
        // ANSI pairs are the explicitly documented Rodtty adaptation contracts.
        for (family, light, dark) in [
            ("catppuccin", ["#d20f39", "#de293e"], ["#f38ba8", "#f37799"]),
            ("dracula", ["#cb3a2a", "#cb3a2a"], ["#ff5555", "#ff6e6e"]),
            (
                "tokyo_night",
                ["#8c4351", "#8c4351"],
                ["#f7768e", "#f7768e"],
            ),
            ("nord", ["#994950", "#994950"], ["#bf616a", "#bf616a"]),
            ("gruvbox", ["#cc241d", "#9d0006"], ["#cc241d", "#fb4934"]),
            (
                "one_dark_pro",
                ["#c23f4a", "#c23f4a"],
                ["#e05561", "#ff616e"],
            ),
            ("night_owl", ["#de3d3b", "#de3d3b"], ["#EF5350", "#EF5350"]),
            ("ayu", ["#f07171", "#f07171"], ["#f07178", "#f07178"]),
            ("solarized", ["#dc322f", "#cb4b16"], ["#dc322f", "#cb4b16"]),
        ] {
            for (mode, expected) in [("light", light), ("dark", dark)] {
                let config: Config = serde_json::from_value(
                    serde_json::json!({"theme_family":family,"appearance":mode}),
                )
                .unwrap();
                config.save(root.path()).unwrap();
                rt.update(|s, _| {
                    s.settings_poll = Instant::now() - Duration::from_secs(1);
                    s.poll_settings();
                });
                for _ in 0..45 {
                    rt.frame(1.0 / 60.0);
                }
                let terminal = rt.presented("terminal").unwrap().0;
                let metrics = rt.state.terminal_metrics;
                for (column, expected) in [expected[0], expected[1], "#ff0000", "#0c2238"]
                    .iter()
                    .enumerate()
                {
                    let x = terminal[0] + (column as f32 + 0.5) * metrics.width;
                    let fill = rt
                        .instances
                        .iter()
                        .find(|i| {
                            (i.rect[1] - terminal[1]).abs() < 0.01
                                && i.rect[3] == metrics.height
                                && i.rect[0] <= x
                                && x < i.rect[0] + i.rect[2]
                                && i.rect[2] < 2.0 * metrics.width
                        })
                        .expect("rendered terminal cell")
                        .fill;
                    let c = rune::theme::hex(expected).unwrap();
                    assert_eq!(
                        fill,
                        [c.r, c.g, c.b, c.a],
                        "{family} {mode} column {column}"
                    );
                }
                assert_eq!(rt.state.selection, Some(((1, 0), (1, 5))));
            }
        }
    }

    #[test]
    fn ayu_light_settings_labels_remain_readable_on_the_light_canvas() {
        let root = tempfile::tempdir().unwrap();
        let config: Config =
            serde_json::from_value(serde_json::json!({"theme_family":"ayu","appearance":"light"}))
                .unwrap();
        config.save(root.path()).unwrap();
        let mut rt = runtime(root.path());
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("theme"));
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        let label = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Theme family"))
            .unwrap()
            .rect;
        let glyph = rt
            .instances
            .iter()
            .find(|i| {
                i.params[1] == rune::render::KIND_GLYPH
                    && i.rect[0] >= label[0]
                    && i.rect[0] < label[0] + label[2]
                    && i.rect[1] >= label[1]
                    && i.rect[1] < label[1] + label[3]
            })
            .expect("drawn theme label");
        let luminance = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let fg = luminance(glyph.fill);
        let bg = luminance(rt.clear_color());
        assert!(
            (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05) >= 4.5,
            "theme label contrast: {:?}",
            glyph.fill
        );
    }

    #[test]
    fn editor_and_read_only_diff_draw_the_selected_familys_keyword_palette() {
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        for (family, light, dark) in [
            ("catppuccin", "#8839ef", "#cba6f7"),
            ("dracula", "#a3144d", "#ff79c6"),
            ("tokyo_night", "#65359d", "#bb9af7"),
            ("one_dark_pro", "#884bb3", "#c678dd"),
            ("night_owl", "#994cc3", "#c792ea"),
        ] {
            for (mode, expected) in [("light", light), ("dark", dark)] {
                let config: Config = serde_json::from_value(
                    serde_json::json!({"theme_family":family,"appearance":mode}),
                )
                .unwrap();
                config.save(root.path()).unwrap();
                for diff in [false, true] {
                    rt.update(move |s, _| {
                        s.settings_poll = Instant::now() - Duration::from_secs(1);
                        s.poll_settings();
                        s.workspace.tabs.clear();
                        workspace::demo(s, diff);
                    });
                    for _ in 0..45 {
                        rt.frame(1.0 / 60.0);
                    }
                    let c = rune::theme::hex(expected).unwrap();
                    assert!(
                        rt.instances
                            .iter()
                            .any(|i| i.params[1] == rune::render::KIND_GLYPH
                                && i.fill == [c.r, c.g, c.b, c.a]),
                        "missing {family} {mode} keyword in diff={diff}"
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_settings_theme_dropdown_is_searchable_and_selectable() {
        let root = tempfile::tempdir().unwrap();
        let mut rt = runtime(root.path());
        rt.set_viewport((680.0, 800.0), 1.0);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("theme"));
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        click(&mut rt, "theme-family-trigger");
        assert!(rt.find("theme-family-option-Dracula").is_some());
        rt.paste("solar");
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        click(&mut rt, "theme-family-option-Solarized");
        assert_canvas(&rt, "#002b36");
        assert_eq!(
            Config::load(root.path()).unwrap().theme_family.label(),
            "Solarized"
        );
    }

    #[test]
    fn owner_changes_and_resets_independent_live_font_sizes() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        runtime.update(|s, _| s.settings_query.replace_text("font size"));
        runtime.frame(0.0);
        let before = runtime
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Settings"))
            .unwrap()
            .rect[3];
        click(&mut runtime, "more-ui-font-size");
        let after = runtime
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Settings"))
            .unwrap()
            .rect[3];
        assert!(
            after > before,
            "UI text applies the chosen size immediately"
        );
        click(&mut runtime, "more-terminal-font-size");
        click(&mut runtime, "more-editor-font-size");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["fonts"]["ui_size"], 14.0);
        assert_eq!(saved["fonts"]["terminal_size"], 15.0);
        assert_eq!(saved["fonts"]["editor_size"], 15.0);
        click(&mut runtime, "reset-ui-font-size");
        click(&mut runtime, "reset-terminal-font-size");
        click(&mut runtime, "reset-editor-font-size");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["fonts"]["ui_size"], 13.0);
        assert_eq!(saved["fonts"]["terminal_size"], 14.0);
        assert_eq!(saved["fonts"]["editor_size"], 14.0);
    }
    #[test]
    fn system_tracks_live_host_changes_while_explicit_appearance_stays_fixed() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        assert_canvas(&runtime, "#1e1e2e");
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-dark");
        runtime.set_system_dark(true);
        runtime.frame(0.0);
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#1e1e2e");
        click(&mut runtime, "appearance-light");
        runtime.set_system_dark(true);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "reset-appearance");
        assert_canvas(&runtime, "#1e1e2e");
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        assert_eq!(
            Config::load(root.path()).unwrap().appearance,
            crate::config::Appearance::System
        );
        runtime.update(|s, _| s.settings_query.replace_text("latte"));
        runtime.frame(0.0);
        assert!(runtime.find("appearance-system").is_some());
    }
    #[test]
    fn switching_appearance_recolors_ansi_but_preserves_extended_output_and_selection() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        runtime.update(|s, _| {
            let mut parser = vt100::Parser::new(8, 30, 0);
            parser.process(b"\x1b[41m \x1b[48;5;196m \x1b[48;2;12;34;56m \x1b[0m output stays");
            s.preview = Some(parser.screen().clone());
            s.selection = Some(((1, 0), (1, 5)));
        });
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        let cell_background = |runtime: &rune::Runtime<State>, col: usize| {
            let r = runtime.presented("terminal").unwrap().0;
            let metrics = runtime.state.terminal_metrics;
            let x = r[0] + (col as f32 + 0.5) * metrics.width;
            runtime
                .instances
                .iter()
                .find(|i| {
                    (i.rect[1] - r[1]).abs() < 0.01
                        && i.rect[3] == metrics.height
                        && i.rect[0] <= x
                        && x < i.rect[0] + i.rect[2]
                        && i.rect[2] < 2.0 * metrics.width
                })
                .expect("terminal cell background")
                .fill
        };
        let rgb = |hex: &str| {
            let c = rune::theme::hex(hex).unwrap();
            [c.r, c.g, c.b, c.a]
        };
        assert_eq!(cell_background(&runtime, 0), rgb("#f38ba8"));
        assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
        assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-light");
        assert_eq!(cell_background(&runtime, 0), rgb("#d20f39"));
        assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
        assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
        assert!(
            runtime
                .state
                .with_screen(|screen| screen.contents().contains("output stays"))
        );
        assert_eq!(runtime.state.selection, Some(((1, 0), (1, 5))));
    }
    #[test]
    fn owner_selects_persists_and_resets_autosave_preferences() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        runtime.update(|s, _| s.settings_query.replace_text("autosave"));
        runtime.frame(0.0);
        click(&mut runtime, "autosave-after-delay");
        click(&mut runtime, "less-autosave-delay");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["autosave"]["policy"], "after_delay");
        assert_eq!(saved["autosave"]["delay_ms"], 900);
        click(&mut runtime, "autosave-editor-focus-loss");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["autosave"]["policy"], "editor_focus_loss");
        click(&mut runtime, "reset-autosave");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["autosave"]["policy"], "explicit");
        assert_eq!(saved["autosave"]["delay_ms"], 1000);
    }

    #[test]
    fn owner_changes_appearance_live_and_reopens_with_saved_choice() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = self::runtime(root.path());
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-light");
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "appearance-dark");
        assert_canvas(&runtime, "#1e1e2e");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["appearance"], "dark");
        assert_eq!(saved["theme_family"], "catppuccin");
        let mut reopened = self::runtime(root.path());
        reopened.set_system_dark(false);
        for _ in 0..45 {
            reopened.frame(1.0 / 60.0);
        }
        assert_canvas(&reopened, "#1e1e2e");
    }
    #[test]
    fn owner_searches_resets_and_edits_actual_json_beside_the_terminal() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        assert!(runtime.find("editor-pane").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("settings-tab").is_some());
        runtime.key(KeyEvent {
            key: Key::Text("history".into()),
            mods: Default::default(),
        });
        runtime.frame(0.0);
        assert!(runtime.find("history").is_some());
        assert!(runtime.find("utility-cat").is_none());
        click(&mut runtime, "history");
        assert!(
            !Config::load(root.path()).unwrap().history,
            "{}; active history {}",
            runtime.state.message,
            runtime.state.config.history
        );
        click(&mut runtime, "reset-history");
        assert!(Config::load(root.path()).unwrap().history);
        runtime.update(|s, _| s.settings_query.replace_text("cat"));
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "utility-cat");
        assert_eq!(Config::load(root.path()).unwrap().disabled, ["cat"]);
        click(&mut runtime, "reset-utility-cat");
        assert!(Config::load(root.path()).unwrap().disabled.is_empty());
        runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "more-scrollback");
        assert_eq!(Config::load(root.path()).unwrap().scrollback, 11_000);
        click(&mut runtime, "reset-scrollback");
        assert_eq!(Config::load(root.path()).unwrap().scrollback, 10_000);
        click(&mut runtime, "open-settings-json");
        let start = Instant::now();
        while runtime.state.workspace.tabs.is_empty() && start.elapsed() < Duration::from_secs(5) {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(0.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            runtime.state.workspace.tabs[0].document.path,
            root.path().join("settings.json")
        );
        assert!(runtime.find("file-editor-1").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        runtime.update(|_, tx| tx.focus("file-editor-1", true));
        runtime.frame(1.0 / 60.0);
        runtime.key(KeyEvent {
            key: Key::Command('a'),
            mods: rune::Mods {
                cmd: true,
                ctrl: true,
                ..Default::default()
            },
        });
        runtime.key(KeyEvent {
            key: Key::Text(r#"{"history":false,"scrollback":3000}"#.into()),
            mods: Default::default(),
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "file-save");
        let start = Instant::now();
        while runtime.state.workspace.tabs[0].saving && start.elapsed() < Duration::from_secs(5) {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(1.0 / 60.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        assert!(!Config::load(root.path()).unwrap().history);
        assert!(!runtime.state.config.history);
        assert_eq!(runtime.state.config.scrollback, 3000);
    }
    #[test]
    fn external_json_updates_preferences_but_invalid_edits_keep_the_last_valid_choices() {
        let root = tempfile::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        let path = root.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"appearance":"light","history":false,"scrollback":2000,"future":{"choice":7}}"#,
        )
        .unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        assert!(!runtime.state.config.history);
        assert_eq!(runtime.state.config.scrollback, 2000);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        std::fs::write(&path, "{invalid JSON").unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        assert!(!runtime.state.config.history);
        assert!(
            runtime
                .state
                .message
                .contains("keeping last valid preferences")
        );
        assert_canvas(&runtime, "#eff1f5");
        // A UI action must not destroy the invalid JSON being repaired externally.
        runtime.update(|s, _| {
            s.settings_query.replace_text("history");
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "reset-history");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{invalid JSON");
        assert!(!runtime.state.config.history);
        std::fs::write(&path, r#"{"history":false,"future":{"choice":7}}"#).unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "reset-history");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["history"], true);
        assert_eq!(saved["future"]["choice"], 7);
        assert!(runtime.state.settings_error.is_none());
    }
    #[test]
    fn a_ui_edit_preserves_unrelated_external_changes_before_the_next_poll() {
        let root = tempfile::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
        runtime.frame(1.0 / 60.0);
        std::fs::write(
            root.path().join("settings.json"),
            r#"{"history":false,"scrollback":10000}"#,
        )
        .unwrap();
        click(&mut runtime, "more-scrollback");
        let saved = Config::load(root.path()).unwrap();
        assert!(!saved.history);
        assert_eq!(saved.scrollback, 11000);
    }
}
