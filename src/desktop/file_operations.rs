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
        let copying = matches!(operation, FileOperation::Copy { .. });
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
            FileOperation::Copy { source } => source
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

    /// Shared worker/decision flow. Rename and move can extend FileOperation without new prompts.
    pub(super) fn start_file_operation(
        &mut self,
        request: OperationRequest,
        decision: Option<(Collision, CollisionChoice)>,
        tx: &mut Tx,
    ) {
        if self.workspace.operation_busy {
            return;
        }
        self.workspace.operation_id += 1;
        if self.io(Job::FileOperation {
            id: self.workspace.operation_id,
            request,
            decision,
        }) {
            self.workspace.operation_busy = true;
            self.pending = None;
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
        match result {
            Ok(OperationOutcome::Collision(collision)) => {
                self.message = "Destination exists. Choose Replace, Keep Both, or Cancel.".into();
                self.pending = Some(Pending::Operation(OperationPrompt::Collision(collision)));
                tx.focus_when_shown("operation-cancel", false);
            }
            Ok(OperationOutcome::Completed(path)) => {
                self.message = format!("Created {}", path.display());
                self.reveal_file_result(path, tx);
            }
            Ok(OperationOutcome::Cancelled) => self.message = "File operation cancelled.".into(),
            Err(error) => self.message = format!("File operation failed: {error}"),
        }
    }

    pub(super) fn file_context_menu(&mut self, press: rune::PointerPress, tx: &mut Tx) {
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
            .border(1.0, a.hover_bg());
        for (key, title, action) in [
            ("context-create-file", "New File…", AppAction::CreateFile),
            (
                "context-create-folder",
                "New Folder…",
                AppAction::CreateFolder,
            ),
            ("context-copy-file", "Copy To…", AppAction::CopyFile),
        ] {
            let enabled = s.action_disabled(action).is_none();
            menu = menu.child(
                button(a, key, title, move |s, tx| {
                    s.workspace.operation_context = None;
                    s.dispatch(action, tx);
                })
                .w_full()
                .opacity(if enabled { 1.0 } else { 0.45 }),
            );
        }
        return Some(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .child(menu.absolute(
                    x.min((s.workspace.body_width - 230.0).max(0.0)),
                    y.min(400.0),
                )),
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
            };
            content = content.child(text(title).font(TextStyle::new(18.0, 600.0)).color(a.ink()));
            if let FileOperation::Copy { source } = operation {
                content = content.child(
                    text(format!("From {}", source.display()))
                        .font(LABEL)
                        .color(a.muted())
                        .wrap_anywhere(),
                );
            }
            content = content.child(text("Name").font(LABEL).color(a.muted())).child(
                text_input(&s.workspace.operation_name,LABEL).key("operation-name").label("File or folder name").single_line().w_full().h(34.0).color(a.ink()).caret(a.accent(),a.selection()).on_edit(|s: &mut State| &mut s.workspace.operation_name).on_submit(|s: &mut State, tx| s.submit_file_operation(tx)))
                .child(text("Destination folder").font(LABEL).color(a.muted())).child(
                text_input(&s.workspace.operation_destination,LABEL).key("operation-destination").label("Destination folder").single_line().w_full().h(34.0).color(a.ink()).caret(a.accent(),a.selection()).on_edit(|s: &mut State| &mut s.workspace.operation_destination).on_submit(|s: &mut State, tx| s.submit_file_operation(tx)))
                .child(text("Choose an existing host folder. Relative paths start at the explorer root.").font(LABEL).color(a.muted()).wrap());
            if let Some(error) = &s.workspace.operation_error {
                content = content.child(text(error).font(LABEL).color(a.danger()).wrap());
            }
            buttons = buttons.child(button(a, "operation-submit", verb, |s, tx| {
                s.submit_file_operation(tx)
            }));
        }
        OperationPrompt::Collision(collision) => {
            content = content.child(text("An item already exists").font(TextStyle::new(18.0,600.0)).color(a.ink()))
                .child(text(collision.request.target.to_string_lossy()).font(LABEL).color(a.muted()).wrap_anywhere())
                .child(text("Replace removes the existing item after the new item is ready. Keep Both creates a separate copy with an available name.").font(LABEL).color(a.muted()).wrap());
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
            .child(content.child(buttons)),
    )
}
