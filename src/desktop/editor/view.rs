use crate::desktop::workspace::*;
use crate::desktop::*;
use crate::git::Comparison;
use rune_kit::{
    diff::DiffView,
    syntax::{self, SyntaxTheme},
};

pub(crate) fn editor(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
    let mut pane = col()
        .key("editor-pane")
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .fill(a.bg())
        .on_layout(|s: &mut State, r| s.workspace.editor_width = r[2]);
    let mut tabs = row()
        .key("editor-tabs")
        .role("tablist")
        .label("Editor tabs")
        .h(38.0_f32.max(ui_label().size * 1.5 + 16.0))
        .px(6.0)
        .gap(3.0)
        .center_y()
        .w_full()
        .scroll_x()
        .fill(a.panel());
    if width < 880.0 {
        tabs = tabs.child(
            icon_button(
                a,
                "return-terminal",
                &rune_kit::glyphs::ARROW_LEFT,
                "Return to terminal",
                |s, tx| s.dispatch(AppAction::ShowTerminal, tx),
            )
            .fill(a.hover_bg()),
        );
    }
    let mut tab_items = Vec::new();
    for (i, tab) in workspace.tabs.iter().enumerate() {
        let id = tab.id;
        let filename = tab.document.path.file_name().unwrap_or_default();
        let duplicate = workspace
            .tabs
            .iter()
            .filter(|other| other.document.path.file_name() == Some(filename))
            .count()
            > 1;
        let name = if duplicate {
            let parent = tab.document.path.parent().unwrap_or(&tab.document.path);
            let parent_name = parent
                .file_name()
                .unwrap_or(parent.as_os_str())
                .to_string_lossy();
            // Keep extending the suffix when equal immediate parent names would still collide.
            let equal_parent = workspace.tabs.iter().any(|other| {
                other.id != id
                    && other.document.path.file_name() == Some(filename)
                    && other.document.path.parent().and_then(|p| p.file_name())
                        == parent.file_name()
            });
            format!(
                "{} — {}",
                filename.to_string_lossy(),
                if equal_parent {
                    parent.to_string_lossy()
                } else {
                    parent_name
                }
            )
        } else {
            filename.to_string_lossy().into_owned()
        };
        tab_items.push(
            row()
                .key(format!("editor-group-{id}"))
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if i == workspace.active && !s.settings && !s.search_tab {
                    a.hover_bg()
                } else {
                    a.panel()
                })
                .hover(a.hover_bg())
                .children(
                    (i == workspace.active && !s.settings && !s.search_tab)
                        .then(|| crate::desktop::tab_indicator(a, "indicator-editor-tabs")),
                )
                .child(crate::desktop::tabs::interactive(
                    el().key({
                        let key = format!("editor-tab-{id}");
                        crate::desktop::pressable(&key);
                        key
                    })
                    .role("tab")
                    .focus_selected(i == workspace.active && !s.settings && !s.search_tab)
                    .label(name.clone())
                    .help(tab.document.path.to_string_lossy())
                    .description(if tab.preview {
                        format!("{PREVIEW_TAB_DESCRIPTION}. {}", tab.document.path.display())
                    } else {
                        tab.document.path.to_string_lossy().into_owned()
                    })
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .max_w(190.0)
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .gap(6.0)
                    .on_press_tx(move |s: &mut State, tx| {
                        tx.blur();
                        s.dispatch(AppAction::ActivateEditor(i), tx);
                    })
                    .on_double_press(move |s: &mut State| {
                        if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                            tab.preview = false;
                        }
                    })
                    .child(
                        rune::icon(
                            file_icons::resolve(&tab.document.path, false, false, a.dark),
                            16.0,
                        )
                        .color(a.muted())
                        .key(format!("editor-file-icon-{id}")),
                    )
                    .child(
                        text(name.clone())
                            .key(format!("editor-title-{id}"))
                            .font(tab_title_font(tab.preview))
                            .color(match (tab.preview, i == workspace.active) {
                                (true, selected) => a.temporary_tab_ink(selected),
                                (false, true) => a.ink(),
                                (false, false) => a.muted(),
                            })
                            .max_w(120.0)
                            .truncate(),
                    )
                    .children(tab.dirty().then(|| {
                        text("•")
                            .key(format!("editor-dirty-{id}"))
                            .font(ui_label())
                            .color(a.accent())
                    })),
                    crate::desktop::tabs::Target::Editor(id),
                ))
                .child(
                    icon_button(
                        a,
                        format!("editor-close-{id}"),
                        &rune_kit::glyphs::CLOSE,
                        "Close editor tab",
                        move |s, tx| s.dispatch(AppAction::CloseEditor(Some(id)), tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
                ),
        );
    }
    if s.settings_open {
        tab_items.insert(
            s.tab_strips.settings_position.min(tab_items.len()),
            row()
                .key("settings-group")
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if s.settings { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .children(
                    s.settings
                        .then(|| crate::desktop::tab_indicator(a, "indicator-editor-tabs")),
                )
                .child(crate::desktop::tabs::interactive(
                    el().key({
                        crate::desktop::pressable("settings-tab");
                        "settings-tab"
                    })
                    .role("tab")
                    .focus_selected(s.settings)
                    .label("Settings")
                    .help("Settings")
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .on_press_tx(|s: &mut State, tx| {
                        tx.blur();
                        s.dispatch(AppAction::Settings, tx);
                    })
                    .child(
                        text("Settings")
                            .font(ui_label())
                            .color(if s.settings { a.ink() } else { a.muted() })
                            .truncate(),
                    ),
                    crate::desktop::tabs::Target::Settings,
                ))
                .child(
                    icon_button(
                        a,
                        "settings-close",
                        &rune_kit::glyphs::CLOSE,
                        "Close Settings tab",
                        |s, tx| s.dispatch(AppAction::CloseSettings, tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
                ),
        );
    }
    if s.search_tab_open {
        let shown = s.search_tab;
        tab_items.insert(
            s.tab_strips.search_position.min(tab_items.len()),
            row()
                .key("search-group")
                .role("none")
                .gap(0.0)
                .center_y()
                .radius(6.0)
                .fill(if shown { a.hover_bg() } else { a.panel() })
                .hover(a.hover_bg())
                .children(shown.then(|| crate::desktop::tab_indicator(a, "indicator-editor-tabs")))
                .child(crate::desktop::tabs::interactive(
                    el().key({
                        crate::desktop::pressable("search-tab");
                        "search-tab"
                    })
                    .role("tab")
                    .focus_selected(shown)
                    .label("Search")
                    .help(format!("Search: {}", s.explorer_search.query.text))
                    .focusable(true)
                    .focus_ring(2.0, 1.0, a.accent())
                    .h(29.0)
                    .center()
                    .px(8.0)
                    .gap(6.0)
                    .on_press_tx(|s: &mut State, tx| s.dispatch(AppAction::ShowSearch, tx))
                    .child(rune::icon(&rune_kit::glyphs::SEARCH, 16.0).color(a.muted()))
                    .child(
                        text("Search")
                            .font(ui_label())
                            .color(if shown { a.ink() } else { a.muted() })
                            .truncate(),
                    ),
                    crate::desktop::tabs::Target::Search,
                ))
                .child(
                    icon_button(
                        a,
                        "search-close",
                        &rune_kit::glyphs::CLOSE,
                        "Close Search tab",
                        |s, tx| s.dispatch(AppAction::CloseSearch, tx),
                    )
                    .w(24.0)
                    .min_h(24.0)
                    .fill(Color::TRANSPARENT)
                    .hover(a.hover_bg()),
                ),
        );
    }
    pane = pane.child(crate::desktop::tabs::scroll_strip(
        tabs.children(tab_items),
        s,
        false,
    ));
    if s.settings {
        return pane.child(crate::desktop::settings::content(s, a));
    }
    if s.search_tab {
        return pane.child(
            col()
                .key("search-content")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .fill(a.bg())
                .child(
                    col()
                        .w_full()
                        .grow(1.0)
                        .min_h(0.0)
                        .px(10.0)
                        .pt(8.0)
                        .child(explorer_search::results(s, a)),
                )
                .child(explorer_search::footer(s, a)),
        );
    }
    let Some(tab) = workspace.tabs.get(workspace.active) else {
        // Settings closed its tab but the pane is still drawn while it exits;
        // an editor pane with nothing open renders nothing rather than a placeholder.
        return pane.child(el().grow(1.0).fill(a.bg()));
    };
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
    // The editor's status line: mode icons on the left, position and encoding on
    // the right, with breathing room on all sides.
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
    let footer = if let Some(status) = status_text {
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
    };
    // Tabs and compact navigation stay above the content; the file's status,
    // Git diff controls and file actions sit below it, the actions last.
    let mut below = Vec::new();
    if let Some(error) = &tab.disk_error {
        below.push(
            text(format!("Disk: {error}"))
                .font(ui_label())
                .color(a.danger())
                .wrap_anywhere(),
        );
    }
    if tab.external.is_some() && !tab.dismissed {
        below.push(
            col()
                .key("external-change")
                .w_full()
                .fill(a.warning_bg())
                .py(6.0)
                .child(
                    text("Changed on disk. Your unsaved edits are preserved.")
                        .font(ui_label())
                        .color(a.ink())
                        .wrap(),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(button(
                            a,
                            "external-reload",
                            "Reload · discard edits",
                            move |s, _| s.reload_external(id),
                        ))
                        .child(button(a, "external-keep", "Keep edits", move |s, _| {
                            if let Some(t) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                                t.dismissed = true;
                            }
                        })),
                ),
        );
    }
    if workspace.find && tab.mode == View::Edit {
        pane = pane.child(
            col()
                .w_full()
                .gap(4.0)
                .py(4.0)
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            text_input(&workspace.find_query, ui_label())
                                .caret(a.accent(), a.selection())
                                .key("editor-find")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .px(8.0)
                                .py(4.0)
                                .color(a.ink())
                                .placeholder("Find text", a.muted())
                                .border(1.0, a.kit().c.border_base)
                                .radius(6.0)
                                .on_edit(|s: &mut State| &mut s.workspace.find_query)
                                .on_submit(|s: &mut State, tx| s.find_editor(false, tx)),
                        )
                        .child(button(a, "find-next", "Next", |s, tx| {
                            s.find_editor(false, tx)
                        }))
                        .child(button(a, "find-close", "×", |s, _| {
                            s.workspace.find = false
                        })),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            text_input(&workspace.replacement, ui_label())
                                .caret(a.accent(), a.selection())
                                .key("editor-replace")
                                .single_line()
                                .w_full()
                                .shrink()
                                .grow(1.0)
                                .px(8.0)
                                .py(4.0)
                                .color(a.ink())
                                .placeholder("Replace with", a.muted())
                                .border(1.0, a.kit().c.border_base)
                                .radius(6.0)
                                .on_edit(|s: &mut State| &mut s.workspace.replacement),
                        )
                        .child(button(a, "replace-next", "Replace", |s, tx| {
                            s.find_editor(true, tx)
                        }))
                        .child(button(a, "replace-all", "All", |s, _| s.replace_all())),
                ),
        );
    }
    if tab.mode == View::Preview {
        // Rendered markdown: the same text the editor shows, laid out and styled
        // by Rune Kit, in the editor's scroll area.
        let rendered = rune_kit::Markdown::new(tab.edit.text.clone())
            .autolink(true)
            .build(a.kit());
        let viewport = col()
            .key(format!("editor-scroll-{id}"))
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .px(16.0)
            .py(12.0)
            .child(rendered);
        pane = pane.child(a.kit().scroll_bar(viewport));
    } else if tab.mode == View::Edit {
        let language = syntax::lang_for_path(&tab.document.path.to_string_lossy());
        let highlights = syntax::highlight(language, &tab.edit.text);
        let spans = SyntaxTheme::from_kit(a.kit()).spans(&highlights, 0);
        let mut field = text_input(&tab.edit, fonts::editor())
            .caret(a.accent(), a.selection())
            .key(format!("file-editor-{id}"))
            .label("File editor")
            .disabled(workspace.removal.is_some())
            .w_full()
            .color(a.ink())
            .wrap_anywhere()
            .spans(spans)
            .on_edit(move |s: &mut State| {
                let workspace = &mut s.workspace;
                if let Some(tab) = workspace.tabs.iter_mut().find(|t| t.id == id) {
                    &mut tab.edit
                } else {
                    &mut workspace.orphan_edit
                }
            })
            .on_input_change(move |s: &mut State, _| s.changed_editor(id))
            .on_input_key(move |s: &mut State, event, tx| s.file_key(id, event, tx))
            .on_input_paste(move |s: &mut State, clipboard, _| {
                if s.workspace.removal.is_some() {
                    return true;
                }
                if let Some(tab) = s.workspace.tabs.iter_mut().find(|t| t.id == id) {
                    tab.edit
                        .insert(&clipboard.text().replace("\r\n", "\n").replace('\r', "\n"));
                    tab.revision += 1;
                    tab.preview = false;
                }
                true
            });
        // A Search Files match keeps a keyed mark while it is still selected,
        // for scrolling to and so it reads while the editor is unfocused.
        if let Some((_, range)) = workspace
            .search_reveal
            .as_ref()
            .filter(|(tab_id, range)| *tab_id == id && tab.edit.range() == (range.start, range.end))
        {
            field = field.text_highlight(
                rune::TextHighlight::new(range.clone(), a.accent().alpha(0.22))
                    .key(explorer_search::search_match_key(id)),
            );
        }
        let viewport = col()
            .key(format!("editor-scroll-{id}"))
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .px(10.0)
            .py(8.0)
            .child(field);
        pane = pane.child(a.kit().scroll_bar(viewport));
    } else {
        let mut diff_below = Vec::new();
        if let Some(error) = &workspace.diff_error {
            diff_below.push(
                text(error)
                    .font(ui_label())
                    .color(a.muted())
                    .wrap_anywhere(),
            );
        }
        if workspace.diff_patch.is_empty() {
            pane = pane.child(el().grow(1.0));
        } else {
            // Pinned Rune Kit fixes diff rows at 13px / 20px internally. Scale its
            // coordinate space and inverse-size its viewport together: virtualization,
            // scrolling, selection and hit testing retain their native coordinates.
            let zoom = s.config.fonts.editor_size / 13.0;
            let (width, height) = workspace.diff_viewport;
            let diff = DiffView::new(
                |s: &State| &s.workspace.diff,
                |s: &mut State| &mut s.workspace.diff,
            )
            .key("git-diff")
            .build(s, a.kit())
            .focusable(true)
            .absolute(0.0, 0.0)
            .size(width / zoom, height / zoom)
            .origin(0.0, 0.0)
            .scale(zoom);
            pane = pane.child(
                el().key("git-diff-viewport")
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .clip_soft(0.0)
                    .instant_changes()
                    .on_layout(|s: &mut State, rect| s.workspace.diff_viewport = (rect[2], rect[3]))
                    .child(diff),
            );
        }
        below.splice(0..0, diff_below);
    }
    pane.children(below).child(footer)
}

/// The accessible description that marks a preview editor tab, whose title
/// shows the temporary state only as italic, dimmed text.
pub(crate) const PREVIEW_TAB_DESCRIPTION: &str =
    "Preview tab, replaced when another file is previewed";

/// An editor tab title's type: a preview editor tab's is italic.
pub(crate) fn tab_title_font(temporary: bool) -> rune::TextStyle {
    let font = ui_label();
    if temporary { font.italic() } else { font }
}

/// Whether the path names a markdown file Rodtty can render as a preview.
pub(crate) fn is_markdown(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

pub(crate) fn editor_available(s: &State) -> bool {
    // No open editor tab and no Settings tab: the pane closes rather than
    // showing an empty placeholder, so the terminal keeps the workspace.
    s.workspace.layout.editor
        && (!s.workspace.tabs.is_empty() || s.settings_open || s.search_tab_open)
}
