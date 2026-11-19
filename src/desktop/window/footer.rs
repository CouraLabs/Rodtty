use crate::desktop::*;

/// A display path with the owner's home folder as `~`, so `/Users/rodcoura/…`
/// and `/home/…/…` both read as `~/…`. The owner's actual home folder, whatever
/// the host calls it, comes from the same source the app root does.
pub(crate) fn tilde_path(path: &std::path::Path) -> String {
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()) else {
        return path.display().to_string();
    };
    if path == home {
        return "~".into();
    }
    if let Some(rest) = path
        .strip_prefix(&home)
        .ok()
        .filter(|rest| !rest.as_os_str().is_empty())
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// The footer's status: the latest action's message, or the palette hint.
/// Each new message is its own element, so it enters as action feedback.
pub(crate) fn footer_message(state: &State, a: &Appearance) -> El {
    let message = if state.message.is_empty() {
        format!("{}  Actions", state.shortcut_label("command-palette"))
    } else {
        state.message.clone()
    };
    let generation = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        message.hash(&mut hasher);
        hasher.finish()
    };
    el().key(format!("footer-feedback-{generation:x}"))
        .min_w(0.0)
        .shrink()
        .enters(a, Entrance::Feedback)
        .child(
            text(message)
                .key("footer-message")
                .font(ui_label())
                .color(a.muted())
                .truncate()
                .min_w(0.0)
                .shrink(),
        )
}

/// The strip along the window's bottom: app identity, the active terminal's
/// folder and the latest action's message. The owner can hide it in Settings.
pub(crate) fn app_footer(state: &State, a: &Appearance, cwd: &std::path::Path) -> El {
    row()
        .key("app-footer")
        .w_full()
        // A one-point frame on every side keeps the footer off the window edge.
        .padding(1.0)
        .fill(a.bg())
        .child(
            row()
                .key("app-footer-content")
                .w_full()
                .h(25.0_f32.max(ui_label().size * 1.5 + 4.0))
                .px(10.0)
                .gap(10.0)
                .center_y()
                .fill(a.panel())
                .child(
                    text(concat!("Rodtty ", env!("CARGO_PKG_VERSION")))
                        .key("footer-app-name")
                        .font(ui_style(13.0, 700.0))
                        .color(a.ink())
                        .shrink(),
                )
                .child(
                    text(tilde_path(cwd))
                        .key("footer-path")
                        .font(ui_label())
                        .color(a.muted())
                        .truncate()
                        .min_w(0.0)
                        .grow(1.0),
                )
                .child(footer_message(state, a)),
        )
}
