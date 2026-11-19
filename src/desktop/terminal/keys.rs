use crate::desktop::*;

/// The control character Ctrl plus `c` types, as xterm sends it: Ctrl+A to
/// Ctrl+Z are 1 to 26, Ctrl+Space and Ctrl+@ are NUL, Ctrl+[ \ ] ^ _ are 27
/// to 31 and Ctrl+? is DEL.
pub(crate) fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

pub fn encode_key(event: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1
        + u8::from(event.mods.shift)
        + 2 * u8::from(event.mods.alt)
        + 4 * u8::from(event.mods.ctrl);
    let arrow = |end: char| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{end}")
        } else if application_cursor {
            format!("\x1bO{end}")
        } else {
            format!("\x1b[{end}")
        }
    };
    let sequence = match &event.key {
        // macOS names Ctrl+C by the key's own character, `Text("c")` with
        // Ctrl held, where other platforms send `Command('c')`: both are the
        // control character, or Ctrl+C would type "c" and never interrupt.
        Key::Text(value) if event.mods.ctrl && !event.mods.cmd => {
            let mut chars = value.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let byte = control_byte(c)?;
            return Some(if event.mods.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }
        Key::Text(value) => {
            if event.mods.alt {
                format!("\x1b{value}")
            } else {
                value.clone()
            }
        }
        Key::Command(c) if event.mods.ctrl && c.is_ascii() => return Some(vec![(*c as u8) & 0x1f]),
        Key::Enter => "\r".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::Tab => {
            if event.mods.shift {
                "\x1b[Z".into()
            } else {
                "\t".into()
            }
        }
        Key::Up => arrow('A'),
        Key::Down => arrow('B'),
        Key::Right => arrow('C'),
        Key::Left => arrow('D'),
        Key::Home => arrow('H'),
        Key::End => arrow('F'),
        Key::Insert => "\x1b[2~".into(),
        Key::Delete => "\x1b[3~".into(),
        Key::PageUp => "\x1b[5~".into(),
        Key::PageDown => "\x1b[6~".into(),
        Key::Function(n) if (1..=4).contains(n) => format!("\x1bO{}", char::from(b'P' + n - 1)),
        Key::Function(n) if (5..=12).contains(n) => format!(
            "\x1b[{}~",
            [15, 17, 18, 19, 20, 21, 23, 24][(*n - 5) as usize]
        ),
        _ => return None,
    };
    Some(sequence.into_bytes())
}
