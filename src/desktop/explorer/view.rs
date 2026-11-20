use crate::desktop::*;
use crate::git::Change;
use rune_kit::{Tree, TreeItem, TreeStyle};

/// The status row under a folder that has no rows of its own. **Empty
/// folder** needs a successful listing of that folder that found nothing
/// visible; until then it is loading, and a failed listing says why.
pub(crate) fn folder_status(error: Option<&str>, listed: bool) -> String {
    match error {
        Some(error) => format!("Could not load: {error}"),
        None if listed => "Empty folder".into(),
        None => "Loading…".into(),
    }
}

/// An action at the trailing edge of the explorer root row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum RootControl {
    Search,
    Refresh,
    Follow,
    Hidden,
    Ignore,
}

impl RootControl {
    /// In row order. The row's width budget counts every control.
    pub(crate) const ALL: [RootControl; 5] = [
        RootControl::Search,
        RootControl::Refresh,
        RootControl::Follow,
        RootControl::Hidden,
        RootControl::Ignore,
    ];
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Search => "explorer-search-files",
            Self::Refresh => "explorer-refresh",
            Self::Follow => "explorer-follow",
            Self::Hidden => "explorer-hidden",
            Self::Ignore => "explorer-ignore",
        }
    }
    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Search => &rune_kit::glyphs::SEARCH,
            Self::Refresh => &rune_kit::glyphs::REFRESH,
            Self::Follow => &rune_kit::glyphs::CROSSHAIR,
            Self::Hidden => &rune_kit::glyphs::EYE,
            Self::Ignore => &rune_kit::glyphs::FILTER,
        }
    }
    /// The overflow menu's item title.
    fn title(self) -> &'static str {
        match self {
            Self::Search => "Search Files",
            Self::Refresh => "Refresh Files",
            Self::Follow => "Follow Terminal",
            Self::Hidden => "Show Hidden Files",
            Self::Ignore => "Respect Git Ignore Rules",
        }
    }
    /// A toggle's current state; `None` for a plain action.
    fn active(self, s: &State) -> Option<bool> {
        match self {
            Self::Search | Self::Refresh => None,
            Self::Follow => Some(s.config.explorer.follow_terminal),
            Self::Hidden => Some(s.config.explorer.show_hidden),
            Self::Ignore => Some(s.config.explorer.respect_ignore),
        }
    }
    /// The accessible name; a toggle reports its state separately.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Search => "Search files",
            Self::Refresh => "Refresh files",
            Self::Follow => "Follow terminal",
            Self::Hidden => "Show hidden files",
            Self::Ignore => "Respect Git ignore rules",
        }
    }
    /// The tooltip, naming a toggle's state for sight.
    fn help(self, s: &State) -> String {
        let name = self.name();
        match self.active(s) {
            Some(true) => format!("{name}: on"),
            Some(false) => format!("{name}: off"),
            None => name.into(),
        }
    }
    fn action(self) -> AppAction {
        match self {
            Self::Search => AppAction::SearchFiles,
            Self::Refresh => AppAction::RefreshExplorer,
            Self::Follow => AppAction::ToggleFollow,
            Self::Hidden => AppAction::ToggleHidden,
            Self::Ignore => AppAction::ToggleIgnore,
        }
    }
}

/// The explorer's horizontal padding, on each side.
pub(crate) const EXPLORER_PADDING: f32 = 10.0;
/// A root-row control's hit target: the header's icon button width.
pub(crate) const ROOT_CONTROL_WIDTH: f32 = 30.0;
pub(crate) const ROOT_CONTROL_GAP: f32 = 2.0;
/// The tree row's own chrome before its label and after its trailing content:
/// end padding (6 each side), chevron (16), icon (16) and the gaps between (4 each).
pub(crate) const TREE_ROW_CHROME: f32 = 6.0 + 16.0 + 4.0 + 16.0 + 4.0 + 4.0 + 6.0;
/// The least of the root's name the row keeps beside its controls.
pub(crate) const ROOT_NAME_MINIMUM: f32 = 32.0;

/// Whether every root-row control fits at full hit-target size in an explorer
/// this wide, leaving the root's name some room. Otherwise one overflow
/// button stands in for them.
pub(crate) fn root_controls_fit(explorer_width: f32) -> bool {
    let count = RootControl::ALL.len() as f32;
    let controls = count * ROOT_CONTROL_WIDTH + (count - 1.0) * ROOT_CONTROL_GAP;
    explorer_width - 2.0 * EXPLORER_PADDING - TREE_ROW_CHROME - controls >= ROOT_NAME_MINIMUM
}

/// The root row's trailing controls: each toggle shows whether it is on, or
/// one overflow button when the explorer is too narrow for them all.
pub(crate) fn root_controls(s: &State, a: &Appearance, width: f32, row_height: f32) -> El {
    let mut controls = row()
        .key("explorer-root-controls")
        .gap(ROOT_CONTROL_GAP)
        .center_y();
    let button = |key: &'static str, glyph, help: String, action: AppAction| {
        icon_button(a, key, glyph, help, move |s, tx| s.dispatch(action, tx))
            .min_h(row_height)
            .fill(Color::TRANSPARENT)
    };
    if root_controls_fit(width) {
        for control in RootControl::ALL {
            let mut button = button(
                control.key(),
                control.glyph(),
                control.help(s),
                control.action(),
            )
            .label(control.name());
            if let Some(on) = control.active(s) {
                button = button.toggle_state(a, on);
            }
            controls = controls.child(button);
        }
    } else {
        controls = controls.child(
            icon_button(
                a,
                "explorer-more",
                &rune_kit::glyphs::ELLIPSIS,
                "More explorer actions",
                |s, tx| {
                    s.workspace.explorer_menu = !s.workspace.explorer_menu;
                    if s.workspace.explorer_menu {
                        tx.focus_when_shown(
                            RootControl::ALL[0]
                                .key()
                                .replace("explorer-", "explorer-menu-"),
                            true,
                        );
                    }
                },
            )
            .min_h(row_height)
            .expanded(s.workspace.explorer_menu)
            .fill(if s.workspace.explorer_menu {
                a.hover_bg()
            } else {
                Color::TRANSPARENT
            }),
        );
    }
    controls
}

/// The overflow button's menu: each root-row action, with each toggle's state.
pub(crate) fn explorer_menu(s: &State, a: &Appearance) -> Option<El> {
    if !s.workspace.explorer_menu {
        return None;
    }
    let close = |s: &mut State, tx: &mut Tx| {
        s.workspace.explorer_menu = false;
        tx.focus("explorer-more", true);
    };
    let mut menu = col()
        .key("explorer-menu")
        .role("menu")
        .label("Explorer actions")
        .w(240.0)
        .px(6.0)
        .py(6.0)
        .gap(3.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg())
        .shadow(6.0, 20.0, a.scrim())
        .enters(a, Entrance::Menu(motion::FROM_TOP_TRAILING))
        .modal_tx(true, close)
        .anchored("explorer-more", rune::Side::Bottom, rune::Align::End, 4.0);
    for control in RootControl::ALL {
        let help = control.help(s);
        let action = control.action();
        let key = control.key().replace("explorer-", "explorer-menu-");
        crate::desktop::pressable(&key);
        let mut item = row()
            .key(key)
            .role(if control.active(s).is_some() {
                "menuitemcheckbox"
            } else {
                "menuitem"
            })
            .label(control.name())
            .help(help)
            .focusable(true)
            .focus_ring(2.0, 1.0, a.accent())
            .w_full()
            .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
            .px(10.0)
            .gap(8.0)
            .center_y()
            .radius(5.0)
            .hover(a.hover_bg())
            .on_press_tx(move |s: &mut State, tx| {
                s.workspace.explorer_menu = false;
                tx.focus("explorer-more", false);
                s.dispatch(action, tx);
            })
            .child(rune::icon(control.glyph(), 16.0).color(a.ink()))
            .child(
                text(control.title())
                    .font(ui_label())
                    .color(a.ink())
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate(),
            );
        if let Some(on) = control.active(s) {
            item = item.checked(on).child(
                text(if on { "On" } else { "Off" })
                    .key("state")
                    // The item's checked state says this to assistive technology.
                    .a11y_hidden()
                    .font(ui_label())
                    .color(if on { a.accent() } else { a.muted() }),
            );
        }
        menu = menu.child(item);
    }
    Some(menu)
}

pub(crate) fn explorer(s: &State, a: &Appearance, width: f32) -> El {
    if s.changes.open {
        return changes::view(s, a, width);
    }
    let workspace = &s.workspace;
    let mut pane = col()
        .key("explorer")
        .w(width)
        .min_w(160.0)
        .h_full()
        .min_h(0.0)
        .fill(a.panel())
        .px(EXPLORER_PADDING)
        .pt(6.0)
        .gap(6.0);
    if let Some(cwd) = &workspace.outside_root {
        pane = pane
            .child(
                text(format!(
                    "Terminal is outside this host folder: {}",
                    cwd.display()
                ))
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
            )
            .child(button(
                a,
                "explorer-use-terminal-root",
                "Use terminal folder",
                |s, tx| s.dispatch(AppAction::ExplorerTerminalRoot, tx),
            ));
    }
    if let Some(path) = &workspace.operation_result
        && workspace
            .directory
            .as_ref()
            .is_none_or(|root| !path.starts_with(root))
    {
        pane = pane
            .child(
                text(format!("Last result: {}", path.display()))
                    .font(ui_label())
                    .color(a.muted())
                    .wrap_anywhere(),
            )
            .child(button(
                a,
                "explorer-reveal-result",
                "Show destination",
                |s, tx| s.dispatch(AppAction::RevealFileOperation, tx),
            ));
    }
    let danger = a.danger();
    let accent = a.accent();
    let success = a.success();
    let dark = a.dark;
    let workspace_root_path = workspace.directory.clone().unwrap_or_default();
    let mut tree_style = TreeStyle::from_kit(a.kit())
        .label(ui_label(), a.ink(), a.ink())
        .radius(6.0);
    tree_style.row_height = 28.0_f32.max(ui_label().size * 1.5 + 6.0);
    tree_style.selected = a.hover_bg();
    // The fill alone changes too little to mark the selected row: an accent
    // bar along its leading edge reads at 3:1 or more in every appearance.
    tree_style.selected_mark = Some(a.accent());
    // The root row's controls rest hidden and appear while the row is hovered
    // or holds keyboard focus (the row's hover track lights for both). A
    // selected row draws a fill instead of a hover track, so there they show.
    let controls = root_controls(s, a, width, tree_style.row_height);
    let list = Tree::new(
        |s: &State| &s.workspace.tree,
        |s: &mut State| &mut s.workspace.tree,
        |node| &node.label,
    )
    .name("Host files")
    .style(tree_style)
    .row(move |node, row| {
        let mut item = TreeItem::new(&node.label);
        if !node.badge.is_empty() {
            item = item.badge(&node.badge);
        }
        if node.root {
            // The workspace folder's own row: drawn open, rooted at the folder's
            // name, which truncates before the controls at its trailing edge.
            item = item
                .icon(explorer::icons::resolve(
                    &workspace_root_path,
                    true,
                    true,
                    dark,
                ))
                .trailing(if row.selected {
                    controls.clone()
                } else {
                    controls.clone().group_opacity(0.0, 1.0)
                });
        } else if let Some(entry) = &node.entry {
            item = item.icon(explorer::icons::resolve(
                &entry.path,
                entry.directory,
                row.expanded == Some(true),
                dark,
            ));
            // Git information takes the place of a size: a changed file's
            // name is tinted by its change, and an updated file counts the
            // lines it adds and removes.
            if let Some((added, removed)) =
                node.lines.filter(|_| node.change == Some(Change::Modified))
            {
                item = item.changes(added, removed);
            }
        }
        match node.change {
            Some(Change::Modified) => item.label_color(accent),
            Some(Change::Added) => item.label_color(success),
            Some(Change::Deleted) => item.label_color(danger).strike(),
            None => item,
        }
    })
    .on_select(|s, _, tx| {
        s.workspace.set_terminal_focus(false);
        s.dispatch(AppAction::PreviewExplorerSelection, tx);
    })
    .on_open(|s, _, tx| s.dispatch(AppAction::ExplorerOpenSelection, tx))
    .build(s, a.kit())
    .instant_changes()
    .pb(ui_label().size * 0.25)
    .grow(1.0)
    .min_h(0.0)
    .on_secondary_press(|s: &mut State, press, tx| s.file_context_menu(press, tx));
    pane = pane.child(list);
    if workspace.operation_busy {
        pane = pane.child(text("Working on files…").font(ui_label()).color(a.muted()));
    }
    if workspace.scan_busy {
        pane = pane.child(text("Loading…").font(ui_label()).color(a.muted()));
    } else if workspace.entries.is_empty() && workspace.explorer_error.is_none() {
        pane = pane.child(text("No visible files").font(ui_label()).color(a.muted()));
    }
    if let Some(error) = &workspace.explorer_error {
        pane = pane.child(
            text(format!("Could not load folder: {error}"))
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    }
    if let Some(error) = &workspace.git_error {
        pane = pane.child(
            text(error)
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    }
    pane
}

pub(crate) fn explorer_minimum_width(s: &State) -> f32 {
    if editor_available(s) && s.workspace.layout.terminal {
        1120.0
    } else {
        880.0
    }
}
