//! The terminal grid: one positioned run per cell, painted from the
//! appearance's terminal palette, with selection and search highlights, and
//! the pointer gestures (selection drag, wheel scroll) over it.
use crate::desktop::*;

pub(crate) fn grid(state: &State, a: &Appearance) -> El {
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
                Some(terminal::search::Highlight::Current) => {
                    background = p.search_current;
                    fg = p.search_current_text;
                    fill_key = format!("terminal-search-current-{r}-{start}");
                    highlighted = true;
                }
                Some(terminal::search::Highlight::Match) => {
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
                // Letter spacing widens the cell; the glyph sits centered in it.
                let spacing = metrics.spacing;
                grid = grid.child(
                    text(content)
                        .key(format!("terminal-cell-{r}-{start}"))
                        .font(style)
                        .color(fg)
                        .absolute(start as f32 * cw + spacing / 2.0, r as f32 * ch)
                        .size(width + 1.0 + spacing.abs(), ch),
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
    .on_secondary_press(terminal::clipboard::terminal_menu)
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
    pub(crate) fn observe_terminal_output(&mut self) -> bool {
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
    pub(crate) fn copy(&self, tx: &mut Tx) {
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
pub(crate) const SELECTION_THRESHOLD: f32 = 4.0;

#[cfg(test)]
mod tests;
