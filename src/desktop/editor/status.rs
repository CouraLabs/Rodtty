use crate::desktop::workspace::*;
use crate::desktop::*;
use crate::git::Comparison;
/// The status line's left side: view modes, diff controls and Save.
pub(crate) fn mode_controls(s: &State, a: &Appearance, tab: &Tab) -> El {
    let workspace = &s.workspace;
    let id = tab.id;
    let markdown = is_markdown(&tab.document.path);
    let mut modes = row().min_w(0.0).gap(2.0).scroll_x().child(
        icon_button(
            a,
            "file-edit",
            &rune_kit::glyphs::PENCIL,
            "Edit",
            move |s: &mut State, tx| s.dispatch(AppAction::EditFile, tx),
        )
        .toggle_state(a, tab.mode == View::Edit)
        .hover(a.hover_bg()),
    );
    if markdown {
        modes = modes.child(
            icon_button(
                a,
                "file-preview",
                &rune_kit::glyphs::PREVIEW_TEXT,
                "Preview",
                move |s: &mut State, tx| {
                    if let Some(tab) = s.workspace.tabs.get_mut(s.workspace.active) {
                        tab.mode = if tab.mode == View::Preview {
                            View::Edit
                        } else {
                            View::Preview
                        };
                    }
                    s.workspace.set_terminal_focus(false);
                    tx.blur();
                },
            )
            .toggle_state(a, tab.mode == View::Preview)
            .hover(a.hover_bg()),
        );
    }
    for (key, label, mode) in [
        (
            "diff-current",
            "Compare with saved file",
            Comparison::Current,
        ),
        (
            "diff-unstaged",
            "Compare unstaged changes",
            Comparison::Unstaged,
        ),
        ("diff-staged", "Compare staged changes", Comparison::Staged),
    ] {
        // A comparison shows only when it has changes to show. The explorer's
        // Git status decides; without it (another repository, or the explorer
        // closed) every comparison stays available. The open one stays too.
        let path = &tab.document.path;
        let available = tab.mode == View::Diff(mode)
            || workspace
                .repo
                .as_ref()
                .filter(|repo| path.starts_with(&repo.root))
                .is_none_or(|repo| {
                    repo.compares(path, mode) || (mode == Comparison::Current && tab.dirty())
                });
        if !available {
            continue;
        }
        modes = modes.child(
            icon_button(a, key, &rune_kit::glyphs::COMPARE, label, move |s, tx| {
                s.dispatch(AppAction::Compare(mode), tx)
            })
            .toggle_state(a, tab.mode == View::Diff(mode))
            .hover(a.hover_bg()),
        );
    }
    // A thin vertical rule between groups of status line controls.
    let separator = || {
        el().w(1.0)
            .h(18.0)
            .mx(4.0)
            .fill(a.kit().c.border_base)
            .a11y_hidden()
    };
    if matches!(tab.mode, View::Diff(_)) {
        // Git diff controls sit in the status line, before the save button,
        // instead of over the diff.
        modes = modes
            .child(separator())
            .child(
                row()
                    .key("diff-controls")
                    .role("radiogroup")
                    .label("Diff layout")
                    .gap(2.0)
                    .center_y()
                    .children(
                        [("auto", "Auto"), ("inline", "Inline"), ("split", "Split")]
                            .into_iter()
                            .zip(0u8..)
                            .map(|((key, title), layout)| {
                                // One of three layouts: a radio whose active
                                // choice is outlined, not only tinted.
                                let name = format!("{title} diff layout");
                                button(a, format!("diff-{key}"), title, move |s, _| {
                                    s.workspace.diff_layout = layout
                                })
                                .role("radio")
                                .label(name.clone())
                                .help(name)
                                .toggle_state(a, s.workspace.diff_layout == layout)
                                .hover(a.hover_bg())
                            }),
                    ),
            )
            .child(icon_button(
                a,
                "diff-prev",
                &rune_kit::glyphs::ARROW_UP,
                "Previous change",
                |s: &mut State, tx| {
                    s.workspace.diff.next_hunk(false, tx);
                },
            ))
            .child(icon_button(
                a,
                "diff-next",
                &rune_kit::glyphs::ARROW_DOWN,
                "Next change",
                |s: &mut State, tx| {
                    s.workspace.diff.next_hunk(true, tx);
                },
            ));
    }
    let save_help = s.shortcut_help("save-file", "Save file");
    modes = modes.child(separator()).child(
        icon_button(
            a,
            "file-save",
            &rune_kit::glyphs::SAVE,
            if tab.saving { "Saving…" } else { &save_help },
            move |s: &mut State, tx| s.dispatch(AppAction::SaveEditor(Some(id)), tx),
        )
        .disabled(tab.saving),
    );
    modes
}

/// The status line: mode controls, then the cursor position and encoding, a
/// preview label, or what a diff compares.
pub(crate) fn status_line(s: &State, a: &Appearance, tab: &Tab, modes: El) -> El {
    let workspace = &s.workspace;
    let status_text = if tab.mode == View::Edit {
        let position = tab.edit.focus;
        let line = tab.edit.text[..position]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1;
        let column = tab.edit.text[..position]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .count()
            + 1;
        Some(format!(
            "UTF-8{} · {:?} · Ln {line}, Col {column}",
            if tab.document.saved.bom { " BOM" } else { "" },
            tab.document.saved.preferred
        ))
    } else if tab.mode == View::Preview {
        Some("Markdown preview".to_owned())
    } else {
        None
    };
    let footer = row()
        .key("editor-status")
        .w_full()
        .fill(a.panel())
        .border_sides([1.0, 0.0, 0.0, 0.0], a.hover_bg())
        .px(10.0)
        .py(4.0)
        .gap(6.0)
        .center_y()
        .child(modes)
        .child(el().grow(1.0));
    if let Some(status) = status_text {
        footer.child(text(status).font(ui_label()).color(a.muted()).shrink())
    } else if matches!(tab.mode, View::Diff(_)) {
        // What the diff compares, in place of the editor's position.
        footer.child(
            text(&workspace.diff_label)
                .key("diff-label")
                .font(ui_label())
                .color(a.muted())
                .shrink()
                .truncate(),
        )
    } else {
        footer
    }
}
