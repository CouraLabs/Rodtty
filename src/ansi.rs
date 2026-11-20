//! Plain text from raw terminal output.

/// `bytes` as text without escape sequences or other control characters.
/// CR LF and lone CR become LF; tabs stay. Invalid UTF-8 is replaced.
pub fn strip(bytes: &[u8]) -> String {
    #[derive(Clone, Copy)]
    enum Mode {
        Text,
        Escape,
        Csi,
        /// OSC, DCS, APC, PM and SOS strings run to BEL or ST.
        String,
        StringEscape,
    }
    let mut mode = Mode::Text;
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().copied().peekable();
    while let Some(byte) = iter.next() {
        mode = match (mode, byte) {
            (Mode::Text, 0x1b) => Mode::Escape,
            (Mode::Text, b'\r') => {
                if iter.peek() != Some(&b'\n') {
                    out.push(b'\n');
                }
                Mode::Text
            }
            (Mode::Text, b'\n' | b'\t') => {
                out.push(byte);
                Mode::Text
            }
            (Mode::Text, 0x9b) => Mode::Csi,
            (Mode::Text, b) if b < 0x20 || b == 0x7f => Mode::Text,
            (Mode::Text, b) => {
                out.push(b);
                Mode::Text
            }
            (Mode::Escape, b'[') => Mode::Csi,
            (Mode::Escape, b']' | b'P' | b'_' | b'^' | b'X') => Mode::String,
            // Charset designations take one more byte.
            (Mode::Escape, b'(' | b')' | b'*' | b'+' | b'#' | b'%') => {
                iter.next();
                Mode::Text
            }
            (Mode::Escape, _) => Mode::Text,
            (Mode::Csi, 0x40..=0x7e) => Mode::Text,
            (Mode::Csi, _) => Mode::Csi,
            (Mode::String, 0x07) => Mode::Text,
            (Mode::String, 0x1b) => Mode::StringEscape,
            (Mode::String, _) => Mode::String,
            (Mode::StringEscape, b'\\') => Mode::Text,
            (Mode::StringEscape, _) => Mode::String,
        };
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::strip;

    #[test]
    fn removes_colors_titles_and_markers_but_keeps_text() {
        let raw = b"\x1b[31mred\x1b[0m \xc3\xa9t\xc3\xa9\r\n\x1b]0;title\x07next\x1b]777;rodtty;x;A;0\x1b\\\tend\x1b(B";
        assert_eq!(strip(raw), "red \u{e9}t\u{e9}\nnext\tend");
    }

    #[test]
    fn lone_carriage_returns_become_newlines() {
        assert_eq!(strip(b"a\rb\r\nc"), "a\nb\nc");
    }
}
