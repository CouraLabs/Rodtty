use super::*;

pub(super) fn window_close(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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

pub(super) fn terminal_startup(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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

pub(super) fn new_terminal_directory(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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
pub(super) fn specified_directory(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    if s.config.behavior.new_terminal_directory != crate::config::NewTerminalDirectory::Specified {
        return el();
    }
    folder_field(
        a,
        &s.behavior_directory,
        "specified-directory",
        "Specified terminal folder",
        |s| &mut s.behavior_directory,
        |c, path| c.behavior.specified_directory = path,
    )
}

pub(super) fn startup_directory(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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
pub(super) fn startup_specified_directory(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    if s.config.behavior.startup_directory != crate::config::StartupDirectory::Specified {
        return el();
    }
    folder_field(
        a,
        &s.startup_directory,
        "startup-specified-directory",
        "Specified startup folder",
        |s| &mut s.startup_directory,
        |c, path| c.behavior.startup_specified_directory = path,
    )
}

pub(super) fn terminal_tab_close(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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

pub(super) fn right_click(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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

pub(super) fn copy_on_selection(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
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

pub(super) fn history(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "history",
        "Save command history",
        s.config.history,
        |s, _| s.save_config(|c| c.history = !c.history),
    )
}

pub(super) fn scrollback(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
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

pub(super) fn autosave(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
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

pub(super) fn autosave_delay(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
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

pub(super) fn single_click(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "single-click-keeps-open",
        "Single click keeps files open",
        s.config.single_click_keeps_open,
        |s, _| s.save_config(|c| c.single_click_keeps_open = !c.single_click_keeps_open),
    )
}

/// A file explorer preference's switch; its action keeps the explorer in step.
pub(super) fn file_switch(s: &State, a: &Appearance, action: AppAction) -> El {
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
