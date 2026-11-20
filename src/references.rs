//! Source references in terminal text: `path:line[:column]` as compilers,
//! test runners and agents print them. Relative, Unix absolute, `~/` and
//! Windows drive paths are recognized; URLs (`scheme://…`) and remote
//! `host:path` forms are not local files and are left alone.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// Character offsets of the reference within the line.
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub line: usize,
    pub column: Option<usize>,
}

fn path_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            ':' | '"' | '\'' | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}' | ',' | '|' | '`'
        )
}

/// Every reference in `line`.
pub fn find(line: &str) -> Vec<Reference> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // A path starts at a path character after a non-path one.
        if !path_char(chars[i])
            || (i > 0 && path_char(chars[i - 1])) && !(i == 2 && chars[1] == ':')
        {
            i += 1;
            continue;
        }
        let start = i;
        // Windows drive: `C:\` or `C:/`.
        let mut j = i;
        if j + 2 < chars.len()
            && chars[j].is_ascii_alphabetic()
            && chars[j + 1] == ':'
            && matches!(chars[j + 2], '\\' | '/')
        {
            j += 2;
        }
        while j < chars.len() && path_char(chars[j]) {
            j += 1;
        }
        let path: String = chars[start..j].iter().collect();
        let number = |from: usize| -> Option<(usize, usize)> {
            if from >= chars.len() || chars[from] != ':' {
                return None;
            }
            let digits: String = chars[from + 1..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if digits.is_empty() {
                return None;
            }
            Some((digits.parse().ok()?, from + 1 + digits.len()))
        };
        let looks_like_file = path.contains('.') || path.contains('/') || path.contains('\\');
        let url = path.starts_with("//")
            || path.contains("://")
            || chars.get(j..j + 3).is_some_and(|s| s == [':', '/', '/']);
        if let (true, false, Some((line_number, after))) = (looks_like_file, url, number(j))
            && line_number > 0
        {
            let (column, end) = match number(after) {
                Some((column, end)) if column > 0 => (Some(column), end),
                _ => (None, after),
            };
            out.push(Reference {
                start,
                end,
                path,
                line: line_number,
                column,
            });
            i = end;
        } else {
            i = j.max(i + 1);
        }
    }
    out
}

/// The reference covering character `column`, if any.
pub fn at(line: &str, column: usize) -> Option<Reference> {
    find(line)
        .into_iter()
        .find(|r| (r.start..r.end).contains(&column))
}

/// Resolve a reference against `directory` (the command's directory, or
/// the session's). Only an existing local file resolves.
pub fn resolve(
    reference: &Reference,
    directory: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let path = if let Some(rest) = reference.path.strip_prefix("~/") {
        home.ok_or("no home folder")?.join(rest)
    } else {
        directory.join(&reference.path)
    };
    if path.is_file() {
        Ok(path)
    } else if path.exists() {
        Err(format!("{} is not a file", path.display()))
    } else {
        Err(format!(
            "{} does not exist in {}",
            reference.path,
            directory.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_test_and_agent_references_are_found() {
        let r = find("error: --> src/main.rs:12:5 and tests/ünï code.rs:3");
        assert_eq!(r[0].path, "src/main.rs");
        assert_eq!((r[0].line, r[0].column), (12, Some(5)));
        assert_eq!(r.len(), 2, "{r:?}");
        let r = find("at /Users/me/app/index.ts:40:2)");
        assert_eq!(
            (r[0].path.as_str(), r[0].line),
            ("/Users/me/app/index.ts", 40)
        );
        let r = find(r"  C:\src\lib.rs:7:1 failed");
        assert_eq!(r[0].path, r"C:\src\lib.rs");
        let r = find("see ~/notes/todo.md:9");
        assert_eq!(r[0].path, "~/notes/todo.md");
        assert!(find("https://example.com:443/x").is_empty());
        assert!(find("10:30 meeting, ratio 3:4").is_empty());
        assert!(find("main.rs:0").is_empty(), "line numbers start at 1");
        let line = "  --> src/main.rs:12:5";
        assert_eq!(at(line, 8).unwrap().path, "src/main.rs");
        assert!(at(line, 1).is_none());
    }

    #[test]
    fn relative_references_resolve_against_their_directory_only() {
        let a = crate::test_support::tempdir().unwrap();
        let b = crate::test_support::tempdir().unwrap();
        std::fs::create_dir(a.path().join("src")).unwrap();
        std::fs::write(a.path().join("src/main.rs"), "").unwrap();
        let reference = &find("src/main.rs:1")[0];
        assert_eq!(
            resolve(reference, a.path(), None).unwrap(),
            a.path().join("src/main.rs")
        );
        assert!(
            resolve(reference, b.path(), None)
                .unwrap_err()
                .contains("does not exist")
        );
    }
}
