use super::*;
use crate::files::{
    Entry,
    operations::{Collision, CollisionChoice, FileOperation, OperationOutcome, OperationRequest},
};
use crate::work::Job;

pub(super) enum OperationPrompt {
    Input(FileOperation),
    Collision(Collision),
}

impl State {
    pub(super) fn selected_entry(&self) -> Option<Entry> {
        let selected = self.workspace.tree.cursor()?;
        self.workspace
            .entries
            .iter()
            .find(|e| e.path.to_string_lossy() == selected)
            .cloned()
    }

    pub(super) fn begin_file_operation(&mut self, operation: FileOperation, tx: &mut Tx) {
        let selected = self.selected_entry();
        let copying = matches!(
            operation,
            FileOperation::Copy { .. } | FileOperation::Move { .. }
        );
        let directory = selected
            .as_ref()
            .and_then(|e| {
                if e.directory && !copying {
                    Some(e.path.clone())
                } else {
                    e.path.parent().map(PathBuf::from)
                }
            })
            .or_else(|| self.workspace.directory.clone())
            .unwrap_or_else(|| self.directory.clone());
        self.workspace.operation_name = TextEdit::new(match &operation {
            FileOperation::Copy { source } | FileOperation::Move { source } => source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            _ => String::new(),
        });
        self.workspace.operation_destination = TextEdit::new(directory.to_string_lossy());
        self.workspace.operation_error = None;
        self.workspace.operation_context = None;
        self.pending = Some(Pending::Operation(OperationPrompt::Input(operation)));
        tx.focus_when_shown(
            if copying {
                "operation-destination"
            } else {
                "operation-name"
            },
            true,
        );
    }

    pub(super) fn submit_file_operation(&mut self, tx: &mut Tx) {
        let Some(Pending::Operation(OperationPrompt::Input(operation))) = &self.pending else {
            return;
        };
        let name = self.workspace.operation_name.text.clone();
        let mut parts = std::path::Path::new(&name).components();
        if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
            || parts.next().is_some()
            || name.contains(['/', '\\'])
        {
            self.workspace.operation_error = Some("Enter a single file or folder name.".into());
            tx.focus_when_shown("operation-name", true);
            return;
        }
        let directory = PathBuf::from(&self.workspace.operation_destination.text);
        let directory = if directory.is_absolute() {
            directory
        } else {
            self.workspace
                .directory
                .as_ref()
                .unwrap_or(&self.directory)
                .join(directory)
        };
        let request = OperationRequest {
            operation: operation.clone(),
            target: directory.join(name),
        };
        self.start_file_operation(request, None, tx);
    }

    /// Shared worker/decision flow for creation, copying, renaming, and moving.
    pub(super) fn start_file_operation(
        &mut self,
        request: OperationRequest,
        decision: Option<(Collision, CollisionChoice)>,
        tx: &mut Tx,
    ) {
        if self.workspace.operation_busy {
            return;
        }
        if self.workspace.removal.is_some()
            && !matches!(request.operation, FileOperation::SaveAs { .. })
        {
            return;
        }
        let moving = matches!(request.operation, FileOperation::Move { .. });
        if moving
            && (self.workspace.tabs.iter().any(|tab| tab.saving)
                || self.workspace.has_pending_opens())
        {
            self.message = "Wait for open files and saves to finish before moving items.".into();
            return;
        }
        if moving
            && decision
                .as_ref()
                .is_some_and(|(_, choice)| *choice == CollisionChoice::Replace)
            && self
                .workspace
                .tabs
                .iter()
                .any(|tab| tab.document.path.starts_with(&request.target) && tab.dirty())
        {
            self.message = "Destination has unsaved edits. Save or close its editor before Replace, or choose Keep Both or Cancel.".into();
            self.workspace.operation_error = Some(self.message.clone());
            return;
        }
        self.workspace.operation_id += 1;
        if self.io(Job::FileOperation {
            id: self.workspace.operation_id,
            request,
            decision: decision.map(Box::new),
        }) {
            self.workspace.operation_busy = true;
            self.workspace.operation_moving = moving;
            self.workspace.operation_result = None;
            self.pending = if self.workspace.removal.is_some() {
                Some(Pending::Removal(super::removal::RemovalPrompt::Busy))
            } else {
                None
            };
            self.workspace.operation_error = None;
            tx.blur();
            self.workspace.terminal_focus = true;
            self.message = "Working on files…".into();
        }
    }

    fn decide_file_collision(&mut self, choice: CollisionChoice, tx: &mut Tx) {
        let Some(Pending::Operation(OperationPrompt::Collision(collision))) = &self.pending else {
            return;
        };
        if choice == CollisionChoice::Cancel {
            if self.workspace.removal.is_some() {
                self.cancel_removal(tx);
                return;
            }
            self.pending = None;
            self.message = "File operation cancelled.".into();
            tx.blur();
            return;
        }
        let collision = collision.clone();
        self.start_file_operation(collision.request.clone(), Some((collision, choice)), tx);
    }

    pub(super) fn finish_file_operation(
        &mut self,
        id: u64,
        result: Result<OperationOutcome, String>,
        tx: &mut Tx,
    ) {
        if id != self.workspace.operation_id {
            return;
        }
        self.workspace.operation_busy = false;
        self.workspace.operation_moving = false;
        match result {
            Ok(OperationOutcome::SavedAs(document)) => self.finish_removal_save_as(document, tx),
            Ok(OperationOutcome::Collision(collision)) => {
                self.message = "Destination exists. Choose Replace, Keep Both, or Cancel.".into();
                self.pending = Some(Pending::Operation(OperationPrompt::Collision(collision)));
                tx.focus_when_shown("operation-cancel", false);
            }
            Ok(OperationOutcome::Completed { request, path }) => {
                let verb = match &request.operation {
                    FileOperation::Move { source } => {
                        self.remap_moved_paths(source, &path);
                        "Moved"
                    }
                    FileOperation::Copy { .. } => "Copied",
                    _ => "Created",
                };
                self.message = format!("{verb} {}", path.display());
                self.workspace.operation_result = Some(path.clone());
                self.reveal_file_result(path, tx);
                if matches!(request.operation, FileOperation::Move { .. }) {
                    self.persist_workspace();
                }
            }
            Ok(OperationOutcome::Cancelled) => self.message = "File operation cancelled.".into(),
            Err(error) => {
                if self.workspace.removal.is_some() {
                    self.removal_save_as_failed(error, tx);
                } else {
                    self.message = format!("File operation failed: {error}");
                }
            }
        }
    }

    pub(super) fn file_context_menu(&mut self, press: rune::PointerPress, tx: &mut Tx) {
        if self.workspace.removal.is_some() {
            return;
        }
        tx.element_at_in(
            "file-list",
            press.x,
            press.y,
            move |s: &mut State, picked, tx| {
                let selected = picked.and_then(|picked| {
                    s.workspace
                        .entries
                        .iter()
                        .filter(|entry| {
                            picked
                                .path()
                                .contains(entry.path.to_string_lossy().as_ref())
                        })
                        .max_by_key(|entry| entry.path.as_os_str().len())
                        .map(|entry| entry.path.clone())
                });
                if let Some(path) = selected {
                    s.workspace.tree.select(&path.to_string_lossy());
                }
                s.workspace.operation_context = Some((press.x, press.y));
                tx.focus_when_shown("file-context-menu", false);
            },
        );
    }
}

fn cancel(s: &mut State, tx: &mut Tx) {
    if s.workspace.removal.is_some() {
        s.cancel_removal(tx);
        return;
    }
    s.pending = None;
    s.workspace.operation_context = None;
    tx.blur();
}

pub(super) fn view(s: &State, a: &Appearance) -> Option<El> {
    if let Some((x, y)) = s.workspace.operation_context {
        let mut menu = col()
            .key("file-context-menu")
            .modal_tx(true, cancel)
            .w(220.0)
            .px(6.0)
            .py(6.0)
            .gap(4.0)
            .radius(8.0)
            .fill(a.panel())
            .border(1.0, a.hover_bg())
            .shadow(6.0, 20.0, a.scrim());
        for (key, title, action) in [
            (
                "context-open-file",
                "Open",
                AppAction::ExplorerOpenSelection,
            ),
            ("context-create-file", "New File…", AppAction::CreateFile),
            (
                "context-create-folder",
                "New Folder…",
                AppAction::CreateFolder,
            ),
            ("context-copy-file", "Copy To…", AppAction::CopyFile),
            ("context-trash-file", "Move to Trash…", AppAction::TrashFile),
            ("context-rename-file", "Rename…", AppAction::RenameFile),
            ("context-move-file", "Move To…", AppAction::MoveFile),
        ] {
            let enabled = s.action_disabled(action).is_none();
            menu = menu.child(
                button(a, key, title, move |s, tx| {
                    s.workspace.operation_context = None;
                    s.dispatch(action, tx);
                })
                .help(s.action_help(action, title))
                .w_full()
                .justify_start()
                .opacity(if enabled { 1.0 } else { 0.45 }),
            );
        }
        return Some(
            el().absolute(0.0, 0.0).w_full().h_full().child(
                menu.absolute(
                    x.clamp(8.0, (s.workspace.operation_viewport.0 - 228.0).max(8.0)),
                    y.clamp(
                        8.0,
                        (s.workspace.operation_viewport.1
                            - (ui_label().size * 1.5 + 8.0).max(29.0) * 7.0
                            - 28.0)
                            .max(8.0),
                    ),
                ),
            ),
        );
    }
    let Some(Pending::Operation(prompt)) = &s.pending else {
        return None;
    };
    let mut content = col()
        .key("operation-dialog")
        .modal_tx(true, cancel)
        .w_full()
        .max_w(490.0)
        .max_h((s.workspace.operation_viewport.1 - 32.0).max(120.0))
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .px(24.0)
        .py(24.0)
        .gap(14.0)
        .radius(10.0)
        .fill(a.panel())
        .border(1.0, a.hover_bg());
    let mut buttons = row().w_full().gap(8.0).child(el().grow(1.0)).child(button(
        a,
        "operation-cancel",
        "Cancel",
        cancel,
    ));
    match prompt {
        OperationPrompt::Input(operation) => {
            let (title, verb) = match operation {
                FileOperation::CreateFile => ("New file", "Create file"),
                FileOperation::CreateFolder => ("New folder", "Create folder"),
                FileOperation::Copy { .. } => ("Copy item", "Copy"),
                FileOperation::SaveAs { .. } => ("Save work as", "Save As"),
                FileOperation::Move { .. } => ("Rename or move item", "Move"),
            };
            content = content.child(text(title).font(ui_style(18.0, 600.0)).color(a.ink()));
            if let FileOperation::Copy { source } | FileOperation::Move { source } = operation {
                content = content.child(
                    text(format!("From {}", source.display()))
                        .font(ui_label())
                        .color(a.muted())
                        .wrap_anywhere(),
                );
            }
            content = content.child(text("Name").font(ui_label()).color(a.muted())).child(
                text_input(&s.workspace.operation_name,ui_label()).key("operation-name").label("File or folder name").single_line().w_full().h((ui_label().size * 1.5 + 12.0).max(34.0)).color(a.ink()).fill(a.bg()).border(1.0,a.hover_bg()).radius(5.0).px(8.0).caret(a.accent(),a.selection()).on_edit(|s: &mut State| &mut s.workspace.operation_name).on_submit(|s: &mut State, tx| s.submit_file_operation(tx)))
                .child(text("Destination folder").font(ui_label()).color(a.muted())).child(
                text_input(&s.workspace.operation_destination,ui_label()).key("operation-destination").label("Destination folder").single_line().w_full().h((ui_label().size * 1.5 + 12.0).max(34.0)).color(a.ink()).fill(a.bg()).border(1.0,a.hover_bg()).radius(5.0).px(8.0).caret(a.accent(),a.selection()).on_edit(|s: &mut State| &mut s.workspace.operation_destination).on_submit(|s: &mut State, tx| s.submit_file_operation(tx)))
                .child(text("Choose an existing host folder. Relative paths start at the explorer root.").font(ui_label()).color(a.muted()).wrap());
            if let Some(error) = &s.workspace.operation_error {
                content = content.child(text(error).font(ui_label()).color(a.danger()).wrap());
            }
            buttons = buttons.child(
                button(a, "operation-submit", verb, |s, tx| {
                    s.submit_file_operation(tx)
                })
                .border(1.0, a.accent()),
            );
        }
        OperationPrompt::Collision(collision) => {
            content = content.child(text("An item already exists").font(ui_style(18.0,600.0)).color(a.ink()))
                .child(text(collision.request.target.to_string_lossy()).font(ui_label()).color(a.muted()).wrap_anywhere())
                .child(text("Replace removes the existing item after the new item is ready. Keep Both uses a separate, available name.").font(ui_label()).color(a.muted()).wrap());
            if let Some(error) = &s.workspace.operation_error {
                content = content.child(text(error).font(ui_label()).color(a.danger()).wrap());
            }
            buttons = buttons
                .child(button(a, "operation-keep-both", "Keep Both", |s, tx| {
                    s.decide_file_collision(CollisionChoice::KeepBoth, tx)
                }))
                .child(button(a, "operation-replace", "Replace", |s, tx| {
                    s.decide_file_collision(CollisionChoice::Replace, tx)
                }));
        }
    }
    Some(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(a.scrim())
            .center()
            .px(16.0)
            .py(16.0)
            .child(content.child(buttons)),
    )
}

/// Labelled renderer scene, using a temporary sample rather than any owner file.
pub(super) fn collision_demo(s: &mut State) {
    let Ok(root) = tempfile::Builder::new()
        .prefix("rodtty-copy-preview-")
        .tempdir()
    else {
        return;
    };
    let target = root.path().join("notes.txt");
    if std::fs::write(&target, "existing preview notes").is_err() {
        return;
    }
    let request = OperationRequest {
        operation: FileOperation::CreateFile,
        target,
    };
    if let Ok(OperationOutcome::Collision(collision)) = request.execute(None) {
        s.pending = Some(Pending::Operation(OperationPrompt::Collision(collision)));
    }
}
