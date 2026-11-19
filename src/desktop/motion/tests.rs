use super::super::tests::{open_palette, press, runtime_for};
use super::super::{Pending, State};
use super::*;
use rune::{ElementInfo, Key, Runtime};
use std::path::Path;

/// Frames are stepped at 240 Hz so a duration reads to about 4 ms.
const DT: f64 = 1.0 / 240.0;
/// The accepted span of an entrance or exit, in seconds, with one frame of slack.
const SPAN: std::ops::RangeInclusive<f64> = 0.12..=(0.16 + DT);

fn live(rt: &Runtime<State>, key: &str) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
}

fn any(rt: &Runtime<State>, key: &str) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
}

fn landed(e: &ElementInfo) -> bool {
    e.rect
        .iter()
        .zip(e.target)
        .all(|(r, t)| (r - t).abs() < 0.5)
}

fn at_rest(e: &ElementInfo) -> bool {
    e.opacity >= 0.99 && (e.scale - 1.0).abs() < 0.01 && landed(e)
}

/// Seconds from the input until `key` shows whole and in place. Panics if it
/// was already whole on the first frame, which would mean no transition ran.
fn entrance(rt: &mut Runtime<State>, key: &str) -> f64 {
    let mut t = 0.0;
    loop {
        rt.frame(DT);
        t += DT;
        let e = live(rt, key).unwrap_or_else(|| panic!("{key} never mounted"));
        if at_rest(&e) {
            assert!(t > DT, "{key} showed whole at once: no entrance ran");
            return t;
        }
        assert!(t < 1.0, "{key} never settled: {e:?}");
    }
}

/// Seconds from the input until `key` has faded out (or left the tree).
fn exit(rt: &mut Runtime<State>, key: &str) -> f64 {
    let mut t = 0.0;
    loop {
        rt.frame(DT);
        t += DT;
        match any(rt, key) {
            None => return t,
            Some(e) if e.opacity <= 0.01 => return t,
            Some(e) => {
                assert!(e.exiting, "{key} is still live: {e:?}");
                assert!(t < 1.0, "{key} never left: {e:?}");
            }
        }
    }
}

/// One frame after the input, `key` and everything in it are whole and in
/// place (`shown`), or gone.
fn instant(rt: &mut Runtime<State>, key: &str, shown: bool) {
    rt.frame(DT);
    match (shown, any(rt, key)) {
        (true, Some(e)) => {
            let moving: Vec<_> = rt
                .elements()
                .into_iter()
                .filter(|d| d.path.starts_with(&e.path) && (d.moving || !landed(d)))
                .map(|d| (d.path, d.opacity, d.scale))
                .collect();
            assert!(
                moving.is_empty(),
                "{key} animated under Reduce Motion: {moving:?}"
            );
        }
        (false, None) => {}
        (true, None) => panic!("{key} is missing"),
        (false, Some(e)) => panic!("{key} lingered under Reduce Motion: {e:?}"),
    }
}

fn runtime(root: &Path, reduced: bool) -> Runtime<State> {
    let mut state = State::with_root(root.join("profile"));
    state.directory = root.to_owned();
    state.workspace_setup();
    state.navigate(root.to_owned());
    state.workspace.layout.explorer = true;
    let mut rt = runtime_for(state);
    rt.set_system_reduced_motion(reduced);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<State>) {
    settle_until(rt, |_| true);
}

fn settle_until(rt: &mut Runtime<State>, done: impl Fn(&State) -> bool) {
    let start = std::time::Instant::now();
    loop {
        rt.update(|s, tx| {
            s.workspace_poll(tx);
        });
        rt.settle_all();
        rt.frame(1.0);
        if done(&rt.state) {
            return;
        }
        assert!(
            start.elapsed().as_secs() < 8,
            "timed out: {}",
            rt.state.message
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
}

fn escape(rt: &mut Runtime<State>) {
    rt.key(rune::KeyEvent {
        key: Key::Escape,
        mods: Default::default(),
    });
}

/// Opens and closes one element under both motion preferences: animated
/// over 120–160 ms normally, and in the same frame under Reduce Motion.
fn opens_and_closes(
    key: &str,
    setup: impl Fn(&mut Runtime<State>),
    open: impl Fn(&mut Runtime<State>),
    close: impl Fn(&mut Runtime<State>),
) {
    for reduced in [false, true] {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("notes.txt"), "notes").unwrap();
        let mut rt = runtime(root.path(), reduced);
        setup(&mut rt);
        open(&mut rt);
        if reduced {
            instant(&mut rt, key, true);
        } else {
            let t = entrance(&mut rt, key);
            assert!(SPAN.contains(&t), "{key} entered over {t:.3} s");
        }
        rt.settle_all();
        rt.frame(DT);
        close(&mut rt);
        if reduced {
            instant(&mut rt, key, false);
        } else {
            let t = exit(&mut rt, key);
            assert!(SPAN.contains(&t), "{key} exited over {t:.3} s");
        }
    }
}

const ENTRANCES: [Entrance; 6] = [
    Entrance::Holder,
    Entrance::Menu(FROM_TOP_TRAILING),
    Entrance::Dialog,
    Entrance::Fade,
    Entrance::Drawer(16.0, 0.0),
    Entrance::Feedback,
];

#[test]
fn transitions_use_short_ease_out_springs_that_reduce_motion_lands_at_once() {
    let normal = Motion::new(false);
    for spring in [
        normal.spring(),
        normal.springs().enter,
        normal.springs().exit,
    ] {
        assert!((0.12..=0.16).contains(&spring.duration), "{spring:?}");
        // Critically damped: an ease-out with no overshoot.
        assert_eq!(spring.bounce, 0.0);
    }
    for entrance in ENTRANCES {
        assert_eq!(normal.transition(entrance).springs, normal.springs());
    }
    let reduced = Motion::new(true);
    assert_eq!(reduced.spring(), Spring::NONE);
    for entrance in ENTRANCES {
        assert_eq!(reduced.transition(entrance), Transition::none());
    }
}

#[test]
fn the_view_follows_the_system_and_runtime_motion_preference() {
    let root = crate::test_support::tempdir().unwrap();
    // The runtime's own override (`Cx::reduced_motion`) and the system's
    // preference both reach the appearance the view resolves.
    let mut rt = runtime(root.path(), true);
    rt.update(|s, tx| {
        s.workspace_menu = true;
        tx.animated();
    });
    instant(&mut rt, "workspace-menu-popover", true);
    assert!(rt.reduced_motion);
    rt.set_system_reduced_motion(false);
    rt.frame(DT);
    assert!(!rt.reduced_motion);
}

#[test]
fn workspace_menu_enters_and_leaves_briefly() {
    opens_and_closes(
        "workspace-menu-popover",
        |_| {},
        |rt| tap(rt, "workspace-menu"),
        escape,
    );
}

#[test]
fn explorer_overflow_menu_enters_and_leaves_briefly() {
    opens_and_closes(
        "explorer-menu",
        |rt| {
            // A narrow explorer folds its root actions into the "…" menu.
            rt.update(|s, _| s.workspace.layout.explorer_width = 200.0);
            let start = std::time::Instant::now();
            while rt.find("explorer-more").is_none() {
                assert!(start.elapsed().as_secs() < 8, "the explorer never listed");
                std::thread::sleep(std::time::Duration::from_millis(10));
                settle(rt);
            }
        },
        |rt| tap(rt, "explorer-more"),
        escape,
    );
}

#[test]
fn file_context_menu_enters_and_leaves_briefly() {
    opens_and_closes(
        "file-context-menu",
        |_| {},
        |rt| {
            let (x, y) = rt.find("file-list").unwrap();
            rt.secondary_down(x, y);
            rt.secondary_up(x, y);
        },
        escape,
    );
}

#[test]
fn command_palette_slides_in_and_out_briefly() {
    opens_and_closes("command-palette", |_| {}, open_palette, escape);
}

#[test]
fn confirmation_dialog_and_its_backdrop_enter_and_leave_briefly() {
    opens_and_closes(
        "confirmation",
        |_| {},
        |rt| {
            rt.update(|s, tx| {
                s.pending = Some(Pending::Quit);
                tx.animated();
            })
        },
        |rt| tap(rt, "cancel"),
    );
}

#[test]
fn compact_files_overlay_slides_in_and_out_briefly() {
    opens_and_closes(
        "explorer-overlay",
        |rt| {
            rt.update(|s, _| s.workspace.layout.explorer = false);
            rt.set_viewport((640.0, 480.0), 1.0);
            settle(rt);
        },
        |rt| tap(rt, "files"),
        |rt| tap(rt, "files"),
    );
}

#[test]
fn a_new_footer_message_fades_in_as_action_feedback() {
    for reduced in [false, true] {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path(), reduced);
        rt.update(|s, tx| {
            s.message = "Copied 3 lines".into();
            tx.animated();
        });
        rt.frame(0.0);
        let key = rt
            .elements()
            .into_iter()
            .find(|e| {
                !e.exiting
                    && e.key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("footer-feedback-"))
            })
            .and_then(|e| e.key)
            .unwrap();
        if reduced {
            instant(&mut rt, &key, true);
        } else {
            let t = entrance(&mut rt, &key);
            assert!(SPAN.contains(&t), "the message entered over {t:.3} s");
        }
    }
}

#[test]
fn a_held_button_shows_press_feedback() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = runtime(root.path(), false);
    let (x, y) = rt.find("new-tab").unwrap();
    rt.pointer_down(x, y);
    for _ in 0..12 {
        rt.frame(DT);
    }
    assert!(live(&rt, "new-tab").unwrap().press > 0.5);
}

fn two_editor_tabs(rt: &mut Runtime<State>, root: &Path) {
    for name in ["a.txt", "b.txt"] {
        let path = root.join(name);
        std::fs::write(&path, name).unwrap();
        rt.update({
            let path = path.clone();
            move |s, _| s.open_file(path)
        });
        settle_until(rt, |s| {
            s.workspace.tabs.iter().any(|t| t.document.path == path)
        });
    }
    assert_eq!(rt.state.workspace.tabs.len(), 2);
}

#[test]
fn the_editor_tab_indicator_slides_to_the_new_tab_briefly() {
    for reduced in [false, true] {
        let root = crate::test_support::tempdir().unwrap();
        let mut rt = runtime(root.path(), reduced);
        two_editor_tabs(&mut rt, root.path());
        let id = rt.state.workspace.tabs[0].id;
        let from = live(&rt, "indicator-editor-tabs").unwrap().rect[0];
        tap(&mut rt, &format!("editor-tab-{id}"));
        if reduced {
            instant(&mut rt, "indicator-editor-tabs", true);
        } else {
            // Seconds until it has covered 99% of the way to the new tab.
            let mut t = 0.0;
            loop {
                rt.frame(DT);
                t += DT;
                let e = live(&rt, "indicator-editor-tabs").unwrap();
                let to = e.target[0];
                assert!((from - to).abs() > 20.0, "the tabs share a place");
                let progress = (e.rect[0] - from) / (to - from);
                if progress >= 0.99 {
                    break;
                }
                assert!(t < 1.0, "the indicator never arrived: {e:?}");
            }
            assert!(t > DT, "the indicator jumped");
            assert!(SPAN.contains(&t), "the indicator travelled over {t:.3} s");
        }
        assert_eq!(rt.state.workspace.active, 0);
    }
}

/// `key` is `pattern`, or starts with it when `pattern` ends in `-`.
fn matches(key: &str, pattern: &str) -> bool {
    if pattern.ends_with('-') {
        key.starts_with(pattern)
    } else {
        key == pattern
    }
}

/// Every element keyed by one of `keys` is whole and in place in each
/// frame after `act`.
fn stays_immediate(rt: &mut Runtime<State>, keys: &[&str], act: impl Fn(&mut Runtime<State>)) {
    act(rt);
    for _ in 0..6 {
        rt.frame(DT);
        let moving: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| {
                e.key
                    .as_deref()
                    .is_some_and(|k| keys.iter().any(|p| matches(k, p)))
            })
            .filter(|e| e.exiting || !at_rest(e))
            .map(|e| (e.key, e.opacity, e.rect, e.target))
            .collect();
        assert!(moving.is_empty(), "animated: {moving:?}");
    }
}

fn terminal_runtime(root: &Path) -> Runtime<State> {
    let mut state = State::with_root(root.join("profile"));
    state.directory = root.to_owned();
    state.workspace_setup();
    state.navigate(root.to_owned());
    let mut parser = vt100::Parser::new(24, 80, 100);
    parser.process(b"rodtty $ echo motion\r\nmotion\r\nrodtty $ ");
    state.preview = Some(parser.screen().clone());
    let mut rt = runtime_for(state);
    settle(&mut rt);
    rt
}

/// The terminal grid, its cells, fills (selection, search), caret, and the
/// editor's text.
const TERMINAL: &[&str] = &[
    "terminal",
    "terminal-cell-",
    "terminal-fill-",
    "terminal-caret",
    "file-editor-",
];

#[test]
fn terminal_and_editor_text_land_at_once_when_panes_resize() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = terminal_runtime(root.path());
    two_editor_tabs(&mut rt, root.path());
    assert!(live(&rt, "terminal-caret").is_some());
    // Showing and hiding Files resizes the terminal and editor panes.
    stays_immediate(&mut rt, TERMINAL, |rt| tap(rt, "files"));
    stays_immediate(&mut rt, TERMINAL, |rt| tap(rt, "files"));
    // So does switching editor tabs beside the terminal.
    let id = rt.state.workspace.tabs[0].id;
    stays_immediate(&mut rt, TERMINAL, |rt| tap(rt, &format!("editor-tab-{id}")));
}

#[test]
fn terminal_selection_caret_and_output_land_at_once() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = terminal_runtime(root.path());
    let [x, y, ..] = live(&rt, "terminal").unwrap().rect;
    // A drag selects at once and each step lands where it is drawn.
    stays_immediate(&mut rt, TERMINAL, |rt| {
        rt.pointer_down(x + 2.0, y + 2.0);
        rt.pointer_move(x + 80.0, y + 30.0);
    });
    stays_immediate(&mut rt, TERMINAL, |rt| rt.pointer_up(x + 80.0, y + 30.0));
    assert!(rt.state.selection.is_some());
    assert!(rt.elements().iter().any(|e| {
        e.key
            .as_deref()
            .is_some_and(|k| k.starts_with("terminal-fill-"))
    }));
    // New output moves the caret and replaces cells in the same frame.
    stays_immediate(&mut rt, TERMINAL, |rt| {
        rt.update(|s, tx| {
            let mut parser = vt100::Parser::new(24, 80, 100);
            parser.process(b"rodtty $ echo motion\r\nmotion\r\nrodtty $ ls\r\nnotes\r\n$ ");
            s.preview = Some(parser.screen().clone());
            s.selection = None;
            tx.animated();
        })
    });
    // Typing over the terminal lands at once too.
    stays_immediate(&mut rt, TERMINAL, |rt| {
        press(rt, Key::Text("x".into()), Default::default())
    });
}

#[test]
fn divider_drags_resize_panes_at_once() {
    let root = crate::test_support::tempdir().unwrap();
    let mut rt = terminal_runtime(root.path());
    rt.update(|s, _| s.workspace.layout.explorer = true);
    two_editor_tabs(&mut rt, root.path());
    let panes = &[
        "explorer",
        "explorer-divider",
        "editor-divider",
        "terminal",
        "terminal-cell-",
        "terminal-caret",
        "file-editor-",
    ][..];
    for divider in ["explorer-divider", "editor-divider"] {
        let (x, y) = rt
            .find(divider)
            .unwrap_or_else(|| panic!("missing {divider}"));
        let before = live(&rt, "terminal").unwrap().rect;
        stays_immediate(&mut rt, panes, |rt| {
            rt.pointer_down(x, y);
            rt.pointer_move(x + 40.0, y);
        });
        stays_immediate(&mut rt, panes, |rt| rt.pointer_up(x + 40.0, y));
        assert_ne!(
            live(&rt, "terminal").unwrap().rect,
            before,
            "{divider} did not drag"
        );
    }
}
