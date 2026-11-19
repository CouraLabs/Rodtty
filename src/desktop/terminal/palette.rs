//! The one palette every terminal cell is painted from, resolved per appearance.
//!
//! Cells keep the color the program asked for (`vt100::Color`): the default
//! slot, an index, or RGB. Mapping happens here at render time, so a theme
//! change recolors retained scrollback and prompts without new output. Only
//! the defaults and the sixteen conventional ANSI slots are themed; indices
//! 16–255 and truecolor stay exactly as the program requested.
use rune::Color;

pub(crate) struct TerminalPalette {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub selection: Color,
    /// Fill behind every match of a terminal search except the current one.
    pub search_match: Color,
    /// Fill behind the current match; its glyphs use `search_current_text`.
    pub search_current: Color,
    pub search_current_text: Color,
    pub ansi: [Color; 16],
}

impl TerminalPalette {
    pub fn color(&self, color: vt100::Color, foreground: bool) -> Color {
        match color {
            vt100::Color::Default if foreground => self.foreground,
            vt100::Color::Default => self.background,
            vt100::Color::Idx(i) if i < 16 => self.ansi[usize::from(i)],
            vt100::Color::Idx(i) => {
                let (r, g, b) = xterm_extended(i);
                crate::desktop::rgb(r, g, b)
            }
            vt100::Color::Rgb(r, g, b) => crate::desktop::rgb(r, g, b),
        }
    }
}

/// The standard xterm 6×6×6 cube (16–231) and grayscale ramp (232–255).
pub(crate) fn xterm_extended(i: u8) -> (u8, u8, u8) {
    if i >= 232 {
        let v = 8 + (i - 232) * 10;
        (v, v, v)
    } else {
        let n = i.saturating_sub(16);
        let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
        (level(n / 36), level(n / 6 % 6), level(n % 6))
    }
}

/// A straight-alpha theme color laid over the terminal canvas, as an opaque fill.
pub(crate) fn over(color: Color, background: Color) -> Color {
    background.mix(color.alpha(1.0), color.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_indices_follow_the_xterm_table() {
        assert_eq!(xterm_extended(16), (0, 0, 0));
        assert_eq!(xterm_extended(196), (255, 0, 0));
        assert_eq!(xterm_extended(21), (0, 0, 255));
        assert_eq!(xterm_extended(231), (255, 255, 255));
        assert_eq!(xterm_extended(232), (8, 8, 8));
        assert_eq!(xterm_extended(255), (238, 238, 238));
    }
}
