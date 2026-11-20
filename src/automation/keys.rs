//! Key chords as automation clients name them: `Enter`, `Ctrl+C`,
//! `Shift+Tab`, `Alt+Left`, `F5`, `a`, `é`.
use super::ToolError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyName {
    /// A character key.
    Char(char),
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Function(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Cmd on macOS, the Windows or Super key elsewhere.
    pub meta: bool,
}

impl Modifiers {
    pub fn any(self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: KeyName,
    pub mods: Modifiers,
}

/// The modifier a name stands for, if it is one.
pub fn modifier(name: &str) -> Option<fn(&mut Modifiers)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "shift" => |m| m.shift = true,
        "ctrl" | "control" => |m| m.ctrl = true,
        "alt" | "option" | "opt" => |m| m.alt = true,
        "cmd" | "command" | "meta" | "super" | "win" => |m| m.meta = true,
        _ => return None,
    })
}

pub fn key_name(name: &str) -> Option<KeyName> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyName::Char(c));
    }
    Some(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => KeyName::Enter,
        "backspace" => KeyName::Backspace,
        "delete" | "del" => KeyName::Delete,
        "escape" | "esc" => KeyName::Escape,
        "tab" => KeyName::Tab,
        "space" => KeyName::Space,
        "up" | "arrowup" => KeyName::Up,
        "down" | "arrowdown" => KeyName::Down,
        "left" | "arrowleft" => KeyName::Left,
        "right" | "arrowright" => KeyName::Right,
        "home" => KeyName::Home,
        "end" => KeyName::End,
        "pageup" => KeyName::PageUp,
        "pagedown" => KeyName::PageDown,
        "insert" => KeyName::Insert,
        f if f.starts_with('f') && f.len() > 1 => {
            let n: u8 = f[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            KeyName::Function(n)
        }
        _ => return None,
    })
}

/// Parse `Ctrl+Shift+K`. A trailing `+` names the plus key (`Ctrl++`).
pub fn parse(chord: &str) -> Result<Chord, ToolError> {
    let invalid = || ToolError::invalid(format!("Unknown key chord {chord:?}"));
    if chord.is_empty() {
        return Err(invalid());
    }
    let (prefix, last) = match chord.strip_suffix("++") {
        Some(prefix) => (prefix, "+"),
        None if chord == "+" => ("", "+"),
        None => match chord.rsplit_once('+') {
            Some((prefix, last)) => (prefix, last),
            None => ("", chord),
        },
    };
    let mut mods = Modifiers::default();
    for part in prefix.split('+').filter(|p| !p.is_empty()) {
        modifier(part).ok_or_else(invalid)?(&mut mods);
    }
    let key = key_name(last).ok_or_else(invalid)?;
    Ok(Chord { key, mods })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_modifiers_and_named_keys() {
        let c = parse("Ctrl+C").unwrap();
        assert_eq!(c.key, KeyName::Char('C'));
        assert!(c.mods.ctrl && !c.mods.shift);
        assert_eq!(parse("shift+tab").unwrap().key, KeyName::Tab);
        assert_eq!(parse("F12").unwrap().key, KeyName::Function(12));
        assert_eq!(parse("Ctrl++").unwrap().key, KeyName::Char('+'));
        assert_eq!(parse("é").unwrap().key, KeyName::Char('é'));
        assert!(parse("Hyper+X").is_err());
        assert!(parse("F99").is_err());
    }
}
