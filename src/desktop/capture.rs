//! Screenshots of single surfaces, saved as PNG artifacts.
//!
//! A terminal screenshot is drawn from the session's own screen state with
//! Rodtty's bundled monospace font and the current terminal palette, so it
//! shows exactly that terminal whether or not its tab is visible, and
//! nothing else on the desktop. Browser screenshots come from the browser's
//! own rendered frames (`browser`). No capture reads the desktop.
use super::*;
use crate::automation::{ErrorCode, ToolError, ToolResult};
use serde_json::{Value, json};
use std::path::Path;

/// The colors a terminal screenshot uses, as 8-bit sRGB.
#[derive(Clone)]
pub(crate) struct Palette {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub ansi: [[u8; 3]; 16],
    pub cursor: [u8; 3],
}

fn srgb(color: rune::Color) -> [u8; 3] {
    let encode = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).round() as u8
    };
    [encode(color.r), encode(color.g), encode(color.b)]
}

impl Palette {
    pub(crate) fn from_state(state: &State) -> Self {
        let dark = state.config.appearance != crate::config::Appearance::Light;
        let a = Appearance::resolve(state.config.theme_family, state.config.appearance, dark);
        let p = a.terminal_palette();
        Self {
            foreground: srgb(p.foreground),
            background: srgb(p.background),
            ansi: p.ansi.map(srgb),
            cursor: srgb(p.cursor),
        }
    }

    fn color(&self, color: vt100::Color, foreground: bool) -> [u8; 3] {
        match color {
            vt100::Color::Default if foreground => self.foreground,
            vt100::Color::Default => self.background,
            vt100::Color::Idx(i) if i < 16 => self.ansi[usize::from(i)],
            vt100::Color::Idx(i) => {
                let (r, g, b) = terminal::palette::xterm_extended(i);
                [r, g, b]
            }
            vt100::Color::Rgb(r, g, b) => [r, g, b],
        }
    }
}

pub(crate) struct TerminalShot {
    pub target: String,
    pub screen: vt100::Screen,
    pub palette: Palette,
}

/// An artifact path that is new and inside `artifacts`.
pub(crate) fn artifact_path(
    artifacts: &Path,
    prefix: &str,
    extension: &str,
) -> Result<(String, PathBuf), ToolError> {
    std::fs::create_dir_all(artifacts).map_err(|e| {
        ToolError::new(
            ErrorCode::Internal,
            format!("Can't create {}: {e}", artifacts.display()),
        )
    })?;
    let id = format!("art_{}", crate::automation::token());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Ok((
        id.clone(),
        artifacts.join(format!("{prefix}-{stamp}-{}.{extension}", &id[4..12])),
    ))
}

pub(crate) fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), ToolError> {
    let fail = |e: &dyn std::fmt::Display| {
        ToolError::new(
            ErrorCode::Internal,
            format!("Can't write {}: {e}", path.display()),
        )
    };
    let file = std::fs::File::create(path).map_err(|e| fail(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| fail(&e))?;
    writer.write_image_data(rgba).map_err(|e| fail(&e))?;
    writer.finish().map_err(|e| fail(&e))?;
    Ok(())
}

/// Draw a terminal screen into RGBA pixels at `scale` (1 for normal, 2 for HiDPI).
pub(crate) fn render_terminal(shot: &TerminalShot, scale: f32) -> (u32, u32, Vec<u8>) {
    use swash::{
        FontRef,
        scale::{Render, ScaleContext, Source},
        zeno::Format,
    };
    let regular =
        FontRef::from_index(asset_bytes!("fonts/GeistMono-Regular.ttf"), 0).expect("bundled font");
    let bold =
        FontRef::from_index(asset_bytes!("fonts/GeistMono-SemiBold.ttf"), 0).expect("bundled font");
    let px = 13.0 * scale;
    let metrics = regular.metrics(&[]).scale(px);
    let advance = regular
        .glyph_metrics(&[])
        .scale(px)
        .advance_width(regular.charmap().map('M'))
        .ceil()
        .max(1.0) as u32;
    let line = (metrics.ascent + metrics.descent + metrics.leading)
        .ceil()
        .max(px * 1.2)
        .ceil() as u32;
    let baseline = metrics.ascent.ceil() as i32
        + ((line as f32 - metrics.ascent - metrics.descent) / 2.0).floor() as i32;
    let pad = (8.0 * scale) as u32;
    let (rows, cols) = shot.screen.size();
    let width = u32::from(cols) * advance + 2 * pad;
    let height = u32::from(rows) * line + 2 * pad;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let bg = shot.palette.background;
    for p in pixels.chunks_exact_mut(4) {
        p.copy_from_slice(&[bg[0], bg[1], bg[2], 255]);
    }
    let fill = |pixels: &mut [u8], x0: u32, y0: u32, w: u32, h: u32, c: [u8; 3]| {
        for y in y0..(y0 + h).min(height) {
            for x in x0..(x0 + w).min(width) {
                let i = ((y * width + x) * 4) as usize;
                pixels[i..i + 3].copy_from_slice(&c);
            }
        }
    };
    let mut context = ScaleContext::new();
    let mut scalers = [context.builder(regular).size(px).hint(true).build()];
    let mut bold_context = ScaleContext::new();
    let mut bold_scaler = bold_context.builder(bold).size(px).hint(true).build();
    let (cursor_row, cursor_col) = shot.screen.cursor_position();
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = shot.screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let mut fg = shot.palette.color(cell.fgcolor(), true);
            let mut back = shot.palette.color(cell.bgcolor(), false);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut back);
            }
            let cells = if cell.is_wide() { 2 } else { 1 };
            let x = pad + u32::from(col) * advance;
            let y = pad + u32::from(row) * line;
            if back != bg {
                fill(&mut pixels, x, y, advance * cells, line, back);
            }
            if row == cursor_row && col == cursor_col && !shot.screen.hide_cursor() {
                fill(
                    &mut pixels,
                    x,
                    y,
                    advance * cells,
                    line,
                    shot.palette.cursor,
                );
                fg = back;
            }
            let contents = cell.contents();
            let Some(c) = contents.chars().next() else {
                continue;
            };
            if c == ' ' {
                continue;
            }
            let (font, scaler) = if cell.bold() {
                (bold, &mut bold_scaler)
            } else {
                (regular, &mut scalers[0])
            };
            let glyph = font.charmap().map(c);
            if glyph == 0 {
                continue;
            }
            let Some(image) = Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .render(scaler, glyph)
            else {
                continue;
            };
            let p = image.placement;
            for gy in 0..p.height {
                for gx in 0..p.width {
                    let alpha = image.data[(gy * p.width + gx) as usize] as u32;
                    if alpha == 0 {
                        continue;
                    }
                    let px_x = x as i32 + p.left + gx as i32;
                    let px_y = y as i32 + baseline - p.top + gy as i32;
                    if px_x < 0 || px_y < 0 || px_x as u32 >= width || px_y as u32 >= height {
                        continue;
                    }
                    let i = ((px_y as u32 * width + px_x as u32) * 4) as usize;
                    for k in 0..3 {
                        let under = pixels[i + k] as u32;
                        pixels[i + k] =
                            ((fg[k] as u32 * alpha + under * (255 - alpha)) / 255) as u8;
                    }
                }
            }
            if cell.underline() {
                fill(
                    &mut pixels,
                    x,
                    y + line - (2.0 * scale) as u32,
                    advance * cells,
                    scale.max(1.0) as u32,
                    fg,
                );
            }
        }
    }
    (width, height, pixels)
}

/// Save a terminal screenshot at normal and HiDPI scale.
pub(crate) fn terminal_png(artifacts: &Path, shot: TerminalShot) -> ToolResult {
    let scale = 2.0;
    let (width, height, pixels) = render_terminal(&shot, scale);
    let (id, path) = artifact_path(artifacts, "terminal", "png")?;
    write_png(&path, width, height, &pixels)?;
    let (rows, cols) = shot.screen.size();
    Ok(json!({
        "artifact": id,
        "path": path,
        "format": "png",
        "width": width,
        "height": height,
        "scale": scale,
        "source": {"kind": "terminal", "target": shot.target, "rows": rows, "cols": cols, "renderer": "rodtty terminal screen state"},
        "timestamp_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
    }))
}

/// The Rodtty window. Rune draws the window on the GPU without a readback
/// path, and Rodtty never captures the desktop to stand in for one.
pub(crate) fn window_png(_host: &automation::Host, _artifacts: &Path) -> ToolResult {
    Err(ToolError::new(
        ErrorCode::Unsupported,
        "Window screenshots need a frame readback from the UI renderer, which this build does not have. Capture a terminal or browser target instead; Rodtty never falls back to desktop capture.",
    ))
}

#[allow(dead_code)]
fn _unused(_: Value) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_screenshot_shows_only_that_screen_at_both_scales() {
        let mut parser = vt100::Parser::new(4, 20, 0);
        parser.process("héllo \x1b[31mred\x1b[0m".as_bytes());
        let shot = TerminalShot {
            target: "term_x_1".into(),
            screen: parser.screen().clone(),
            palette: Palette {
                foreground: [255, 255, 255],
                background: [0, 0, 0],
                ansi: [[200, 0, 0]; 16],
                cursor: [0, 255, 0],
            },
        };
        let (w1, h1, pixels) = render_terminal(&shot, 1.0);
        let (w2, h2, _) = render_terminal(&shot, 2.0);
        assert!(
            w2 >= w1 * 2 - 2 && h2 >= h1 * 2 - 2,
            "{w1}x{h1} vs {w2}x{h2}"
        );
        assert!(
            pixels.chunks_exact(4).any(|p| p[0] > 150 && p[1] < 60),
            "red text drawn"
        );
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200),
            "white text drawn"
        );
        let dir = crate::test_support::tempdir().unwrap();
        let result = terminal_png(dir.path(), shot).unwrap();
        let path = PathBuf::from(result["path"].as_str().unwrap());
        assert!(path.starts_with(dir.path()));
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let reader = decoder.read_info().unwrap();
        assert_eq!(
            reader.info().width,
            result["width"].as_u64().unwrap() as u32
        );
    }
}
