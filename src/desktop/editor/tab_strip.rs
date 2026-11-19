use crate::desktop::workspace::{Tab, Workspace};
use crate::desktop::*;
/// The pane's tab strip: file tabs, Settings and Search, plus a return
/// button in the compact layout.
pub(crate) fn tab_strip(s: &State, a: &Appearance, width: f32) -> El {
    let workspace = &s.workspace;
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
    let file_selected = !s.settings && !s.search_tab;
    for (i, tab) in workspace.tabs.iter().enumerate() {
        tab_items.push(file_tab(
            s,
            a,
            i,
            tab,
            file_selected && i == workspace.active,
        ));
    }
    if s.settings_open {
        tab_items.insert(
            s.tab_strips.settings_position.min(tab_items.len()),
            settings_tab(a, s.settings),
        );
    }
    if s.search_tab_open {
        tab_items.insert(
            s.tab_strips.search_position.min(tab_items.len()),
            search_tab(s, a),
        );
    }
    crate::desktop::tabs::scroll_strip(tabs.children(tab_items), s, false)
}

/// An open file's tab, named by its file name plus as much of its parent
/// path as tells it apart from other open files with the same name.
fn file_tab(s: &State, a: &Appearance, i: usize, tab: &Tab, selected: bool) -> El {
    let workspace = &s.workspace;
    let id = tab.id;
    let name = tab_name(workspace, tab);
    let body = tab_body(a, format!("editor-tab-{id}"), selected)
        .label(name.clone())
        .help(tab.document.path.to_string_lossy())
        .description(if tab.preview {
            format!("{PREVIEW_TAB_DESCRIPTION}. {}", tab.document.path.display())
        } else {
            tab.document.path.to_string_lossy().into_owned()
        })
        .max_w(190.0)
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
            text(name)
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
        }));
    tab_pill(
        a,
        format!("editor-group-{id}"),
        "indicator-editor-tabs",
        selected,
        crate::desktop::tabs::interactive(body, crate::desktop::tabs::Target::Editor(id)),
        close_button(
            a,
            format!("editor-close-{id}"),
            "Close editor tab",
            move |s, tx| s.dispatch(AppAction::CloseEditor(Some(id)), tx),
        ),
    )
}

/// A file tab's title: its file name, plus its parent folder's name (or the
/// whole parent path, when those collide too) while another open file has
/// the same name.
fn tab_name(workspace: &Workspace, tab: &Tab) -> String {
    let id = tab.id;
    let filename = tab.document.path.file_name().unwrap_or_default();
    let duplicate = workspace
        .tabs
        .iter()
        .filter(|other| other.document.path.file_name() == Some(filename))
        .count()
        > 1;
    if !duplicate {
        return filename.to_string_lossy().into_owned();
    }
    let parent = tab.document.path.parent().unwrap_or(&tab.document.path);
    let parent_name = parent
        .file_name()
        .unwrap_or(parent.as_os_str())
        .to_string_lossy();
    // Keep extending the suffix when equal immediate parent names would still collide.
    let equal_parent = workspace.tabs.iter().any(|other| {
        other.id != id
            && other.document.path.file_name() == Some(filename)
            && other.document.path.parent().and_then(|p| p.file_name()) == parent.file_name()
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
}

fn settings_tab(a: &Appearance, selected: bool) -> El {
    let body = tab_body(a, "settings-tab", selected)
        .label("Settings")
        .help("Settings")
        .on_press_tx(|s: &mut State, tx| {
            tx.blur();
            s.dispatch(AppAction::Settings, tx);
        })
        .child(tab_title(a, "Settings", selected));
    tab_pill(
        a,
        "settings-group",
        "indicator-editor-tabs",
        selected,
        crate::desktop::tabs::interactive(body, crate::desktop::tabs::Target::Settings),
        close_button(a, "settings-close", "Close Settings tab", |s, tx| {
            s.dispatch(AppAction::CloseSettings, tx)
        }),
    )
}

fn search_tab(s: &State, a: &Appearance) -> El {
    let selected = s.search_tab;
    let body = tab_body(a, "search-tab", selected)
        .label("Search")
        .help(format!("Search: {}", s.explorer_search.query.text))
        .on_press_tx(|s: &mut State, tx| s.dispatch(AppAction::ShowSearch, tx))
        .child(rune::icon(&rune_kit::glyphs::SEARCH, 16.0).color(a.muted()))
        .child(tab_title(a, "Search", selected));
    tab_pill(
        a,
        "search-group",
        "indicator-editor-tabs",
        selected,
        crate::desktop::tabs::interactive(body, crate::desktop::tabs::Target::Search),
        close_button(a, "search-close", "Close Search tab", |s, tx| {
            s.dispatch(AppAction::CloseSearch, tx)
        }),
    )
}

/// An editor strip tab's focusable, pressable body; callers add its name,
/// activation and contents.
fn tab_body(a: &Appearance, key: impl Into<String>, selected: bool) -> El {
    let key = key.into();
    crate::desktop::pressable(&key);
    el().key(key)
        .role("tab")
        .focus_selected(selected)
        .focusable(true)
        .focus_ring(2.0, 1.0, a.accent())
        .h(29.0)
        .center()
        .px(8.0)
        .gap(6.0)
}

fn tab_title(a: &Appearance, title: &str, selected: bool) -> El {
    text(title)
        .font(ui_label())
        .color(if selected { a.ink() } else { a.muted() })
        .truncate()
}

fn close_button(
    a: &Appearance,
    key: impl Into<String>,
    tooltip: &'static str,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    icon_button(a, key, &rune_kit::glyphs::CLOSE, tooltip, action)
        .w(24.0)
        .min_h(24.0)
        .fill(Color::TRANSPARENT)
        .hover(a.hover_bg())
}
