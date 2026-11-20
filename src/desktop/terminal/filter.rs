//! Filter Terminal Output: a view over one session's retained output that
//! lists only the lines matching a substring or regular expression, and keeps
//! adding matching lines as output arrives. Lines come from the session's raw
//! output log (control sequences removed), so window resizes and soft wraps
//! never split or merge them. The PTY, scrollback and other sessions are
//! untouched; closing the view returns the terminal to where it was scrolled.
use crate::desktop::*;
use std::collections::VecDeque;

/// Lines kept for filtering, newest last. Older ones drop as the log does.
const KEPT_LINES: usize = 50_000;
/// Bytes read from the log per poll, so a flood never stalls a frame.
const READ_PER_POLL: usize = 256 * 1024;
/// Matches listed in the view; the count covers them all.
pub(crate) const SHOWN: usize = 500;

#[derive(Clone, Debug)]
struct Line {
    /// Where the line started in the session's output log.
    offset: u64,
    text: String,
}

pub(crate) struct LogFilter {
    pub open: bool,
    pub query: TextEdit,
    pub regex: bool,
    pub case_sensitive: bool,
    /// The session this view filters, by identity.
    session: usize,
    /// The scroll position to restore on close.
    restore_scrollback: usize,
    lines: VecDeque<Line>,
    /// The next log offset to read, and the start of an unfinished line.
    cursor: u64,
    partial: Vec<u8>,
    partial_offset: u64,
    /// Whether output before the first line had already left the log.
    pub truncated: bool,
    compiled: Option<(String, bool, bool, Result<Matcher, String>)>,
    /// What the view shows, recomputed by [`LogFilter::refresh`].
    pub total: usize,
    pub shown: Vec<(String, usize)>,
    pub error: Option<String>,
    refreshed: Option<(String, bool, bool, u64)>,
}

enum Matcher {
    Substring(String, bool),
    Regex(regex_lite::Regex),
}

impl Matcher {
    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Substring(needle, true) => line.contains(needle.as_str()),
            Self::Substring(needle, false) => line.to_lowercase().contains(needle.as_str()),
            Self::Regex(re) => re.is_match(line),
        }
    }
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            open: false,
            query: TextEdit::new(""),
            regex: false,
            case_sensitive: false,
            session: 0,
            restore_scrollback: 0,
            lines: VecDeque::new(),
            cursor: 0,
            partial: Vec::new(),
            partial_offset: 0,
            truncated: false,
            compiled: None,
            total: 0,
            shown: Vec::new(),
            error: None,
            refreshed: None,
        }
    }
}

/// One line as the terminal finally showed it: a carriage return without a
/// newline (a progress bar) rewrites the line from its start.
fn finished_text(bytes: &[u8]) -> String {
    // `strip` turns a lone carriage return into a newline.
    let text = crate::ansi::strip(bytes);
    text.trim_end_matches('\n')
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .to_owned()
}

impl LogFilter {
    pub fn session(&self) -> usize {
        self.session
    }

    /// Read new output from `log`, a session's output log, splitting it into
    /// lines and dropping the ones the log has since evicted.
    pub(crate) fn ingest(&mut self, log: &crate::session::OutputLog) {
        let start = log.start();
        if self.cursor < start {
            // Output we never read was evicted: start over at what's left.
            self.truncated = true;
            self.partial.clear();
            self.cursor = start;
            self.partial_offset = start;
        }
        let (from, bytes, next) = log.read(self.cursor, READ_PER_POLL);
        if from != self.cursor {
            self.truncated = true;
            self.partial.clear();
            self.partial_offset = from;
        }
        self.cursor = next;
        let mut line_start = 0;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                self.partial.extend_from_slice(&bytes[line_start..i]);
                let text = finished_text(&self.partial);
                self.lines.push_back(Line {
                    offset: self.partial_offset,
                    text,
                });
                self.partial.clear();
                line_start = i + 1;
                self.partial_offset = from + line_start as u64;
            }
        }
        self.partial.extend_from_slice(&bytes[line_start..]);
        while self.lines.len() > KEPT_LINES || self.lines.front().is_some_and(|l| l.offset < start)
        {
            self.lines.pop_front();
            self.truncated = true;
        }
    }

    fn compile(&mut self) {
        let query = self.query.text.clone();
        let fresh = self.compiled.as_ref().is_some_and(|(q, r, c, _)| {
            *q == query && *r == self.regex && *c == self.case_sensitive
        });
        if fresh {
            return;
        }
        let matcher = if self.regex {
            regex_lite::RegexBuilder::new(&query)
                .case_insensitive(!self.case_sensitive)
                .size_limit(1 << 20)
                .build()
                .map(Matcher::Regex)
                .map_err(|e| {
                    e.to_string()
                        .lines()
                        .last()
                        .unwrap_or("invalid pattern")
                        .trim()
                        .to_owned()
                })
        } else if self.case_sensitive {
            Ok(Matcher::Substring(query.clone(), true))
        } else {
            Ok(Matcher::Substring(query.to_lowercase(), false))
        };
        self.compiled = Some((query, self.regex, self.case_sensitive, matcher));
    }

    /// Recompute the count, the listed lines and any pattern error when the
    /// query, its options or the lines changed. The newest [`SHOWN`] matches
    /// are listed, each with the number of identical matches after it.
    pub fn refresh(&mut self) {
        let key = (
            self.query.text.clone(),
            self.regex,
            self.case_sensitive,
            self.cursor,
        );
        if self.refreshed.as_ref() == Some(&key) {
            return;
        }
        self.refreshed = Some(key);
        self.total = 0;
        self.shown.clear();
        self.error = None;
        if self.query.text.is_empty() {
            return;
        }
        self.compile();
        let matcher = match self.compiled.as_ref().map(|(_, _, _, m)| m) {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                self.error = Some(e.clone());
                return;
            }
            None => return,
        };
        let partial = (!self.partial.is_empty()).then(|| finished_text(&self.partial));
        let all: Vec<&str> = self
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .chain(partial.as_deref())
            .filter(|l| matcher.is_match(l))
            .collect();
        let total = all.len();
        let first = total.saturating_sub(SHOWN);
        let mut seen: std::collections::HashMap<&str, usize> = Default::default();
        let mut shown: Vec<(String, usize)> = all[first..]
            .iter()
            .rev()
            .map(|line| {
                let after = seen.entry(line).or_default();
                let entry = ((*line).to_owned(), *after);
                *after += 1;
                entry
            })
            .collect();
        shown.reverse();
        self.total = total;
        self.shown = shown;
    }
}

impl State {
    /// Open the filter view on the active terminal.
    pub(crate) fn open_log_filter(&mut self, tx: &mut Tx) {
        let Some(session) = self.active() else {
            return;
        };
        let id = session.id();
        let scrolled = session.with_screen(vt100::Screen::scrollback);
        if !self.log_filter.open || self.log_filter.session != id {
            let query = std::mem::replace(&mut self.log_filter.query, TextEdit::new(""));
            let (regex, case) = (self.log_filter.regex, self.log_filter.case_sensitive);
            self.log_filter = LogFilter {
                open: true,
                query,
                regex,
                case_sensitive: case,
                session: id,
                restore_scrollback: scrolled,
                ..LogFilter::default()
            };
        }
        self.workspace.layout.terminal = true;
        self.poll_log_filter();
        tx.focus_when_shown("log-filter-query", true);
    }

    /// Close the view: the terminal shows what it showed before.
    pub(crate) fn close_log_filter(&mut self, tx: &mut Tx) {
        if !self.log_filter.open {
            return;
        }
        let (id, restore) = (self.log_filter.session, self.log_filter.restore_scrollback);
        self.log_filter.open = false;
        self.log_filter.lines.clear();
        self.log_filter.partial.clear();
        if let Some(session) = self.sessions.iter().find(|s| s.id() == id) {
            session.with_screen_mut(|screen| screen.set_scrollback(restore));
        }
        self.workspace.set_terminal_focus(true);
        tx.blur();
    }

    /// Clear the query; the view stays open.
    pub(crate) fn clear_log_filter(&mut self) {
        self.log_filter.query = TextEdit::new("");
        self.log_filter.refresh();
    }

    /// Read the filtered session's new output. Closes the view when its
    /// session ended.
    pub(crate) fn poll_log_filter(&mut self) -> bool {
        if !self.log_filter.open {
            return false;
        }
        let id = self.log_filter.session;
        let Some(session) = self.sessions.iter().find(|s| s.id() == id) else {
            self.log_filter.open = false;
            self.message = "The filtered terminal has ended.".into();
            return true;
        };
        let state = session.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = (self.log_filter.cursor, self.log_filter.refreshed.clone());
        self.log_filter.ingest(&state.output);
        drop(state);
        self.log_filter.refresh();
        before != (self.log_filter.cursor, self.log_filter.refreshed.clone())
    }

    /// Close the view and scroll the terminal to a listed line, when it is
    /// still in the terminal's scrollback. `after` is how many identical
    /// lines follow it, so repeated lines land on the right one.
    pub(crate) fn jump_to_filtered_line(&mut self, line: &str, after: usize, tx: &mut Tx) {
        let id = self.log_filter.session;
        self.close_log_filter(tx);
        let Some(session) = self.sessions.iter().find(|s| s.id() == id) else {
            return;
        };
        let found = session.with_screen_mut(|screen| {
            // Only a whole-line match: a longer line containing it is another.
            let whole: Vec<_> = terminal::search::find_lines(screen, line, usize::MAX)
                .into_iter()
                .filter(|m| m.at.start.col == 0 && m.text == line)
                .map(|m| m.at)
                .collect();
            let m = whole.iter().rev().nth(after).copied()?;
            terminal::search::reveal(screen, m);
            Some(())
        });
        if found.is_none() {
            self.message = "That line is no longer in the terminal's scrollback.".into();
        }
    }
}

/// The filter bar and the list of matching lines, in place of the grid.
pub(crate) fn view(s: &State, a: &Appearance) -> El {
    let error = s.log_filter.error.clone();
    let (total, shown) = (s.log_filter.total, s.log_filter.shown.clone());
    let status = match &error {
        Some(e) => format!("Invalid pattern: {e}"),
        None if s.log_filter.query.text.is_empty() => {
            "Type to filter this terminal's output".into()
        }
        None if total > shown.len() => {
            format!("{total} lines match (showing the last {})", shown.len())
        }
        None => format!("{total} line{} match", if total == 1 { "" } else { "es" }),
    };
    let status = if s.log_filter.truncated && error.is_none() {
        format!("{status} · earlier output is no longer retained")
    } else {
        status
    };
    let bar = row()
        .key("log-filter")
        .w_full()
        .px(12.0)
        .py(6.0)
        .gap(6.0)
        .fill(a.panel())
        .child(
            text_input(&s.log_filter.query, ui_label())
                .key("log-filter-query")
                .role("searchbox")
                .label("Filter terminal output")
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .px(8.0)
                .py(5.0)
                .placeholder(
                    if s.log_filter.regex {
                        "Filter by regular expression…"
                    } else {
                        "Filter lines containing…"
                    },
                    a.muted(),
                )
                .color(a.ink())
                .border(
                    1.0,
                    if error.is_some() {
                        a.danger()
                    } else {
                        a.kit().c.border_base
                    },
                )
                .radius(6.0)
                .caret(a.accent(), a.selection())
                .on_edit(|s: &mut State| &mut s.log_filter.query)
                .on_input_change(|s: &mut State, _| s.log_filter.refresh())
                .on_input_key(|s: &mut State, event, tx| match event.key {
                    Key::Escape => {
                        s.close_log_filter(tx);
                        true
                    }
                    _ => false,
                }),
        )
        .child(
            button(a, "log-filter-regex", ".*", |s, _| {
                s.log_filter.regex = !s.log_filter.regex;
                s.log_filter.refresh();
            })
            .label("Regular expression")
            .help("Regular expression")
            .toggle_state(a, s.log_filter.regex),
        )
        .child(
            button(a, "log-filter-case", "Aa", |s, _| {
                s.log_filter.case_sensitive = !s.log_filter.case_sensitive;
                s.log_filter.refresh();
            })
            .label("Match case")
            .help("Match case")
            .toggle_state(a, s.log_filter.case_sensitive),
        )
        .child(button(a, "log-filter-clear", "Clear", |s, _| {
            s.clear_log_filter()
        }))
        .child(glyph_button(
            a,
            "log-filter-close",
            "×",
            "Close filter",
            |s, tx| s.close_log_filter(tx),
        ));
    let style = fonts::Metrics::measure().style;
    let mut list = col()
        .key("log-filter-lines")
        .role("list")
        .label("Matching lines")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(8.0)
        .scroll(rune::ScrollSpec { stick_bottom: true });
    for (i, (line, after)) in shown.into_iter().enumerate() {
        let target = line.clone();
        list = list.child(
            row()
                .key(format!("log-filter-line-{i}"))
                .role("button")
                .label(line.clone())
                .w_full()
                .px(8.0)
                .py(1.0)
                .hover(a.hover_bg())
                .focusable(true)
                .on_press_tx(move |s: &mut State, tx| s.jump_to_filtered_line(&target, after, tx))
                .child(text(line).font(style).color(a.ink())),
        );
    }
    col()
        .key("log-filter-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(bar)
        .child(
            text(status)
                .key("log-filter-status")
                .role("status")
                .color(if error.is_some() {
                    a.danger()
                } else {
                    a.muted()
                })
                .px(12.0)
                .py(4.0),
        )
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(chunks: &[&[u8]], capacity: usize) -> crate::session::OutputLog {
        let mut log = crate::session::OutputLog::new(capacity);
        for c in chunks {
            log.push(c);
        }
        log
    }

    #[test]
    fn substring_regex_case_and_incoming_lines_filter_without_touching_the_log() {
        let mut f = LogFilter::default();
        let mut l = log(
            &[
                b"\x1b[32mINFO\x1b[0m req=1 ok\r\nWARN req=2 slow\r\n",
                b"progress 10%\rprogress 100%\r\ninfo req=1 done\r\nhalf",
            ],
            1 << 20,
        );
        f.ingest(&l);
        f.query = TextEdit::new("req=1");
        f.refresh();
        let (n, lines) = (f.total, f.shown.clone());
        assert_eq!(n, 2);
        assert_eq!(lines[0].0, "INFO req=1 ok", "ANSI colors are gone");
        f.case_sensitive = true;
        f.query = TextEdit::new("info");
        f.refresh();
        assert_eq!(f.total, 1);
        f.regex = true;
        f.query = TextEdit::new(r"^progress \d+%$");
        f.refresh();
        assert_eq!(
            f.shown[0].0, "progress 100%",
            "a carriage return rewrites the line"
        );
        f.query = TextEdit::new("req=(");
        f.refresh();
        assert!(f.error.is_some(), "invalid regex");
        assert_eq!(f.total, 0);
        // New output arrives: the incomplete line finishes and more appends.
        l.push(b"-line req=3\r\nother\r\n");
        f.ingest(&l);
        f.regex = false;
        f.case_sensitive = false;
        f.query = TextEdit::new("req=");
        f.refresh();
        let (n, lines) = (f.total, f.shown.clone());
        assert_eq!(n, 4);
        assert_eq!(lines.last().unwrap().0, "half-line req=3");
        assert!(!f.truncated);
        assert_eq!(l.start(), 0, "the log is untouched");
    }

    #[test]
    fn evicted_output_drops_its_lines_and_is_reported() {
        let mut f = LogFilter::default();
        let mut l = log(&[b"old 1\nold 2\n"], 64);
        f.ingest(&l);
        for i in 0..20 {
            l.push(format!("new {i}\n").as_bytes());
        }
        f.ingest(&l);
        f.query = TextEdit::new("old");
        f.refresh();
        assert_eq!(f.total, 0, "evicted lines can't be matched or jumped to");
        assert!(f.truncated);
        f.query = TextEdit::new("new 19");
        f.refresh();
        assert_eq!(f.total, 1);
        // A flood: each poll reads a bounded slice and stays quick.
        let mut l = log(&[], 1 << 20);
        let mut f = LogFilter::default();
        f.query = TextEdit::new("req=9");
        for i in 0..200_000 {
            l.push(format!("2024-01-01T00:00:00Z INFO req={i} handled\n").as_bytes());
        }
        let mut polls = 0;
        loop {
            let started = std::time::Instant::now();
            let before = f.cursor;
            f.ingest(&l);
            f.refresh();
            assert!(
                started.elapsed() < std::time::Duration::from_millis(250),
                "poll took {:?}",
                started.elapsed()
            );
            polls += 1;
            if f.cursor == before {
                break;
            }
        }
        assert!(polls > 2, "the flood is read over several polls");
        assert!(
            f.truncated,
            "most of the flood was evicted before it was read"
        );
        assert!(f.shown.iter().all(|(l, _)| l.contains("req=9")));
    }
}
