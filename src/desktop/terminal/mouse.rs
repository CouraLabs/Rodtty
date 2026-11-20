//! Mouse reporting: the xterm mouse protocols an application turns on with
//! DECSET 9, 1000, 1002 and 1003, in the default, UTF-8 (1005) and SGR (1006)
//! encodings, plus the arrow keys a wheel sends on the alternate screen when
//! the application asked for no mouse events.
use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};

/// What the pointer did, over the cell at `row`, `col` (zero-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Press(Button),
    Release(Button),
    /// The pointer moved to a new cell with `Some` button held, or none.
    Motion(Option<Button>),
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the grid reports only the primary button so far; the others are encoded and tested"
)]
pub(crate) enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    fn code(self) -> u32 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl From<rune::Mods> for Modifiers {
    fn from(mods: rune::Mods) -> Self {
        Self {
            shift: mods.shift,
            alt: mods.alt,
            ctrl: mods.ctrl,
        }
    }
}

/// Whether the application wants pointer input reported instead of the
/// terminal selecting and scrolling. Shift keeps the terminal's own
/// selection and scrollback, as xterm does.
pub(crate) fn reporting(screen: &vt100::Screen, mods: Modifiers) -> bool {
    screen.mouse_protocol_mode() != Mode::None && !mods.shift
}

/// The bytes reporting `action` at (`row`, `col`) under the screen's mouse
/// mode and encoding, or `None` when the mode doesn't report it or the
/// encoding can't name the cell.
pub(crate) fn encode(
    screen: &vt100::Screen,
    action: Action,
    row: u16,
    col: u16,
    mods: Modifiers,
) -> Option<Vec<u8>> {
    let mode = screen.mouse_protocol_mode();
    let reported = match (mode, action) {
        (Mode::None, _) => false,
        (_, Action::Press(_) | Action::WheelUp | Action::WheelDown) => true,
        (Mode::Press, _) => false,
        (_, Action::Release(_)) => true,
        (Mode::ButtonMotion, Action::Motion(held)) => held.is_some(),
        (Mode::AnyMotion, Action::Motion(_)) => true,
        (Mode::PressRelease, Action::Motion(_)) => false,
    };
    if !reported {
        return None;
    }
    let sgr = screen.mouse_protocol_encoding() == Encoding::Sgr;
    let mut code = match action {
        Action::Press(button) => button.code(),
        // Only SGR names the released button; the older encodings send 3.
        Action::Release(button) if sgr => button.code(),
        Action::Release(_) => 3,
        Action::Motion(held) => 32 + held.map_or(3, Button::code),
        Action::WheelUp => 64,
        Action::WheelDown => 65,
    };
    // X10 mode reports no modifiers.
    if mode != Mode::Press {
        code += 4 * u32::from(mods.shift) + 8 * u32::from(mods.alt) + 16 * u32::from(mods.ctrl);
    }
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    match screen.mouse_protocol_encoding() {
        Encoding::Sgr => {
            let end = if matches!(action, Action::Release(_)) {
                'm'
            } else {
                'M'
            };
            Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes())
        }
        Encoding::Default => {
            // Each value is one byte offset by 32, so cells past 223 can't be named.
            let byte = |v: u32| u8::try_from(v + 32).ok();
            Some(vec![0x1b, b'[', b'M', byte(code)?, byte(x)?, byte(y)?])
        }
        Encoding::Utf8 => {
            let mut out = b"\x1b[M".to_vec();
            for v in [code, x, y] {
                // UTF-8 mode encodes values up to 2015 as characters.
                let c = char::from_u32(v + 32).filter(|_| v + 32 < 0x800)?;
                let mut buffer = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            }
            Some(out)
        }
    }
}

/// The arrow key a wheel step sends to an alternate-screen application that
/// asked for no mouse events, so `less`, `man` and editors scroll.
pub(crate) fn wheel_arrow(screen: &vt100::Screen, up: bool) -> &'static [u8] {
    match (screen.application_cursor(), up) {
        (true, true) => b"\x1bOA",
        (true, false) => b"\x1bOB",
        (false, true) => b"\x1b[A",
        (false, false) => b"\x1b[B",
    }
}

#[cfg(test)]
mod tests;
