use crate::desktop::*;

pub(crate) fn workspace_menu_items() -> [(&'static str, &'static str, AppAction); 6] {
    [
        ("palette", "Command Palette…", AppAction::CommandPalette),
        ("menu-settings", "Settings", AppAction::Settings),
        ("find", "Find in Terminal…", AppAction::FindTerminal),
        ("close-tab", "End Terminal Session…", AppAction::EndTerminal),
        ("menu-close-window", "Close Window", AppAction::CloseWindow),
        ("quit", "Quit Rodtty…", AppAction::Quit),
    ]
}

pub(crate) fn workspace_menu(state: &State, a: &Appearance) -> El {
    let mut menu = col()
        .key("workspace-menu-popover")
        .w(276.0)
        .px(6.0)
        .py(6.0)
        .gap(3.0)
        .radius(10.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg())
        .enters(a, Entrance::Menu(motion::FROM_TOP_TRAILING))
        .modal_tx(true, |s: &mut State, tx| {
            s.workspace_menu = false;
            tx.focus("workspace-menu", true);
        });
    for (key, title, action) in workspace_menu_items() {
        menu = menu.child(
            button(a, key, title, move |s, tx| s.dispatch(action, tx))
                .help(state.action_help(action, title))
                .w_full()
                .justify_start()
                .px(12.0),
        );
    }
    menu.pin(None, Some(header_height()), Some(8.0), None)
}
