use crate::desktop::*;

pub(crate) fn header_height() -> f32 {
    44.0_f32.max(ui_label().size * 1.5 + 18.0)
}

/// A titlebar tab's height, in points. Shorter than the band so the pills keep
/// a margin above and below at any UI zoom.
pub(crate) const HEADER_TAB_HEIGHT: f32 = 26.0;

pub(crate) fn view(state: &State, cx: &mut Cx<State>) -> El {
    view_with_frame(state, cx, window::frame::placement())
}

pub(crate) fn view_with_frame(
    state: &State,
    cx: &mut Cx<State>,
    placement: window::frame::Placement,
) -> El {
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
    let (show_explorer, show_terminal, show_editor) = pane_visibility(state, width);
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
        .padding(if window::frame::borderless() {
            window::frame::BORDER
        } else {
            0.0
        })
        .border(
            if window::frame::borderless() {
                1.0
            } else {
                0.0
            },
            a.hover_bg(),
        )
        .child(
            row()
                .key("workspace-header")
                .window_drag()
                .instant_changes()
                .w_full()
                .h(header_height())
                // A hosted titlebar draws the window's own controls over this
                // band: keep the header's content clear of them.
                .pl(if placement == window::frame::Placement::Hosted {
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
                .child(window::frame::controls(state, a, placement)),
        );
    root = root.child(body(state, a, width));

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
    if let Some(dialog) = file_dialog(state, a) {
        root = root.child(dialog);
    }
    if state.workspace_menu {
        root = root.child(workspace_menu(state, a));
    }
    if let Some(menu) = explorer_menu(state, a) {
        root = root.child(menu);
    }
    if let Some(dialog) = removal::view(state, a) {
        root = root.child(dialog);
    }
    if let Some(dialog) = file_operations::view(state, a) {
        root = root.child(dialog);
    }
    if let Some(palette) = window::palette::view(state, a) {
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
