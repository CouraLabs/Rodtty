//! Search Files: file names and saved UTF-8 contents under an explorer root.
//!
//! Matching is literal and case-insensitive: the query and each candidate are
//! lowered with Rust's Unicode `to_lowercase` (not full case folding, so `ß`
//! does not match `SS`), and no character in the query is special. Contents
//! are matched one line at a time, with lines ending as the editor splits
//! them (`\n`, `\r\n` or a lone `\r`), so a content match's line and column
//! land on the same place in the opened file.
//!
//! The walk applies the explorer's own filter ([`super::filtered_out`]), never
//! follows a symbolic link to a folder, never enters `.git`, and reads only
//! regular files (a linked file counts). Contents over the editor's
//! [`MAX_FILE_BYTES`] limit, with a NUL byte, or not valid UTF-8 are not
//! searched; their names still are. The walk holds no locks and checks its
//! stop signal per directory entry and per line, so a superseded search ends
//! promptly.
use super::{MAX_FILE_BYTES, filtered_out};
use crate::paths::Canonical as _;
use std::{
    fs,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

/// The most result rows (file-name and content matches; path headers are not
/// counted) one search reports.
pub const RESULT_LIMIT: usize = 200;
/// A snippet's longest text, in characters, before ellipses.
const SNIPPET_CHARS: usize = 100;
/// Characters a long line's snippet keeps before its match.
const SNIPPET_BEFORE: usize = 24;

#[derive(Clone, Debug)]
pub struct Query {
    /// The explorer root; results are named relative to it.
    pub root: PathBuf,
    pub text: String,
    /// Hidden Files: dot-named entries are searched.
    pub hidden: bool,
    /// Paths Git ignores, when Respect Git Ignore is on; see [`ignored_paths`].
    pub ignored: Vec<PathBuf>,
    pub limit: usize,
}

/// A line's text as a result row shows it, with the match's byte range in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Snippet {
    pub text: String,
    pub highlight: Range<usize>,
}

/// The first match on one line of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct LineMatch {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column of the match's first character, counted in characters.
    pub column: usize,
    /// The match's length in characters.
    pub length: usize,
    pub snippet: Snippet,
}

/// One file's matches: its name, its lines, or both.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMatch {
    pub path: PathBuf,
    /// The path under the root, `/`-separated.
    pub relative: String,
    /// The match's byte range in the file name, when the name matches.
    pub name: Option<Range<usize>>,
    pub lines: Vec<LineMatch>,
}

impl FileMatch {
    pub fn rows(&self) -> usize {
        usize::from(self.name.is_some()) + self.lines.len()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Result rows reported, at most the query's limit.
    pub rows: usize,
    /// More rows matched than the limit allowed.
    pub truncated: bool,
    /// The stop signal ended the walk before it finished.
    pub cancelled: bool,
    /// Files whose contents were read, for observing an early stop.
    pub files_read: usize,
}

/// The paths Git ignores in the repository containing `root`, through the same
/// Git query the explorer tree uses. Empty outside a repository.
pub fn ignored_paths(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    match crate::git::inspect(root)? {
        Some(repo) => crate::git::ignored(&repo),
        None => Ok(Vec::new()),
    }
}

/// Search `query.root`, calling `emit` with each file that has matches, in
/// tree order (folders first, then by name). `stop` is polled often; once it
/// returns true the walk ends and the summary says it was cancelled.
pub fn search(
    query: &Query,
    mut stop: impl FnMut() -> bool,
    mut emit: impl FnMut(FileMatch),
) -> Summary {
    let mut summary = Summary::default();
    let Some(needle) = Needle::new(&query.text) else {
        return summary;
    };
    let Ok(root) = query.root.canonical() else {
        return summary;
    };
    let mut lowered = Lowered::default();
    let mut stack = vec![children(&root, query)];
    while let Some(level) = stack.last_mut() {
        let Some((path, name, folder)) = level.next() else {
            stack.pop();
            continue;
        };
        if stop() {
            summary.cancelled = true;
            return summary;
        }
        if folder {
            stack.push(children(&path, query));
            continue;
        }
        let mut found = FileMatch {
            relative: relative(&root, &path),
            name: needle.find(&name, &mut lowered),
            lines: Vec::new(),
            path,
        };
        let mut budget = query.limit.saturating_sub(summary.rows);
        if found.name.is_some() {
            if budget == 0 {
                summary.truncated = true;
                return summary;
            }
            budget -= 1;
        }
        if let Some(text) = read_contents(&found.path) {
            summary.files_read += 1;
            for (index, line) in lines(&text).enumerate() {
                if stop() {
                    summary.cancelled = true;
                    summary.rows += found.rows();
                    if found.rows() > 0 {
                        emit(found);
                    }
                    return summary;
                }
                let Some(range) = needle.find(line, &mut lowered) else {
                    continue;
                };
                if budget == 0 {
                    summary.truncated = true;
                    break;
                }
                budget -= 1;
                found.lines.push(LineMatch {
                    line: index + 1,
                    column: line[..range.start].chars().count() + 1,
                    length: line[range.clone()].chars().count(),
                    snippet: snippet(line, range),
                });
            }
        }
        summary.rows += found.rows();
        if found.rows() > 0 {
            emit(found);
        }
        if summary.truncated {
            return summary;
        }
    }
    summary
}

/// A folder's visible children, folders first, each with its name and
/// whether to descend into it.
fn children(folder: &Path, query: &Query) -> std::vec::IntoIter<(PathBuf, String, bool)> {
    let mut entries = Vec::new();
    if let Ok(read) = fs::read_dir(folder) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if filtered_out(&name, &path, query.hidden, &query.ignored) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let folder = if kind.is_symlink() {
                // A linked file is searched as the tree shows it; a linked
                // folder is never entered, which also rules out link cycles.
                match fs::metadata(&path) {
                    Ok(target) if target.is_file() => false,
                    _ => continue,
                }
            } else if kind.is_dir() {
                if name == ".git" {
                    continue;
                }
                true
            } else if kind.is_file() {
                false
            } else {
                // Pipes, sockets and devices: opening one can block.
                continue;
            };
            entries.push((path, name, folder));
        }
    }
    entries.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
            .then_with(|| a.1.cmp(&b.1))
    });
    entries.into_iter()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// A file's saved text, or `None` when it is too large, binary or not UTF-8.
fn read_contents(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut raw = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut raw).ok()?;
    if raw.len() as u64 > MAX_FILE_BYTES || raw.contains(&0) {
        return None;
    }
    if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        raw.drain(..3);
    }
    String::from_utf8(raw).ok()
}

/// Lines as the editor splits them: at `\n`, `\r\n` and a lone `\r`.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let current = rest?;
        match current.find(['\n', '\r']) {
            Some(at) => {
                let skip = if current[at..].starts_with("\r\n") {
                    2
                } else {
                    1
                };
                rest = Some(&current[at + skip..]);
                Some(&current[..at])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

/// A lowered query.
struct Needle(String);

/// A reused buffer: a lowered line and, per lowered byte, the byte in the
/// original line whose character it came from.
#[derive(Default)]
struct Lowered {
    text: String,
    origin: Vec<usize>,
}

impl Needle {
    fn new(query: &str) -> Option<Self> {
        (!query.is_empty()).then(|| Self(query.to_lowercase()))
    }

    /// The first match's byte range in `haystack`.
    fn find(&self, haystack: &str, buffer: &mut Lowered) -> Option<Range<usize>> {
        let needle = self.0.as_str();
        if haystack.is_ascii() {
            // ASCII lowers byte for byte, so offsets need no mapping, and no
            // ASCII text lowers to a non-ASCII needle.
            if !needle.is_ascii() || needle.len() > haystack.len() {
                return None;
            }
            let (h, n) = (haystack.as_bytes(), needle.as_bytes());
            return (0..=h.len() - n.len())
                .find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
                .map(|i| i..i + n.len());
        }
        buffer.text.clear();
        buffer.origin.clear();
        for (at, c) in haystack.char_indices() {
            for lower in c.to_lowercase() {
                buffer.text.push(lower);
                buffer.origin.resize(buffer.text.len(), at);
            }
        }
        let start = buffer.text.find(needle)?;
        let last = buffer.origin[start + needle.len() - 1];
        let width = haystack[last..].chars().next().map_or(0, char::len_utf8);
        Some(buffer.origin[start]..last + width)
    }
}

/// The line trimmed of surrounding whitespace and, when long, cut to a window
/// around the match with ellipses where text was left out.
fn snippet(line: &str, matched: Range<usize>) -> Snippet {
    let lead = (line.len() - line.trim_start().len()).min(matched.start);
    let tail = line.trim_end().len().max(matched.end);
    let content = &line[lead..tail];
    let matched = matched.start - lead..matched.end - lead;
    let bounds: Vec<usize> = content
        .char_indices()
        .map(|(at, _)| at)
        .chain([content.len()])
        .collect();
    let chars = bounds.len() - 1;
    if chars <= SNIPPET_CHARS {
        return Snippet {
            text: content.to_owned(),
            highlight: matched,
        };
    }
    let match_char = bounds.partition_point(|&b| b < matched.start);
    let first = match_char
        .saturating_sub(SNIPPET_BEFORE)
        .min(chars - SNIPPET_CHARS);
    let end = first + SNIPPET_CHARS;
    let (from, to) = (bounds[first], bounds[end]);
    let prefix = if first > 0 { "…" } else { "" };
    let suffix = if end < chars { "…" } else { "" };
    let shift = prefix.len();
    Snippet {
        text: format!("{prefix}{}{suffix}", &content[from..to]),
        highlight: matched.start - from + shift..matched.end.min(to) - from + shift,
    }
}

#[cfg(test)]
mod tests;
