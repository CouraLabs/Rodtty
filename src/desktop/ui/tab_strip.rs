use crate::desktop::*;

/// The active tab's accent bar along its pill's lower edge: the tab's
/// selection mark beyond its fill, at 3:1 or more against the tab fills.
/// One bar per tab strip (`strip`): when the active tab changes, Rune's
/// shared-element motion slides it from the old tab to the new one.
pub(crate) fn tab_indicator(a: &Appearance, strip: &'static str) -> El {
    el().key(strip)
        .pin(Some(8.0), None, Some(8.0), Some(0.0))
        .h(2.0)
        .radius(1.0)
        .fill(a.accent())
        .matched(strip)
        .motion(a.motion().spring())
        .enters_animated(a, Entrance::Fade)
}

/// A tab's label: the text, tinted for whether the tab is the active one and
/// sized to sit in the strip.
pub(crate) fn tab_label(a: &Appearance, label: &str, selected: bool) -> El {
    el().px(9.0).h(HEADER_TAB_HEIGHT).center().child(
        text(label)
            .font(ui_label())
            .color(if selected { a.ink() } else { a.muted() })
            .max_w(150.0)
            .truncate(),
    )
}

/// A tab's pill: it groups the tab and its close button for sight only
/// (`role: none`; the tab list holds the tabs), fills and marks the selected
/// one with `strip`'s indicator, and highlights on hover.
pub(crate) fn tab_pill(
    a: &Appearance,
    key: impl Into<String>,
    strip: &'static str,
    selected: bool,
    tab: El,
    close: El,
) -> El {
    row()
        .key(key)
        .role("none")
        .gap(0.0)
        .center_y()
        .radius(6.0)
        .fill(if selected { a.hover_bg() } else { a.panel() })
        .hover(a.hover_bg())
        .children(selected.then(|| tab_indicator(a, strip)))
        .child(tab)
        .child(close)
}
