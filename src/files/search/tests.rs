use super::*;
use std::fs;

fn query(root: &Path, text: &str) -> Query {
    Query {
        root: root.to_owned(),
        text: text.into(),
        hidden: false,
        ignored: Vec::new(),
        limit: RESULT_LIMIT,
    }
}

fn run(query: &Query) -> (Vec<FileMatch>, Summary) {
    let mut found = Vec::new();
    let summary = search(query, || false, |m| found.push(m));
    (found, summary)
}

fn relatives(found: &[FileMatch]) -> Vec<&str> {
    found.iter().map(|m| m.relative.as_str()).collect()
}

#[test]
fn names_and_contents_match_case_insensitively_and_literally() {
    let root = crate::test_support::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src").join("Alpha.rs"),
        "one\n  let Alpha = 1;\r\nthree",
    )
    .unwrap();
    fs::write(root.path().join("notes.txt"), "x\ry\nan ALPHA here").unwrap();
    fs::write(root.path().join("other.txt"), "nothing").unwrap();
    let (found, summary) = run(&query(root.path(), "alpha"));
    assert_eq!(relatives(&found), ["src/Alpha.rs", "notes.txt"]);
    let alpha = &found[0];
    assert_eq!(alpha.name, Some(0..5), "the file name matches");
    assert_eq!(alpha.lines.len(), 1);
    let line = &alpha.lines[0];
    assert_eq!((line.line, line.column, line.length), (2, 7, 5));
    assert_eq!(line.snippet.text, "let Alpha = 1;", "trimmed");
    assert_eq!(&line.snippet.text[line.snippet.highlight.clone()], "Alpha");
    // A lone `\r` ends a line, as in the editor.
    let notes = &found[1];
    assert_eq!(notes.name, None);
    assert_eq!((notes.lines[0].line, notes.lines[0].column), (3, 4));
    assert_eq!(summary.rows, 3);
    assert!(!summary.truncated && !summary.cancelled);

    // Regex metacharacters are literal.
    fs::write(root.path().join("re.txt"), "axb\na.b\n(a+)").unwrap();
    let (found, _) = run(&query(root.path(), "a.b"));
    assert_eq!(relatives(&found), ["re.txt"]);
    assert_eq!(found[0].lines.len(), 1);
    assert_eq!(found[0].lines[0].line, 2);
    let (found, _) = run(&query(root.path(), "(A+)"));
    assert_eq!(found[0].lines[0].line, 3);
    // An empty query finds nothing.
    assert_eq!(run(&query(root.path(), "")).1, Summary::default());
}

#[test]
fn unicode_lowercase_maps_back_to_the_original_text() {
    let root = crate::test_support::tempdir().unwrap();
    fs::write(root.path().join("u.txt"), "Ünïcode ÄBC straße\n").unwrap();
    let (found, _) = run(&query(root.path(), "äbc"));
    let line = &found[0].lines[0];
    assert_eq!((line.column, line.length), (9, 3));
    assert_eq!(&line.snippet.text[line.snippet.highlight.clone()], "ÄBC");
    // `İ` lowers to two characters; the match still maps to whole characters.
    let mut buffer = Lowered::default();
    let needle = Needle::new("i\u{307}x").unwrap();
    assert_eq!(needle.find("a\u{130}x", &mut buffer), Some(1..4));
}

#[test]
fn hidden_and_ignored_entries_follow_the_settings() {
    let root = crate::test_support::tempdir().unwrap();
    let root = root.path();
    fs::create_dir(root.join(".config")).unwrap();
    fs::write(root.join(".config").join("match.txt"), "needle").unwrap();
    fs::write(root.join(".env"), "needle").unwrap();
    fs::create_dir(root.join("build")).unwrap();
    fs::write(root.join("build").join("out.txt"), "needle").unwrap();
    fs::write(root.join("kept.txt"), "needle").unwrap();
    fs::write(root.join("debug.log"), "needle").unwrap();
    fs::write(root.join(".gitignore"), "build/\n*.log\n").unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-q"]);
    let mut q = query(root, "needle");
    let (found, _) = run(&q);
    assert_eq!(
        relatives(&found),
        ["build/out.txt", "debug.log", "kept.txt"]
    );
    q.hidden = true;
    let (found, _) = run(&q);
    assert_eq!(
        relatives(&found),
        [
            ".config/match.txt",
            "build/out.txt",
            ".env",
            "debug.log",
            "kept.txt"
        ],
        "`.git` itself is never searched"
    );
    q.ignored = ignored_paths(root).unwrap();
    let (found, _) = run(&q);
    assert_eq!(relatives(&found), [".config/match.txt", ".env", "kept.txt"]);
}

#[cfg(unix)]
#[test]
fn a_linked_folder_is_not_followed_but_a_linked_file_is_searched() {
    let root = crate::test_support::tempdir().unwrap();
    let outside = crate::test_support::tempdir().unwrap();
    fs::write(outside.path().join("far.txt"), "needle").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    // A link back to the root would loop forever if followed.
    std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("far.txt"),
        root.path().join("file-link.txt"),
    )
    .unwrap();
    let (found, _) = run(&query(root.path(), "needle"));
    assert_eq!(relatives(&found), ["file-link.txt"]);
}

#[test]
fn binary_and_oversized_contents_are_skipped_but_their_names_match() {
    let root = crate::test_support::tempdir().unwrap();
    fs::write(root.path().join("needle.bin"), b"needle\0needle").unwrap();
    fs::write(root.path().join("latin1.txt"), b"needle \xff").unwrap();
    let mut big = "needle\n".repeat(10);
    big.push_str(&"x".repeat(MAX_FILE_BYTES as usize));
    fs::write(root.path().join("big-needle.txt"), &big).unwrap();
    fs::write(root.path().join("small.txt"), "a needle").unwrap();
    let (found, summary) = run(&query(root.path(), "needle"));
    assert_eq!(
        relatives(&found),
        ["big-needle.txt", "needle.bin", "small.txt"]
    );
    for name_only in &found[..2] {
        assert!(name_only.name.is_some() && name_only.lines.is_empty());
    }
    assert_eq!(found[2].lines.len(), 1);
    assert_eq!(summary.files_read, 1, "only the small text file is read");
}

#[test]
fn results_stop_at_the_limit_and_say_more_matched() {
    let root = crate::test_support::tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "hit\n".repeat(150)).unwrap();
    fs::write(root.path().join("b.txt"), "hit\n".repeat(150)).unwrap();
    let (found, summary) = run(&query(root.path(), "hit"));
    assert_eq!(summary.rows, RESULT_LIMIT);
    assert!(summary.truncated);
    assert_eq!(found.iter().map(FileMatch::rows).sum::<usize>(), 200);
    assert_eq!(found[1].lines.len(), 50);
    // Exactly the limit is not truncated.
    fs::write(root.path().join("b.txt"), "hit\n".repeat(50)).unwrap();
    let (_, summary) = run(&query(root.path(), "hit"));
    assert_eq!((summary.rows, summary.truncated), (200, false));
}

#[test]
fn a_stopped_search_ends_early() {
    let root = crate::test_support::tempdir().unwrap();
    for i in 0..400 {
        fs::write(
            root.path().join(format!("{i:03}.txt")),
            "needle\n".repeat(3),
        )
        .unwrap();
    }
    let mut checks = 0;
    let mut found = 0;
    let summary = search(
        &query(root.path(), "zzz"),
        || {
            checks += 1;
            checks > 20
        },
        |_| found += 1,
    );
    assert!(summary.cancelled);
    assert!(
        summary.files_read < 10,
        "stopped after {} files",
        summary.files_read
    );
    // Stopped mid-way through a matching file, what it found is still sent.
    let mut checks = 0;
    let summary = search(
        &query(root.path(), "needle"),
        || {
            checks += 1;
            checks > 3
        },
        |_| found += 1,
    );
    assert!(summary.cancelled);
    assert_eq!(found, 1);
    assert_eq!(summary.rows, 2);
}

#[test]
fn long_lines_are_cut_around_the_match() {
    let line = format!("{}MATCH{}", "a".repeat(300), "b".repeat(300));
    let s = snippet(&line, 300..305);
    assert!(s.text.starts_with('…') && s.text.ends_with('…'));
    assert_eq!(&s.text[s.highlight.clone()], "MATCH");
    assert_eq!(s.text.chars().count(), SNIPPET_CHARS + 2);
    // Near the end, the window keeps its full width before the match.
    let line = format!("{}end", "é".repeat(300));
    let s = snippet(&line, line.len() - 3..line.len());
    assert!(s.text.starts_with('…') && !s.text.ends_with('…'));
    assert_eq!(&s.text[s.highlight.clone()], "end");
}
