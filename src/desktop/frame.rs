//! Application-owned chrome. Host adapters opt in only when movement, sizing,
//! minimization and lifecycle have an implementation for their native platform.
use super::*;

pub(super) const BORDER: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    Native,
    Left,
    Right,
}

pub(super) fn placement() -> Placement {
    if cfg!(target_os = "macos") {
        Placement::Left
    } else if cfg!(any(target_os = "linux", target_os = "windows")) {
        Placement::Right
    } else {
        Placement::Native
    }
}

pub(super) fn enabled() -> bool {
    placement() != Placement::Native
}

pub(super) fn options() -> rune::WindowOptions {
    rune::WindowOptions {
        borderless: enabled(),
        custom_frame_resize_border: if enabled() { BORDER } else { 0.0 },
        ..Default::default()
    }
}

pub(super) fn controls(state: &State, a: &Appearance, placement: Placement) -> El {
    if placement == Placement::Native {
        return el();
    }
    let minimize = button(a, "window-minimize", "−", |_, tx| tx.minimize())
        .w(32.0)
        .px(0.0)
        .label("Minimize window")
        .help("Minimize window");
    let maximize = button(
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
    });
    let close = button(a, "window-close", "×", |s, tx| {
        s.dispatch(AppAction::CloseWindow, tx)
    })
    .w(32.0)
    .px(0.0)
    .label("Close window")
    .help("Close window");
    let controls = row()
        .key("window-controls")
        .no_window_drag()
        .gap(2.0)
        .center_y();
    if placement == Placement::Left {
        controls.child(close).child(minimize).child(maximize)
    } else {
        controls.child(minimize).child(maximize).child(close)
    }
}
