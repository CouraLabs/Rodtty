//! The terminal grid: one positioned run per cell, painted from the
//! appearance's terminal palette, with selection and search highlights, and
//! the pointer gestures (selection drag, wheel scroll) over it.
use super::*;

pub(super) fn grid(state: &State, a: &Appearance) -> El {
    state.with_screen(|screen| grid_view(state, screen, a))
}

fn grid_view(state: &State, screen: &vt100::Screen, a: &Appearance) -> El {
    let metrics = fonts::Metrics::measure();
    let (cw, ch) = (metrics.width, metrics.height);
    let (rows, cols) = screen.size();
    let selection = state
        .selection
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    let p = a.terminal_palette();
    let highlight = state
        .terminal_search
        .highlighter(screen, state.screen_key());
    let mut grid = el()
        .key("terminal")
        .instant_changes()
        .role("terminal")
        .label("Terminal")
        .w_full()
        .grow(1.0)
        .clip_soft(0.0)
        .fill(p.background);
    // Position runs on a fixed cell grid. Wide glyphs get their own run so
    // fallback-font advances cannot shift the cells following them.
    for r in 0..rows {
        let mut c = 0;
        while c < cols {
            let Some(cell) = screen.cell(r, c) else {
                c += 1;
                continue;
            };
            if cell.is_wide_continuation() {
                c += 1;
                continue;
            }
            let selected = |c| selection.is_some_and(|(a, b)| (r, c) >= a && (r, c) < b);
            let start = c;
            // Each terminal cell has an explicit origin. Font fallback, ligatures,
            // combining sequences and pixel rounding must never shift later cells.
            let content = if cell.contents().is_empty() {
                " "
            } else {
                cell.contents()
            };
            let end = c + if cell.is_wide() { 2 } else { 1 };
            // Colors resolve from the cell's color index on every frame, so a
            // theme change recolors retained output without new output.
            let mut fg = p.color(cell.fgcolor(), true);
            let mut background = p.color(cell.bgcolor(), false);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut background);
            }
            let mut fill_key = format!("terminal-fill-{r}-{start}");
            let mut highlighted = false;
            match highlight(r, start) {
                Some(terminal_search::Highlight::Current) => {
                    background = p.search_current;
                    fg = p.search_current_text;
                    fill_key = format!("terminal-search-current-{r}-{start}");
                    highlighted = true;
                }
                Some(terminal_search::Highlight::Match) => {
                    background = p.search_match;
                    fill_key = format!("terminal-search-match-{r}-{start}");
                    highlighted = true;
                }
                None => {}
            }
            if selected(start) {
                background = p.selection;
            }
            let width = (end - start) as f32 * cw;
            if background != p.background || highlighted {
                grid = grid.child(
                    el().key(fill_key)
                        .absolute(start as f32 * cw, r as f32 * ch)
                        .size(width, ch)
                        .fill(background),
                );
            }
            if !content.trim().is_empty() {
                let mut style = metrics.style;
                if cell.bold() {
                    style.weight = 600.0;
                }
                style.italic = cell.italic();
                grid = grid.child(
                    text(content)
                        .key(format!("terminal-cell-{r}-{start}"))
                        .font(style)
                        .color(fg)
                        .absolute(start as f32 * cw, r as f32 * ch)
                        .size(width + 1.0, ch),
                );
            }
            if cell.underline() {
                grid = grid.child(
                    el().absolute(start as f32 * cw, r as f32 * ch + ch - 3.0)
                        .size(width, 1.0)
                        .fill(fg),
                );
            }
            c = end.max(c + 1);
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (r, c) = screen.cursor_position();
        grid = grid.child(
            el().key("terminal-caret")
                .absolute(c as f32 * cw, r as f32 * ch + ch - 3.0)
                .size(cw, 2.0)
                .fill(p.cursor)
                .ime_cursor(),
        );
    }
    // A metric/session probe also reports when the viewport itself has not changed.
    // Rune's on_layout fires only for changed rectangles; a font-only update still
    // needs to notify the existing PTY before its next redraw.
    let session_key = state
        .active()
        .map(|s| Arc::as_ptr(&s.state) as usize)
        .unwrap_or(0);
    grid = grid.child(
        el().key(format!(
            "terminal-metrics-{session_key}-{}-{cw}-{ch}",
            rune::text::font_epoch()
        ))
        .absolute(0.0, 0.0)
        .size(cw, ch)
        .on_layout(move |s: &mut State, _| s.resize_terminal(metrics)),
    );
    grid.on_layout(move |s: &mut State, rect| {
        s.terminal_rect = Some(rect);
        s.resize_terminal(metrics);
    })
    .on_background_press_tx(|s: &mut State, tx| {
        s.supersede_pending_preview();
        s.workspace.set_terminal_focus(true);
        // The selection is the drag's to keep or drop: a pointer press reaches
        // the drag handler first, whose `Down` drops any earlier selection, and
        // this press also follows the release of a drag that just selected, so
        // clearing here would erase every selection the moment it was made.
        tx.blur();
    })
    .on_secondary_press(clipboard::terminal_menu)
    .on_drag(move |s: &mut State, drag, tx| {
        s.supersede_pending_preview();
        s.workspace.set_terminal_focus(true);
        tx.blur();
        let (rows, cols) = s.with_screen(vt100::Screen::size);
        let point = |x: f32, y: f32| {
            (
                (y / ch).floor().clamp(0.0, (rows - 1) as f32) as u16,
                (x / cw).floor().clamp(0.0, cols as f32) as u16,
            )
        };
        if drag.cancelled() {
            s.selection = None;
            s.drag_origin = None;
            return;
        }
        match drag.phase {
            Phase::Down => {
                // Record the press but select nothing yet: a click that drifts
                // a few points on a trackpad would otherwise highlight a cell
                // the owner never meant to touch, and a click is not a select.
                s.selection = None;
                s.drag_origin = Some((drag.x, drag.y));
            }
            Phase::Move => {
                let Some((sx, sy)) = s.drag_origin else {
                    return;
                };
                if s.selection.is_none() {
                    let travel = (drag.x - sx).hypot(drag.y - sy);
                    if travel < SELECTION_THRESHOLD {
                        return;
                    }
                    s.selection = Some((point(sx, sy), point(drag.x, drag.y)));
                } else {
                    let (start, _) = s.selection.unwrap_or((point(sx, sy), point(sx, sy)));
                    s.selection = Some((start, point(drag.x, drag.y)));
                }
            }
            Phase::Up => {
                let extended = s.selection.is_some();
                s.drag_origin = None;
                if extended && s.config.terminal_clipboard.copy_on_selection {
                    s.copy(tx);
                }
                // A press that never became a selection leaves nothing behind.
                if !extended {
                    s.selection = None;
                }
            }
        }
    })
    .on_wheel(move |s: &mut State, event, _| {
        if let Some(session) = s.active() {
            session.scroll((-event.dy / ch).round() as isize);
        }
        true
    })
}

impl State {
    /// Notes new output in every session since the last poll, and reports
    /// whether there was any. Output moves the cells a selection or search
    /// highlight was drawn over, so those are dropped rather than left over
    /// the wrong text.
    pub(super) fn observe_terminal_output(&mut self) -> bool {
        let generations: Vec<_> = self
            .sessions
            .iter()
            .map(|session| {
                session
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .generation
            })
            .collect();
        let changed = generations != self.generations;
        // New output in the shown session moves the cells search matches were
        // found in; drop them rather than highlight the wrong text.
        if generations.get(self.active) != self.generations.get(self.active) {
            self.terminal_search.clear();
        }
        self.generations = generations;
        if changed {
            // New output moves the cells a highlight was drawn over. Keep it only
            // while a gesture is still extending it, where the drag re-derives the
            // range on its next move anyway.
            if self.drag_origin.is_none() {
                self.selection = None;
            }
        }
        changed
    }

    /// Copies the terminal selection's text, when it covers any.
    pub(super) fn copy(&self, tx: &mut Tx) {
        if let Some((a, b)) = self.selection {
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            if a != b {
                let text = self.with_screen(|screen| screen.contents_between(a.0, a.1, b.0, b.1));
                if !text.is_empty() {
                    tx.copy(text);
                }
            }
        }
    }
}

/// How far the pointer travels before a terminal press becomes a selection, in
/// points. Below this it is a click: a trackpad's contact wobble and a slow
/// click both stay under it, so neither highlights a cell the owner never
/// meant to select and a click never copies.
pub(super) const SELECTION_THRESHOLD: f32 = 4.0;

#[cfg(test)]
mod tests {
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
}
