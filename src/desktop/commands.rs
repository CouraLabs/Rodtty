//! Command results in the window: Previous Command and Next Command scroll
//! the active terminal to the prompt line of a shell-reported command, and
//! opt-in notifications report a long command that finished while Rodtty was
//! in the background or its window was closed. Notifications never include
//! the command's text.
use super::*;
use crate::session::{CommandOutcome, CommandRecord};
use std::time::{Duration, Instant};

/// Notifications remembered per process, so one command notifies once.
const NOTIFIED: usize = 256;
/// The default minimum duration when the owner hasn't chosen one.
pub(crate) const DEFAULT_NOTIFY_SECONDS: u64 = 30;

fn scrollback_len(screen: &mut vt100::Screen) -> usize {
    let offset = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let len = screen.scrollback();
    screen.set_scrollback(offset);
    len
}

/// Every row in the scrollback and on screen, oldest first, trimmed. The
/// scroll position is left as it was.
fn all_rows(screen: &mut vt100::Screen) -> Vec<String> {
    let offset = screen.scrollback();
    let scrollback = scrollback_len(screen);
    let (rows, cols) = screen.size();
    let mut out = Vec::with_capacity(scrollback + usize::from(rows));
    let mut line = 0;
    let total = scrollback + usize::from(rows);
    while line < total {
        let top = line.min(scrollback);
        screen.set_scrollback(scrollback - top);
        let end = (top + usize::from(rows)).min(total);
        let visible: Vec<String> = screen
            .rows(0, cols)
            .map(|r| r.trim_end().to_owned())
            .collect();
        for absolute in line..end {
            out.push(visible.get(absolute - top).cloned().unwrap_or_default());
        }
        line = end;
    }
    screen.set_scrollback(offset);
    out
}

/// The absolute row of `records[index]`'s prompt line, when it is still
/// retained. Rows are matched by text and in order from the newest command,
/// so a command whose row scrolled out of retention, or was cleared, is not
/// confused with unrelated text: identical rows map to identical commands
/// counted from the end, and a command with too few matches has none.
pub(crate) fn locate(rows: &[String], records: &[CommandRecord], index: usize) -> Option<usize> {
    let wanted = records.get(index)?.prompt_line.as_deref()?;
    let later = records[index + 1..]
        .iter()
        .filter(|r| r.prompt_line.as_deref() == Some(wanted))
        .count();
    rows.iter()
        .enumerate()
        .rev()
        .filter(|(_, r)| r.as_str() == wanted)
        .nth(later)
        .map(|(i, _)| i)
}

pub(crate) fn describe(record: &CommandRecord) -> String {
    let outcome = match (record.outcome(), record.exit) {
        (CommandOutcome::Running, _) => "running".to_owned(),
        (CommandOutcome::Succeeded, _) => "succeeded".to_owned(),
        (CommandOutcome::Failed, Some(code)) => format!("failed (exit {code})"),
        (CommandOutcome::Failed, None) => "failed".to_owned(),
        (CommandOutcome::Interrupted, _) => "interrupted".to_owned(),
        (CommandOutcome::Unknown, _) => "result unavailable (the shell exited)".to_owned(),
    };
    format!("{outcome} · {}", duration(record.elapsed()))
}

pub(crate) fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, s / 60 % 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{:.1}s", d.as_secs_f32())
    }
}

impl State {
    /// Scroll the active terminal to the previous or next command's prompt
    /// line. Past the newest command, Next returns to the live screen.
    pub(crate) fn step_command(&mut self, backward: bool) {
        let Some(session) = self.active() else {
            return;
        };
        let session_id = session.id();
        let records = session.commands();
        if session.with_screen(vt100::Screen::alternate_screen) {
            self.message =
                "A full-screen app is showing; command navigation works in the shell's own screen."
                    .into();
            return;
        }
        if records.is_empty() {
            self.message = "No commands reported in this terminal yet.".into();
            return;
        }
        let current = self
            .command_cursor
            .filter(|(s, _)| *s == session_id)
            .and_then(|(_, id)| records.iter().position(|r| r.id == id));
        let target = match (current, backward) {
            (None, true) => Some(records.len() - 1),
            (None, false) => None,
            (Some(0), true) => Some(0),
            (Some(i), true) => Some(i - 1),
            (Some(i), false) if i + 1 < records.len() => Some(i + 1),
            (Some(_), false) => None,
        };
        let Some(index) = target else {
            session.with_screen_mut(|screen| screen.set_scrollback(0));
            self.command_cursor = None;
            self.message = "Back at the live screen.".into();
            return;
        };
        let record = records[index].clone();
        let found = session.with_screen_mut(|screen| {
            let rows = all_rows(screen);
            let line = locate(&rows, &records, index)?;
            let scrollback = scrollback_len(screen);
            screen.set_scrollback(scrollback.saturating_sub(line));
            Some(line)
        });
        self.command_cursor = Some((session_id, record.id));
        let place = record
            .directory
            .as_deref()
            .map(|d| format!(" in {}", window::tilde_path(d)))
            .unwrap_or_default();
        self.message = match found {
            Some(_) => format!(
                "Command {} of {}: {}{place}.",
                index + 1,
                records.len(),
                describe(&record)
            ),
            None => format!(
                "Command {} of {} ({}) is no longer in the retained output.",
                index + 1,
                records.len(),
                describe(&record)
            ),
        };
    }

    /// The window gained or lost focus.
    pub(crate) fn window_focus_changed(&mut self, focused: bool) {
        self.window_focused = focused;
    }

    fn backgrounded(&self) -> bool {
        !self.window_focused || self.handle.as_ref().is_some_and(|h| !h.is_visible())
    }

    /// Notify once for each long command that finished while Rodtty was in
    /// the background, when the owner turned notifications on.
    pub(crate) fn poll_command_notifications(&mut self) {
        let enabled = self.config.behavior.command_notifications;
        let threshold =
            Duration::from_secs(match self.config.behavior.command_notification_seconds {
                0 => DEFAULT_NOTIFY_SECONDS,
                s => s,
            });
        let background = self.backgrounded();
        let mut due = Vec::new();
        for (position, session) in self.sessions.iter().enumerate() {
            for record in session.commands().iter().rev().take(4) {
                let Some(ended) = record.ended else { continue };
                // Only a command that just ended: turning notifications on
                // never reports old work.
                if ended.elapsed() > Duration::from_secs(5) {
                    continue;
                }
                let key = (session.id(), record.id);
                if self.notified.contains(&key) {
                    continue;
                }
                self.notified.push_back(key);
                if self.notified.len() > NOTIFIED {
                    self.notified.pop_front();
                }
                if enabled && background && record.elapsed() >= threshold {
                    due.push(format!(
                        "A command in terminal {} {}.",
                        position + 1,
                        describe(record)
                    ));
                }
            }
        }
        for body in due {
            self.notifications_sent += 1;
            notify("Rodtty", &body);
        }
    }
}

/// Show a desktop notification through the host's own notifier, off the UI
/// thread. Failures are silent: a notification is a courtesy.
fn notify(title: &str, body: &str) {
    if cfg!(test) {
        return;
    }
    let (title, body) = (title.to_owned(), body.to_owned());
    std::thread::spawn(move || {
        let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut command = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("osascript");
            c.arg("-e").arg(format!(
                "display notification \"{}\" with title \"{}\"",
                quote(&body),
                quote(&title)
            ));
            c
        } else if cfg!(windows) {
            let script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $t = $x.GetElementsByTagName('text'); $t.Item(0).AppendChild($x.CreateTextNode('{}')) > $null; $t.Item(1).AppendChild($x.CreateTextNode('{}')) > $null; \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Rodtty').Show([Windows.UI.Notifications.ToastNotification]::new($x))",
                title.replace('\'', "''"),
                body.replace('\'', "''")
            );
            let mut c = std::process::Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            c
        } else {
            let mut c = std::process::Command::new("notify-send");
            c.args(["--app-name=Rodtty", &title, &body]);
            c
        };
        let _ = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u64, prompt: &str) -> CommandRecord {
        CommandRecord {
            id,
            directory: None,
            started: Instant::now(),
            started_at: std::time::SystemTime::now(),
            ended: Some(Instant::now()),
            exit: Some(0),
            output_start: 0,
            output_end: Some(0),
            prompt_line: Some(prompt.into()),
        }
    }

    #[test]
    fn identical_prompt_lines_map_in_order_and_expired_ones_find_nothing() {
        let rows: Vec<String> = ["$ make", "out 1", "$ ls", "$ make", "out 2"]
            .map(String::from)
            .to_vec();
        let records = vec![
            record(1, "$ make"),
            record(2, "$ make"),
            record(3, "$ ls"),
            record(4, "$ make"),
        ];
        // The newest `make` is the last row; the one before it the first.
        assert_eq!(locate(&rows, &records, 3), Some(3));
        assert_eq!(locate(&rows, &records, 2), Some(2));
        assert_eq!(locate(&rows, &records, 1), Some(0));
        // The oldest scrolled out of retention: it finds no unrelated row.
        assert_eq!(locate(&rows, &records, 0), None);
    }
}
