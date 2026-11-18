//! One resolved appearance for chrome, editing, diff inspection, and terminal rendering.
//! Families supply data; callers consume semantic roles rather than palette names.
use super::terminal_palette::{TerminalPalette, over};
use crate::config::{Appearance as Mode, ThemeFamily};
use rune::{Color, HelpLook};
use rune_kit::{
    Kit, StateColors,
    syntax::{Syntax, SyntaxTheme},
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
struct Palette {
    name: String,
    colors: BTreeMap<String, String>,
    ansi: [String; 16],
    #[serde(default)]
    syntax: BTreeMap<String, String>,
    #[serde(default)]
    selection: Option<String>,
    #[serde(default)]
    attribution: String,
}

pub(super) struct Appearance {
    pub dark: bool,
    active_alpha: f32,
    pub name: &'static str,
    pub notes: &'static str,
    kit: Kit,
    terminal: TerminalPalette,
}
impl Appearance {
    pub fn resolve(family: ThemeFamily, mode: Mode, system_dark: bool) -> Self {
        let dark = mode.dark(system_dark);
        static CATPPUCCIN: OnceLock<BTreeMap<String, Palette>> = OnceLock::new();
        static FAMILIES: OnceLock<BTreeMap<String, Palette>> = OnceLock::new();
        let p = if family == ThemeFamily::Catppuccin {
            let palettes = CATPPUCCIN.get_or_init(|| {
                serde_json::from_str(include_str!("../../assets/themes/catppuccin.json"))
                    .expect("bundled Catppuccin")
            });
            &palettes[if dark { "mocha" } else { "latte" }]
        } else {
            let palettes = FAMILIES.get_or_init(|| {
                serde_json::from_str(include_str!("../../assets/themes/families.json"))
                    .expect("bundled appearance palettes")
            });
            &palettes[&format!("{}-{}", family.key(), if dark { "dark" } else { "light" })]
        };
        let c = |name: &str| rune::theme::hex(&p.colors[name]).expect("bundled palette color");
        let mut kit = Kit::neutral(dark);
        let k = &mut kit.c;
        let base = c("base");
        let panel = c("mantle");
        let raised = c("surface0");
        let syntax_source = |role: &str, fallback: &str| {
            p.syntax.get(role).map_or_else(
                || c(fallback),
                |s| rune::theme::hex(s).expect("bundled syntax color"),
            )
        };
        // Body text reads at 4.5:1 on every surface it sits on: the canvas,
        // the panels, and the raised tab and hover fills. A family whose own
        // text falls short there (Solarized) moves toward black or white.
        let extreme = if dark { Color::WHITE } else { Color::BLACK };
        let surfaces = [base, panel, raised];
        let ink = reach(c("text"), &surfaces, 4.5, extreme);
        // Diff lines tint the canvas toward the inserted and deleted hues;
        // body text keeps 4.5:1 on those tints as well.
        let inserted = readable(syntax_source("inserted", "green"), base, ink);
        let deleted = readable(syntax_source("deleted", "red"), base, ink);
        let text = reach(
            ink,
            &[
                diff_line(base, inserted, dark),
                diff_line(base, deleted, dark),
            ],
            4.5,
            extreme,
        );
        let muted = reach(c("subtext0"), &surfaces, 4.5, text);
        let focus = reach(c("blue"), &surfaces, 4.5, text);
        k.page = base;
        k.bg_base = base;
        k.bg_deep = c("crust");
        k.layer_01 = panel;
        k.layer_02 = raised;
        k.layer_03 = c("surface1");
        k.layer_04 = c("surface2");
        k.bg_inverse = text;
        k.bg_contrast = text;
        k.bg_button_neutral = panel;
        k.bg_icon_button_contrast = text;
        k.bg_accent = focus;
        k.submit_fg = base;
        k.submit_icon = base;
        k.text_base = text;
        k.text_strong = text;
        k.text_weak = muted;
        k.text_muted = muted;
        k.text_faint = reach(c("subtext1"), &surfaces, 4.5, text);
        k.text_inverse = base;
        k.text_contrast = text;
        k.text_accent = focus;
        k.text_invert_strong = base;
        k.icon_base = text;
        k.icon_muted = muted;
        k.icon_faint = muted;
        k.icon_weaker = muted;
        k.icon_inverse = base;
        k.icon_contrast = base;
        k.icon_accent = focus;
        k.border_muted = raised;
        k.border_base = c("surface1");
        k.border_strong = c("surface2");
        k.border_focus = focus;
        k.border_weak = raised;
        k.border_hover = c("surface2");
        k.border_active = focus;
        k.border_disabled = raised;
        k.hover = text.alpha(0.08);
        k.pressed = text.alpha(0.14);
        k.scrim = Color::BLACK.alpha(if dark { 0.65 } else { 0.35 });
        k.knob = text;
        k.knob_on = base;
        let status = |hue: Color| StateColors {
            bg: base.mix(hue, if dark { 0.13 } else { 0.10 }),
            fg: if dark { hue } else { hue.mix(text, 0.35) },
            border: hue.alpha(0.5),
            icon: hue,
        };
        k.success = status(c("green"));
        k.danger = status(c("red"));
        k.warning = status(c("yellow"));
        k.info = status(focus);
        k.surface_base = base;
        k.surface_weak = panel;
        k.surface_hover = raised;
        k.surface_raised = raised;
        k.surface_raised_base = panel;
        k.surface_raised_hover = c("surface1");
        k.surface_float = panel;
        k.surface_disabled = panel;
        k.std_border = c("surface1");
        k.std_icon = text;
        k.std_icon_invert = base;
        k.std_icon_strong = text;
        k.std_text = text;
        kit.blend_over = Some(base);
        // Keep one explicit syntax palette for the editor and Rune Kit DiffView.
        // Small text uses contrast-adjusted upstream hues, without changing ANSI output.
        let mut syntax = SyntaxTheme::from_kit(&kit);
        for (kind, role, fallback) in [
            (Syntax::Keyword, "keyword", "mauve"),
            (Syntax::String, "string", "green"),
            (Syntax::Number, "number", "peach"),
            (Syntax::Function, "function", "blue"),
            (Syntax::Type, "type", "yellow"),
            (Syntax::Comment, "comment", "subtext0"),
            (Syntax::Property, "property", "red"),
            (Syntax::Punctuation, "punctuation", "text"),
            (Syntax::Inserted, "inserted", "green"),
            (Syntax::Deleted, "deleted", "red"),
            (Syntax::Parameter, "parameter", "text"),
            (Syntax::Heading, "heading", "mauve"),
            (Syntax::Link, "link", "blue"),
            (Syntax::Operator, "operator", "text"),
            (Syntax::Regex, "regex", "green"),
            (Syntax::Attribute, "attribute", "yellow"),
            (Syntax::Variable, "variable", "text"),
        ] {
            // Mixed toward the canvas-readable ink computed before the diff
            // adjustment, so the inserted and deleted hues are the ones the
            // diff tints above were measured with.
            syntax = syntax.with(kind, readable(syntax_source(role, fallback), base, ink));
        }
        kit.c.success.icon = syntax.color(Syntax::Inserted);
        kit.c.danger.icon = syntax.color(Syntax::Deleted);
        // Status text (dialog errors, diff counts and the diff gutter's `+`
        // and `−`) reads at 4.5:1 on the surfaces and on its gutter tint.
        for state in [&mut kit.c.success, &mut kit.c.danger] {
            let on = [base, panel, raised, diff_gutter(base, state.icon, dark)];
            state.fg = reach(reach(state.fg, &on, 4.5, text), &on, 4.5, extreme);
        }
        // An active toggle's tint stays light enough for its icon (ink) at
        // 4.5:1 and its accent ring at 3:1, over the panel or the canvas.
        let active_alpha = (0..=9)
            .map(|step| 0.18 - step as f32 * 0.015)
            .find(|&alpha| {
                [base, panel].iter().all(|&under| {
                    let fill = composite(focus.alpha(alpha), under);
                    contrast(text, fill) >= 4.5 && contrast(focus, fill) >= 3.0
                })
            })
            .unwrap_or(0.0);
        kit.syntax = Some(syntax);
        kit.selection_color = p
            .selection
            .as_ref()
            .map(|s| rune::theme::hex(s).expect("bundled selection color"));
        // The terminal's own palette: canvas and ink are the appearance's, the
        // sixteen ANSI slots are the family's terminal table, and selection,
        // cursor and search highlights are drawn opaquely over the canvas.
        let terminal = TerminalPalette {
            foreground: text,
            background: base,
            cursor: focus,
            selection: kit
                .selection_color
                .map_or_else(|| base.mix(focus, 0.28), |s| over(s, base)),
            search_match: base.mix(c("yellow"), if dark { 0.30 } else { 0.38 }),
            search_current: base.mix(c("peach"), 0.85),
            search_current_text: base,
            ansi: p
                .ansi
                .each_ref()
                .map(|s| rune::theme::hex(s).expect("bundled ANSI color")),
        };
        Self {
            dark,
            active_alpha,
            name: if family == ThemeFamily::Catppuccin {
                if dark {
                    "Catppuccin Mocha"
                } else {
                    "Catppuccin Latte"
                }
            } else {
                p.name.as_str()
            },
            notes: if family == ThemeFamily::Catppuccin {
                "Catppuccin palette © 2021 Catppuccin · MIT"
            } else {
                p.attribution.as_str()
            },
            kit: super::fonts::kit(kit),
            terminal,
        }
    }
    pub fn kit(&self) -> &Kit {
        &self.kit
    }
    pub fn bg(&self) -> Color {
        self.kit.c.bg_base
    }
    pub fn panel(&self) -> Color {
        self.kit.c.layer_01
    }
    pub fn ink(&self) -> Color {
        self.kit.c.text_base
    }
    pub fn muted(&self) -> Color {
        self.kit.c.text_muted
    }
    /// A preview editor tab's title: dimmed from the regular title color
    /// (ink when selected, muted otherwise) toward muted and the tab fill, then
    /// brought back toward ink until it reads at 4.5:1 on both the idle and the
    /// hovered or selected tab fill. Some families' muted equals their ink, so
    /// the fill supplies the dimming there; where ink itself reads below 4.5:1
    /// on a tab fill (Solarized), it moves past ink toward black or white.
    pub fn temporary_tab_ink(&self, selected: bool) -> Color {
        let readable =
            |c: Color| contrast(c, self.panel()).min(contrast(c, self.hover_bg())) >= 4.5;
        let strong = if readable(self.ink()) {
            self.ink()
        } else if self.dark {
            Color::WHITE
        } else {
            Color::BLACK
        };
        let regular = if selected { self.ink() } else { self.muted() };
        let mut color = regular.mix(self.muted(), 0.5).mix(self.hover_bg(), 0.2);
        for _ in 0..64 {
            if readable(color) {
                break;
            }
            color = color.mix(strong, 0.12);
        }
        color
    }
    pub fn accent(&self) -> Color {
        self.kit.c.text_accent
    }
    /// The tint under an active toggle, drawn over the panel or canvas.
    pub fn active_fill(&self) -> Color {
        self.accent().alpha(self.active_alpha)
    }
    pub fn hover_bg(&self) -> Color {
        self.kit.c.layer_02
    }
    pub fn selection(&self) -> Color {
        self.kit.selection()
    }
    pub fn danger(&self) -> Color {
        self.kit.c.danger.fg
    }
    pub fn warning_bg(&self) -> Color {
        self.kit.c.warning.bg
    }
    pub fn scrim(&self) -> Color {
        self.kit.c.scrim
    }
    pub fn tooltip(&self) -> HelpLook {
        HelpLook::system(self.dark)
            .fill(self.panel())
            .ink(self.ink())
            .ring(1.0, self.kit.c.border_base)
    }
    pub fn terminal_palette(&self) -> &TerminalPalette {
        &self.terminal
    }
    /// A program's cell color under this appearance; see [`TerminalPalette`].
    pub fn terminal(&self, color: vt100::Color, foreground: bool) -> Color {
        self.terminal.color(color, foreground)
    }
}

/// Bring `color` toward `toward` until it reads at `minimum` on every one of
/// `backgrounds`, or as close as `toward` itself allows.
fn reach(mut color: Color, backgrounds: &[Color], minimum: f32, toward: Color) -> Color {
    for _ in 0..64 {
        if backgrounds.iter().all(|&b| contrast(color, b) >= minimum) {
            break;
        }
        color = color.mix(toward, 0.08);
    }
    color
}

/// Rune Kit DiffView's line tint for a changed line of `hue`.
fn diff_line(base: Color, hue: Color, dark: bool) -> Color {
    oklab_tint(base, hue, if dark { 0.13 } else { 0.1 })
}

/// Rune Kit DiffView's gutter tint, under the numbers and `+`/`−`.
fn diff_gutter(base: Color, hue: Color, dark: bool) -> Color {
    oklab_tint(base, hue, if dark { 0.2 } else { 0.17 })
}

/// Preserve palette hues while bringing low-contrast upstream small text toward ink.
fn readable(mut color: Color, background: Color, ink: Color) -> Color {
    for _ in 0..32 {
        if contrast(color, background) >= 4.5 {
            break;
        }
        color = color.mix(ink, 0.12);
    }
    color
}

/// WCAG contrast ratio between two opaque colors. Rune Color channels are
/// already linear sRGB; decoding again overestimates the contrast of pale
/// colors on a light surface.
pub(super) fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (a.luminance(), b.luminance());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The color `top` (with its alpha) shows over the opaque `under`.
pub(super) fn composite(top: Color, under: Color) -> Color {
    Color {
        r: under.r + (top.r - under.r) * top.a,
        g: under.g + (top.g - under.g) * top.a,
        b: under.b + (top.b - under.b) * top.a,
        a: 1.0,
    }
}

/// Rune Kit's diff tint: `t` of the way from `base` toward `hue` in OKLab.
fn oklab_tint(base: Color, hue: Color, t: f32) -> Color {
    let [a, b] = [base.to_oklab(), hue.to_oklab()];
    Color::from_oklab([0, 1, 2, 3].map(|i| {
        if i == 3 {
            a[3]
        } else {
            a[i] + (b[i] - a[i]) * t
        }
    }))
}

/// The role pairs whose contrast the workspace relies on, as
/// `(what, foreground, background, minimum)`: 4.5:1 for text, 3:1 for the
/// essential non-text indicators (focus ring, toggle ring, tab and row
/// selection marks).
#[cfg(test)]
pub(super) fn contrast_pairs(a: &Appearance) -> Vec<(&'static str, Color, Color, f32)> {
    let (bg, panel, hover) = (a.bg(), a.panel(), a.hover_bg());
    let active = composite(a.active_fill(), panel);
    let active_on_canvas = composite(a.active_fill(), bg);
    let c = &a.kit.c;
    let dark = a.dark;
    let gutter = |hue| diff_gutter(bg, hue, dark);
    let line = |hue| diff_line(bg, hue, dark);
    vec![
        ("ink on canvas", a.ink(), bg, 4.5),
        ("ink on panel", a.ink(), panel, 4.5),
        ("ink on tab fill / hover", a.ink(), hover, 4.5),
        ("ink on active toggle", a.ink(), active, 4.5),
        ("muted on canvas", a.muted(), bg, 4.5),
        ("muted on panel", a.muted(), panel, 4.5),
        ("muted on tab fill / hover", a.muted(), hover, 4.5),
        ("accent text on panel", a.accent(), panel, 4.5),
        ("danger text on panel", a.danger(), panel, 4.5),
        (
            "added sign on diff gutter",
            c.success.fg,
            gutter(c.success.icon),
            4.5,
        ),
        (
            "removed sign on diff gutter",
            c.danger.fg,
            gutter(c.danger.icon),
            4.5,
        ),
        ("ink on added line", a.ink(), line(c.success.icon), 4.5),
        ("ink on removed line", a.ink(), line(c.danger.icon), 4.5),
        ("focus ring on canvas", a.accent(), bg, 3.0),
        ("focus ring on panel", a.accent(), panel, 3.0),
        ("focus ring on tab fill / hover", a.accent(), hover, 3.0),
        (
            "ink on active toggle (canvas)",
            a.ink(),
            active_on_canvas,
            4.5,
        ),
        ("toggle ring on its fill", a.accent(), active, 3.0),
        (
            "toggle ring on its fill (canvas)",
            a.accent(),
            active_on_canvas,
            3.0,
        ),
        ("active tab bar on tab fill", a.accent(), hover, 3.0),
        ("selected row bar on selected fill", a.accent(), hover, 3.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_role_pairs_meet_wcag_contrast_in_every_appearance() {
        let mut failures = Vec::new();
        let mut table = String::new();
        for family in ThemeFamily::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                let a = Appearance::resolve(*family, mode, false);
                let mut worst = (f32::INFINITY, "");
                for (what, fg, bg, minimum) in contrast_pairs(&a) {
                    let ratio = contrast(fg, bg);
                    if ratio / minimum < worst.0 {
                        worst = (ratio / minimum, what);
                    }
                    if ratio < minimum {
                        failures.push(format!("{}: {what} {ratio:.2}:1 < {minimum}:1", a.name));
                    }
                }
                table.push_str(&format!(
                    "{:<24} worst margin {:.2}x ({})\n",
                    a.name, worst.0, worst.1
                ));
            }
        }
        let mut lowest: Vec<(&str, f32, f32, &str)> = Vec::new();
        for family in ThemeFamily::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                let a = Appearance::resolve(*family, mode, false);
                for (i, (what, fg, bg, minimum)) in contrast_pairs(&a).into_iter().enumerate() {
                    let ratio = contrast(fg, bg);
                    if i == lowest.len() {
                        lowest.push((what, minimum, ratio, a.name));
                    } else if ratio < lowest[i].2 {
                        lowest[i].2 = ratio;
                        lowest[i].3 = a.name;
                    }
                }
            }
        }
        for (what, minimum, ratio, name) in lowest {
            table.push_str(&format!(
                "| {what} | {minimum}:1 | {ratio:.2}:1 | {name} |\n"
            ));
        }
        println!("{table}");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn temporary_tab_ink_is_dimmed_yet_readable_on_tab_fills_in_every_theme() {
        for family in ThemeFamily::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                let a = Appearance::resolve(*family, mode, false);
                for selected in [true, false] {
                    let ink = a.temporary_tab_ink(selected);
                    for fill in [a.panel(), a.hover_bg()] {
                        let ratio = contrast(ink, fill);
                        assert!(ratio >= 4.5, "{} {selected}: {ratio:.2}:1", a.name);
                    }
                    // Dimmer than a selected regular title wherever that
                    // title itself leaves room above 4.5:1.
                    if contrast(a.ink(), a.panel()).min(contrast(a.ink(), a.hover_bg())) >= 4.5 {
                        assert!(
                            contrast(ink, a.hover_bg()) < contrast(a.ink(), a.hover_bg()),
                            "{} {selected} reads dimmer than a regular title",
                            a.name
                        );
                    }
                }
            }
        }
    }
}
