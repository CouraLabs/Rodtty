//! Find in Terminal: matches over retained scrollback and the live screen, in
//! terminal cell coordinates, and the current match the grid highlights.
//!
//! Text is matched per logical line (rows joined across soft wraps). Each
//! character keeps the cell it came from, so wide characters (two cells, the
//! second a continuation) and combining sequences map back to the right
//! columns. Matching is case-sensitive, as the earlier search was.
use crate::desktop::*;

/// A cell position counted from the oldest retained scrollback line, so it
/// stays put while the owner scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CellPos {
    pub line: usize,
    pub col: u16,
}

/// The cells from `start` up to, not including, `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Match {
    pub start: CellPos,
    pub end: CellPos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Highlight {
    Match,
    Current,
}

pub(crate) struct TerminalSearch {
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

    /// Make `at`, a match of `query` in `screen`, the current match and scroll
    /// to it, as a search result row asks. Returns whether it was still there.
    pub fn select(
        &mut self,
        screen: &mut vt100::Screen,
        key: usize,
        query: &str,
        at: Match,
    ) -> bool {
        self.query = TextEdit::new(query);
        self.matches = find_in_lines(screen, query, usize::MAX, false, true)
            .into_iter()
            .map(|found| found.at)
            .collect();
        self.found = Some(Found {
            query: query.to_owned(),
            screen: key,
            scrollback: scrollback_len(screen),
        });
        self.current = self
            .matches
            .iter()
            .position(|m| *m == at)
            .or_else(|| self.matches.len().checked_sub(1));
        if let Some(current) = self.current() {
            reveal(screen, current);
        }
        self.current().is_some_and(|m| m == at)
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
pub(crate) fn find_all(screen: &mut vt100::Screen, needle: &str) -> Vec<Match> {
    find_in_lines(screen, needle, usize::MAX, false, false)
        .into_iter()
        .map(|found| found.at)
        .collect()
}

/// A match with the logical line it is in, for listing as a search result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LineMatch {
    pub at: Match,
    /// The line's text, leading spaces trimmed.
    pub text: String,
    /// The match's bytes within `text`.
    pub highlight: std::ops::Range<usize>,
}

/// Up to `limit` matches of `needle` with their lines, oldest first, ignoring
/// ASCII case as the file search does. The screen's scroll position is left
/// as it was.
pub(crate) fn find_lines(screen: &mut vt100::Screen, needle: &str, limit: usize) -> Vec<LineMatch> {
    find_in_lines(screen, needle, limit, true, true)
}

fn find_in_lines(
    screen: &mut vt100::Screen,
    needle: &str,
    limit: usize,
    with_text: bool,
    ignore_case: bool,
) -> Vec<LineMatch> {
    if needle.is_empty() || limit == 0 {
        return Vec::new();
    }
    // ASCII case folding keeps every byte where it was, so offsets found in
    // the folded text hold in the original.
    let needle = if ignore_case {
        needle.to_ascii_lowercase()
    } else {
        needle.to_owned()
    };
    let needle = needle.as_str();
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
        let folded;
        let haystack = if ignore_case {
            folded = text.to_ascii_lowercase();
            folded.as_str()
        } else {
            text.as_str()
        };
        for (start, found) in haystack.match_indices(needle) {
            if matches.len() >= limit {
                break;
            }
            let last = start + found.len() - 1;
            let cell = |byte: usize| cells[cells.partition_point(|c| c.0 <= byte) - 1];
            let (_, first, _) = cell(start);
            let (_, end, width) = cell(last);
            let at = Match {
                start: first,
                end: CellPos {
                    line: end.line,
                    col: end.col + width,
                },
            };
            let (line, highlight) = if with_text {
                let trimmed = text.trim_start();
                let skip = text.len() - trimmed.len();
                (
                    trimmed.to_owned(),
                    start.saturating_sub(skip)..(start + found.len()).saturating_sub(skip),
                )
            } else {
                (String::new(), 0..0)
            };
            matches.push(LineMatch {
                at,
                text: line,
                highlight,
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
pub(crate) fn reveal(screen: &mut vt100::Screen, m: Match) {
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
    pub(crate) fn screen_key(&self) -> usize {
        self.active().map_or(0, Session::id)
    }

    /// Enter and Shift+Enter in the search field: step and report the result.
    /// Focus stays in the field.
    pub(crate) fn step_terminal_search(&mut self, backward: bool) {
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
    pub(crate) fn open_terminal_search(&mut self, tx: &mut Tx) {
        self.workspace.layout.terminal = true;
        self.workspace.set_terminal_focus(true);
        self.workspace.explorer_revealed = false;
        self.terminal_search.open = true;
        tx.focus_when_shown("search-query", true);
    }

    /// Escape or the close button: hide the bar, drop the highlight, and
    /// return the keyboard to the terminal.
    pub(crate) fn close_terminal_search(&mut self, tx: &mut Tx) {
        self.terminal_search.close();
        self.workspace.set_terminal_focus(true);
        tx.blur();
    }
}

/// The search bar above the terminal grid.
pub(crate) fn bar(s: &State, a: &Appearance) -> El {
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
                .px(8.0)
                .py(5.0)
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
mod tests;
