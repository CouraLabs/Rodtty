use super::*;

pub(crate) fn on_key(state: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    if state.shortcut_recording.is_some() {
        return state.record_shortcut(event, tx);
    }
    if state.terminal_context_open || state.tab_strips.context_open {
        return true;
    }
    // Return presses the focused button, tab or menu item as Space does. The
    // workspace menu dispatches its own items on Return.
    if event.key == Key::Enter
        && event.mods == rune::Mods::default()
        && !state.workspace_menu
        && tx.focused().is_some_and(presses_on_return)
    {
        if !tx.key_repeat() {
            tx.send_key(KeyEvent {
                key: Key::Text(" ".into()),
                mods: Default::default(),
            });
        }
        return true;
    }
    if state.removal_background_busy() {
        if state.dispatch_shortcut(event, tx) {
            return true;
        }
        if state.application_overlay_open()
            || !state.workspace.terminal_focus()
            || !state.workspace.layout.terminal
            || tx.focused().is_some()
        {
            return true;
        }
        if let Some(bytes) = encode_key(event, state.with_screen(vt100::Screen::application_cursor))
        {
            state.send(bytes);
        }
        return true;
    }
    if state.pending.is_some() {
        if event.key == Key::Escape {
            if state.workspace.removal.is_some() {
                state.cancel_removal(tx);
            } else {
                state.cancel_file_action();
            }
            return true;
        }
        return !matches!(
            state.pending,
            Some(Pending::Operation(_) | Pending::Removal(removal::RemovalPrompt::SaveAs(_)))
        );
    }
    if state.workspace.operation_context.is_some() {
        if event.key == Key::Escape {
            state.workspace.operation_context = None;
            return true;
        }
        return false;
    }
    if state.palette.is_some() {
        return state.palette_key(event, tx);
    }
    if state.workspace_menu {
        let items = workspace_menu_items();
        let index = items
            .iter()
            .position(|(key, _, _)| Some(*key) == tx.focused())
            .unwrap_or(0);
        match event.key {
            Key::Escape => {
                state.workspace_menu = false;
                tx.focus("workspace-menu", true);
            }
            Key::Up | Key::Down => {
                let next = if event.key == Key::Down {
                    (index + 1) % items.len()
                } else {
                    (index + items.len() - 1) % items.len()
                };
                tx.focus(items[next].0, true);
            }
            Key::Enter => state.dispatch(items[index].2, tx),
            _ => return false,
        }
        return true;
    }
    if state.workspace.explorer_menu {
        let items =
            RootControl::ALL.map(|control| control.key().replace("explorer-", "explorer-menu-"));
        let index = items
            .iter()
            .position(|key| Some(key.as_str()) == tx.focused())
            .unwrap_or(0);
        match event.key {
            Key::Escape => {
                state.workspace.explorer_menu = false;
                tx.focus("explorer-more", true);
            }
            Key::Up | Key::Down => {
                let next = if event.key == Key::Down {
                    (index + 1) % items.len()
                } else {
                    (index + items.len() - 1) % items.len()
                };
                tx.focus(items[next].clone(), true);
            }
            _ => return false,
        }
        return true;
    }
    if event.key == Key::Escape && state.dismiss_explorer_overlay(tx) {
        return true;
    }
    if state.dispatch_shortcut(event, tx) {
        return true;
    }
    if tx.focused().is_some() {
        return false;
    }
    if !state.workspace.terminal_focus()
        && state.workspace.layout.editor
        && !state.workspace.tabs.is_empty()
    {
        return false;
    }
    if event.mods.shift && event.key == Key::PageUp {
        if let Some(session) = state.active() {
            session.scroll(20);
        }
        return true;
    }
    if event.mods.shift && event.key == Key::PageDown {
        if let Some(session) = state.active() {
            session.scroll(-20);
        }
        return true;
    }
    if let Some(bytes) = encode_key(event, state.with_screen(vt100::Screen::application_cursor)) {
        state.send(bytes);
        true
    } else {
        false
    }
}
