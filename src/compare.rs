//! Comparing two command runs' output, from the command boundaries the
//! session's shell reported. Formatting is normalized (escape sequences
//! removed, carriage returns folded) while whitespace inside lines and
//! Unicode are kept. Nothing is executed or repeated.
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub command: u64,
    pub outcome: String,
    pub exit_status: Option<i32>,
    pub elapsed_ms: u64,
    /// Whether all of the run's output was still retained.
    pub complete: bool,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    pub before: Run,
    pub after: Run,
    pub added: usize,
    pub removed: usize,
    pub unified: String,
}

pub fn compare(before: Run, after: Run) -> Comparison {
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_secs(1))
        .diff_lines(&before.text, &after.text);
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("run {} (exit {:?})", before.command, before.exit_status),
            &format!("run {} (exit {:?})", after.command, after.exit_status),
        )
        .to_string();
    Comparison {
        before,
        after,
        added,
        removed,
        unified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(command: u64, text: &str) -> Run {
        Run {
            command,
            outcome: "failed".into(),
            exit_status: Some(1),
            elapsed_ms: 0,
            complete: true,
            text: crate::ansi::strip(text.as_bytes()),
        }
    }

    #[test]
    fn colors_do_not_create_false_changes() {
        let a = run(
            1,
            "\x1b[32mok\x1b[0m test a\r\n\x1b[31mFAIL\x1b[0m test b\r\n",
        );
        let b = run(2, "ok test a\nok test b\n");
        let c = compare(a, b);
        assert_eq!((c.added, c.removed), (1, 1));
        assert!(c.unified.contains("-FAIL test b") && c.unified.contains("+ok test b"));
    }
}
