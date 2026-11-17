//! Typography shared by every view; terminal geometry is measured from its drawn face.
use super::*;
use crate::config::FontPreferences;
use std::cell::Cell;

pub(super) const BUNDLED: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../../assets/fonts/Geist-Medium.ttf"),
    include_bytes!("../../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Geist-Italic.ttf"),
    include_bytes!("../../assets/fonts/Geist-SemiBoldItalic.ttf"),
    include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../../assets/fonts/GeistMono-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/GeistMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/GeistMono-SemiBoldItalic.ttf"),
];

#[derive(Clone, Copy)]
struct Typography {
    ui: f32,
    terminal: f32,
    editor: f32,
    scale: f32,
}
thread_local! {
    static CURRENT: Cell<Typography> = const { Cell::new(Typography { ui: 13.0, terminal: 14.0, editor: 14.0, scale: 1.0 }) };
}
// View construction is synchronous on one thread. Restore the outer scope for nested
// views, headless runtimes and unwinding, without global cross-window preferences.
pub(super) struct Scope(Typography);
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.set(self.0);
    }
}
pub(super) fn scope(p: &FontPreferences, scale: f32) -> Scope {
    Scope(CURRENT.replace(Typography {
        ui: p.ui_size,
        terminal: p.terminal_size,
        editor: p.editor_size,
        scale,
    }))
}
pub(super) fn ui(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size * CURRENT.get().ui / 13.0, weight)
}
pub(super) fn label() -> TextStyle {
    ui(13.0, 500.0)
}
pub(super) fn editor() -> TextStyle {
    let size = CURRENT.get().editor;
    TextStyle::new(size, 400.0)
        .family(Family::Mono)
        .lh(size * 1.5)
}

// Rune Kit diff/editor widgets must use the same owner sizes as app-drawn text.
pub(super) fn kit(mut kit: rune_kit::Kit) -> rune_kit::Kit {
    kit.ty.ui = ui(13.0, 400.0);
    kit.ty.ui_medium = ui(13.0, 500.0);
    kit.ty.ui_compact = ui(13.0, 400.0);
    kit.ty.small = ui(11.0, 400.0);
    kit.ty.small_medium = ui(11.0, 500.0);
    kit.ty.body = ui(14.0, 400.0);
    kit.ty.body_medium = ui(14.0, 500.0);
    kit.ty.text_small = ui(13.0, 400.0);
    kit.ty.text_small_medium = ui(13.0, 500.0);
    kit.ty.text_xsmall = ui(12.0, 400.0);
    kit.ty.text_xsmall_medium = ui(12.0, 500.0);
    kit.ty.mono = editor();
    kit
}

#[derive(Clone, Copy)]
pub(super) struct Metrics {
    pub width: f32,
    pub height: f32,
    pub style: TextStyle,
    scale: f32,
    epoch: u64,
}
impl Metrics {
    pub fn measure() -> Self {
        let t = CURRENT.get();
        let style = TextStyle::new(t.terminal, 400.0)
            .family(Family::Mono)
            .lh(t.terminal * 1.5);
        let snap = |v: f32| (v * t.scale).round().max(1.0) / t.scale;
        let width = snap(rune::text::advance("MMMMMMMMMM", style) / 10.0);
        let height = snap(rune::text::measure("M", &[], style, None).1);
        Self {
            width,
            height,
            style: style.lh(height),
            scale: t.scale,
            epoch: rune::text::font_epoch(),
        }
    }
    pub fn dimensions(self, rect: [f32; 4]) -> (u16, u16) {
        (
            (rect[3] / self.height).floor().max(2.0) as u16,
            (rect[2] / self.width).floor().max(10.0) as u16,
        )
    }
}

impl State {
    pub(super) fn refresh_terminal_metrics(&mut self) -> bool {
        let previous = self.terminal_metrics;
        if previous.style.size == self.config.fonts.terminal_size
            && previous.epoch == rune::text::font_epoch()
        {
            return false;
        }
        let _scope = scope(&self.config.fonts, previous.scale);
        self.resize_terminal(Metrics::measure());
        true
    }
    pub(super) fn resize_terminal(&mut self, metrics: Metrics) {
        self.terminal_metrics = metrics;
        if let Some(rect) = self.terminal_rect {
            let (rows, cols) = metrics.dimensions(rect);
            if let Some(session) = self.sessions.get_mut(self.active)
                && let Err(error) = session.resize(rows, cols)
            {
                self.message = error.to_string();
            }
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            style: TextStyle::new(14.0, 400.0).family(Family::Mono),
            scale: 1.0,
            epoch: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn rect(rt: &Runtime<State>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("missing {key}"))
            .rect
    }
    fn frame(rt: &mut Runtime<State>) {
        for _ in 0..4 {
            rt.frame(1.0 / 60.0);
        }
    }
    fn click(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        frame(rt);
    }
    #[test]
    fn terminal_unicode_cursor_and_selection_follow_live_sizes_and_display_scale() {
        let _families = rune::text::with_families("Geist", "Geist Mono");
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        let mut parser = vt100::Parser::new(24, 80, 100);
        parser.process("Wi日本語🌿e\u{301}Z".as_bytes());
        state.preview = Some(parser.screen().clone());
        let mut rt = Runtime::with_text(
            state,
            view,
            (1200.0, 900.0),
            Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        frame(&mut rt);
        let grid = rect(&rt, "terminal");
        // Geist Mono's hmtx advance is 600 / 1000 em. At 14px the cell
        // rounds to 8 device pixels; wide/combining glyphs keep VT cell indices.
        assert_eq!(rect(&rt, "terminal-cell-0-11")[0] - grid[0], 88.0);
        assert_eq!(rect(&rt, "terminal-caret")[0] - grid[0], 96.0);
        click(&mut rt, "settings");
        rt.update(|s, _| s.settings_query.replace_text("font size"));
        frame(&mut rt);
        for _ in 0..6 {
            click(&mut rt, "more-terminal-font-size");
        }
        let grid = rect(&rt, "terminal");
        assert_eq!(rect(&rt, "terminal-cell-0-11")[0] - grid[0], 132.0);
        assert_eq!(rect(&rt, "terminal-caret")[0] - grid[0], 144.0);
        rt.pointer_down(grid[0] + 1.0, grid[1] + 8.0);
        rt.pointer_move(grid[0] + 145.0, grid[1] + 8.0);
        rt.pointer_up(grid[0] + 145.0, grid[1] + 8.0);
        frame(&mut rt);
        rt.update(|s, tx| {
            on_key(
                s,
                &KeyEvent {
                    key: Key::Command('c'),
                    mods: rune::Mods {
                        cmd: true,
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                },
                tx,
            );
        });
        assert_eq!(
            rt.copy_request.take().unwrap().content().text(),
            "Wi日本語🌿e\u{301}Z"
        );
        click(&mut rt, "reset-terminal-font-size");
        rt.set_viewport((1200.0, 900.0), 1.5);
        frame(&mut rt);
        let grid = rect(&rt, "terminal");
        // 8.4 logical px rounds to 13 physical px at scale 1.5.
        assert!((rect(&rt, "terminal-caret")[0] - grid[0] - 104.0).abs() < 0.02);
    }
    #[test]
    fn external_font_preferences_apply_with_bundled_fallback_and_keep_unsaved_editor_work() {
        use std::time::{Duration, Instant};
        let _families = rune::text::with_families("Geist", "Geist Mono");
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        workspace_preview(&mut state);
        let text = state.workspace.tabs[0].edit.text.clone();
        let mut rt = Runtime::with_text(
            state,
            view,
            (1600.0, 1000.0),
            Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        frame(&mut rt);
        let before = rect(&rt, "file-editor-1")[3];
        let tree_label_height = |rt: &Runtime<State>| {
            rt.elements()
                .into_iter()
                .find(|e| e.text.as_deref() == Some("Cargo.toml"))
                .expect("visible explorer label")
                .rect[3]
        };
        let tree_before = tree_label_height(&rt);
        std::fs::write(root.path().join("settings.json"), r#"{"fonts":{"ui_family":"Rodtty Missing UI Face 1827","terminal_family":"Rodtty Missing Mono Face 1827","ui_size":16,"terminal_size":18,"editor_size":22}}"#).unwrap();
        rt.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        frame(&mut rt);
        assert!(rt.font_families().0.ends_with("\"Geist\""));
        assert!(rt.font_families().1.ends_with("\"Geist Mono\""));
        assert!(rect(&rt, "file-editor-1")[3] > before);
        assert!(
            tree_label_height(&rt) > tree_before,
            "explorer labels follow UI sizing"
        );
        assert_eq!(
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("file-editor-1"))
                .unwrap()
                .text
                .as_deref(),
            Some(text.as_str())
        );
        let before = rect(&rt, "terminal-caret");
        std::fs::write(
            root.path().join("settings.json"),
            r#"{"fonts":{"terminal_size":0}}"#,
        )
        .unwrap();
        rt.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        frame(&mut rt);
        assert_eq!(rect(&rt, "terminal-caret"), before);
        assert!(rt.elements().iter().any(|e| {
            e.text
                .as_ref()
                .is_some_and(|t| t.contains("font size must be between"))
        }));
    }
    #[test]
    fn git_diff_code_follows_the_live_editor_size() {
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        diff_preview(&mut state);
        let mut rt = Runtime::with_text(
            state,
            view,
            (1440.0, 860.0),
            Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        frame(&mut rt);
        let code_height = |rt: &Runtime<State>| {
            rt.elements()
                .into_iter()
                .find(|e| e.path.contains("git-diff/") && e.text.as_deref() == Some("fn main() {"))
                .expect("rendered diff code")
                .rect[3]
        };
        let before = code_height(&rt);
        std::fs::write(
            root.path().join("settings.json"),
            r#"{"fonts":{"editor_size":22}}"#,
        )
        .unwrap();
        rt.update(|s, tx| {
            s.settings_poll = std::time::Instant::now() - std::time::Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        frame(&mut rt);
        assert!(
            code_height(&rt) > before * 1.4,
            "Git diff code must reflect editor sizing"
        );
        let code = rt
            .elements()
            .into_iter()
            .find(|e| e.path.contains("git-diff/") && e.text.as_deref() == Some("fn main() {"))
            .unwrap()
            .rect;
        rt.pointer_move(code[0] + 0.1, code[1] + code[3] / 2.0);
        rt.pointer_down(code[0] + 0.1, code[1] + code[3] / 2.0);
        frame(&mut rt);
        rt.pointer_move(code[0] + 26.5, code[1] + code[3] / 2.0);
        frame(&mut rt);
        rt.pointer_up(code[0] + 26.5, code[1] + code[3] / 2.0);
        frame(&mut rt);
        assert_eq!(rt.selected_text().as_deref(), Some("fn"));
    }
}
