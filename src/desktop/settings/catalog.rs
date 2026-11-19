use super::*;

pub(super) const FIELDSETS: &[Fieldset] = &[
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

pub(super) const THEME_SEARCH: &str = "theme family Catppuccin Latte Mocha Dracula Alucard Tokyo Night Nord Gruvbox medium One Dark Pro Rodtty adaptation Light Owl Ayu Solarized Rosé Pine Dawn Moon Kanagawa Lotus Wave Everforest Light Dark System palette";
