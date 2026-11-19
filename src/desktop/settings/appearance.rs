use super::*;

pub(super) fn appearance(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::Appearance as Mode;
    choice(
        s,
        a,
        layers,
        "appearance",
        "Appearance",
        labels(&[
            ("Light", Mode::Light),
            ("Dark", Mode::Dark),
            ("System", Mode::System),
        ]),
        s.config.appearance,
        |c, v| c.appearance = v,
    )
}

pub(super) fn theme_previews(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    use crate::config::Appearance as Mode;
    col()
        .w_full()
        .gap(8.0)
        .child(
            text(format!("{} · applies immediately", a.name))
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap(),
        )
        .child(palette_preview(s.config.theme_family, Mode::Light))
        .child(palette_preview(s.config.theme_family, Mode::Dark))
        .child(
            text(a.notes)
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap_anywhere(),
        )
}

pub(super) fn show_footer(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    toggle(
        a,
        "show-footer",
        "Show footer",
        s.config.show_footer,
        |s, _| s.save_config(|c| c.show_footer = !c.show_footer),
    )
}

/// The UI, terminal or editor font size.
pub(super) fn font_size(s: &State, a: &Appearance, target: &'static str) -> El {
    let fonts = &s.config.fonts;
    let (size, subject, change): (f32, &str, fn(&mut Config, f32)) = match target {
        "ui" => (fonts.ui_size, "UI font size", |c, d| {
            c.fonts.ui_size = (c.fonts.ui_size + d).clamp(8.0, 32.0)
        }),
        "terminal" => (fonts.terminal_size, "terminal font size", |c, d| {
            c.fonts.terminal_size = (c.fonts.terminal_size + d).clamp(8.0, 32.0)
        }),
        _ => (fonts.editor_size, "editor font size", |c, d| {
            c.fonts.editor_size = (c.fonts.editor_size + d).clamp(8.0, 32.0)
        }),
    };
    stepper(
        a,
        &format!("{target}-font-size"),
        subject,
        format!("{size}px"),
        change,
    )
}

pub(super) fn line_height(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "terminal-line-height",
        "terminal line height",
        format!("{:.1}×", s.config.fonts.terminal_line_height),
        |c, sign| {
            let f = &mut c.fonts;
            f.terminal_line_height = step(
                f.terminal_line_height,
                0.1 * sign,
                config::TERMINAL_LINE_HEIGHT,
            );
        },
    )
}

pub(super) fn letter_spacing(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    stepper(
        a,
        "terminal-letter-spacing",
        "terminal letter spacing",
        format!("{:+.1}px", s.config.fonts.terminal_letter_spacing),
        |c, sign| {
            let f = &mut c.fonts;
            f.terminal_letter_spacing = step(
                f.terminal_letter_spacing,
                0.5 * sign,
                config::TERMINAL_LETTER_SPACING,
            );
        },
    )
}

pub(super) fn family_picker(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::config::ThemeFamily;
    let options: Vec<_> = ThemeFamily::ALL
        .iter()
        .map(|family| rune_kit::ComboboxOption {
            id: family.label().into(),
            label: family.label().into(),
            detail: match family {
                ThemeFamily::OneDarkPro => "Rodtty light adaptation / Dark",
                ThemeFamily::Gruvbox => "Medium contrast Light / Dark",
                ThemeFamily::Dracula => "Alucard / Dracula",
                ThemeFamily::NightOwl => "Light Owl / Night Owl",
                ThemeFamily::Catppuccin => "Latte / Mocha",
                ThemeFamily::TokyoNight => "Light / Night",
                ThemeFamily::RosePine => "Dawn / Moon",
                ThemeFamily::Kanagawa => "Lotus / Wave",
                ThemeFamily::Everforest => "Light / Dark Green",
                _ => "Light / Dark",
            }
            .into(),
            disabled: false,
        })
        .collect();
    pressable("theme-family-value-trigger");
    rune_kit::Combobox::new(
        "theme-family",
        &s.theme_family_picker,
        &options,
        |s: &mut State| {
            s.font_ui.close();
            s.font_terminal.close();
            s.settings_select = None;
            &mut s.theme_family_picker
        },
    )
    .label("Theme family")
    .current(Some(s.config.theme_family.label()))
    .search_placeholder("Search theme families")
    .width(240.0)
    .max_height(256.0)
    .on_choose(|s, label, _| {
        if let Some(&family) = ThemeFamily::ALL.iter().find(|f| f.label() == label) {
            s.save_config(|c| c.theme_family = family);
        }
    })
    .build(a.kit(), layers)
    .shrink()
    .min_w(0.0)
}

pub(super) fn prompt_style(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::prompt::PromptStyle;
    let options = PromptStyle::ALL
        .iter()
        .map(|&style| (style.label().to_owned(), style))
        .collect();
    choice(
        s,
        a,
        layers,
        "prompt-style",
        "Prompt style",
        options,
        s.config.prompt.style,
        |c, v| c.prompt.style = v,
    )
}

pub(super) fn prompt_style_preview(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    let mut preview = col()
        .key("prompt-style-preview")
        .gap(4.0)
        .child(prompt_preview(s.config.prompt, false, a));
    if s.config.prompt.style.reports_status() {
        preview = preview
            .child(
                text("After a failed command:")
                    .font(ui_style(12.0, 400.0))
                    .color(a.muted()),
            )
            .child(prompt_preview(s.config.prompt, true, a));
    }
    preview
}

pub(super) fn prompt_symbol(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    use crate::prompt::PromptSymbol;
    let options = PromptSymbol::ALL
        .iter()
        .map(|&symbol| (symbol_name(symbol), symbol))
        .collect();
    choice(
        s,
        a,
        layers,
        "prompt-symbol",
        "Prompt symbol",
        options,
        s.config.prompt.symbol,
        |c, v| c.prompt.symbol = v,
    )
}

/// The line holding the symbol, as drawn after success and, when the style
/// reports it, after failure.
pub(super) fn prompt_symbol_preview(s: &State, a: &Appearance, _: &mut Vec<El>) -> El {
    let prefs = s.config.prompt;
    let last_line = |failed| {
        let lines = prefs.preview(failed, PREVIEW_DIRECTORY);
        prompt_line(lines.last().map(Vec::as_slice).unwrap_or_default(), a)
    };
    let mut preview = row()
        .key("prompt-symbol-preview")
        .gap(12.0)
        .scroll_x()
        .child(last_line(false));
    if prefs.style.reports_status() {
        preview = preview.child(last_line(true));
    }
    preview
}

/// A prompt symbol's spoken name: a screen reader may skip or misread the
/// glyph alone.
pub(super) fn symbol_name(symbol: crate::prompt::PromptSymbol) -> String {
    use crate::prompt::PromptSymbol;
    let word = match symbol {
        PromptSymbol::Dollar => "Dollar sign",
        PromptSymbol::Chevron => "Chevron",
        PromptSymbol::Triangle => "Triangle",
        PromptSymbol::DoubleChevron => "Double chevron",
    };
    format!("{word} {}", symbol.glyph())
}

pub(super) const PREVIEW_DIRECTORY: &str = "~/project";

/// The prompt drawn with the active theme's ANSI palette, as the terminal would.
pub(super) fn prompt_preview(
    prefs: crate::prompt::PromptPreferences,
    failed: bool,
    a: &Appearance,
) -> El {
    let mut lines = col()
        .px(8.0)
        .py(6.0)
        .radius(6.0)
        .fill(a.bg())
        .label(prefs.preview_text(failed, PREVIEW_DIRECTORY));
    for line in prefs.preview(failed, PREVIEW_DIRECTORY) {
        lines = lines.child(prompt_line(&line, a));
    }
    lines
}

pub(super) fn prompt_line(line: &[(String, crate::prompt::Segment)], a: &Appearance) -> El {
    let font = ui_label().family(Family::Mono);
    let mut spans = row().label(line.iter().map(|(t, _)| t.as_str()).collect::<String>());
    for (content, segment) in line {
        let color = segment
            .fg
            .map_or(a.ink(), |i| a.terminal(vt100::Color::Idx(i), true));
        let mut font = font;
        if segment.bold {
            font.weight = 700.0;
        }
        let mut span = row().child(text(content).font(font).color(color));
        if let Some(bg) = segment.bg {
            span = span.fill(a.terminal(vt100::Color::Idx(bg), false));
        }
        spans = spans.child(span);
    }
    spans
}

pub(super) fn palette_preview(
    family: crate::config::ThemeFamily,
    mode: crate::config::Appearance,
) -> El {
    let p = Appearance::resolve(family, mode, false);
    let mut swatches = row().gap(3.0);
    for i in 0..8 {
        swatches = swatches.child(
            el().size(16.0, 10.0)
                .radius(2.0)
                .fill(p.terminal(vt100::Color::Idx(i + 1), true)),
        );
    }
    col()
        .key(format!(
            "palette-preview-{}",
            if p.dark { "dark" } else { "light" }
        ))
        .w_full()
        .min_w(0.0)
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .radius(6.0)
        .fill(p.bg())
        .border(1.0, p.kit().c.border_base)
        .child(text(p.name).font(ui_label()).color(p.ink()).wrap())
        .child(
            text("rodtty:~/projects $ cargo build")
                .font(ui_label())
                .color(p.accent())
                .wrap(),
        )
        .child(swatches)
}
