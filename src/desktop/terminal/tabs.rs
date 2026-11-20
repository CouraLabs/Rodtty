use crate::desktop::*;

/// The terminal tabs, laid out as the titlebar's own strip: each tab a pill,
/// the active one filled, its close button beside the label, and the new-tab
/// button after the last tab. The strip takes the width it is given and
/// scrolls rather than pushing the header's actions off the edge.
pub(crate) fn terminal_tabs(state: &State, a: &Appearance) -> El {
    let mut tabs = row()
        .key("tabs")
        .h(HEADER_TAB_HEIGHT)
        .gap(4.0)
        .min_w(0.0)
        .center_y();
    // Sized to its tabs so the new-tab button follows the last one, but free
    // to shrink below them so a crowded strip scrolls inside the header.
    let mut list = row()
        .key("terminal-tabs")
        .role("tablist")
        .label("Terminal tabs")
        .shrink()
        .scroll_x()
        .gap(4.0)
        .center_y();
    // A split tab's second pane draws inside the first's tab.
    let count = state
        .sessions
        .iter()
        .filter(|s| !state.is_second_pane(s.id()))
        .count();
    let active_id = state.active().map(Session::id);
    let mut position = 0;
    for (i, session) in state.sessions.iter().enumerate() {
        if state.is_second_pane(session.id()) {
            continue;
        }
        let split = state.split_of(session.id());
        let mut label = match split {
            Some(_) => format!("{} ⫴", session.tab_title()),
            None => session.tab_title(),
        };
        // Agent status is the agent's own report, apart from shell activity.
        if let Some(agent) = session.agent_status() {
            label = format!("{label} · Agent {}", agent.label());
        }
        // The label names the program; the tooltip says where it runs.
        let location = tilde_path(&session.cwd());
        let id = tabs::session_id(session);
        let selected = i == state.active || split.is_some_and(|x| Some(x.second) == active_id);
        let shown = position;
        position += 1;
        // The label carries the activation, because interactive()'s drag and
        // middle handlers make it its own press target and would otherwise
        // swallow a press on the pill.
        let tab = tabs::interactive(
            terminal_tab(a, i, shown, count, &label, selected)
                .help(location)
                .on_press_tx(move |s: &mut State, tx| {
                    tx.blur();
                    s.dispatch(AppAction::ActivateTerminal(i), tx);
                }),
            tabs::Target::Terminal(id),
        );
        let close = icon_button(
            a,
            format!("terminal-close-{i}"),
            &rune_kit::glyphs::CLOSE,
            "End terminal session",
            move |s, tx| s.dispatch(AppAction::CloseTab(id), tx),
        )
        // Each close button names the tab it ends, since every tab has one
        // and a reader meets them out of the strip.
        .label(format!(
            "End terminal session: {label} (tab {} of {count})",
            shown + 1
        ))
        .description("")
        .w(22.0)
        .min_h(22.0)
        .fill(if selected { a.hover_bg() } else { a.panel() });
        list = list.child(
            tab_pill(
                a,
                format!("terminal-group-{id}"),
                "indicator-terminal-tabs",
                selected,
                tab,
                close,
            )
            .no_window_drag(),
        );
    }
    if state.preview.is_some() {
        list = list.child(
            icon_button(
                a,
                "preview-tab",
                &rune_kit::glyphs::TERMINAL,
                "Show terminal",
                |s, tx| s.dispatch(AppAction::ShowTerminal, tx),
            )
            .fill(a.hover_bg()),
        );
    }
    tabs = tabs
        .child(tabs::scroll_strip(list, state, true))
        .child(icon_button(
            a,
            "new-tab",
            &rune_kit::glyphs::PLUS,
            "New terminal tab",
            |s, tx| s.dispatch(AppAction::NewTerminal, tx),
        ));
    tabs
}

/// A terminal tab within the strip's tab list: a focusable `tab` that
/// reports whether it is the active session's and where it sits among the
/// tabs, and that Return, Space or an assistive press activates.
pub(crate) fn terminal_tab(
    a: &Appearance,
    i: usize,
    shown: usize,
    count: usize,
    label: &str,
    selected: bool,
) -> El {
    let key = format!("tab-{i}");
    pressable(&key);
    tab_label(a, label, selected)
        .key(key)
        .role("tab")
        .label(label)
        .focusable(true)
        .focus_selected(selected)
        .position_in_set(shown + 1, count)
        .focus_ring(2.0, 1.0, a.accent())
        .radius(6.0)
        .no_window_drag()
}
