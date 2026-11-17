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
    pub(super) fn font_families(&self) -> (String, String) {
        let mut preferences = self.config.fonts.clone();
        if self.font_catalog.ready {
            if !self
                .font_catalog
                .families
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(&preferences.ui_family))
            {
                preferences.ui_family = "Geist".into();
            }
            if !self
                .font_catalog
                .families
                .iter()
                .any(|f| f.monospace && f.name.eq_ignore_ascii_case(&preferences.terminal_family))
            {
                preferences.terminal_family = "Geist Mono".into();
            }
        }
        preferences.families()
    }
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
    fn owner_searches_and_selects_ui_and_monospace_terminal_families() {
        let _families = rune::text::with_families("Geist", "Geist Mono");
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.open_settings(&mut Tx::default());
        state.settings_query.replace_text("font family");
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
        click(&mut rt, "font-ui-trigger");
        assert!(rt.find("font-ui-option-Geist").is_some());
        assert!(rt.find("font-ui-option-Geist Mono").is_some());
        click(&mut rt, "font-ui-option-Geist Mono");
        assert_eq!(
            crate::config::Config::load(root.path())
                .unwrap()
                .fonts
                .ui_family,
            "Geist Mono"
        );
        click(&mut rt, "font-terminal-trigger");
        assert!(rt.find("font-terminal-option-Geist").is_none());
        assert!(rt.find("font-terminal-option-Geist Mono").is_some());
        click(&mut rt, "font-terminal-option-Geist Mono");
        click(&mut rt, "reset-ui-font-family");
        assert_eq!(
            crate::config::Config::load(root.path())
                .unwrap()
                .fonts
                .ui_family,
            "Geist"
        );
        click(&mut rt, "font-ui-trigger");
        rt.set_viewport((640.0, 900.0), 1.0);
        frame(&mut rt);
        assert!(rt.find("font-ui-option-Geist").is_some());
        click(&mut rt, "workspace-menu");
        assert!(
            rt.find("font-ui-option-Geist").is_none(),
            "pressing outside the font dropdown must dismiss it"
        );
        click(&mut rt, "workspace-menu");
        assert!(rt.find("workspace-menu-popover").is_some());
        assert!(
            rt.find("font-ui-option-Geist").is_none(),
            "the workspace menu must suppress an already open font dropdown"
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Default::default(),
        });
        frame(&mut rt);
        click(&mut rt, "font-ui-trigger");
        rt.update(|s, tx| s.dispatch(AppAction::ShowTerminal, tx));
        frame(&mut rt);
        assert!(
            rt.find("font-ui-option-Geist").is_none(),
            "a hidden compact settings pane must not retain its dropdown overlay"
        );
    }
    #[test]
    #[ignore = "requires a native desktop host with installed proportional and monospace fonts"]
    fn owner_finds_installed_fonts_by_keyboard_and_keeps_editor_size() {
        let _families = rune::text::with_families("Geist", "Geist Mono");
        let families = Catalog::discover();
        let installed_mono = families
            .iter()
            .find(|f| !f.bundled && f.monospace)
            .expect("native host has a monospace family")
            .name
            .clone();
        assert!(families.iter().any(|f| !f.bundled && !f.monospace));
        eprintln!(
            "native font inventory: {} families; {} monospace; selecting {}",
            families.len(),
            families.iter().filter(|f| f.monospace).count(),
            installed_mono
        );
        let root = tempfile::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        workspace_preview(&mut state);
        state.save_config(|c| c.fonts.editor_size = 22.0);
        state.open_settings(&mut Tx::default());
        state.settings_query.replace_text("font family");
        state.font_catalog.families = families;
        state.font_catalog.ready = true;
        let mut rt = Runtime::with_text(
            state,
            view,
            (1600.0, 1000.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
            rune::text::TextSystem::new(BUNDLED, ("Geist", "Geist Mono")),
        );
        frame(&mut rt);
        click(&mut rt, "font-terminal-trigger");
        rt.paste(installed_mono.clone());
        frame(&mut rt);
        assert!(
            rt.find(&format!("font-terminal-option-{installed_mono}"))
                .is_some()
        );
        assert!(
            rt.find("font-terminal-option-Geist Mono").is_none(),
            "search filters unrelated families"
        );
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Default::default(),
        });
        frame(&mut rt);
        let saved = Config::load(root.path()).unwrap();
        assert_eq!(saved.fonts.terminal_family, installed_mono);
        assert_eq!(saved.fonts.editor_size, 22.0);
        assert!(
            rt.font_families()
                .1
                .starts_with(&format!("\"{installed_mono}\""))
        );
        click(&mut rt, "reset-terminal-font-family");
        assert!(rt.font_families().1.starts_with("\"Geist Mono\""));
        assert_eq!(Config::load(root.path()).unwrap().fonts.editor_size, 22.0);
    }
    #[test]
    fn disappeared_saved_families_show_the_bundled_fallback_without_rewriting_preferences() {
        let _families = rune::text::with_families("Geist", "Geist Mono");
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.fonts.ui_family = "Rodtty Vanished UI 1229".into();
        config.fonts.terminal_family = "Rodtty Vanished Mono 1229".into();
        config.save(root.path()).unwrap();
        let mut state = State::with_root(root.path().into());
        state.font_catalog.ready = true;
        state.open_settings(&mut Tx::default());
        state.settings_query.replace_text("font family");
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
        rt.update(|s, tx| {
            poll(s, 0.0, tx);
        });
        frame(&mut rt);
        assert_eq!(
            rt.font_families(),
            ("\"Geist\", \"Geist\"", "\"Geist Mono\", \"Geist Mono\"")
        );
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.text.as_ref().is_some_and(|text| text.contains(
                    "Rodtty Vanished Mono 1229 is unavailable; using bundled Geist Mono"
                )))
        );
        assert_eq!(
            Config::load(root.path()).unwrap().fonts.terminal_family,
            "Rodtty Vanished Mono 1229"
        );
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

// Discovery owns native font collections on a worker: Fontconfig on Linux,
// DirectWrite on Windows and CoreText on macOS. Render/input only read this snapshot.
#[derive(Clone)]
pub(super) struct FontFamily {
    pub name: String,
    pub monospace: bool,
    pub bundled: bool,
}
pub(super) struct Catalog {
    pub families: Vec<FontFamily>,
    pending: Option<std::sync::mpsc::Receiver<Vec<FontFamily>>>,
    pub ready: bool,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            families: vec![
                FontFamily {
                    name: "Geist".into(),
                    monospace: false,
                    bundled: true,
                },
                FontFamily {
                    name: "Geist Mono".into(),
                    monospace: true,
                    bundled: true,
                },
            ],
            pending: None,
            ready: false,
        }
    }
}
impl Catalog {
    pub(super) fn start(&mut self, wake: rune::Waker) {
        if self.ready || self.pending.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(Self::discover());
            wake();
        });
    }
    pub(super) fn poll(&mut self) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        match rx.try_recv() {
            Ok(families) => {
                self.families = families;
                self.ready = true;
                self.pending = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ready = true;
                self.pending = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
        }
    }
    pub fn discover() -> Vec<FontFamily> {
        let mut families = Self::default().families;
        let mut collection = fontique::Collection::new(Default::default());
        collection.load_system_fonts();
        let names: Vec<_> = collection.family_names().map(str::to_owned).collect();
        let mut cache = fontique::SourceCache::new(Default::default());
        for name in names {
            if families.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let Some(family) = collection.family_by_name(&name) else {
                continue;
            };
            let fonts = family.fonts();
            let monospace = !fonts.is_empty()
                && fonts.iter().all(|font| {
                    font.load(Some(&mut cache))
                        .is_some_and(|data| fixed_pitch(data.as_ref(), font.index()))
                });
            families.push(FontFamily {
                name,
                monospace,
                bundled: false,
            });
        }
        families.sort_by_cached_key(|f| f.name.to_lowercase());
        families.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        families
    }
}
fn fixed_pitch(data: &[u8], index: u32) -> bool {
    use read_fonts::{FontRef, TableProvider};
    let Ok(font) = FontRef::from_index(data, index) else {
        return false;
    };
    // Validate actual printable ASCII advances as well as the post metadata.
    // This excludes icon faces and proportional fonts with misleading names/flags.
    let Ok(post) = font.post() else { return false };
    if post.is_fixed_pitch() == 0 {
        return false;
    }
    let (Ok(cmap), Ok(hmtx)) = (font.cmap(), font.hmtx()) else {
        return false;
    };
    let mut width = None;
    for ch in ' '..='~' {
        let Some(glyph) = cmap.map_codepoint(ch) else {
            return false;
        };
        let Some(advance) = hmtx.advance(glyph).filter(|a| *a > 0) else {
            return false;
        };
        if width.is_some_and(|w| w != advance) {
            return false;
        }
        width = Some(advance);
    }
    true
}
fn dropdown(s: &State, a: &Appearance, terminal: bool, layers: &mut Vec<El>) -> El {
    let options: Vec<_> = s
        .font_catalog
        .families
        .iter()
        .filter(|f| !terminal || f.monospace)
        .map(|f| rune_kit::ComboboxOption {
            id: f.name.clone(),
            label: f.name.clone(),
            detail: if f.bundled { "Bundled" } else { "Installed" }.into(),
            disabled: false,
        })
        .collect();
    let (key, state, current, label) = if terminal {
        (
            "font-terminal",
            &s.font_terminal,
            &s.config.fonts.terminal_family,
            "Terminal font family",
        )
    } else {
        (
            "font-ui",
            &s.font_ui,
            &s.config.fonts.ui_family,
            "UI font family",
        )
    };
    rune_kit::Combobox::new(key, state, &options, move |s: &mut State| {
        if terminal {
            &mut s.font_terminal
        } else {
            &mut s.font_ui
        }
    })
    .label(label)
    .current(Some(current))
    .placeholder(current)
    .search_placeholder("Search font families")
    .width(260.0)
    .max_height(190.0)
    .on_choose(move |s, family, tx| {
        s.save_config(|c| {
            if terminal {
                c.fonts.terminal_family = family.into();
            } else {
                c.fonts.ui_family = family.into();
            }
        });
        let (sans, mono) = s.font_families();
        tx.set_font_families(sans, mono);
    })
    .build(a.kit(), layers)
}
pub(super) fn selectors(s: &State, a: &Appearance) -> El {
    let mut result = col().w_full().gap(6.0);
    for terminal in [false, true] {
        let key = if terminal { "terminal" } else { "ui" };
        let current = if terminal {
            &s.config.fonts.terminal_family
        } else {
            &s.config.fonts.ui_family
        };
        result = result
            .child(
                text(if terminal {
                    "Terminal font family · editor follows"
                } else {
                    "UI font family"
                })
                .font(label())
                .color(a.ink()),
            )
            .child(
                row()
                    .gap(6.0)
                    .scroll_x()
                    .child(dropdown(s, a, terminal, &mut Vec::new()))
                    .child(button(
                        a,
                        format!("reset-{key}-font-family"),
                        "Reset",
                        move |s, tx| {
                            s.save_config(|c| {
                                if terminal {
                                    c.fonts.terminal_family = "Geist Mono".into();
                                } else {
                                    c.fonts.ui_family = "Geist".into();
                                }
                            });
                            let (sans, mono) = s.font_families();
                            tx.set_font_families(sans, mono);
                        },
                    )),
            )
            .child(
                text("Aa Bb 0123456789 · Wi il {} =>")
                    .key(format!("font-{key}-preview"))
                    .font(if terminal { editor() } else { ui(15.0, 400.0) })
                    .color(a.ink())
                    .wrap(),
            );
        if s.font_catalog.ready
            && !s
                .font_catalog
                .families
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(current) && (!terminal || f.monospace))
        {
            result = result.child(
                text(format!(
                    "{current} is unavailable; using bundled {}",
                    if terminal { "Geist Mono" } else { "Geist" }
                ))
                .font(label())
                .color(a.muted())
                .wrap(),
            );
        }
    }
    if !s.font_catalog.ready {
        result = result.child(
            text("Finding installed font families…")
                .font(label())
                .color(a.muted()),
        );
    }
    result
}
pub(super) fn overlays(s: &State, a: &Appearance, width: f32) -> Vec<El> {
    let mut layers = Vec::new();
    if s.settings
        && s.pending.is_none()
        && s.palette.is_none()
        && workspace::pane_visibility(s, width).2
        && "appearance ui font family terminal font family installed monospace preview"
            .contains(&s.settings_query.text.trim().to_lowercase())
    {
        for terminal in [false, true] {
            let _ = dropdown(s, a, terminal, &mut layers);
        }
    }
    layers
}
