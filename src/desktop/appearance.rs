//! One resolved appearance for chrome, editing, diff inspection, and terminal rendering.
//! Families supply data; callers consume semantic roles rather than palette names.
use crate::config::{Appearance as Mode, ThemeFamily};
use rune::{Color, HelpLook};
use rune_kit::{Kit, StateColors};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
struct Palette {
    name: String,
    colors: BTreeMap<String, String>,
    ansi: [String; 16],
}

pub(super) struct Appearance {
    pub dark: bool,
    pub name: &'static str,
    kit: Kit,
    ansi: [Color; 16],
}
impl Appearance {
    pub fn resolve(family: ThemeFamily, mode: Mode, system_dark: bool) -> Self {
        let dark = mode.dark(system_dark);
        match family {
            ThemeFamily::Catppuccin => Self::catppuccin(dark),
        }
    }
    fn catppuccin(dark: bool) -> Self {
        static PALETTES: OnceLock<BTreeMap<String, Palette>> = OnceLock::new();
        let palettes = PALETTES.get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/themes/catppuccin.json"))
                .expect("bundled Catppuccin palette")
        });
        let p = &palettes[if dark { "mocha" } else { "latte" }];
        let c = |name: &str| rune::theme::hex(&p.colors[name]).expect("bundled palette color");
        let mut kit = Kit::neutral(dark);
        let k = &mut kit.c;
        let base = c("base");
        let panel = c("mantle");
        let raised = c("surface0");
        let text = c("text");
        let muted = c("subtext0");
        let focus = c("blue");
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
        k.text_faint = c("subtext1");
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
        // Keep a single derived SyntaxTheme for the editor and Rune Kit DiffView.
        Self {
            dark,
            name: if p.name == "Mocha" {
                "Catppuccin Mocha"
            } else {
                "Catppuccin Latte"
            },
            kit: super::fonts::kit(kit),
            ansi: p
                .ansi
                .each_ref()
                .map(|s| rune::theme::hex(s).expect("bundled ANSI color")),
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
    /// Only the sixteen conventional ANSI slots belong to the selected palette.
    /// Extended indexed and truecolor output stay exactly as the program requested.
    pub fn terminal(&self, color: vt100::Color, foreground: bool) -> Color {
        match color {
            vt100::Color::Default => {
                if foreground {
                    self.ink()
                } else {
                    self.bg()
                }
            }
            vt100::Color::Rgb(r, g, b) => super::rgb(r, g, b),
            vt100::Color::Idx(i) if i < 16 => self.ansi[usize::from(i)],
            vt100::Color::Idx(i) => {
                let (r, g, b) = if i >= 232 {
                    let v = 8 + (i - 232) * 10;
                    (v, v, v)
                } else {
                    let n = i - 16;
                    let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
                    (level(n / 36), level(n / 6 % 6), level(n % 6))
                };
                super::rgb(r, g, b)
            }
        }
    }
}
