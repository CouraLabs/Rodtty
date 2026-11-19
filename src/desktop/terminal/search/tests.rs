use super::*;
use crate::desktop::tests::{press, runtime_for};

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
