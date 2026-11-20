//! Terminal excerpts as Markdown, for issues and chat.

/// Wrap `text` in a fenced code block whose fence is longer than any run
/// of backticks inside it, after removing terminal control sequences.
/// Unicode and line breaks are kept; trailing spaces on each line go.
/// `command` and `directory` are added only when given.
pub fn markdown(text: &str, command: Option<&str>, directory: Option<&str>) -> String {
    let clean = crate::ansi::strip(text.as_bytes());
    let body: Vec<&str> = clean.lines().map(str::trim_end).collect();
    let body = body.join("\n");
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let mut out = String::new();
    if let Some(directory) = directory {
        out.push_str(&format!("In `{directory}`:\n\n"));
    }
    out.push_str(&format!("{fence}console\n"));
    if let Some(command) = command {
        out.push_str(&format!("$ {command}\n"));
    }
    out.push_str(body.trim_end_matches('\n'));
    out.push_str(&format!("\n{fence}\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::markdown;

    #[test]
    fn fences_outlast_embedded_backticks_and_colors_are_removed() {
        let md = markdown(
            "\x1b[31merror\x1b[0m: use ```rust``` blocks ✓   \nnext",
            None,
            None,
        );
        assert_eq!(
            md,
            "````console\nerror: use ```rust``` blocks ✓\nnext\n````\n"
        );
        let md = markdown("ok", Some("cargo test"), Some("~/p"));
        assert_eq!(md, "In `~/p`:\n\n```console\n$ cargo test\nok\n```\n");
    }
}
