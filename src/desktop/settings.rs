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
        );
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
            matches(s, section, PROMPT_STYLE_SEARCH)
                || matches(s, section, PROMPT_SYMBOL_SEARCH)
                || matches(s, section, "history save shell commands")
                || matches(s, section, "scrollback retained lines")
                || matches(
                    s,
                    section,
                    "clipboard right click context menu paste copy on selection",
                )
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
            .role("group")
            .label(section)
            .w_full()
            .gap(8.0)
            .px(14.0)
            .py(12.0)
            .radius(8.0)
            .fill(a.panel())
            .border(1.0, a.kit().c.border_base)
            .child(
                text(section)
                    .role("heading")
                    .level(2)
                    .font(ui_style(13.0, 600.0))
                    .color(a.accent()),
            );
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
            let subject = format!("{} font size", if target == "ui" { "UI" } else { target });
            for (action, title, delta, name) in [
                ("less", "−", -1.0, "Decrease"),
                ("more", "+", 1.0, "Increase"),
                ("reset", "Reset", 0.0, "Reset"),
            ] {
                let name = format!("{name} {subject}");
                buttons = buttons.child(
                    button(
                        a,
                        format!("{action}-{target}-font-size"),
                        title,
                        move |s, _| {
                            s.save_config(|c| {
                                let defaults = config::FontPreferences::default();
                                let (size, default) = match target {
                                    "ui" => (&mut c.fonts.ui_size, defaults.ui_size),
                                    "terminal" => {
                                        (&mut c.fonts.terminal_size, defaults.terminal_size)
                                    }
                                    _ => (&mut c.fonts.editor_size, defaults.editor_size),
                                };
                                *size = if delta == 0.0 {
                                    default
                                } else {
                                    (*size + delta).clamp(8.0, 32.0)
                                };
                            });
                        },
                    )
                    .label(name.clone())
                    .help(name),
                );
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
                let mut choices = row()
                    .role("radiogroup")
                    .label("Autosave")
                    .gap(6.0)
                    .scroll_x();
                for (id, title, policy) in [
                    ("explicit", "Explicit Save", AutosavePolicy::Explicit),
                    ("after-delay", "After Delay", AutosavePolicy::AfterDelay),
                    (
                        "editor-focus-loss",
                        "Editor Focus Loss",
                        AutosavePolicy::EditorFocusLoss,
                    ),
                ] {
                    let on = s.config.autosave.policy == policy;
                    choices = choices.child(state_button(
                        a,
                        format!("autosave-{id}"),
                        format!("{}{title}", if on { "✓ " } else { "" }),
                        "radio",
                        title,
                        on,
                        move |s, _| {
                            s.save_config(|c| c.autosave.policy = policy);
                        },
                    ));
                }
                group = group
                    .child(text("Autosave").font(ui_label()).color(a.ink()))
                    .child(choices)
                    .child(button(
                        a,
                        "reset-autosave-policy",
                        "Reset autosave policy",
                        |s, _| {
                            s.save_config(|c| c.autosave.policy = AutosavePolicy::default());
                        },
                    ))
                    .child(
                        text(format!("Autosave delay: {} ms", s.config.autosave.delay_ms))
                            .font(ui_label())
                            .color(a.ink()),
                    )
                    .child(
                        row()
                            .gap(6.0)
                            .child(
                                button(a, "less-autosave-delay", "− 100 ms", |s, _| {
                                    s.save_config(|c| {
                                        c.autosave.delay_ms =
                                            c.autosave.delay_ms.saturating_sub(100).max(100)
                                    });
                                })
                                .label("Decrease autosave delay by 100 ms")
                                .help("Decrease autosave delay by 100 ms"),
                            )
                            .child(
                                button(a, "more-autosave-delay", "+ 100 ms", |s, _| {
                                    s.save_config(|c| {
                                        c.autosave.delay_ms =
                                            (c.autosave.delay_ms + 100).min(60_000)
                                    });
                                })
                                .label("Increase autosave delay by 100 ms")
                                .help("Increase autosave delay by 100 ms"),
                            )
                            .child(
                                button(a, "reset-autosave-delay", "Reset delay", |s, _| {
                                    s.save_config(|c| {
                                        c.autosave.delay_ms =
                                            config::AutosavePreferences::default().delay_ms
                                    });
                                })
                                .label("Reset autosave delay")
                                .help("Reset autosave delay"),
                            )
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
                    let mut choices = radio_group("Terminal startup", 8.0);
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
                        choices = choices.child(
                            radio_button(
                                a,
                                key,
                                title,
                                s.config.behavior.terminal_startup == choice,
                                move |s, _| {
                                    s.save_config(|c| c.behavior.terminal_startup = choice);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(choices).child(button(a, "reset-terminal-startup", "Reset terminal startup", |s, _| {
                        s.save_config(|c| c.behavior.terminal_startup = TerminalStartup::default());
                    })).child(text("Applies after Quit or restart. Reopening a running host keeps its sessions. One fresh terminal uses the new-terminal directory choice; first launch keeps the startup directory.").font(ui_label()).color(a.muted()).wrap());
                }
                if matches(
                    s,
                    section,
                    "single click keeps open file preview editor tabs",
                ) {
                    group = group
                    .child(switch_button(
                        a,
                        "single-click-keeps-open",
                        "Single click keeps files open",
                        s.config.single_click_keeps_open,
                        |s, _| {
                            s.save_config(|c| c.single_click_keeps_open = !c.single_click_keeps_open);
                        },
                    ))
                    .child(button(a, "reset-single-click-keeps-open", "Reset", |s, _| {
                        s.save_config(|c| {
                            c.single_click_keeps_open = Config::default().single_click_keeps_open;
                        });
                    }).label("Reset single click keeps files open").help("Reset single click keeps files open"))
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
                    let mut choices = radio_group("Window close", 8.0);
                    for (key, label, policy) in [
                        (
                            "window-close-keep",
                            "Keep sessions running",
                            Window::KeepSessionsRunning,
                        ),
                        ("window-close-ask", "Ask each time", Window::AskEachTime),
                        ("window-close-quit", "Quit Rodtty", Window::QuitRodtty),
                    ] {
                        choices = choices.child(
                            radio_button(
                                a,
                                key,
                                label,
                                s.config.behavior.window_close == policy,
                                move |s, _| {
                                    s.save_config(|c| c.behavior.window_close = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(choices).child(button(
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
                    let mut choices = radio_group("Terminal tab close", 8.0);
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
                        choices = choices.child(
                            radio_button(
                                a,
                                key,
                                label,
                                s.config.behavior.terminal_tab_close == policy,
                                move |s, _| {
                                    s.save_config(|c| c.behavior.terminal_tab_close = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(choices).child(button(
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
                    let mut choices = radio_group("New terminal directory", 8.0);
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
                        choices = choices.child(
                            radio_button(
                                a,
                                key,
                                label,
                                s.config.behavior.new_terminal_directory == policy,
                                move |s, _| {
                                    s.save_config(|c| c.behavior.new_terminal_directory = policy);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group
                        .child(choices)
                        .child(button(a, "reset-new-directory", "Reset new terminal directory", |s, _| {
                            s.save_config(|c| c.behavior.new_terminal_directory = Directory::default());
                        }))
                        .child(text_input(&s.behavior_directory, ui_label())
                            .caret(a.accent(), a.selection()).key("specified-directory")
                            .label("Specified terminal folder").single_line().w_full().px(8.0).py(5.0).color(a.ink())
                            .placeholder("Absolute folder path", a.muted())
                            .border(1.0, a.kit().c.border_base)
                            .radius(6.0)
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
                let mut choices = row()
                    .role("radiogroup")
                    .label("Appearance")
                    .gap(6.0)
                    .scroll_x();
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
                if matches(s, section, PROMPT_STYLE_SEARCH) {
                    group = group.child(prompt_style_row(s, a));
                }
                if matches(s, section, PROMPT_SYMBOL_SEARCH) {
                    group = group.child(prompt_symbol_row(s, a));
                }
                if matches(
                    s,
                    section,
                    "clipboard right click context menu paste copy on selection",
                ) {
                    use crate::config::TerminalRightClick;
                    group =
                        group.child(text("Terminal clipboard").font(ui_label()).color(a.muted()));
                    let mut choices = radio_group("Terminal right-click", 8.0);
                    for (key, label, value) in [
                        (
                            "terminal-right-click-menu",
                            "Right-click: context menu",
                            TerminalRightClick::ContextMenu,
                        ),
                        (
                            "terminal-right-click-paste",
                            "Right-click: paste",
                            TerminalRightClick::Paste,
                        ),
                    ] {
                        choices = choices.child(
                            radio_button(
                                a,
                                key,
                                label,
                                s.config.terminal_clipboard.right_click == value,
                                move |s, _| {
                                    s.save_config(|c| c.terminal_clipboard.right_click = value);
                                },
                            )
                            .w_full(),
                        );
                    }
                    group = group.child(choices).child(button(
                        a,
                        "reset-terminal-right-click",
                        "Reset right-click",
                        |s, _| {
                            s.save_config(|c| {
                                c.terminal_clipboard.right_click = Default::default()
                            });
                        },
                    ));
                    group = group
                        .child(
                            switch_button(
                                a,
                                "copy-on-selection",
                                "Copy on selection",
                                s.config.terminal_clipboard.copy_on_selection,
                                |s, _| {
                                    s.save_config(|c| {
                                        c.terminal_clipboard.copy_on_selection =
                                            !c.terminal_clipboard.copy_on_selection
                                    });
                                },
                            )
                            .w_full(),
                        )
                        .child(button(
                            a,
                            "reset-copy-on-selection",
                            "Reset copy on selection",
                            |s, _| {
                                s.save_config(|c| {
                                    c.terminal_clipboard.copy_on_selection =
                                        config::TerminalClipboardPreferences::default()
                                            .copy_on_selection
                                });
                            },
                        ));
                }
                if matches(s, section, "history save shell commands") {
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(switch_button(
                                a,
                                "history",
                                "History",
                                s.config.history,
                                |s, _| {
                                    s.save_config(|config| config.history = !config.history);
                                },
                            ))
                            .child(
                                button(a, "reset-history", "Reset", |s, _| {
                                    s.save_config(|config| {
                                        config.history = Config::default().history
                                    });
                                })
                                .label("Reset history")
                                .help("Reset history"),
                            ),
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
                            .child(
                                button(a, "less-scrollback", "− 1000", |s, _| {
                                    s.save_config(|config| {
                                        config.scrollback =
                                            config.scrollback.saturating_sub(1000).max(100)
                                    });
                                })
                                .label("Decrease scrollback by 1000 lines")
                                .help("Decrease scrollback by 1000 lines"),
                            )
                            .child(
                                button(a, "more-scrollback", "+ 1000", |s, _| {
                                    s.save_config(|config| {
                                        config.scrollback = (config.scrollback + 1000).min(100_000)
                                    });
                                })
                                .label("Increase scrollback by 1000 lines")
                                .help("Increase scrollback by 1000 lines"),
                            )
                            .child(
                                button(a, "reset-scrollback", "Reset", |s, _| {
                                    s.save_config(|config| {
                                        config.scrollback = Config::default().scrollback
                                    });
                                })
                                .label("Reset scrollback")
                                .help("Reset scrollback"),
                            ),
                    );
                }
                group = group.child(
                    text(
                        "Prompt choices apply to running Rodtty shells at their next prompt, unless the shell sets its own PS1. Command, history, and scrollback choices apply to new terminal sessions.",
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
                                .child(switch_button(a, key, title, enabled, move |s, tx| {
                                    s.dispatch(action, tx)
                                }))
                                .child(
                                    button(a, format!("reset-{key}"), "Reset", move |s, _| {
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
                                                    config.explorer.show_hidden =
                                                        defaults.show_hidden
                                                }
                                                AppAction::ToggleIgnore => {
                                                    config.explorer.respect_ignore =
                                                        defaults.respect_ignore
                                                }
                                                _ => unreachable!(),
                                            }
                                        });
                                    })
                                    .label(format!("Reset {title}"))
                                    .help(format!("Reset {title}")),
                                ),
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
                    let name_reset = name.clone();
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(state_button(
                                a,
                                format!("utility-{name}"),
                                format!("{} {name}", if enabled { "✓" } else { "−" }),
                                "checkbox",
                                name.clone(),
                                enabled,
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
                            .child(
                                button(
                                    a,
                                    format!("reset-utility-{reset}"),
                                    "Reset",
                                    move |s, _| {
                                        s.save_config(|config| {
                                            config.disabled.retain(|n| n != &reset)
                                        });
                                    },
                                )
                                .label(format!("Reset {name_reset}"))
                                .help(format!("Reset {name_reset}")),
                            ),
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

const THEME_SEARCH: &str = "theme family Catppuccin Latte Mocha Dracula Alucard Tokyo Night Nord Gruvbox medium One Dark Pro Rodtty adaptation Light Owl Ayu Solarized Rosé Pine Dawn Moon Kanagawa Lotus Wave Everforest Light Dark System palette";

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
            &mut s.theme_family_picker
        },
    )
    .label("Theme family")
    .current(Some(s.config.theme_family.label()))
    .search_placeholder("Search theme families")
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

const PROMPT_STYLE_SEARCH: &str = "prompt style classic directory rodtty powerline two-line ps1";
const PROMPT_SYMBOL_SEARCH: &str = "prompt symbol dollar chevron triangle ps1";

fn prompt_style_row(s: &State, a: &Appearance) -> El {
    use crate::prompt::PromptStyle;
    let current = s.config.prompt.style;
    let mut choices = row()
        .role("radiogroup")
        .label("Prompt style")
        .w_full()
        .gap(6.0)
        // Wrap rather than clip the last choice when Settings is narrow.
        .flex_wrap();
    for &style in PromptStyle::ALL {
        choices = choices.child(radio_button(
            a,
            format!("prompt-style-{}", style.key()),
            style.label(),
            current == style,
            move |s, _| s.save_config(|c| c.prompt.style = style),
        ));
    }
    let mut preview = col()
        .key("prompt-style-preview")
        .gap(4.0)
        .child(prompt_preview(s.config.prompt, false, a));
    if current.reports_status() {
        preview = preview.child(
            text("After a failed command:")
                .font(ui_label())
                .color(a.muted()),
        );
        preview = preview.child(prompt_preview(s.config.prompt, true, a));
    }
    col()
        .key("prompt-style")
        .w_full()
        .gap(6.0)
        .child(text("Prompt style").font(ui_label()).color(a.muted()))
        .child(choices)
        .child(preview)
        .child(button(
            a,
            "reset-prompt-style",
            "Reset prompt style",
            |s, _| {
                s.save_config(|c| c.prompt.style = Default::default());
            },
        ))
}

fn prompt_symbol_row(s: &State, a: &Appearance) -> El {
    use crate::prompt::PromptSymbol;
    let prefs = s.config.prompt;
    let mut choices = row()
        .role("radiogroup")
        .label("Prompt symbol")
        .w_full()
        .gap(6.0)
        // Wrap rather than clip the last choice when Settings is narrow.
        .flex_wrap();
    for &symbol in PromptSymbol::ALL {
        let on = prefs.symbol == symbol;
        choices = choices.child(state_button(
            a,
            format!("prompt-symbol-{}", symbol.key()),
            format!("{} {}", if on { "●" } else { "○" }, symbol.glyph()),
            "radio",
            symbol_name(symbol),
            on,
            move |s, _| s.save_config(|c| c.prompt.symbol = symbol),
        ));
    }
    // The line holding the symbol, as drawn after success and, when the
    // style reports it, after failure.
    let mut preview = row().key("prompt-symbol-preview").gap(12.0).scroll_x();
    let last_line = |failed| {
        let lines = prefs.preview(failed, PREVIEW_DIRECTORY);
        prompt_line(lines.last().map(Vec::as_slice).unwrap_or_default(), a)
    };
    preview = preview.child(last_line(false));
    if prefs.style.reports_status() {
        preview = preview.child(last_line(true));
    }
    col()
        .key("prompt-symbol")
        .w_full()
        .gap(6.0)
        .child(text("Prompt symbol").font(ui_label()).color(a.muted()))
        .child(choices)
        .child(preview)
        .child(button(
            a,
            "reset-prompt-symbol",
            "Reset prompt symbol",
            |s, _| {
                s.save_config(|c| c.prompt.symbol = Default::default());
            },
        ))
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
    fn open_settings(runtime: &mut rune::Runtime<State>) {
        runtime.update(|s, tx| s.dispatch(AppAction::Settings, tx));
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
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
    fn prompt_choices_wrap_inside_a_narrow_settings_pane_instead_of_clipping() {
        use crate::desktop::fonts::BUNDLED;
        let _families = rune::text::with_families("Geist", "Geist Mono");
        // The default 1080 pt workspace gives Settings about 450 pt, where the
        // five prompt styles used to run past the section and clip "Two-line".
        for (size, share) in [((1080.0, 720.0), 0.42), ((1200.0, 800.0), 0.24)] {
            let root = crate::test_support::tempdir().unwrap();
            let mut state = State::with_root(root.path().into());
            state.workspace.layout.editor_share = share;
            state.open_settings(&mut Tx::default());
            state.settings_query.replace_text("prompt");
            let mut rt = rune::Runtime::with_text(
                state,
                view,
                size,
                rune::Env {
                    system_dark: true,
                    scale: 1.0,
                },
                rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
            );
            for _ in 0..3 {
                rt.frame(0.0);
                rt.settle_all();
            }
            let elements = rt.elements();
            let rect = |key: &str| {
                elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                    .unwrap_or_else(|| panic!("missing {key}"))
            };
            let section = rect("settings-section-Terminal").rect;
            let right = |r: [f32; 4]| r[0] + r[2];
            let keys = crate::prompt::PromptStyle::ALL
                .iter()
                .map(|style| format!("prompt-style-{}", style.key()))
                .chain(
                    crate::prompt::PromptSymbol::ALL
                        .iter()
                        .map(|symbol| format!("prompt-symbol-{}", symbol.key())),
                );
            for key in keys {
                let e = rect(&key);
                assert!(
                    right(e.rect) <= right(section) + 0.5,
                    "{size:?} {key} stays inside its section: {:?} in {section:?}",
                    e.rect
                );
                assert!(
                    (e.visible[2] - e.rect[2]).abs() < 0.5,
                    "{size:?} {key} is not clipped: {:?}",
                    e.visible
                );
            }
        }
    }
    #[test]
    fn file_preferences_reset_individually_without_changing_other_choices() {
        let root = crate::test_support::tempdir().unwrap();
        let mut config = Config::default();
        config.explorer.confirm_deletion = false;
        config.explorer.follow_terminal = true;
        config.explorer.show_hidden = true;
        config.explorer.respect_ignore = false;
        config.save(root.path()).unwrap();
        let mut rt = runtime(root.path());
        open_settings(&mut rt);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path());
        open_settings(&mut rt);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path());
        open_settings(&mut rt);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
    fn all_families_apply_both_previews_live_system_json_and_independent_resets() {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path());
        open_settings(&mut rt);
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
            (
                "Rosé Pine",
                "rose_pine",
                "#faf4ed",
                "#191724",
                "Rosé Pine Dawn",
                "Rosé Pine",
            ),
            (
                "Kanagawa",
                "kanagawa",
                "#f2ecbc",
                "#1f1f28",
                "Kanagawa Lotus",
                "Kanagawa",
            ),
            (
                "Everforest",
                "everforest",
                "#fdf0d5",
                "#2f383e",
                "Everforest Light",
                "Everforest",
            ),
        ];
        assert_eq!(crate::config::ThemeFamily::ALL.len(), 12);
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
            for size in [(640.0, 360.0), (1440.0, 900.0)] {
                rt.set_viewport(size, 1.0);
                for (host_dark, canvas) in [(false, light), (true, dark), (false, light)] {
                    rt.set_system_dark(host_dark);
                    for _ in 0..45 {
                        rt.frame(1.0 / 60.0);
                    }
                    assert_canvas(&rt, canvas);
                    assert!(rt.find("theme-family-trigger").is_some());
                }
            }
            rt.set_system_dark(true);
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
        let root = crate::test_support::tempdir().unwrap();
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
        let root = crate::test_support::tempdir().unwrap();
        let config: Config =
            serde_json::from_value(serde_json::json!({"theme_family":"ayu","appearance":"light"}))
                .unwrap();
        config.save(root.path()).unwrap();
        let mut rt = runtime(root.path());
        open_settings(&mut rt);
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
        let root = crate::test_support::tempdir().unwrap();
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
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path());
        rt.set_viewport((680.0, 800.0), 1.0);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
        open_settings(&mut rt);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        assert_canvas(&runtime, "#1e1e2e");
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        open_settings(&mut runtime);
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
        let root = crate::test_support::tempdir().unwrap();
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
        open_settings(&mut runtime);
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
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
        click(&mut runtime, "reset-autosave-delay");
        let saved = Config::load(root.path()).unwrap();
        assert_eq!(
            saved.autosave.policy,
            crate::config::AutosavePolicy::EditorFocusLoss
        );
        assert_eq!(saved.autosave.delay_ms, 1000);
        click(&mut runtime, "less-autosave-delay");
        click(&mut runtime, "reset-autosave-policy");
        let saved = Config::load(root.path()).unwrap();
        assert_eq!(
            saved.autosave.policy,
            crate::config::AutosavePolicy::Explicit
        );
        assert_eq!(saved.autosave.delay_ms, 900);
        click(&mut runtime, "reset-autosave");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["autosave"]["policy"], "explicit");
        assert_eq!(saved["autosave"]["delay_ms"], 1000);
    }

    #[test]
    fn owner_changes_appearance_live_and_reopens_with_saved_choice() {
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = self::runtime(root.path());
        open_settings(&mut runtime);
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
    fn prompt_style_and_symbol_preview_reset_persist_and_reach_a_running_shell() {
        use crate::prompt::{PromptStyle, PromptSymbol};
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.directory = root.path().into();
        state.shell = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|p| p.join("rodtty-shell"))
            .find(|p| p.is_file());
        state.workspace_setup();
        if state.sessions.is_empty() {
            state.new_tab();
        }
        // Bash's `\w` shows a directory under $HOME as `~/…`; the README gate
        // keeps TMPDIR under target/, which is inside $HOME for most checkouts.
        let dir = match std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .and_then(|home| root.path().strip_prefix(home).ok().map(|p| p.to_owned()))
        {
            Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
            _ => root.path().display().to_string(),
        };
        fn terminal(rt: &rune::Runtime<State>) -> &Session {
            rt.state.active().unwrap()
        }
        let wait_prompt = |rt: &mut rune::Runtime<State>, expected: &str| {
            let start = Instant::now();
            loop {
                // The settings pane narrows the terminal, so prompts may wrap.
                let row = terminal(rt).with_screen(|s| s.contents().trim_end().to_owned());
                if row.ends_with(expected.trim_end()) {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "screen {row:?}, expected prompt {expected:?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let mut rt = rune::Runtime::new(
            state,
            view,
            (1200.0, 800.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        wait_prompt(&mut rt, &format!("rodtty:{dir}$ "));
        open_settings(&mut rt);
        rt.update(|s, _| s.settings_query.replace_text("prompt"));
        rt.frame(1.0 / 60.0);
        assert!(
            rt.find("history").is_none(),
            "search narrows to prompt rows"
        );
        let labelled = |rt: &rune::Runtime<State>, label: &str| {
            rt.elements()
                .iter()
                .any(|e| e.label.as_deref() == Some(label))
        };
        assert!(rt.find("prompt-style-preview").is_some());
        assert!(rt.find("prompt-symbol-preview").is_some());
        assert!(
            labelled(&rt, "rodtty:~/project$ "),
            "default Classic $ preview"
        );

        click(&mut rt, "prompt-style-two_line");
        click(&mut rt, "prompt-symbol-chevron");
        let saved = Config::load(root.path()).unwrap().prompt;
        assert_eq!(saved.style, PromptStyle::TwoLine);
        assert_eq!(saved.symbol, PromptSymbol::Chevron);
        assert!(labelled(&rt, "~/project\n› "), "live style preview");
        assert!(labelled(&rt, "› "), "live symbol preview");
        terminal(&rt).send(b"\r".to_vec()).unwrap();
        wait_prompt(&mut rt, "› ");

        click(&mut rt, "reset-prompt-style");
        let saved = Config::load(root.path()).unwrap().prompt;
        assert_eq!(saved.style, PromptStyle::Classic);
        assert_eq!(
            saved.symbol,
            PromptSymbol::Chevron,
            "resets are independent"
        );
        assert!(labelled(&rt, "rodtty:~/project› "));
        terminal(&rt).send(b"\r".to_vec()).unwrap();
        wait_prompt(&mut rt, &format!("rodtty:{dir}› "));

        // External JSON edits apply to running shells as well.
        let mut external = Config::load(root.path()).unwrap();
        external.prompt.style = PromptStyle::Rodtty;
        external.save(root.path()).unwrap();
        rt.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        rt.frame(1.0 / 60.0);
        assert!(labelled(&rt, "Rodtty › "));
        terminal(&rt).send(b"\r".to_vec()).unwrap();
        wait_prompt(&mut rt, "Rodtty › ");

        click(&mut rt, "reset-prompt-symbol");
        click(&mut rt, "reset-prompt-style");
        assert_eq!(
            Config::load(root.path()).unwrap().prompt,
            Default::default()
        );
        terminal(&rt).send(b"\r".to_vec()).unwrap();
        wait_prompt(&mut rt, &format!("rodtty:{dir}$ "));
    }
    #[test]
    fn owner_searches_resets_and_edits_actual_json_beside_the_terminal() {
        let root = crate::test_support::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
        assert!(
            runtime.state.workspace.tabs[0]
                .document
                .path
                .ends_with("settings.json")
        );
        assert!(runtime.find("file-editor-1").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        runtime.update(|_, tx| tx.focus("file-editor-1", true));
        runtime.frame(1.0 / 60.0);
        runtime.key(KeyEvent {
            key: Key::Command('a'),
            mods: rune::Mods {
                cmd: true,
                ctrl: !cfg!(target_os = "macos"),
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
        let root = crate::test_support::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
        let root = crate::test_support::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        open_settings(&mut runtime);
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
