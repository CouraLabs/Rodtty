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
        let text = c("text");
        let muted = readable(c("subtext0"), base, text);
        let focus = readable(c("blue"), base, text);
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
        k.text_faint = readable(c("subtext1"), base, text);
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
            let color = p.syntax.get(role).map_or_else(
                || c(fallback),
                |s| rune::theme::hex(s).expect("bundled syntax color"),
            );
            syntax = syntax.with(kind, readable(color, base, text));
        }
        kit.c.success.icon = syntax.color(Syntax::Inserted);
        kit.c.danger.icon = syntax.color(Syntax::Deleted);
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

#[cfg(test)]
mod tests {
    use super::*;

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
