use crate::desktop::*;

thread_local! {
    /// Keys of the app's buttons, tabs and menu items, which Return presses
    /// as Space does. Rune follows AppKit, where Return goes on to the
    /// window's `on_key` rather than pressing the focused button; text
    /// fields never register, so Return keeps submitting them.
    static PRESSABLE: std::cell::RefCell<std::collections::HashSet<String>> =
        Default::default();
}

/// Let Return press the control keyed `key` while it has keyboard focus.
pub(crate) fn pressable(key: &str) {
    PRESSABLE.with(|keys| {
        if !keys.borrow().contains(key) {
            keys.borrow_mut().insert(key.to_owned());
        }
    });
}

pub(crate) fn presses_on_return(key: &str) -> bool {
    PRESSABLE.with(|keys| keys.borrow().contains(key))
}

/// An on/off control's state, for assistive technology (`checked`) and for
/// sight: an active control is tinted and outlined in the accent color, a
/// 1.5 pt ring that reads at 3:1 or more against the fills around it in
/// every appearance, so its state never rests on a subtle fill alone. An
/// inactive one keeps a clear ring of the same width, so toggling never
/// shifts its content.
pub(crate) trait ToggleState {
    fn toggle_state(self, a: &Appearance, on: bool) -> Self;
}
impl ToggleState for El {
    fn toggle_state(self, a: &Appearance, on: bool) -> Self {
        self.checked(on)
            .fill(if on {
                a.active_fill()
            } else {
                Color::TRANSPARENT
            })
            .border(
                TOGGLE_RING,
                if on { a.accent() } else { Color::TRANSPARENT },
            )
    }
}

/// The width of an active toggle's accent ring, in points.
pub(crate) const TOGGLE_RING: f32 = 1.5;

pub(crate) fn button(
    a: &Appearance,
    key: impl Into<String>,
    label: impl Into<String>,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let label = label.into();
    let key = key.into();
    let help = match key.as_str() {
        "settings" => "Open Settings",
        "workspace-menu" => "Workspace menu",
        "return-terminal" => "Return to terminal",
        "new-tab" => "New terminal tab",
        "explorer-refresh" => "Refresh files",
        "settings-close" => "Close Settings tab",
        "find-close" => "Close editor search",
        "close-search" => "Close terminal search",
        "diff-prev" => "Previous change",
        "diff-next" => "Next change",
        _ if key.starts_with("editor-close-") => "Close editor tab",
        _ => &label,
    };
    pressable(&key);
    el().key(key)
        .label(help)
        .help(help)
        .focusable(true)
        .no_window_drag()
        .focus_ring(2.0, 1.0, a.accent())
        .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
        .px(10.0)
        .center()
        .radius(5.0)
        .fill(a.panel())
        // A thin, dimmed outline marks a text button as pressable, so it
        // never reads as a plain label.
        .border(1.0, a.button_border())
        .hover(a.hover_bg())
        .press(a.pressed())
        .on_press_tx(move |state: &mut State, tx| {
            tx.blur();
            action(state, tx);
        })
        .child(
            text(label)
                .font(ui_label())
                .color(a.ink())
                .min_w(0.0)
                .truncate(),
        )
}

pub(crate) fn icon_button(
    a: &Appearance,
    key: impl Into<String>,
    glyph: &'static rune::Icon,
    tooltip: impl Into<String>,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let key = key.into();
    let tooltip = tooltip.into();
    pressable(&key);
    el().key(key)
        .label(tooltip.clone())
        .help(tooltip)
        .focusable(true)
        .no_window_drag()
        .focus_ring(2.0, 1.0, a.accent())
        .min_h(29.0_f32.max(ui_label().size * 1.5 + 8.0))
        .w(30.0)
        .px(0.0)
        .center()
        .radius(5.0)
        .fill(a.panel())
        .hover(a.hover_bg())
        .press(a.pressed())
        .on_press_tx(move |state: &mut State, tx| {
            tx.blur();
            action(state, tx);
        })
        .child(rune::icon(glyph, 16.0).color(a.ink()))
}
