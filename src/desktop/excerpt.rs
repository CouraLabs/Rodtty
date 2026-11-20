//! Copy as Markdown: a preview of the terminal selection, captured when
//! the owner asks, that they can edit and redact before copying. The
//! command and working directory are offered, off by default. Only the
//! approved preview reaches the clipboard; Cancel copies nothing.
use super::*;

pub(crate) struct ExcerptPreview {
    pub command: Option<String>,
    pub directory: Option<String>,
    pub include_command: bool,
    pub include_directory: bool,
}

impl State {
    /// Capture the selection and open the preview.
    pub(crate) fn begin_markdown_excerpt(&mut self) {
        let Some((a, b)) = self.selection else {
            self.message = "Select terminal text first.".into();
            return;
        };
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let text = self.with_screen(|screen| screen.contents_between(a.0, a.1, b.0, b.1));
        if text.trim().is_empty() {
            self.message = "The selection is empty.".into();
            return;
        }
        let session = self.active();
        let command = session
            .and_then(|s| s.commands().into_iter().rev().find_map(|c| c.prompt_line))
            .map(|line| match line.rfind("$ ") {
                Some(i) => line[i + 2..].to_owned(),
                None => line,
            });
        let directory = session.map(|s| window::tilde_path(&s.cwd()));
        // The snapshot: later output never changes what is copied.
        let body = crate::ansi::strip(text.as_bytes());
        self.excerpt_text = TextEdit::new(body.trim_end_matches('\n'));
        self.excerpt = Some(ExcerptPreview {
            command,
            directory,
            include_command: false,
            include_directory: false,
        });
    }

    /// The Markdown the preview would copy.
    pub(crate) fn excerpt_markdown(&self) -> Option<String> {
        let p = self.excerpt.as_ref()?;
        Some(crate::excerpt::markdown(
            &self.excerpt_text.text,
            p.command.as_deref().filter(|_| p.include_command),
            p.directory.as_deref().filter(|_| p.include_directory),
        ))
    }

    /// Replace the preview's selected text with a redaction mark.
    pub(crate) fn redact_excerpt(&mut self) {
        let (a, b) = self.excerpt_text.range();
        if a == b {
            self.message = "Select the text to redact in the preview.".into();
            return;
        }
        let mut text = self.excerpt_text.text.clone();
        text.replace_range(a..b, "[REDACTED]");
        self.excerpt_text = TextEdit::new(text);
    }

    pub(crate) fn confirm_markdown_excerpt(&mut self, tx: &mut Tx) {
        if let Some(markdown) = self.excerpt_markdown() {
            tx.copy(markdown);
            self.message = "Copied the excerpt as Markdown.".into();
        }
        self.excerpt = None;
    }
}

pub(crate) fn dialog(s: &State, a: &Appearance) -> Option<El> {
    let p = s.excerpt.as_ref()?;
    let markdown = s.excerpt_markdown().unwrap_or_default();
    let mut options = row().gap(8.0);
    if p.command.is_some() {
        options = options.child(
            button(a, "excerpt-command", "Include command", |s, _| {
                if let Some(p) = &mut s.excerpt {
                    p.include_command = !p.include_command;
                }
            })
            .toggle_state(a, p.include_command),
        );
    }
    if p.directory.is_some() {
        options = options.child(
            button(a, "excerpt-directory", "Include folder", |s, _| {
                if let Some(p) = &mut s.excerpt {
                    p.include_directory = !p.include_directory;
                }
            })
            .toggle_state(a, p.include_directory),
        );
    }
    Some(
        col()
            .key("excerpt-preview")
            .role("dialog")
            .label("Copy as Markdown")
            .modal(false, |s: &mut State| s.excerpt = None)
            .w(620.0)
            .px(24.0)
            .py(24.0)
            .gap(10.0)
            .radius(10.0)
            .fill(a.panel())
            .child(text("Copy as Markdown").font(ui_style(17.0, 600.0)).color(a.ink()))
            .child(
                text("Edit the text, or select part of it and press Redact. Only what is shown below is copied.")
                    .font(ui_label())
                    .color(a.muted())
                    .wrap(),
            )
            .child(
                text_input(&s.excerpt_text, ui_label())
                    .key("excerpt-text")
                    .label("Excerpt text")
                    .w_full()
                    .h(200.0)
                    .px(10.0)
                    .py(8.0)
                    .radius(6.0)
                    .fill(a.bg())
                    .color(a.ink())
                    .border(1.0, a.kit().c.border_base)
                    .caret(a.accent(), a.selection())
                    .on_edit(|s: &mut State| &mut s.excerpt_text),
            )
            .child(options)
            .child(
                text(markdown)
                    .key("excerpt-markdown")
                    .label("Markdown to copy")
                    .font(ui_label())
                    .color(a.muted())
                    .wrap_anywhere(),
            )
            .child(
                row()
                    .gap(8.0)
                    .child(button(a, "excerpt-redact", "Redact Selection", |s, _| s.redact_excerpt()))
                    .child(button(a, "excerpt-cancel", "Cancel", |s, _| s.excerpt = None))
                    .child(button(a, "excerpt-copy", "Copy", |s, tx| s.confirm_markdown_excerpt(tx))),
            ),
    )
}
