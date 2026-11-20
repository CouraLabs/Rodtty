//! The Changes view: the explorer pane lists the repository's staged,
//! unstaged and untracked files for the explorer's folder, read off the UI
//! thread through the host's Git. Choosing a file opens its Staged or
//! Unstaged diff. Inspection never writes the index, the working tree or an
//! editor buffer.
use super::*;
use crate::git::{ChangedFile, Changes, Comparison};
use std::sync::mpsc;

type Outcome = (u64, PathBuf, Result<Option<Changes>, String>);

#[derive(Default)]
pub(crate) struct ChangesView {
    pub open: bool,
    /// The folder the shown result is for.
    pub root: Option<PathBuf>,
    pub result: Option<Result<Option<Changes>, String>>,
    pub loading: bool,
    pub selected: usize,
    token: u64,
    scan_seen: u64,
    channel: Option<(mpsc::Sender<Outcome>, mpsc::Receiver<Outcome>)>,
}

impl ChangesView {
    /// Every row in display order with its comparison.
    pub fn rows(&self) -> Vec<(Comparison, ChangedFile, &'static str)> {
        let Some(Ok(Some(changes))) = &self.result else {
            return Vec::new();
        };
        changes
            .staged
            .iter()
            .map(|f| (Comparison::Staged, f.clone(), "Staged"))
            .chain(
                changes
                    .unstaged
                    .iter()
                    .map(|f| (Comparison::Unstaged, f.clone(), "Unstaged")),
            )
            .chain(
                changes
                    .untracked
                    .iter()
                    .map(|f| (Comparison::Unstaged, f.clone(), "Untracked")),
            )
            .collect()
    }
}

fn kind_name(kind: char) -> &'static str {
    match kind {
        'M' => "modified",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'T' => "type changed",
        'U' => "unmerged",
        '?' => "untracked",
        _ => "changed",
    }
}

impl State {
    fn changes_root(&self) -> PathBuf {
        self.workspace
            .directory
            .clone()
            .unwrap_or_else(|| self.active_workspace().1)
    }

    /// Show or hide the Changes view in the explorer pane.
    pub(crate) fn toggle_changes_view(&mut self) {
        self.changes.open = !self.changes.open;
        if self.changes.open {
            self.workspace.layout.explorer = true;
            self.refresh_changes();
        }
    }

    /// Read the changes again, for the explorer's current folder. A result
    /// for an earlier request or another folder is discarded when it lands.
    pub(crate) fn refresh_changes(&mut self) {
        let root = self.changes_root();
        if self.changes.root.as_ref() != Some(&root) {
            self.changes.result = None;
            self.changes.selected = 0;
        }
        self.changes.root = Some(root.clone());
        self.changes.token += 1;
        self.changes.loading = true;
        self.changes.scan_seen = self.workspace.scan_token;
        let token = self.changes.token;
        let sender = self
            .changes
            .channel
            .get_or_insert_with(mpsc::channel)
            .0
            .clone();
        std::thread::spawn(move || {
            let result = crate::git::changes(&root).map_err(|e| format!("{e:#}"));
            let _ = sender.send((token, root, result));
        });
    }

    /// Take finished reads; refresh after the explorer rescans or its
    /// folder changes.
    pub(crate) fn poll_changes(&mut self) -> bool {
        if !self.changes.open {
            return false;
        }
        let mut changed = false;
        let landed: Vec<Outcome> = self
            .changes
            .channel
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for (token, root, result) in landed {
            if token == self.changes.token && self.changes.root.as_ref() == Some(&root) {
                self.changes.result = Some(result);
                self.changes.loading = false;
                let rows = self.changes.rows().len();
                self.changes.selected = self.changes.selected.min(rows.saturating_sub(1));
                changed = true;
            }
        }
        let root = self.changes_root();
        if !self.changes.loading
            && (self.changes.root.as_ref() != Some(&root)
                || self.changes.scan_seen != self.workspace.scan_token)
        {
            self.refresh_changes();
            changed = true;
        }
        changed
    }

    /// Open row `index`'s diff, or explain why it has none to show.
    pub(crate) fn open_change(&mut self, index: usize, tx: &mut Tx) {
        let rows = self.changes.rows();
        let Some((comparison, file, group)) = rows.get(index).cloned() else {
            return;
        };
        self.changes.selected = index;
        if file.kind == 'D' {
            self.message = format!(
                "{} is deleted ({}): there is no file to compare in the editor.",
                file.relative,
                group.to_lowercase()
            );
            return;
        }
        if file.kind == 'U' {
            self.message = format!(
                "{} has merge conflicts: resolve them in the editor or with Git.",
                file.relative
            );
            self.open_file(file.path.clone());
            return;
        }
        if !file.path.is_file() {
            self.message = format!(
                "{} is not a regular file (a folder or submodule); it has no text diff.",
                file.relative
            );
            return;
        }
        self.show_diff(file.path.clone(), comparison, tx);
        self.message = format!("{} · {} {}.", file.relative, group, kind_name(file.kind));
    }

    /// Move to the next or previous changed file and open its diff.
    pub(crate) fn step_change(&mut self, forward: bool, tx: &mut Tx) {
        if !self.changes.open {
            self.toggle_changes_view();
        }
        let rows = self.changes.rows().len();
        if rows == 0 {
            self.message = if self.changes.loading {
                "Reading changes…".into()
            } else {
                "No changes.".into()
            };
            return;
        }
        let next = if forward {
            (self.changes.selected + 1).min(rows - 1)
        } else {
            self.changes.selected.saturating_sub(1)
        };
        self.open_change(next, tx);
    }
}

/// The Changes view, in place of the file tree.
pub(crate) fn view(s: &State, a: &Appearance, width: f32) -> El {
    let view = &s.changes;
    let mut pane = col()
        .key("changes")
        .role("region")
        .label("Changes")
        .w(width)
        .min_w(160.0)
        .h_full()
        .min_h(0.0)
        .fill(a.panel())
        .px(EXPLORER_PADDING)
        .pt(6.0)
        .gap(6.0);
    let context = match &view.result {
        Some(Ok(Some(c))) => {
            let mut context = format!(
                "{}{}",
                window::tilde_path(&c.root),
                if c.linked_worktree {
                    " · linked worktree"
                } else {
                    ""
                }
            );
            if !c.has_commits {
                context.push_str(" · no commits yet");
            }
            context
        }
        Some(Ok(None)) => "This folder is outside a Git repository.".into(),
        Some(Err(e)) => format!("Git could not read the changes: {e}"),
        None => "Reading changes…".into(),
    };
    pane = pane
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    text("Changes")
                        .font(ui_style(13.0, 600.0))
                        .color(a.ink())
                        .grow(1.0),
                )
                .child(button(a, "changes-refresh", "Refresh", |s, _| {
                    s.refresh_changes()
                }))
                .child(glyph_button(
                    a,
                    "changes-close",
                    "×",
                    "Show files",
                    |s, _| s.changes.open = false,
                )),
        )
        .child(
            text(context)
                .key("changes-context")
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    let rows = view.rows();
    if matches!(&view.result, Some(Ok(Some(c))) if c.is_empty()) {
        pane = pane.child(text("No changes.").font(ui_label()).color(a.muted()));
    }
    let mut list = col()
        .key("changes-list")
        .role("listbox")
        .label("Changed files")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(1.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    let mut last_group = "";
    for (index, (_, file, group)) in rows.into_iter().enumerate() {
        if group != last_group {
            last_group = group;
            list = list.child(
                text(group)
                    .key(format!("changes-group-{group}"))
                    .role("heading")
                    .font(ui_style(11.0, 600.0))
                    .color(a.muted())
                    .pt(6.0),
            );
        }
        let renamed = file
            .original
            .as_ref()
            .and_then(|o| o.file_name())
            .map(|o| format!(" ← {}", o.to_string_lossy()))
            .unwrap_or_default();
        let name = format!("{}{renamed}", file.label);
        let description = format!("{group}, {}: {}", kind_name(file.kind), file.relative);
        let selected = index == view.selected;
        list = list.child(
            row()
                .key(format!("changes-row-{index}"))
                .role("option")
                .label(name.clone())
                .description(description.clone())
                .help(description)
                .checked(selected)
                .focusable(true)
                .w_full()
                .px(6.0)
                .py(3.0)
                .gap(6.0)
                .radius(6.0)
                .fill(if selected {
                    a.hover_bg()
                } else {
                    Color::TRANSPARENT
                })
                .hover(a.hover_bg())
                .on_press_tx(move |s: &mut State, tx| s.open_change(index, tx))
                .on_key(move |s: &mut State, event, tx| match event.key {
                    Key::Down => {
                        s.step_change(true, tx);
                        tx.focus(format!("changes-row-{}", s.changes.selected), true);
                        true
                    }
                    Key::Up => {
                        s.step_change(false, tx);
                        tx.focus(format!("changes-row-{}", s.changes.selected), true);
                        true
                    }
                    _ => false,
                })
                .child(
                    text(file.kind.to_string())
                        .font(ui_style(11.0, 600.0))
                        .color(match file.kind {
                            'D' => a.danger(),
                            'A' | '?' => a.success(),
                            _ => a.accent(),
                        })
                        .w(12.0),
                )
                .child(
                    text(name)
                        .font(ui_label())
                        .color(a.ink())
                        .min_w(0.0)
                        .truncate(),
                ),
        );
    }
    pane.child(list)
}
