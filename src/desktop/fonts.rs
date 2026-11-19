//! Typography shared by every view; terminal geometry is measured from its drawn face.
use super::*;
use crate::config::FontPreferences;
use std::cell::Cell;

pub(super) const BUNDLED: &[&[u8]] = &[
    asset_bytes!("fonts/Geist-Regular.ttf"),
    asset_bytes!("fonts/Geist-Medium.ttf"),
    asset_bytes!("fonts/Geist-SemiBold.ttf"),
    asset_bytes!("fonts/Geist-Italic.ttf"),
    asset_bytes!("fonts/Geist-SemiBoldItalic.ttf"),
    asset_bytes!("fonts/GeistMono-Regular.ttf"),
    asset_bytes!("fonts/GeistMono-Medium.ttf"),
    asset_bytes!("fonts/GeistMono-SemiBold.ttf"),
    asset_bytes!("fonts/GeistMono-Italic.ttf"),
    asset_bytes!("fonts/GeistMono-SemiBoldItalic.ttf"),
];

#[derive(Clone, Copy)]
struct Typography {
    ui: f32,
    terminal: f32,
    editor: f32,
    terminal_line_height: f32,
    terminal_letter_spacing: f32,
    scale: f32,
}
thread_local! {
    static CURRENT: Cell<Typography> = const { Cell::new(Typography { ui: 13.0, terminal: 14.0, editor: 14.0, terminal_line_height: 1.2, terminal_letter_spacing: 0.0, scale: 1.0 }) };
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
        terminal_line_height: p.terminal_line_height,
        terminal_letter_spacing: p.terminal_letter_spacing,
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
    /// Points of letter spacing included in `width`; each glyph sits centered
    /// in the extra space.
    pub spacing: f32,
    line_height: f32,
    scale: f32,
    epoch: u64,
}
impl Metrics {
    pub fn measure() -> Self {
        let t = CURRENT.get();
        let style = TextStyle::new(t.terminal, 400.0)
            .family(Family::Mono)
            .lh(t.terminal * t.terminal_line_height);
        let snap = |v: f32| (v * t.scale).round().max(1.0) / t.scale;
        let width =
            snap(rune::text::advance("MMMMMMMMMM", style) / 10.0 + t.terminal_letter_spacing);
        let height = snap(rune::text::measure("M", &[], style, None).1);
        Self {
            width,
            height,
            style: style.lh(height),
            spacing: t.terminal_letter_spacing,
            line_height: t.terminal_line_height,
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
        let fonts = &self.config.fonts;
        if previous.style.size == fonts.terminal_size
            && previous.line_height == fonts.terminal_line_height
            && previous.spacing == fonts.terminal_letter_spacing
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
            spacing: 0.0,
            line_height: 1.2,
            scale: 1.0,
            epoch: 0,
        }
    }
}

#[cfg(test)]
mod tests;

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
    // Return opens the picker's trigger as Space and the arrows do.
    super::pressable(&format!("{key}-value-trigger"));
    rune_kit::Combobox::new(key, state, &options, move |s: &mut State| {
        s.theme_family_picker.close();
        s.settings_select = None;
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
    .width(240.0)
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
/// The UI or terminal font family picker, its list drawn into `layers`.
pub(super) fn family_picker(s: &State, a: &Appearance, terminal: bool, layers: &mut Vec<El>) -> El {
    dropdown(s, a, terminal, layers).shrink().min_w(0.0)
}

/// A sample in the chosen family, and why it is not in use when it is missing.
pub(super) fn family_notes(s: &State, a: &Appearance, terminal: bool) -> El {
    let key = if terminal { "terminal" } else { "ui" };
    let current = if terminal {
        &s.config.fonts.terminal_family
    } else {
        &s.config.fonts.ui_family
    };
    let mut result = col().w_full().gap(4.0).child(
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
    if !s.font_catalog.ready {
        result = result.child(
            text("Finding installed font families…")
                .font(label())
                .color(a.muted()),
        );
    }
    result
}
