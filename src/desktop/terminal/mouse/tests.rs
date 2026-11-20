use super::*;

fn screen(modes: &[u8]) -> vt100::Parser {
    let mut parser = vt100::Parser::new(24, 300, 0);
    parser.process(modes);
    parser
}

fn at(modes: &[u8], action: Action, row: u16, col: u16, mods: Modifiers) -> Option<Vec<u8>> {
    encode(screen(modes).screen(), action, row, col, mods)
}

const NONE: Modifiers = Modifiers {
    shift: false,
    alt: false,
    ctrl: false,
};

#[test]
fn no_mouse_mode_reports_nothing() {
    assert_eq!(at(b"", Action::Press(Button::Left), 0, 0, NONE), None);
    assert!(!reporting(screen(b"").screen(), NONE));
}

#[test]
fn sgr_reports_press_release_and_wheel_with_one_based_cells() {
    let modes = b"\x1b[?1000h\x1b[?1006h";
    let report = |action| String::from_utf8(at(modes, action, 4, 9, NONE).unwrap()).unwrap();
    assert_eq!(report(Action::Press(Button::Left)), "\x1b[<0;10;5M");
    assert_eq!(report(Action::Release(Button::Left)), "\x1b[<0;10;5m");
    assert_eq!(report(Action::Press(Button::Right)), "\x1b[<2;10;5M");
    assert_eq!(report(Action::WheelUp), "\x1b[<64;10;5M");
    assert_eq!(report(Action::WheelDown), "\x1b[<65;10;5M");
    // 1000 reports no motion.
    assert_eq!(
        at(modes, Action::Motion(Some(Button::Left)), 4, 9, NONE),
        None
    );
}

#[test]
fn default_encoding_offsets_by_32_and_releases_as_button_3() {
    let modes = b"\x1b[?1000h";
    assert_eq!(
        at(modes, Action::Press(Button::Left), 0, 0, NONE).unwrap(),
        b"\x1b[M\x20\x21\x21"
    );
    assert_eq!(
        at(modes, Action::Release(Button::Left), 0, 0, NONE).unwrap(),
        b"\x1b[M\x23\x21\x21"
    );
    // A column past 223 has no byte.
    assert_eq!(at(modes, Action::Press(Button::Left), 0, 230, NONE), None);
}

#[test]
fn utf8_encoding_names_wide_columns() {
    let modes = b"\x1b[?1000h\x1b[?1005h";
    let bytes = at(modes, Action::Press(Button::Left), 0, 250, NONE).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text,
        format!("\x1b[M {}!", char::from_u32(251 + 32).unwrap())
    );
}

#[test]
fn motion_follows_the_mode() {
    let drag = b"\x1b[?1002h\x1b[?1006h";
    assert_eq!(
        at(drag, Action::Motion(Some(Button::Left)), 1, 1, NONE).unwrap(),
        b"\x1b[<32;2;2M"
    );
    assert_eq!(at(drag, Action::Motion(None), 1, 1, NONE), None);
    let any = b"\x1b[?1003h\x1b[?1006h";
    assert_eq!(
        at(any, Action::Motion(None), 1, 1, NONE).unwrap(),
        b"\x1b[<35;2;2M"
    );
}

#[test]
fn x10_reports_presses_without_modifiers_or_releases() {
    let modes = b"\x1b[?9h\x1b[?1006h";
    let ctrl = Modifiers { ctrl: true, ..NONE };
    assert_eq!(
        at(modes, Action::Press(Button::Left), 0, 0, ctrl).unwrap(),
        b"\x1b[<0;1;1M"
    );
    assert_eq!(at(modes, Action::Release(Button::Left), 0, 0, NONE), None);
}

#[test]
fn modifiers_add_to_the_button_code() {
    let modes = b"\x1b[?1000h\x1b[?1006h";
    let mods = Modifiers {
        shift: false,
        alt: true,
        ctrl: true,
    };
    assert_eq!(
        at(modes, Action::Press(Button::Left), 0, 0, mods).unwrap(),
        b"\x1b[<24;1;1M"
    );
}

#[test]
fn shift_keeps_the_terminal_selection() {
    let parser = screen(b"\x1b[?1000h");
    assert!(reporting(parser.screen(), NONE));
    assert!(!reporting(
        parser.screen(),
        Modifiers {
            shift: true,
            ..NONE
        }
    ));
}

#[test]
fn wheel_arrows_follow_application_cursor_mode() {
    assert_eq!(wheel_arrow(screen(b"").screen(), true), b"\x1b[A");
    assert_eq!(wheel_arrow(screen(b"\x1b[?1h").screen(), false), b"\x1bOB");
}
