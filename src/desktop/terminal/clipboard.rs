use crate::desktop::*;

pub(crate) fn terminal_menu(s: &mut State, point: rune::PointerPress, tx: &mut Tx) {
    if s.application_overlay_open() {
        return;
    }
    s.supersede_pending_preview();
    s.workspace.set_terminal_focus(true);
    tx.blur();
    if s.config.terminal_clipboard.right_click == config::TerminalRightClick::Paste {
        s.dispatch(AppAction::PasteTerminal, tx);
        return;
    }
    let menu = rune::PopupMenu::new()
        .item(
            rune::PopupItem::new(
                "copy-terminal",
                s.action_help(AppAction::CopyTerminal, "Copy selection"),
            )
            .enabled(s.action_disabled(AppAction::CopyTerminal).is_none()),
        )
        .item(
            rune::PopupItem::new(
                "paste-terminal",
                s.action_help(AppAction::PasteTerminal, "Paste"),
            )
            .enabled(s.action_disabled(AppAction::PasteTerminal).is_none()),
        );
    s.terminal_context_open = true;
    tx.popup_menu(menu, point, |s: &mut State, chosen, tx| {
        s.terminal_context_open = false;
        match chosen {
            Some("copy-terminal") => s.dispatch(AppAction::CopyTerminal, tx),
            Some("paste-terminal") => s.dispatch(AppAction::PasteTerminal, tx),
            _ => {}
        }
    });
}

/// Where a terminal paste was aimed when it was asked for. The clipboard is
/// read off the event thread; if the active session changed or terminal focus
/// moved before the content arrived, the paste is dropped rather than typed
/// into whatever holds focus by then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PasteTarget {
    session: usize,
    focus: u64,
}

/// What a terminal paste did with the clipboard's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PasteOutcome {
    /// This many file or folder paths, escaped as shell words, went to the session.
    Files(usize),
    Text,
    /// The target session or terminal focus changed before the read returned.
    Cancelled,
    /// The clipboard held only an image: it was saved as a PNG file and that
    /// file's path went to the session, which coding agents attach.
    Image,
    /// The clipboard's image could not be saved.
    ImageFailed,
    /// Nothing the terminal can use.
    Empty,
}

impl State {
    fn terminal_paste_target(&self) -> Option<PasteTarget> {
        Some(PasteTarget {
            session: self.active()?.id(),
            focus: self.workspace.focus_generation(),
        })
    }

    /// The one terminal paste handler. The keyboard shortcut, the palette and
    /// menu action, and the terminal context menu all dispatch
    /// `AppAction::PasteTerminal`, which lands here. The read targets the
    /// terminal explicitly, so a palette's exiting modal cannot capture it.
    pub(crate) fn request_terminal_paste(&mut self, tx: &mut Tx) {
        let Some(target) = self.terminal_paste_target() else {
            return;
        };
        tx.read_clipboard(move |s: &mut State, content, tx| {
            s.finish_terminal_paste(target, content, tx);
        });
    }

    /// Deliver what the clipboard read found to the session `target` named.
    /// Files and folders win over the text beside them (Finder also offers
    /// their bare names as text): each becomes an escaped absolute path, joined
    /// by spaces. Nothing is submitted; the shell sees a bracketed paste when
    /// the application asked for one.
    pub(crate) fn finish_terminal_paste(
        &mut self,
        target: PasteTarget,
        content: &rune::ClipboardContent,
        tx: &Tx,
    ) -> PasteOutcome {
        let (_, visible, _) = pane_visibility(self, self.workspace.body_width);
        if !visible
            || !self.workspace.terminal_focus()
            || tx.focused().is_some()
            || self.application_overlay_open()
            || self.terminal_paste_target() != Some(target)
        {
            return PasteOutcome::Cancelled;
        }
        if !content.files().is_empty() {
            let (text, pasted, skipped) = paste_paths(content.files());
            if skipped > 0 {
                self.message = format!(
                    "Skipped {skipped} clipboard {} that {} not an absolute Unicode path",
                    if skipped == 1 { "item" } else { "items" },
                    if skipped == 1 { "is" } else { "are" },
                );
            }
            if pasted == 0 {
                return PasteOutcome::Empty;
            }
            self.paste(&text);
            return PasteOutcome::Files(pasted);
        }
        if !content.text().is_empty() {
            self.paste(content.text());
            return PasteOutcome::Text;
        }
        if let Some(image) = content.image() {
            return match save_pasted_image(image.png()) {
                Ok(path) => {
                    self.paste(&quote_path(&path).unwrap_or_default());
                    PasteOutcome::Image
                }
                Err(error) => {
                    self.message = format!("Could not save the clipboard image: {error}");
                    PasteOutcome::ImageFailed
                }
            };
        }
        PasteOutcome::Empty
    }
}

/// The clipboard's files as shell words, separated by spaces, with how many
/// went in and how many were skipped. A path that is not absolute or not valid
/// Unicode is skipped: Rodtty's shell reads Unicode input, and a relative path
/// from the clipboard has no directory to be relative to.
fn paste_paths(paths: &[std::path::PathBuf]) -> (String, usize, usize) {
    let words: Vec<String> = paths
        .iter()
        .filter(|path| path.is_absolute())
        .filter_map(|path| quote_path(path))
        .collect();
    let skipped = paths.len() - words.len();
    (words.join(" "), words.len(), skipped)
}

/// Saves a pasted image as a PNG in the system temporary folder, for a
/// terminal program (such as a coding agent) to read by path. The file is
/// readable only by the owner and stays until the system clears the folder.
fn save_pasted_image(png: &[u8]) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .prefix("rodtty-paste-")
        .suffix(".png")
        .tempfile()?;
    file.write_all(png)?;
    file.as_file().sync_all()?;
    let (_, path) = file.keep().map_err(|error| error.error)?;
    Ok(path)
}

/// One path as a single word for Rodtty's Bash-style shell, written as
/// Terminal.app and iTerm write a dropped file, so coding agents recognize it
/// as a path to attach: as is when every character is plain, otherwise with a
/// backslash before each space and shell-special ASCII character. A path with
/// control characters uses ANSI-C `$'…'` quoting instead, so a newline or
/// escape in a file name stays in the word and never reaches the terminal raw,
/// where it could submit or be stripped.
fn quote_path(path: &std::path::Path) -> Option<String> {
    let text = path.to_str()?;
    if !text.chars().any(char::is_control) {
        let plain =
            |c: char| !c.is_ascii() || c.is_ascii_alphanumeric() || "/._-+@%:,=".contains(c);
        let mut word = String::with_capacity(text.len());
        for c in text.chars() {
            if !plain(c) {
                word.push('\\');
            }
            word.push(c);
        }
        return Some(word);
    }
    let mut word = String::from("$'");
    for c in text.chars() {
        match c {
            '\\' => word.push_str("\\\\"),
            '\'' => word.push_str("\\'"),
            '\n' => word.push_str("\\n"),
            '\t' => word.push_str("\\t"),
            c if c.is_control() => {
                let mut bytes = [0; 4];
                for byte in c.encode_utf8(&mut bytes).bytes() {
                    word.push_str(&format!("\\x{byte:02x}"));
                }
            }
            c => word.push(c),
        }
    }
    word.push('\'');
    Some(word)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod selection_threshold_tests;

#[cfg(test)]
mod paste_tests;
