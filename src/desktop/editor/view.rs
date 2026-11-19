use crate::desktop::workspace::*;
use crate::desktop::*;
use rune_kit::{
    diff::DiffView,
    syntax::{self, SyntaxTheme},
};

/// The editor pane: its tab strip over Settings, Search or the open file.
pub(crate) fn editor(s: &State, a: &Appearance, width: f32) -> El {
    let pane = col()
        .key("editor-pane")
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .fill(a.bg())
        .on_layout(|s: &mut State, r| s.workspace.editor_width = r[2])
        .child(tab_strip(s, a, width));
    if s.settings {
        return pane.child(crate::desktop::settings::content(s, a));
    }
    if s.search_tab {
        return pane.child(search_content(s, a));
    }
    let workspace = &s.workspace;
    let Some(tab) = workspace.tabs.get(workspace.active) else {
        // Settings closed its tab but the pane is still drawn while it exits;
        // an editor pane with nothing open renders nothing rather than a placeholder.
        return pane.child(el().grow(1.0).fill(a.bg()));
    };
    document(s, a, pane, tab)
}

/// The Search tab: results over their summary footer.
fn search_content(s: &State, a: &Appearance) -> El {
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
                .child(search::results(s, a)),
        )
        .child(search::footer(s, a))
}

/// The open file: notices, find bar, its edit, preview or diff content, and
/// the status line.
fn document(s: &State, a: &Appearance, mut pane: El, tab: &Tab) -> El {
    let workspace = &s.workspace;
    let footer = status_line(s, a, tab, mode_controls(s, a, tab));
    let mut below = notices(a, tab);
    if workspace.find && tab.mode == View::Edit {
        pane = pane.child(find_bar(a, workspace));
    }
    match tab.mode {
        View::Preview => pane = pane.child(preview_content(a, tab)),
        View::Edit => pane = pane.child(edit_content(s, a, tab)),
        View::Diff(_) => {
            let (content, diff_below) = diff_content(s, a);
            pane = pane.child(content);
            below.splice(0..0, diff_below);
        }
    }
    pane.children(below).child(footer)
}

/// Tabs and compact navigation stay above the content; the file's status,
/// Git diff controls and file actions sit below it, the actions last.
fn notices(a: &Appearance, tab: &Tab) -> Vec<El> {
    let id = tab.id;
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
    below
}

/// Find and replace in the open file.
fn find_bar(a: &Appearance, workspace: &Workspace) -> El {
    col()
        .w_full()
        .gap(4.0)
        .py(4.0)
        .child(
            row()
                .gap(4.0)
                .child(
                    find_field(a, &workspace.find_query, "editor-find", "Find text")
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
                    find_field(a, &workspace.replacement, "editor-replace", "Replace with")
                        .on_edit(|s: &mut State| &mut s.workspace.replacement),
                )
                .child(button(a, "replace-next", "Replace", |s, tx| {
                    s.find_editor(true, tx)
                }))
                .child(button(a, "replace-all", "All", |s, _| s.replace_all())),
        )
}

/// A find or replace field: it grows to fill its row beside the buttons.
fn find_field(a: &Appearance, edit: &TextEdit, key: &str, placeholder: &str) -> El {
    text_input(edit, ui_label())
        .caret(a.accent(), a.selection())
        .key(key)
        .single_line()
        .w_full()
        .shrink()
        .grow(1.0)
        .px(8.0)
        .py(4.0)
        .color(a.ink())
        .placeholder(placeholder, a.muted())
        .border(1.0, a.kit().c.border_base)
        .radius(6.0)
}

/// Rendered markdown: the same text the editor shows, laid out and styled
/// by Rune Kit, in the editor's scroll area.
fn preview_content(a: &Appearance, tab: &Tab) -> El {
    let id = tab.id;
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
    a.kit().scroll_bar(viewport)
}

/// The syntax-highlighted text editor.
fn edit_content(s: &State, a: &Appearance, tab: &Tab) -> El {
    let workspace = &s.workspace;
    let id = tab.id;
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
                .key(search::search_match_key(id)),
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
    a.kit().scroll_bar(viewport)
}

/// The Git diff and any diff error to show below it.
fn diff_content(s: &State, a: &Appearance) -> (El, Vec<El>) {
    let workspace = &s.workspace;
    let mut diff_below = Vec::new();
    if let Some(error) = &workspace.diff_error {
        diff_below.push(
            text(error)
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    }
    let content = if workspace.diff_patch.is_empty() {
        el().grow(1.0)
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
        el().key("git-diff-viewport")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .clip_soft(0.0)
            .instant_changes()
            .on_layout(|s: &mut State, rect| s.workspace.diff_viewport = (rect[2], rect[3]))
            .child(diff)
    };
    (content, diff_below)
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
