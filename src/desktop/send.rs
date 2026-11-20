//! Send to Terminal: the editor's selection, or the diff hunk in view, as a
//! Markdown snippet with its project-relative path and line range. The owner
//! reviews and edits it, and picks the destination session, before anything
//! is sent; it arrives as a paste and is never submitted.
use super::*;
use std::path::Path;

/// What a Send to Terminal preview is holding.
pub(crate) struct SendPreview {
    /// The destination, by session identity; `None` once it ended.
    pub session: Option<usize>,
    /// Where the snippet came from, for the dialog.
    pub source: String,
}

fn fence_for(body: &str) -> String {
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

/// `body` fenced, with `heading` above it. No newline follows the closing
/// fence: one would submit the paste in a program without bracketed paste.
pub(crate) fn snippet(heading: &str, language: &str, body: &str) -> String {
    let body = body.trim_end_matches('\n');
    let fence = fence_for(body);
    format!("{heading}\n{fence}{language}\n{body}\n{fence}")
}

fn language_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "rb" => "ruby",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" => "cpp",
        "swift" => "swift",
        "sh" | "bash" | "zsh" => "sh",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "html" => "html",
        "css" => "css",
        _ => "",
    }
}

impl State {
    /// `path` relative to the active project, else with `~` for home.
    fn project_path(&self, path: &Path) -> String {
        let (_, root) = self.active_workspace();
        match path.strip_prefix(&root) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                relative.to_string_lossy().replace('\\', "/")
            }
            _ => window::tilde_path(path),
        }
    }

    /// The snippet for the active editor tab: its selection, or in a diff
    /// the hunk in view (or holding the selected lines).
    pub(crate) fn send_snippet(&self) -> Result<(String, String), String> {
        let tab = self
            .workspace
            .tabs
            .get(self.workspace.active)
            .ok_or("Open a file in the editor first.")?;
        let path = self.project_path(&tab.document.path);
        match tab.mode {
            workspace::View::Diff(comparison) => {
                let (file, hunk) = self
                    .workspace
                    .diff
                    .current_hunk()
                    .ok_or("This comparison has no changes to send.")?;
                let diff = &self.workspace.diff.files()[file];
                let hunk = &diff.hunks[hunk];
                let label = match comparison {
                    crate::git::Comparison::Staged => "staged",
                    crate::git::Comparison::Unstaged => "unstaged",
                    crate::git::Comparison::Current => "current",
                };
                let mut body = hunk.header();
                body.push('\n');
                for line in &hunk.lines {
                    let mark = match line.kind {
                        rune_kit::diff::LineKind::Added => '+',
                        rune_kit::diff::LineKind::Removed => '-',
                        _ => ' ',
                    };
                    body.push(mark);
                    body.push_str(&line.text);
                    body.push('\n');
                }
                let end = hunk.new_start + hunk.new_count.saturating_sub(1);
                let heading = format!("{path}:{}-{end} ({label} change):", hunk.new_start);
                Ok((
                    snippet(&heading, "diff", &body),
                    format!("{path}, {label} hunk"),
                ))
            }
            _ => {
                let (a, b) = tab.edit.range();
                if a == b {
                    return Err("Select text in the editor first.".into());
                }
                let text = &tab.edit.text[a..b];
                let first = tab.edit.line_of_byte(a) + 1;
                // A selection ending at a line's start doesn't include that line.
                let last_byte = if text.ends_with('\n') { b - 1 } else { b };
                let last = tab.edit.line_of_byte(last_byte.max(a)) + 1;
                let lines = if first == last {
                    format!("{first}")
                } else {
                    format!("{first}-{last}")
                };
                let unsaved = if tab.dirty() { " (unsaved)" } else { "" };
                let heading = format!("{path}:{lines}{unsaved}:");
                Ok((
                    snippet(&heading, language_of(&tab.document.path), text),
                    format!("{path}, lines {lines}{unsaved}"),
                ))
            }
        }
    }

    /// Open the Send to Terminal preview, aimed at the terminal in front.
    pub(crate) fn begin_send_to_terminal(&mut self) {
        match self.send_snippet() {
            Ok((text, source)) => {
                self.send_text = TextEdit::new(text);
                self.send_preview = Some(SendPreview {
                    session: self.active().map(Session::id),
                    source,
                });
                if self.sessions.is_empty() {
                    self.message = "Open a terminal to send to.".into();
                }
            }
            Err(error) => self.message = error,
        }
    }

    /// Aim the preview at the next running session.
    pub(crate) fn cycle_send_destination(&mut self) {
        let Some(preview) = &mut self.send_preview else {
            return;
        };
        let ids: Vec<usize> = self.sessions.iter().map(Session::id).collect();
        if ids.is_empty() {
            preview.session = None;
            return;
        }
        let next = preview
            .session
            .and_then(|id| ids.iter().position(|i| *i == id))
            .map_or(0, |i| (i + 1) % ids.len());
        preview.session = Some(ids[next]);
    }

    /// Paste the previewed text into its destination, without Enter. When
    /// the destination ended meanwhile nothing is sent anywhere else and the
    /// preview stays open so the owner can pick another.
    pub(crate) fn confirm_send_to_terminal(&mut self) {
        let Some(preview) = &mut self.send_preview else {
            return;
        };
        let Some(position) = preview
            .session
            .and_then(|id| self.sessions.iter().position(|s| s.id() == id))
        else {
            preview.session = None;
            self.message = "The destination terminal has ended; choose another.".into();
            return;
        };
        let text = self.send_text.text.clone();
        self.send_preview = None;
        self.active = position;
        self.workspace.layout.terminal = true;
        self.workspace.set_terminal_focus(true);
        self.deliver_paste(&text);
        self.message = format!(
            "Sent to {}; review it there and press Enter to submit.",
            self.sessions[position].tab_title()
        );
    }
}

/// The preview dialog.
pub(crate) fn dialog(s: &State, a: &Appearance) -> Option<El> {
    let preview = s.send_preview.as_ref()?;
    let destination = preview
        .session
        .and_then(|id| s.sessions.iter().find(|x| x.id() == id))
        .map(|x| format!("To “{}” ({})", x.tab_title(), window::tilde_path(&x.cwd())))
        .unwrap_or_else(|| "No destination: its terminal has ended".into());
    Some(
        col()
            .key("send-to-terminal")
            .role("dialog")
            .label("Send to Terminal")
            .description(format!("{}. {destination}.", preview.source))
            .modal(false, |s: &mut State| s.send_preview = None)
            .w(620.0)
            .px(24.0)
            .py(24.0)
            .gap(12.0)
            .radius(10.0)
            .fill(a.panel())
            .child(
                text("Send to Terminal")
                    .font(ui_style(17.0, 600.0))
                    .color(a.ink()),
            )
            .child(
                text(preview.source.clone())
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
            )
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(destination)
                            .key("send-destination")
                            .font(ui_label())
                            .color(a.ink())
                            .grow(1.0)
                            .min_w(0.0)
                            .wrap(),
                    )
                    .child(button(
                        a,
                        "send-destination-next",
                        "Change Terminal",
                        |s, _| s.cycle_send_destination(),
                    )),
            )
            .child(
                text_input(&s.send_text, ui_label())
                    .key("send-text")
                    .label("Text to send")
                    .w_full()
                    .h(260.0)
                    .px(10.0)
                    .py(8.0)
                    .radius(6.0)
                    .fill(a.bg())
                    .color(a.ink())
                    .border(1.0, a.kit().c.border_base)
                    .caret(a.accent(), a.selection())
                    .on_edit(|s: &mut State| &mut s.send_text),
            )
            .child(
                text("It is pasted without pressing Enter.")
                    .font(ui_label())
                    .color(a.muted()),
            )
            .child(
                row()
                    .gap(8.0)
                    .child(button(a, "send-cancel", "Cancel", |s, _| {
                        s.send_preview = None
                    }))
                    .child(button(a, "send-confirm", "Send", |s, _| {
                        s.confirm_send_to_terminal()
                    })),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_outlast_backticks_in_the_snippet() {
        assert_eq!(
            snippet("a.md:1:", "markdown", "use ``` here\n"),
            "a.md:1:\n````markdown\nuse ``` here\n````"
        );
    }
}
