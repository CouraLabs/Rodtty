//! Application-owned chrome. Host adapters opt in only when movement, sizing,
//! minimization and lifecycle have an implementation for their native platform.
use super::*;

pub(super) const BORDER: f32 = 6.0;

pub(super) fn enabled() -> bool {
    cfg!(target_os = "linux")
}

pub(super) fn options() -> rune::WindowOptions {
    rune::WindowOptions {
        borderless: enabled(),
        custom_frame_resize_border: if enabled() { BORDER } else { 0.0 },
        ..Default::default()
    }
}

pub(super) fn controls(state: &State, a: &Appearance) -> El {
    if !enabled() {
        return el();
    }
    row()
        .key("window-controls")
        .no_window_drag()
        .gap(2.0)
        .center_y()
        .child(
            button(a, "window-minimize", "−", |_, tx| tx.minimize())
                .w(32.0)
                .px(0.0)
                .label("Minimize window")
                .help("Minimize window"),
        )
        .child(
            button(
                a,
                "window-maximize",
                if state.window_maximized { "❐" } else { "□" },
                |_, tx| tx.toggle_maximized(),
            )
            .w(32.0)
            .px(0.0)
            .label(if state.window_maximized {
                "Restore window"
            } else {
                "Maximize window"
            })
            .help(if state.window_maximized {
                "Restore window"
            } else {
                "Maximize window"
            }),
        )
        .child(
            button(a, "window-close", "×", |s, tx| {
                s.dispatch(AppAction::CloseWindow, tx)
            })
            .w(32.0)
            .px(0.0)
            .label("Close window")
            .help("Close window"),
        )
}
