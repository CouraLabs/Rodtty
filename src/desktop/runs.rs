//! Compare Two Runs: the owner pins a finished command's output, runs it
//! again, and compares the pinned run with the latest one in a read-only
//! diff. Only shell-reported command boundaries are used; nothing is run or
//! repeated. The pin is an in-memory snapshot, bounded in size.
use super::*;
use rune_kit::diff::{DiffState, DiffView};

/// Bytes of output a pin keeps.
const PIN_BYTES: usize = 1 << 20;

pub(crate) struct PinnedRun {
    pub run: crate::compare::Run,
    /// The prompt and command line, as the terminal showed it.
    pub title: String,
    pub session: usize,
}

pub(crate) struct RunComparison {
    pub before: String,
    pub after: String,
    pub added: usize,
    pub removed: usize,
}

fn label(run: &crate::compare::Run, title: &str) -> String {
    let status = match run.exit_status {
        Some(code) => format!("{} (exit {code})", run.outcome),
        None => run.outcome.clone(),
    };
    let elapsed = commands::duration(std::time::Duration::from_millis(run.elapsed_ms));
    let partial = if run.complete {
        ""
    } else {
        " · incomplete: earlier output was no longer retained"
    };
    format!("{title} — {status} · {elapsed}{partial}")
}

impl State {
    fn latest_run(
        &self,
        except: Option<(usize, u64)>,
    ) -> Option<(crate::compare::Run, String, usize)> {
        let session = self.active()?;
        let record = session
            .commands()
            .into_iter()
            .rev()
            .find(|c| c.ended.is_some() && except != Some((session.id(), c.id)))?;
        let mut run = Self::command_run(session, record.id)?;
        if run.text.len() > PIN_BYTES {
            let mut cut = run.text.len() - PIN_BYTES;
            while !run.text.is_char_boundary(cut) {
                cut += 1;
            }
            run.text = run.text[cut..].to_owned();
            run.complete = false;
        }
        let title = record
            .prompt_line
            .clone()
            .unwrap_or_else(|| format!("Command {}", record.id));
        Some((run, title, session.id()))
    }

    /// Pin the active terminal's latest finished command.
    pub(crate) fn pin_last_run(&mut self) {
        match self.latest_run(None) {
            Some((run, title, session)) => {
                self.message = format!("Pinned {} for comparison.", label(&run, &title));
                self.pinned_run = Some(PinnedRun {
                    run,
                    title,
                    session,
                });
            }
            None => self.message = "No finished command to pin in this terminal.".into(),
        }
    }

    /// Compare the pinned run with the active terminal's latest other one.
    pub(crate) fn compare_with_pinned(&mut self) {
        let Some(pinned) = &self.pinned_run else {
            self.message =
                "Pin a command's output first (Terminal: Pin Last Command Output).".into();
            return;
        };
        let except = Some((pinned.session, pinned.run.command));
        let Some((run, title, _)) = self.latest_run(except) else {
            self.message = "Run the command again, then compare.".into();
            return;
        };
        let comparison = crate::compare::compare(pinned.run.clone(), run.clone());
        let before = label(&pinned.run, &pinned.title);
        let after = label(&run, &title);
        self.run_diff = DiffState::new(vec![rune_kit::diff::FileDiff::between(
            Some("pinned run".into()),
            Some("latest run".into()),
            &pinned.run.text,
            &run.text,
        )]);
        self.run_comparison = Some(RunComparison {
            before,
            after,
            added: comparison.added,
            removed: comparison.removed,
        });
    }

    /// Drop the pin and close any comparison; terminals are untouched.
    pub(crate) fn clear_pinned_run(&mut self) {
        self.pinned_run = None;
        self.run_comparison = None;
        self.run_diff = DiffState::default();
        self.message = "Cleared the pinned run.".into();
    }
}

/// The comparison dialog.
pub(crate) fn dialog(s: &State, a: &Appearance) -> Option<El> {
    let c = s.run_comparison.as_ref()?;
    let (width, height) = (900.0, 420.0);
    let summary = if c.added + c.removed == 0 {
        "The two runs printed the same output.".to_owned()
    } else {
        format!("{} lines added, {} removed.", c.added, c.removed)
    };
    let diff = DiffView::new(|s: &State| &s.run_diff, |s: &mut State| &mut s.run_diff)
        .key("run-diff")
        .build(s, a.kit())
        .focusable(true)
        .size(width - 48.0, height);
    Some(
        col()
            .key("run-comparison")
            .role("dialog")
            .label("Compare runs")
            .description(format!(
                "Before: {}. After: {}. {summary}",
                c.before, c.after
            ))
            .modal(false, |s: &mut State| s.run_comparison = None)
            .w(width)
            .px(24.0)
            .py(20.0)
            .gap(10.0)
            .radius(10.0)
            .fill(a.panel())
            .child(
                text("Compare Runs")
                    .font(ui_style(17.0, 600.0))
                    .color(a.ink()),
            )
            .child(
                text(format!("− {}", c.before))
                    .key("run-before")
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
            )
            .child(
                text(format!("+ {}", c.after))
                    .key("run-after")
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
            )
            .child(
                text(summary)
                    .key("run-summary")
                    .font(ui_label())
                    .color(a.ink()),
            )
            .child(
                el().key("run-diff-viewport")
                    .w_full()
                    .h(height)
                    .clip_soft(0.0)
                    .child(diff),
            )
            .child(
                row()
                    .gap(8.0)
                    .child(button(a, "run-prev", "Previous Difference", |s, tx| {
                        s.run_diff.next_hunk(false, tx);
                    }))
                    .child(button(a, "run-next", "Next Difference", |s, tx| {
                        s.run_diff.next_hunk(true, tx);
                    }))
                    .child(button(a, "run-clear", "Clear Pin", |s, _| {
                        s.clear_pinned_run()
                    }))
                    .child(button(a, "run-close", "Close", |s, _| {
                        s.run_comparison = None
                    })),
            ),
    )
}
