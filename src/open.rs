//! `rodtty open`: what a script asks Rodtty to open.
//!
//! ```text
//! rodtty open <path>[:line[:column]]
//! rodtty open --path <path> [--line N] [--column N]   (a path with colons, taken literally)
//! ```
//!
//! Relative paths resolve from the caller's working directory. A trailing
//! `:N` or `:N:M` is a position only when the full text isn't itself an
//! existing file, so `notes:2024.txt` still opens that file. Windows drive
//! paths (`C:\src\main.rs:10`) keep their drive colon.
use std::path::{Path, PathBuf};

/// Exit statuses, documented in `rodtty open --help`.
pub const EXIT_INVALID: i32 = 2;
pub const EXIT_MISSING: i32 = 3;
pub const EXIT_UNSUPPORTED: i32 = 4;
pub const EXIT_DELIVERY: i32 = 5;

pub const USAGE: &str = "usage: rodtty open <path>[:line[:column]]
       rodtty open --path <path> [--line N] [--column N]

Opens a file in the running Rodtty (starting Rodtty if needed) at a line and column.
Relative paths resolve from the current directory. Use --path for file names that end in :<number>.

Exit status: 0 opened, 2 invalid arguments, 3 missing file, 4 not an openable text file
(a folder, or over the editor's 2 MiB limit), 5 Rodtty could not be reached or started.";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

pub struct OpenError {
    pub status: i32,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> OpenError {
    OpenError {
        status: EXIT_INVALID,
        message: message.into(),
    }
}

fn number(text: &str, what: &str) -> Result<usize, OpenError> {
    match text.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(invalid(format!(
            "{what} must be a positive number, not {text:?}"
        ))),
    }
}

/// Split `path:line:column` when the suffix is numeric and the whole text
/// doesn't name an existing file.
fn split_position(text: &str, cwd: &Path) -> (String, Option<usize>, Option<usize>) {
    let whole = cwd.join(text);
    if whole.exists() {
        return (text.to_owned(), None, None);
    }
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts: Vec<&str> = text.rsplitn(3, ':').collect();
    parts.reverse();
    // A drive letter's colon belongs to the path.
    let drive = |p: &str| p.len() == 1 && p.chars().all(|c| c.is_ascii_alphabetic());
    match parts.as_slice() {
        [path, line, column]
            if numeric(line) && numeric(column) && !path.is_empty() && !drive(path) =>
        {
            (path.to_string(), line.parse().ok(), column.parse().ok())
        }
        [a, b, line] if numeric(line) && drive(a) => (format!("{a}:{b}"), line.parse().ok(), None),
        [a, line] | [_, a, line] if numeric(line) && !a.is_empty() && !drive(a) => {
            let path = &text[..text.len() - line.len() - 1];
            (path.to_owned(), line.parse().ok(), None)
        }
        _ => (text.to_owned(), None, None),
    }
}

/// Parse `rodtty open` arguments into a request with an absolute, existing path.
pub fn parse(args: &[String], cwd: &Path) -> Result<OpenRequest, OpenError> {
    let mut literal = None;
    let mut positional = None;
    let mut line = None;
    let mut column = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Err(OpenError {
                    status: 0,
                    message: USAGE.into(),
                });
            }
            "--path" => {
                literal = Some(
                    rest.next()
                        .ok_or_else(|| invalid("--path needs a path"))?
                        .clone(),
                )
            }
            "--line" => {
                line = Some(number(
                    rest.next()
                        .ok_or_else(|| invalid("--line needs a number"))?,
                    "--line",
                )?)
            }
            "--column" => {
                column = Some(number(
                    rest.next()
                        .ok_or_else(|| invalid("--column needs a number"))?,
                    "--column",
                )?)
            }
            flag if flag.starts_with("--") => {
                return Err(invalid(format!("unknown option {flag}\n{USAGE}")));
            }
            value if positional.is_none() => positional = Some(value.to_owned()),
            _ => {
                return Err(invalid(format!(
                    "only one path can be opened at a time\n{USAGE}"
                )));
            }
        }
    }
    let (path, at_line, at_column) = match (literal, positional) {
        (Some(path), None) => (path, None, None),
        (None, Some(text)) => split_position(&text, cwd),
        (Some(_), Some(_)) => {
            return Err(invalid("give either --path or a positional path, not both"));
        }
        (None, None) => return Err(invalid(USAGE)),
    };
    if path.is_empty() {
        return Err(invalid("the path is empty"));
    }
    let line = line.or(at_line);
    let column = column.or(at_column);
    if line == Some(0) || column == Some(0) {
        return Err(invalid("line and column start at 1"));
    }
    if column.is_some() && line.is_none() {
        return Err(invalid("a column needs a line"));
    }
    let path = cwd.join(&path);
    let metadata = std::fs::metadata(&path).map_err(|_| OpenError {
        status: EXIT_MISSING,
        message: format!("no such file: {}", path.display()),
    })?;
    if metadata.is_dir() {
        return Err(OpenError {
            status: EXIT_UNSUPPORTED,
            message: format!("{} is a folder; open a file", path.display()),
        });
    }
    if metadata.len() > crate::files::MAX_FILE_BYTES {
        return Err(OpenError {
            status: EXIT_UNSUPPORTED,
            message: format!("{} is over the editor's 2 MiB limit", path.display()),
        });
    }
    use crate::paths::Canonical as _;
    let path = path.canonical().map_err(|e| OpenError {
        status: EXIT_MISSING,
        message: e.to_string(),
    })?;
    Ok(OpenRequest { path, line, column })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn positions_spaces_unicode_and_colon_names_parse_predictably() {
        let dir = crate::test_support::tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir(cwd.join("sub dir")).unwrap();
        std::fs::write(cwd.join("sub dir/naïve ünïcode.rs"), "fn main() {}\n").unwrap();
        std::fs::write(cwd.join("notes:2024.txt"), "x").unwrap();
        std::fs::write(cwd.join("main.rs"), "x").unwrap();
        let r = parse(&args(&["sub dir/naïve ünïcode.rs:3:7"]), cwd)
            .ok()
            .unwrap();
        assert_eq!((r.line, r.column), (Some(3), Some(7)));
        assert!(r.path.ends_with("sub dir/naïve ünïcode.rs"));
        let r = parse(&args(&["notes:2024.txt"]), cwd).ok().unwrap();
        assert!(r.path.ends_with("notes:2024.txt") && r.line.is_none());
        let r = parse(&args(&["main.rs:12"]), cwd).ok().unwrap();
        assert_eq!((r.line, r.column), (Some(12), None));
        let r = parse(&args(&["--path", "notes:2024.txt", "--line", "1"]), cwd)
            .ok()
            .unwrap();
        assert_eq!(r.line, Some(1));
        assert_eq!(
            parse(&args(&["missing.rs:1"]), cwd).err().unwrap().status,
            EXIT_MISSING
        );
        assert_eq!(
            parse(&args(&["sub dir"]), cwd).err().unwrap().status,
            EXIT_UNSUPPORTED
        );
        assert_eq!(
            parse(&args(&["main.rs", "--line", "0"]), cwd)
                .err()
                .unwrap()
                .status,
            EXIT_INVALID
        );
        assert_eq!(
            parse(&args(&["main.rs", "--column", "2"]), cwd)
                .err()
                .unwrap()
                .status,
            EXIT_INVALID
        );
        assert_eq!(parse(&args(&[]), cwd).err().unwrap().status, EXIT_INVALID);
    }

    #[test]
    fn windows_drive_paths_keep_their_drive_colon() {
        let cwd = Path::new("/nonexistent-for-test");
        assert_eq!(
            split_position(r"C:\src\main.rs:10", cwd),
            (r"C:\src\main.rs".into(), Some(10), None)
        );
        assert_eq!(
            split_position(r"C:\src\main.rs:10:4", cwd),
            (r"C:\src\main.rs".into(), Some(10), Some(4))
        );
        assert_eq!(
            split_position(r"C:\src\main.rs", cwd),
            (r"C:\src\main.rs".into(), None, None)
        );
    }
}
