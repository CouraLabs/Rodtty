use super::*;

/// Actual visible panes, including a temporarily revealed compact explorer.
pub(crate) fn pane_visibility(s: &State, width: f32) -> (bool, bool, bool) {
    let editor = editor_available(s) && (width >= 880.0 || !s.workspace.terminal_focus());
    let terminal = if width < 880.0 {
        !editor
    } else {
        s.workspace.layout.terminal || !editor
    };
    let explorer = s.workspace.layout.explorer
        && (width >= explorer_minimum_width(s) || s.workspace.explorer_revealed);
    (explorer, terminal, editor)
}

pub(crate) fn body(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
    let (explorer_shown, terminal, show_editor) = pane_visibility(s, width);
    let explorer_visible = explorer_shown && width >= explorer_minimum_width(s);
    let mut body = row()
        .key("workspace")
        .instant_changes()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_layout(|s: &mut State, r| s.workspace.body_width = r[2]);
    if explorer_visible {
        body = body
            .child(explorer(s, a, workspace.layout.explorer_width))
            .child(
                el().key("explorer-divider")
                    .w(5.0)
                    .h_full()
                    .center()
                    .child(el().w(1.0).h_full().fill(a.hover_bg()))
                    .hover(a.accent().alpha(0.3))
                    .on_drag(|s: &mut State, d, _| {
                        if d.phase == Phase::Down {
                            s.workspace.drag_width = s.workspace.layout.explorer_width;
                        }
                        s.workspace.layout.explorer_width = (s.workspace.drag_width + d.dx)
                            .clamp(160.0, (s.workspace.body_width * 0.35).max(160.0));
                    }),
            );
    }
    #[cfg(feature = "browser")]
    let (terminal, show_editor) = if s.browser_view_visible() {
        // Wide windows keep the terminal beside the Browser; narrow ones
        // focus the Browser, whose close returns to the terminal.
        (width >= 880.0 && terminal, true)
    } else {
        (terminal, show_editor)
    };
    let mut main = row().grow(1.0).min_w(0.0).h_full().min_h(0.0);
    if terminal {
        let mut pane = col().key("terminal-pane").h_full().min_h(0.0).min_w(0.0);
        if s.terminal_search.open {
            pane = pane.child(terminal::search::bar(s, a));
        }
        if s.log_filter.open && s.active().map(Session::id) == Some(s.log_filter.session()) {
            pane = pane.child(terminal::filter::view(s, a));
        } else {
            pane = pane.child(crate::desktop::split::panes(s, a, width));
        }
        main = main.child(pane.grow(if show_editor {
            1.0 - workspace.layout.editor_share
        } else {
            1.0
        }));
    }
    if show_editor {
        if terminal {
            main = main.child(
                el().key("editor-divider")
                    .w(5.0)
                    .h_full()
                    .center()
                    .child(el().w(1.0).h_full().fill(a.hover_bg()))
                    .hover(a.accent().alpha(0.3))
                    .on_drag(|s: &mut State, d, _| {
                        if d.phase == Phase::Down {
                            s.workspace.drag_share = s.workspace.layout.editor_share;
                        }
                        let width = s.workspace.body_width
                            - if s.workspace.layout.explorer && s.workspace.body_width >= 1120.0 {
                                s.workspace.layout.explorer_width + 5.0
                            } else {
                                0.0
                            };
                        s.workspace.layout.editor_share =
                            (s.workspace.drag_share - d.dx / width.max(1.0)).clamp(0.2, 0.8);
                    }),
            );
        }
        #[cfg(feature = "browser")]
        let content = if s.browser_view_visible() {
            crate::desktop::browser::browser_pane(s, a)
        } else {
            editor(s, a, width)
        };
        #[cfg(not(feature = "browser"))]
        let content = editor(s, a, width);
        main = main.child(content.grow(if terminal {
            workspace.layout.editor_share
        } else {
            1.0
        }));
    }
    body = body.child(main);
    if explorer_shown && !explorer_visible {
        // Compact reveal: Files floats over the right of the workspace, above the
        // terminal and editor, so their layout (and the PTY's rows and columns)
        // stays exactly as it was while the overlay comes and goes.
        // The overlay slides in and out on its own transition, while what it
        // shows lands at once as the inline explorer's contents do.
        let overlay_width = workspace.layout.explorer_width.min(width - 48.0).max(160.0);
        body = body.child(
            el().key("explorer-overlay")
                .pin(None, Some(0.0), Some(0.0), Some(0.0))
                .enters_animated(a, Entrance::Drawer(16.0, 0.0))
                .child(
                    explorer(s, a, overlay_width)
                        .key("explorer")
                        .instant_changes()
                        .border(1.0, a.hover_bg())
                        .shadow(6.0, 20.0, a.scrim()),
                ),
        );
    }
    body
}
