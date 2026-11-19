use super::super::tests::runtime_for;
use super::*;
use crate::config::{Appearance as Mode, ThemeFamily};

/// A runtime showing `bytes` as retained terminal output, in `family` Dark.
fn showing(bytes: &[u8], family: ThemeFamily) -> rune::Runtime<State> {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let mut parser = vt100::Parser::new(6, 40, 100);
    parser.process(bytes);
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    switch(&mut rt, family);
    rt
}

fn switch(rt: &mut rune::Runtime<State>, family: ThemeFamily) {
    rt.update(|s, _| {
        s.save_config(|c| c.theme_family = family);
        s.config.appearance = Mode::Dark;
    });
    // Theme colors animate to their new values; let the flight land first.
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    rt.settle_all();
    rt.frame(0.0);
}

fn palette(family: ThemeFamily) -> Appearance {
    Appearance::resolve(family, Mode::Dark, true)
}

fn element(rt: &rune::Runtime<State>, key: &str) -> rune::ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn ink(rt: &rune::Runtime<State>, key: &str) -> Color {
    element(rt, key).text_color.expect("terminal glyph color")
}

fn close(actual: Color, expected: Color) -> bool {
    (actual.r - expected.r).abs() < 0.001
        && (actual.g - expected.g).abs() < 0.001
        && (actual.b - expected.b).abs() < 0.001
}

#[track_caller]
fn assert_color(actual: Color, expected: Color, what: &str) {
    assert!(
        close(actual, expected),
        "{what}: {actual:?} != {expected:?}"
    );
}

#[test]
fn a_theme_switch_recolors_retained_ansi_output_without_new_output() {
    // What `printf '\e[31mred\e[0m plain'` leaves in the terminal.
    let mut rt = showing(b"\x1b[31mred\x1b[0m plain", ThemeFamily::Solarized);
    let before = rt.state.with_screen(|s| s.contents());
    let solarized = palette(ThemeFamily::Solarized);
    let tokyo = palette(ThemeFamily::TokyoNight);
    assert!(!close(
        solarized.terminal_palette().ansi[1],
        tokyo.terminal_palette().ansi[1]
    ));
    assert_color(
        ink(&rt, "terminal-cell-0-0"),
        solarized.terminal_palette().ansi[1],
        "Solarized red",
    );
    switch(&mut rt, ThemeFamily::TokyoNight);
    assert_eq!(rt.state.with_screen(|s| s.contents()), before);
    assert_color(
        ink(&rt, "terminal-cell-0-0"),
        tokyo.terminal_palette().ansi[1],
        "Tokyo Night red",
    );
    assert_color(
        ink(&rt, "terminal-cell-0-4"),
        tokyo.terminal_palette().foreground,
        "default ink",
    );
}

#[test]
fn extended_indexed_and_truecolor_cells_keep_their_exact_colors_under_any_theme() {
    let bytes = b"\x1b[38;5;196mA\x1b[38;5;244mB\x1b[38;2;12;34;56mC\
        \x1b[0;48;5;21m \x1b[48;2;200;100;50m \x1b[0m";
    let mut rt = showing(bytes, ThemeFamily::Catppuccin);
    for family in [ThemeFamily::Catppuccin, ThemeFamily::Kanagawa] {
        switch(&mut rt, family);
        assert_color(ink(&rt, "terminal-cell-0-0"), rgb(255, 0, 0), "index 196");
        assert_color(
            ink(&rt, "terminal-cell-0-1"),
            rgb(128, 128, 128),
            "index 244",
        );
        assert_color(
            ink(&rt, "terminal-cell-0-2"),
            rgb(12, 34, 56),
            "truecolor fg",
        );
        assert_color(
            element(&rt, "terminal-fill-0-3").fill,
            rgb(0, 0, 255),
            "index 21 background",
        );
        assert_color(
            element(&rt, "terminal-fill-0-4").fill,
            rgb(200, 100, 50),
            "truecolor background",
        );
    }
}

#[test]
fn default_colors_cursor_and_selection_follow_the_theme() {
    let mut rt = showing(b"hello", ThemeFamily::Everforest);
    rt.update(|s, _| s.selection = Some(((0, 0), (0, 2))));
    for family in [ThemeFamily::Everforest, ThemeFamily::RosePine] {
        switch(&mut rt, family);
        rt.update(|s, _| s.selection = Some(((0, 0), (0, 2))));
        rt.frame(0.0);
        let a = palette(family);
        let p = a.terminal_palette();
        assert_color(element(&rt, "terminal").fill, p.background, "background");
        assert_color(ink(&rt, "terminal-cell-0-3"), p.foreground, "foreground");
        assert_color(element(&rt, "terminal-caret").fill, p.cursor, "cursor");
        assert_color(
            element(&rt, "terminal-fill-0-0").fill,
            p.selection,
            "selection",
        );
    }
    assert!(!close(
        palette(ThemeFamily::Everforest)
            .terminal_palette()
            .background,
        palette(ThemeFamily::RosePine).terminal_palette().background
    ));
}

#[test]
fn every_appearance_resolves_its_own_sixteen_ansi_slots() {
    // Each appearance resolves its own table; a family that fell back to
    // another's would share all sixteen colors with it.
    let mut tables = Vec::new();
    for &family in ThemeFamily::ALL {
        for mode in [Mode::Light, Mode::Dark] {
            let a = Appearance::resolve(family, mode, true);
            let ansi = a.terminal_palette().ansi;
            assert!(
                ansi[1..7].iter().any(|c| !close(*c, ansi[0])),
                "{family:?} {mode:?} has no chromatic ANSI slots"
            );
            tables.push(ansi.map(|c| [c.r, c.g, c.b].map(f32::to_bits)));
        }
    }
    let unique: std::collections::BTreeSet<_> = tables.iter().collect();
    assert_eq!(
        unique.len(),
        tables.len(),
        "two appearances share an ANSI table"
    );
}
