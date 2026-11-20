//! Opening a `path:line[:column]` reference printed in a terminal: the owner
//! Cmd-clicks it (Ctrl-click off macOS) or picks **Open Reference** from the
//! terminal's context menu. A relative path resolves against the directory
//! of the command that printed it; text whose command is unknown resolves
//! against the session's directory only when that names exactly one file.
use crate::desktop::*;
use crate::references::{self, Reference};
use std::path::{Path, PathBuf};

/// One line as the program printed it: visual rows joined across soft wraps,
/// with each character's absolute line and column.
struct Logical {
    text: String,
    cells: Vec<(usize, u16)>,
}

fn scrollback_len(screen: &mut vt100::Screen) -> usize {
    let offset = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let len = screen.scrollback();
    screen.set_scrollback(offset);
    len
}

/// Every logical line in the scrollback and on screen, oldest first, and the
/// absolute line visible row 0 shows. The scroll position is left as it was.
fn logical_lines(screen: &mut vt100::Screen) -> (Vec<Logical>, usize) {
    let offset = screen.scrollback();
    let scrollback = scrollback_len(screen);
    let (rows, cols) = screen.size();
    let total = scrollback + usize::from(rows);
    let mut out = Vec::new();
    let mut current = Logical {
        text: String::new(),
        cells: Vec::new(),
    };
    let mut line = 0;
    while line < total {
        let top = line.min(scrollback);
        screen.set_scrollback(scrollback - top);
        let end = (top + usize::from(rows)).min(total);
        for absolute in line..end {
            let row = (absolute - top) as u16;
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let contents = cell.contents();
                let contents = if contents.is_empty() { " " } else { contents };
                for c in contents.chars() {
                    current.text.push(c);
                    current.cells.push((absolute, col));
                }
            }
            if !screen.row_wrapped(row) {
                let kept = current.text.trim_end().chars().count();
                current.text = current.text.chars().take(kept).collect();
                current.cells.truncate(kept);
                out.push(std::mem::replace(
                    &mut current,
                    Logical {
                        text: String::new(),
                        cells: Vec::new(),
                    },
                ));
            }
        }
        line = end;
    }
    if !current.text.is_empty() {
        out.push(current);
    }
    screen.set_scrollback(offset);
    (out, scrollback - offset.min(scrollback))
}

/// The reference under visible cell (`row`, `col`), with its logical line's
/// text and how many identical lines follow it in the terminal.
pub(crate) fn reference_at(
    screen: &mut vt100::Screen,
    row: u16,
    col: u16,
) -> Option<(Reference, String, usize)> {
    let (lines, top) = logical_lines(screen);
    let target = (top + usize::from(row), col);
    let index = lines
        .iter()
        .position(|l| l.cells.iter().any(|&(a, _)| a == target.0))?;
    let line = &lines[index];
    // The character on the cell, or the wide glyph whose second half it is.
    let wide_half = screen
        .cell(row, col)
        .is_some_and(vt100::Cell::is_wide_continuation);
    let at = line
        .cells
        .iter()
        .position(|&(a, c)| a == target.0 && (c == col || (wide_half && c + 1 == col)))?;
    let reference = references::at(&line.text, at)?;
    let after = lines[index + 1..]
        .iter()
        .filter(|l| l.text == line.text)
        .count();
    Some((reference, line.text.clone(), after))
}

/// Where a relative reference resolves, or why it can't.
fn directory_for(
    commands: &[(Option<PathBuf>, String)],
    text: &str,
    after: usize,
) -> Option<Option<PathBuf>> {
    // Walk the commands' output newest first; the line is the `after`-th
    // identical one from the end.
    let mut remaining = after;
    for (directory, output) in commands.iter().rev() {
        let count = output
            .split('\n')
            .filter(|l| l.trim_end_matches('\r').trim_end() == text)
            .count();
        if count > remaining {
            return Some(directory.clone());
        }
        remaining -= count;
    }
    None
}

/// Resolve `reference` to a file and a valid location in it.
pub(crate) fn resolve(
    reference: &Reference,
    origin: Option<Option<PathBuf>>,
    session_directory: &Path,
    other_directories: &[PathBuf],
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let absolute = Path::new(&reference.path).is_absolute() || reference.path.starts_with("~/");
    let path = if absolute {
        references::resolve(reference, Path::new("/"), home)?
    } else {
        match origin {
            Some(Some(directory)) if directory.is_dir() => {
                references::resolve(reference, &directory, home)?
            }
            Some(Some(directory)) => {
                return Err(format!(
                    "{} was printed in {}, which is no longer available.",
                    reference.path,
                    directory.display()
                ));
            }
            // A command whose directory the shell never reported, or text no
            // command owns: only an unambiguous match opens.
            Some(None) | None => {
                let mut found: Vec<PathBuf> = std::iter::once(session_directory.to_path_buf())
                    .chain(other_directories.iter().cloned())
                    .filter_map(|d| references::resolve(reference, &d, home).ok())
                    .collect();
                found.sort();
                found.dedup();
                match found.len() {
                    0 => references::resolve(reference, session_directory, home)?,
                    1 => found.remove(0),
                    _ => {
                        return Err(format!(
                            "{} is ambiguous: it exists in {}. Nothing was opened.",
                            reference.path,
                            found
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join(" and ")
                        ));
                    }
                }
            }
        }
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} can't be read: {e}", path.display()))?;
    let lines = text.split('\n').count().max(1);
    if reference.line > lines {
        return Err(format!(
            "{} has {lines} lines; line {} doesn't exist.",
            path.display(),
            reference.line
        ));
    }
    if let Some(column) = reference.column {
        let width = text
            .split('\n')
            .nth(reference.line - 1)
            .unwrap_or("")
            .trim_end_matches('\r')
            .chars()
            .count();
        if column > width + 1 {
            return Err(format!(
                "Line {} of {} has {width} characters; column {column} doesn't exist.",
                reference.line,
                path.display()
            ));
        }
    }
    Ok(path)
}

impl State {
    /// The reference under the active terminal's visible cell, if any.
    pub(crate) fn terminal_reference_at(&mut self, row: u16, col: u16) -> Option<Reference> {
        self.with_screen_mut(|screen| reference_at(screen, row, col))
            .map(|(r, _, _)| r)
    }

    /// Open the reference under the active terminal's visible cell in the
    /// editor, at its line and column. Explains, without opening anything,
    /// when it doesn't name an existing location.
    pub(crate) fn open_terminal_reference(&mut self, row: u16, col: u16, tx: &mut Tx) -> bool {
        let Some((reference, text, after)) =
            self.with_screen_mut(|screen| reference_at(screen, row, col))
        else {
            return false;
        };
        let Some(session) = self.active() else {
            return false;
        };
        let commands: Vec<(Option<PathBuf>, String)> = session
            .commands()
            .into_iter()
            .map(|c| {
                let output = session
                    .command_output(c.id)
                    .map(|(o, _)| o)
                    .unwrap_or_default();
                (c.directory, output)
            })
            .collect();
        let others: Vec<PathBuf> = commands.iter().filter_map(|(d, _)| d.clone()).collect();
        let origin = directory_for(&commands, &text, after);
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
        match resolve(&reference, origin, &session.cwd(), &others, home.as_deref()) {
            Ok(path) => {
                self.open_file_at(
                    path,
                    false,
                    workspace::SourceLocation {
                        line: reference.line,
                        column: reference.column.unwrap_or(1),
                        length: 0,
                    },
                );
                self.apply_pending_location(tx);
            }
            Err(error) => self.message = error,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(cols: u16, text: &str) -> vt100::Parser {
        let mut parser = vt100::Parser::new(6, cols, 100);
        parser.process(text.as_bytes());
        parser
    }

    #[test]
    fn a_wrapped_multibyte_reference_is_found_from_either_row() {
        // 12 columns: "→ src/ünï.rs:3:2" wraps after "→ src/ünï.r".
        let mut p = screen(12, "→ src/ünï.rs:3:2\r\n");
        let (r, _, _) = reference_at(p.screen_mut(), 0, 4).unwrap();
        assert_eq!(
            (r.path.as_str(), r.line, r.column),
            ("src/ünï.rs", 3, Some(2))
        );
        let (r, _, _) = reference_at(p.screen_mut(), 1, 2).unwrap();
        assert_eq!(r.path, "src/ünï.rs");
        assert!(
            reference_at(p.screen_mut(), 0, 0).is_none(),
            "the arrow is not a reference"
        );
    }

    #[test]
    fn identical_lines_attribute_to_the_command_that_printed_each() {
        let mut p = screen(40, "src/a.rs:1\r\nsrc/a.rs:1\r\n");
        let (_, text, after) = reference_at(p.screen_mut(), 0, 2).unwrap();
        assert_eq!(after, 1);
        let commands = vec![
            (Some(PathBuf::from("/one")), "src/a.rs:1\r\n".to_owned()),
            (Some(PathBuf::from("/two")), "src/a.rs:1\r\n".to_owned()),
        ];
        assert_eq!(
            directory_for(&commands, &text, after),
            Some(Some("/one".into()))
        );
        assert_eq!(
            directory_for(&commands, &text, 0),
            Some(Some("/two".into()))
        );
        assert_eq!(directory_for(&commands, "other.rs:1", 0), None);
    }

    #[test]
    fn missing_directories_ambiguity_and_invalid_locations_open_nothing() {
        let a = crate::test_support::tempdir().unwrap();
        let b = crate::test_support::tempdir().unwrap();
        for d in [a.path(), b.path()] {
            std::fs::create_dir(d.join("src")).unwrap();
            std::fs::write(d.join("src/a.rs"), "one\ntwo\n").unwrap();
        }
        let r = |s: &str| references::find(s).remove(0);
        let gone = PathBuf::from("/nonexistent/rodtty-gone");
        let error = resolve(&r("src/a.rs:1"), Some(Some(gone)), a.path(), &[], None).unwrap_err();
        assert!(error.contains("no longer available"), "{error}");
        let error =
            resolve(&r("src/a.rs:1"), None, a.path(), &[b.path().into()], None).unwrap_err();
        assert!(error.contains("ambiguous"), "{error}");
        assert_eq!(
            resolve(
                &r("src/a.rs:2"),
                Some(Some(b.path().into())),
                a.path(),
                &[],
                None
            )
            .unwrap(),
            b.path().join("src/a.rs")
        );
        let error = resolve(
            &r("src/a.rs:9"),
            Some(Some(a.path().into())),
            a.path(),
            &[],
            None,
        )
        .unwrap_err();
        assert!(error.contains("doesn't exist"), "{error}");
        let error = resolve(
            &r("src/a.rs:1:20"),
            Some(Some(a.path().into())),
            a.path(),
            &[],
            None,
        )
        .unwrap_err();
        assert!(error.contains("column 20"), "{error}");
        let error = resolve(&r("src/none.rs:1"), None, a.path(), &[], None).unwrap_err();
        assert!(error.contains("does not exist"), "{error}");
        let absolute = format!("{}:2:1", a.path().join("src/a.rs").display());
        assert_eq!(
            resolve(&r(&absolute), None, b.path(), &[], None).unwrap(),
            a.path().join("src/a.rs")
        );
    }
}
