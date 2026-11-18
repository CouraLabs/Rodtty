//! Find in Terminal: matches over retained scrollback and the live screen, in
//! terminal cell coordinates, and the current match the grid highlights.
//!
//! Text is matched per logical line (rows joined across soft wraps). Each
//! character keeps the cell it came from, so wide characters (two cells, the
//! second a continuation) and combining sequences map back to the right
//! columns. Matching is case-sensitive, as the earlier search was.
use super::*;

/// A cell position counted from the oldest retained scrollback line, so it
/// stays put while the owner scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CellPos {
    pub line: usize,
    pub col: u16,
}

/// The cells from `start` up to, not including, `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Match {
    pub start: CellPos,
    pub end: CellPos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Highlight {
    Match,
    Current,
}

pub(super) struct TerminalSearch {
    pub open: bool,
    pub query: TextEdit,
    matches: Vec<Match>,
    current: Option<usize>,
    /// What the matches were found in: query, screen identity, and the number
    /// of retained scrollback lines, which places the viewport among them.
    found: Option<Found>,
}

struct Found {
    query: String,
    screen: usize,
    scrollback: usize,
}

impl Default for TerminalSearch {
    fn default() -> Self {
        Self {
            open: false,
            query: TextEdit::new(""),
            matches: Vec::new(),
            current: None,
            found: None,
        }
    }
}

impl TerminalSearch {
    /// Close the bar and drop its highlight; the query is kept for next time.
    pub fn close(&mut self) {
        self.open = false;
        self.clear();
    }

    /// Forget the matches; new output or another session makes them stale.
    pub fn clear(&mut self) {
        self.matches.clear();
        self.current = None;
        self.found = None;
    }

    pub fn current(&self) -> Option<Match> {
        self.current.map(|i| self.matches[i])
    }

    pub fn count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// Move to the next match (`backward` false: toward older output, the way
    /// terminals search scrollback) or the previous one, wrapping. A changed
    /// query, screen, or stale result searches afresh and starts at the most
    /// recent match. Scrolls `screen` so the current match is visible.
    pub fn step(
        &mut self,
        screen: &mut vt100::Screen,
        key: usize,
        backward: bool,
    ) -> Option<Match> {
        let fresh = !self.found.as_ref().is_some_and(|f| {
            f.query == self.query.text && f.screen == key && f.scrollback == scrollback_len(screen)
        });
        if fresh {
            self.matches = find_all(screen, &self.query.text);
            self.found = Some(Found {
                query: self.query.text.clone(),
                screen: key,
                scrollback: scrollback_len(screen),
            });
            self.current = self.matches.len().checked_sub(1);
        } else if let Some(i) = self.current {
            let n = self.matches.len();
            self.current = Some(if backward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            });
        }
        let current = self.current()?;
        reveal(screen, current);
        Some(current)
    }

    /// Which highlight, if any, a visible cell of `screen` carries.
    pub fn highlighter<'a>(
        &'a self,
        screen: &vt100::Screen,
        key: usize,
    ) -> impl Fn(u16, u16) -> Option<Highlight> + 'a {
        let found = self
            .found
            .as_ref()
            .filter(|f| f.screen == key && f.query == self.query.text);
        let top = found.map(|f| f.scrollback.saturating_sub(screen.scrollback()));
        let rows = usize::from(screen.size().0);
        let current = self.current;
        let visible: Vec<(usize, Match)> = match top {
            Some(top) => self
                .matches
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, m)| m.end.line >= top && m.start.line < top + rows)
                .collect(),
            None => Vec::new(),
        };
        move |row, col| {
            let at = CellPos {
                line: top? + usize::from(row),
                col,
            };
            let (i, _) = visible.iter().find(|(_, m)| m.start <= at && at < m.end)?;
            Some(if Some(*i) == current {
                Highlight::Current
            } else {
                Highlight::Match
            })
        }
    }
}

/// How many scrollback lines the screen retains: the deepest scroll offset.
fn scrollback_len(screen: &mut vt100::Screen) -> usize {
    let offset = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let len = screen.scrollback();
    screen.set_scrollback(offset);
    len
}

/// Every non-overlapping match of `needle`, oldest first. The screen's scroll
/// position is left as it was.
pub(super) fn find_all(screen: &mut vt100::Screen, needle: &str) -> Vec<Match> {
    if needle.is_empty() {
        return Vec::new();
    }
    let offset = screen.scrollback();
    let scrollback = scrollback_len(screen);
    let (rows, cols) = screen.size();
    let total = scrollback + usize::from(rows);
    let mut matches = Vec::new();
    // One logical line: its text and, per character run, the byte where the
    // run starts, the cell it came from, and how many cells that cell spans.
    let mut text = String::new();
    let mut cells: Vec<(usize, CellPos, u16)> = Vec::new();
    let mut flush = |text: &mut String, cells: &mut Vec<(usize, CellPos, u16)>| {
        for (start, found) in text.match_indices(needle) {
            let last = start + found.len() - 1;
            let cell = |byte: usize| cells[cells.partition_point(|c| c.0 <= byte) - 1];
            let (_, first, _) = cell(start);
            let (_, end, width) = cell(last);
            matches.push(Match {
                start: first,
                end: CellPos {
                    line: end.line,
                    col: end.col + width,
                },
            });
        }
        text.clear();
        cells.clear();
    };
    let mut line = 0;
    while line < total {
        // Read a screenful at a time: visible row `r` is line `top + r`.
        let top = line.min(scrollback);
        screen.set_scrollback(scrollback - top);
        let end = (top + usize::from(rows)).min(total);
        for absolute in line..end {
            let row = (absolute - top) as u16;
            let mut kept = text.len();
            let mut kept_cells = cells.len();
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let contents = cell.contents();
                cells.push((
                    text.len(),
                    CellPos {
                        line: absolute,
                        col,
                    },
                    if cell.is_wide() { 2 } else { 1 },
                ));
                text.push_str(if contents.is_empty() { " " } else { contents });
                if !contents.is_empty() {
                    kept = text.len();
                    kept_cells = cells.len();
                }
            }
            if !screen.row_wrapped(row) {
                // Blank cells after the last character are not part of the line.
                text.truncate(kept);
                cells.truncate(kept_cells);
                flush(&mut text, &mut cells);
            }
        }
        line = end;
    }
    flush(&mut text, &mut cells);
    screen.set_scrollback(offset);
    matches
}

/// Scroll so `m` is in view, leaving the viewport alone when it already is.
pub(super) fn reveal(screen: &mut vt100::Screen, m: Match) {
    let scrollback = scrollback_len(screen);
    let rows = usize::from(screen.size().0);
    let top = scrollback - screen.scrollback();
    let last = if m.end.col == 0 {
        m.end.line.saturating_sub(1)
    } else {
        m.end.line
    };
    if m.start.line >= top && last < top + rows {
        return;
    }
    // Leave a third of the viewport of context above the match.
    let top = m.start.line.saturating_sub(rows / 3);
    screen.set_scrollback(scrollback.saturating_sub(top));
}

impl State {
    /// Identity of the screen the terminal shows, so matches stay with it.
    pub(super) fn screen_key(&self) -> usize {
        self.active().map_or(0, Session::id)
    }

    /// Enter and Shift+Enter in the search field: step and report the result.
    /// Focus stays in the field.
    pub(super) fn step_terminal_search(&mut self, backward: bool) {
        let key = self.screen_key();
        let mut search = std::mem::take(&mut self.terminal_search);
        let found = self.with_screen_mut(|screen| search.step(screen, key, backward));
        self.message = match (found, search.current_index()) {
            (Some(_), Some(i)) => format!("Match {} of {}", search.count() - i, search.count()),
            _ if search.query.text.is_empty() => String::new(),
            _ => "No match in retained scrollback".into(),
        };
        self.terminal_search = search;
        self.selection = None;
    }

    /// Find in Terminal: show the terminal with the search bar open and the
    /// keyboard in its field.
    pub(super) fn open_terminal_search(&mut self, tx: &mut Tx) {
        self.workspace.layout.terminal = true;
        self.workspace.set_terminal_focus(true);
        self.workspace.explorer_revealed = false;
        self.terminal_search.open = true;
        tx.focus_when_shown("search-query", true);
    }

    /// Escape or the close button: hide the bar, drop the highlight, and
    /// return the keyboard to the terminal.
    pub(super) fn close_terminal_search(&mut self, tx: &mut Tx) {
        self.terminal_search.close();
        self.workspace.set_terminal_focus(true);
        tx.blur();
    }
}

/// The search bar above the terminal grid.
pub(super) fn bar(s: &State, a: &Appearance) -> El {
    row()
        .key("terminal-search")
        .w_full()
        .px(12.0)
        .py(6.0)
        .gap(6.0)
        .fill(a.panel())
        .child(
            text_input(&s.terminal_search.query, ui_label())
                .key("search-query")
                .role("searchbox")
                .label("Find in terminal")
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .h(29.0)
                .placeholder("Find in terminal…", a.muted())
                .color(a.ink())
                .border(1.0, a.kit().c.border_base)
                .radius(6.0)
                .caret(a.accent(), a.selection())
                .on_edit(|s: &mut State| &mut s.terminal_search.query)
                .on_input_key(|s: &mut State, event, tx| match event.key {
                    Key::Enter => {
                        s.step_terminal_search(event.mods.shift);
                        true
                    }
                    Key::Escape => {
                        s.close_terminal_search(tx);
                        true
                    }
                    _ => false,
                }),
        )
        .child(button(a, "close-search", "×", |s, tx| {
            s.close_terminal_search(tx)
        }))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, runtime_for};
    use super::*;

    fn screen(rows: u16, cols: u16, bytes: &[u8]) -> vt100::Screen {
        let mut parser = vt100::Parser::new(rows, cols, 1000);
        parser.process(bytes);
        parser.screen().clone()
    }

    fn at(line: usize, col: u16) -> CellPos {
        CellPos { line, col }
    }

    #[test]
    fn matches_map_to_cells_across_wide_characters() {
        let mut s = screen(4, 40, "日本語 abc 日本語".as_bytes());
        // 日本語 takes cells 0–5, the space 6, abc 7–9, space 10, then 日本 11–14.
        assert_eq!(
            find_all(&mut s, "abc"),
            vec![Match {
                start: at(0, 7),
                end: at(0, 10)
            }]
        );
        let japan = find_all(&mut s, "日本");
        assert_eq!(japan.len(), 2);
        assert_eq!(
            japan[1],
            Match {
                start: at(0, 11),
                end: at(0, 15)
            }
        );
        assert_eq!(find_all(&mut s, "語 a")[0].start, at(0, 4));
        assert_eq!(find_all(&mut s, "語 a")[0].end, at(0, 8));
    }

    #[test]
    fn a_match_split_by_a_soft_wrap_spans_both_rows() {
        let mut s = screen(4, 10, b"12345678abcdef");
        assert_eq!(
            find_all(&mut s, "abcd"),
            vec![Match {
                start: at(0, 8),
                end: at(1, 2)
            }]
        );
        // A hard line break is not a wrap.
        let mut s = screen(4, 10, b"1234567ab\r\ncd");
        assert!(find_all(&mut s, "abcd").is_empty());
    }

    #[test]
    fn matches_in_scrollback_are_found_and_revealed() {
        let mut bytes = b"needle here\r\n".to_vec();
        for i in 0..50 {
            bytes.extend(format!("line {i}\r\n").bytes());
        }
        let mut s = screen(5, 20, &bytes);
        let found = find_all(&mut s, "needle");
        assert_eq!(
            found,
            vec![Match {
                start: at(0, 0),
                end: at(0, 6)
            }]
        );
        assert_eq!(s.scrollback(), 0, "searching leaves the viewport alone");
        reveal(&mut s, found[0]);
        assert_eq!(s.cell(0, 0).unwrap().contents(), "n");
    }

    fn search_runtime(bytes: &[u8]) -> rune::Runtime<State> {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().to_owned());
        let mut parser = vt100::Parser::new(6, 40, 1000);
        parser.process(bytes);
        state.preview = Some(parser.screen().clone());
        let mut rt = runtime_for(state);
        // Find in Terminal needs a live session; a preview screen keeps these
        // regressions deterministic, so open the bar the way that action does.
        rt.update(|s, tx| {
            s.terminal_search.open = true;
            tx.focus_when_shown("search-query", true);
        });
        rt.frame(0.0);
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    fn query_focused(rt: &rune::Runtime<State>) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("search-query") && e.focused)
    }

    fn highlighted(rt: &rune::Runtime<State>, kind: &str) -> Vec<String> {
        let prefix = format!("terminal-search-{kind}-");
        let mut keys: Vec<String> = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.key.filter(|k| k.starts_with(&prefix) && !e.exiting))
            .map(|k| k[prefix.len()..].to_owned())
            .collect();
        keys.sort();
        keys
    }

    fn enter(rt: &mut rune::Runtime<State>, shift: bool) {
        press(
            rt,
            Key::Enter,
            rune::Mods {
                shift,
                ..Default::default()
            },
        );
        rt.settle_all();
        rt.frame(0.0);
    }

    #[test]
    fn enter_highlights_the_current_wide_character_match_and_keeps_focus() {
        let mut rt = search_runtime("日本語 abc 日本語".as_bytes());
        assert!(query_focused(&rt), "Find in Terminal focuses the field");
        rt.update(|s, _| s.terminal_search.query.replace_text("abc"));
        enter(&mut rt, false);
        // One run per cell: abc is cells 7, 8, 9 of row 0.
        assert_eq!(highlighted(&rt, "current"), ["0-7", "0-8", "0-9"]);
        assert!(query_focused(&rt), "Enter keeps focus in the search field");

        rt.update(|s, _| s.terminal_search.query.replace_text("日本"));
        enter(&mut rt, false);
        // The most recent match comes first: the second 日本, cells 11–14,
        // drawn as one run per wide glyph.
        assert_eq!(highlighted(&rt, "current"), ["0-11", "0-13"]);
        assert_eq!(highlighted(&rt, "match"), ["0-0", "0-2"]);
        assert!(query_focused(&rt));
    }

    #[test]
    fn enter_and_shift_enter_cycle_and_escape_closes_and_clears() {
        let mut rt = search_runtime(b"one x\r\ntwo x\r\nthree x\r\n");
        rt.update(|s, _| s.terminal_search.query.replace_text("x"));
        enter(&mut rt, false);
        assert_eq!(highlighted(&rt, "current"), ["2-6"]);
        assert_eq!(rt.state.message, "Match 1 of 3");
        enter(&mut rt, false);
        assert_eq!(highlighted(&rt, "current"), ["1-4"]);
        enter(&mut rt, false);
        assert_eq!(highlighted(&rt, "current"), ["0-4"]);
        enter(&mut rt, false);
        assert_eq!(highlighted(&rt, "current"), ["2-6"], "Enter wraps");
        enter(&mut rt, true);
        assert_eq!(
            highlighted(&rt, "current"),
            ["0-4"],
            "Shift+Enter goes back"
        );
        assert_eq!(highlighted(&rt, "match"), ["1-4", "2-6"]);
        assert!(query_focused(&rt));

        press(&mut rt, Key::Escape, Default::default());
        rt.settle_all();
        rt.frame(0.0);
        assert!(!rt.state.terminal_search.open);
        assert!(rt.find("search-query").is_none(), "Escape closes search");
        assert!(highlighted(&rt, "current").is_empty());
        assert!(highlighted(&rt, "match").is_empty());
        assert!(rt.state.workspace.terminal_focus());
    }

    #[test]
    fn a_match_in_scrollback_scrolls_into_view() {
        let mut bytes = b"needle\r\n".to_vec();
        for i in 0..40 {
            bytes.extend(format!("line {i}\r\n").bytes());
        }
        let mut rt = search_runtime(&bytes);
        assert_eq!(rt.state.with_screen(vt100::Screen::scrollback), 0);
        rt.update(|s, _| s.terminal_search.query.replace_text("needle"));
        enter(&mut rt, false);
        assert!(rt.state.with_screen(vt100::Screen::scrollback) > 0);
        assert_eq!(
            highlighted(&rt, "current"),
            ["0-0", "0-1", "0-2", "0-3", "0-4", "0-5"]
        );
        assert!(query_focused(&rt));
    }
}
