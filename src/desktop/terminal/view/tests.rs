use super::*;
use crate::config::{Appearance as Mode, ThemeFamily};
use crate::desktop::tests::runtime_for;

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
        // "hello" is one run, whatever the selection over its first cells.
        assert_color(ink(&rt, "terminal-cell-0-0"), p.foreground, "foreground");
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

/// A runtime showing one live session, settled at its prompt.
fn live() -> (tempfile::TempDir, rune::Runtime<State>) {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let shell = crate::test_support::shell_binary().unwrap();
    let session = Session::spawn(
        &shell,
        root.path(),
        root.path(),
        crate::config::Config::default(),
        28,
        100,
        Arc::new(|| {}),
    )
    .unwrap();
    session
        .wait_for("rodtty:", std::time::Duration::from_secs(15))
        .unwrap();
    state.sessions.push(session);
    let mut rt = crate::desktop::tests::runtime_for(state);
    rt.frame(0.0);
    (root, rt)
}

/// The window point at the middle of zero-based cell (`row`, `col`).
fn cell_point(rt: &rune::Runtime<State>, row: u16, col: u16) -> (f32, f32) {
    let [x, y, ..] = rt.state.terminal_rect.expect("terminal laid out");
    let m = fonts::Metrics::measure();
    (
        x + (f32::from(col) + 0.5) * m.width,
        y + (f32::from(row) + 0.5) * m.height,
    )
}

#[test]
fn a_mouse_aware_application_receives_clicks_and_wheel_instead_of_a_selection() {
    let (_root, mut rt) = live();
    let session = rt.state.active().unwrap();
    // Turn on press/release reporting in SGR encoding, then echo the
    // reports the terminal sends back. Brush's `read` drops their ESCs.
    session
        .send(
            b"printf '\\033[?1000h\\033[?1006hREA''DY\\n'; read -r line; printf '\\033[?1000l'; echo \"G\"\"OT${line}\"\r"
                .to_vec(),
        )
        .unwrap();
    session
        .wait_for("READY", std::time::Duration::from_secs(10))
        .unwrap();
    let (x, y) = cell_point(&rt, 2, 4);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert!(rt.state.selection.is_none());
    let ch = fonts::Metrics::measure().height;
    rt.wheel_xy(x, y, 0.0, ch);
    rt.frame(0.0);
    rt.state.active().unwrap().send(b"\r".to_vec()).unwrap();
    rt.state
        .active()
        .unwrap()
        .wait_for(
            "GOT[<0;5;3M[<0;5;3m[<64;5;3M",
            std::time::Duration::from_secs(10),
        )
        .unwrap();
}

#[test]
fn the_wheel_sends_arrow_keys_to_an_alternate_screen_application_without_mouse_mode() {
    let (_root, mut rt) = live();
    let session = rt.state.active().unwrap();
    session
        .send(
            b"printf '\\033[?1049hREA''DY'; read -r line; printf '\\033[?1049l'; echo \"G\"\"OT${line}\"\r"
                .to_vec(),
        )
        .unwrap();
    session
        .wait_for("READY", std::time::Duration::from_secs(10))
        .unwrap();
    let (x, y) = cell_point(&rt, 2, 4);
    let ch = fonts::Metrics::measure().height;
    rt.wheel_xy(x, y, 0.0, -2.0 * ch);
    rt.frame(0.0);
    rt.state.active().unwrap().send(b"\r".to_vec()).unwrap();
    rt.state
        .active()
        .unwrap()
        .wait_for("GOT[B[B", std::time::Duration::from_secs(10))
        .unwrap();
}

/// The x of every glyph drawn in each of the grid's first `rows` rows, in
/// device pixels at `scale`.
fn glyph_columns(rt: &rune::Runtime<State>, rows: usize, scale: f32) -> Vec<Vec<f32>> {
    let grid = element(rt, "terminal").rect;
    let ch = rt.state.terminal_metrics.height;
    let mut columns = vec![Vec::new(); rows];
    for glyph in rt
        .instances
        .iter()
        .filter(|i| i.params[1] == rune::render::KIND_GLYPH)
    {
        let row = ((glyph.rect[1] + glyph.rect[3] / 2.0) / scale - grid[1]) / ch;
        let row = row.floor();
        if row >= 0.0 && (row as usize) < rows && glyph.rect[0] / scale >= grid[0] {
            columns[row as usize].push(glyph.rect[0]);
        }
    }
    for row in &mut columns {
        row.sort_by(f32::total_cmp);
    }
    columns
}

#[test]
fn glyphs_sharing_a_run_land_on_the_same_cells_as_glyphs_drawn_one_per_cell() {
    // Row 0 is one run of a hundred glyphs; row 1 alternates between two
    // barely different inks, so each of its glyphs is a run of its own.
    // Every column must line up.
    let mut bytes = b"\x1b[38;2;200;200;200m".to_vec();
    bytes.extend_from_slice("M".repeat(100).as_bytes());
    bytes.extend_from_slice(b"\r\n");
    for c in 0..100 {
        bytes.extend_from_slice(format!("\x1b[38;2;200;200;{}mM", 200 + c % 2).as_bytes());
    }
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().to_owned());
    let mut parser = vt100::Parser::new(6, 120, 100);
    parser.process(&bytes);
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    for (size, spacing, scale) in [
        (14.0, 0.0, 1.0),
        (13.0, 0.0, 1.0),
        (13.0, 0.0, 1.5),
        (15.5, 0.0, 2.0),
        (13.0, 0.7, 1.5),
        (14.0, -0.4, 1.0),
    ] {
        rt.update(|s, _| {
            s.config.fonts.terminal_size = size;
            s.config.fonts.terminal_letter_spacing = spacing;
        });
        rt.set_viewport((1600.0, 700.0), scale);
        for _ in 0..3 {
            rt.frame(0.0);
        }
        let rows = glyph_columns(&rt, 2, scale);
        assert_eq!(rows[0].len(), 100, "size {size} at {scale}x");
        assert_eq!(rows[1].len(), 100, "size {size} at {scale}x");
        let drift = rows[0]
            .iter()
            .zip(&rows[1])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(drift < 0.01, "size {size} at {scale}x drifts {drift}");
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("terminal-cell-0-0")
                    && e.text.as_deref().is_some_and(|t| t.len() == 100)),
            "row 0 is one run"
        );
    }
}

#[test]
fn the_wheel_scrollback_follows_the_system_scroll_direction() {
    let (_root, mut rt) = live();
    let session = rt.state.active().unwrap();
    session.send(b"seq 1 200; echo DO''NE\r".to_vec()).unwrap();
    session
        .wait_for("DONE", std::time::Duration::from_secs(10))
        .unwrap();
    rt.frame(0.0);
    let (x, y) = cell_point(&rt, 2, 4);
    let ch = fonts::Metrics::measure().height;
    let scrollback = |rt: &rune::Runtime<State>| rt.state.with_screen(vt100::Screen::scrollback);
    // Content moving down (positive `dy`, the system's own sign after its
    // natural-scrolling setting) reveals the history above.
    rt.wheel_xy(x, y, 0.0, 3.0 * ch);
    rt.frame(0.0);
    assert_eq!(scrollback(&rt), 3);
    rt.wheel_xy(x, y, 0.0, -ch);
    rt.frame(0.0);
    assert_eq!(scrollback(&rt), 2);
}
