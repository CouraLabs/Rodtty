use super::*;
use crate::desktop::terminal::view::SELECTION_THRESHOLD;
use crate::desktop::tests::runtime_for;

fn grid_origin(rt: &rune::Runtime<State>) -> (f32, f32) {
    rt.find("terminal").expect("terminal grid")
}

/// A press that never travels is a click: it must not highlight a cell.
#[test]
fn a_click_selects_nothing() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(b"hello world\r\n");
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    let (x, y) = grid_origin(&rt);
    rt.pointer_down(x + 20.0, y + 10.0);
    rt.pointer_up(x + 20.0, y + 10.0);
    rt.frame(0.0);
    assert!(
        rt.state.selection.is_none(),
        "a click leaves no selection: {:?}",
        rt.state.selection
    );
}

/// Contact wobble under the threshold is still a click.
#[test]
fn a_press_that_barely_drifts_selects_nothing() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(b"hello world\r\n");
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    let (x, y) = grid_origin(&rt);
    let start = (x + 20.0, y + 10.0);
    rt.pointer_down(start.0, start.1);
    // Less than the threshold, in both axes, over several move events.
    for step in 1..=3 {
        let d = step as f32;
        rt.pointer_move(start.0 + d, start.1);
        rt.frame(0.0);
    }
    rt.pointer_up(start.0 + 3.0, start.1);
    rt.frame(0.0);
    assert!(
        rt.state.selection.is_none(),
        "drift under {SELECTION_THRESHOLD} pt is a click: {:?}",
        rt.state.selection
    );
}

/// Past the threshold the drag selects, and the range spans the travel.
#[test]
fn a_real_drag_selects_and_keeps_the_range() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(b"hello world\r\n");
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    let (x, y) = grid_origin(&rt);
    let start = (x + 20.0, y + 10.0);
    rt.pointer_down(start.0, start.1);
    // Well past the threshold: several cells of travel.
    rt.pointer_move(start.0 + 40.0, start.1);
    rt.frame(0.0);
    assert!(
        rt.state.selection.is_some(),
        "a drag past {SELECTION_THRESHOLD} pt selects"
    );
    let (a, b) = rt.state.selection.unwrap();
    assert_ne!(a, b, "the range covers the travel, not a single cell");
    rt.pointer_up(start.0 + 40.0, start.1);
    rt.frame(0.0);
    assert_eq!(
        rt.state.selection,
        Some((a, b)),
        "releasing the drag keeps the selection it made"
    );
}

/// A press on the grid drops a selection the owner left behind.
#[test]
fn a_press_on_the_grid_clears_a_previous_selection() {
    let root = crate::test_support::tempdir().unwrap();
    let mut state = State::with_root(root.path().into());
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(b"hello world\r\n");
    state.preview = Some(parser.screen().clone());
    state.selection = Some(((0, 0), (0, 5)));
    let mut rt = runtime_for(state);
    let (x, y) = grid_origin(&rt);
    rt.pointer_down(x + 40.0, y + 30.0);
    rt.pointer_up(x + 40.0, y + 30.0);
    rt.frame(0.0);
    assert!(
        rt.state.selection.is_none(),
        "the old highlight is gone: {:?}",
        rt.state.selection
    );
}
