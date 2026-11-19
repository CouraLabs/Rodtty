//! Application-owned chrome. Host adapters opt in only when movement, sizing,
//! minimization and lifecycle have an implementation for their native platform.
//!
//! macOS keeps its native traffic lights over a full-size content view: the
//! window stays decorated, so the OS owns movement, sizing and minimize, and
//! the app only reserves [`ChromeInsets`](rune::ChromeInsets) at the leading
//! edge. Linux and Windows draw their own controls because neither has a
//! native frame Rodtty can style.
use crate::desktop::*;

pub(crate) const BORDER: f32 = 6.0;

/// The band the header occupies, in points. Also the height the host is asked
/// to centre its controls in on macOS.
pub(crate) const HEADER: f32 = 44.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// The host draws the controls; the app keeps clear of `chrome_insets()`.
    Hosted,
    /// The app draws the controls at the trailing edge.
    Trailing,
    /// No app chrome: the host's own decorations stay.
    Native,
}

pub(crate) fn placement() -> Placement {
    if cfg!(target_os = "macos") {
        Placement::Hosted
    } else if cfg!(any(target_os = "linux", target_os = "windows")) {
        Placement::Trailing
    } else {
        Placement::Native
    }
}

/// Whether the app draws its own controls.
pub(crate) fn controls_shown(placement: Placement) -> bool {
    placement == Placement::Trailing
}

/// Whether the window gives up its native frame for a themed six-point border.
/// Only the platforms that draw their own controls do: they have no native
/// titlebar left to keep, so the app supplies the whole frame.
pub(crate) fn borderless() -> bool {
    placement() == Placement::Trailing
}

pub(crate) fn options() -> rune::WindowOptions {
    rune::WindowOptions {
        borderless: borderless(),
        custom_frame_resize_border: if borderless() { BORDER } else { 0.0 },
        ..saved_placement()
    }
}

/// The window's saved place on the desktop, as `WindowOptions` takes it.
///
/// The layout is read here rather than through `State` because the host
/// creates the window before any app state exists. A record that no longer
/// lands on the display it was saved on is dropped, along with its size: the
/// host then places the window as it would on a first run, instead of
/// reopening it off-screen after a monitor change.
fn saved_placement() -> rune::WindowOptions {
    let saved = crate::config::app_root()
        .ok()
        .and_then(|root| crate::work::Layout::load(&root).ok())
        .and_then(|layout| layout.window)
        .and_then(crate::work::WindowGeometry::sanitized)
        .filter(|geometry| geometry.origin_on_display());
    match saved {
        Some(geometry) => rune::WindowOptions {
            position: Some((geometry.x, geometry.y)),
            physical_size: Some((geometry.width, geometry.height)),
            ..Default::default()
        },
        None => rune::WindowOptions::default(),
    }
}

/// The titlebar the host draws for us. Only macOS gets one: it keeps the
/// traffic lights over a full-size content view, and reports their width as
/// `Cx::chrome_insets().leading`. `None` elsewhere, which leaves the native
/// frame in place.
pub(crate) fn titlebar() -> Option<rune::Titlebar> {
    match placement() {
        Placement::Hosted => Some(rune::Titlebar::new(HEADER)),
        _ => None,
    }
}

pub(crate) fn controls(state: &State, a: &Appearance, placement: Placement) -> El {
    if !controls_shown(placement) {
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
    row()
        .key("window-controls")
        .no_window_drag()
        .gap(2.0)
        .center_y()
        .child(minimize)
        .child(maximize)
        .child(close)
}
