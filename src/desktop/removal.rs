mod native;
use super::*;
use crate::{
    files::removal::{RemovalOutcome, RemovalPlan},
    work::Job,
};
pub(super) use native::check as native_check;

pub(super) struct RemovalState {
    pub plan: Option<RemovalPlan>,
    pub discarded: Vec<(u64, u64)>,
    pub save_as: Option<u64>,
    pub permanent: bool,
}
pub(super) enum RemovalPrompt {
    Busy,
    Confirm,
    Dirty(u64),
    SaveAs(u64),
    Permanent(String),
}

impl State {
    pub(super) fn removal_background_busy(&self) -> bool {
        self.workspace.operation_busy
            && matches!(self.pending, Some(Pending::Removal(RemovalPrompt::Busy)))
    }
    pub(super) fn begin_removal(&mut self, tx: &mut Tx) {
        if self.workspace.operation_busy || self.workspace.removal.is_some() {
            return;
        }
        if self.file_io_pending() {
            self.message = "Wait for open files and saves to finish before deleting.".into();
            return;
        }
        let Some(entry) = self.selected_entry() else {
            return;
        };
        self.workspace.operation_id += 1;
        if self.io(Job::PrepareRemoval {
            id: self.workspace.operation_id,
            path: entry.path,
        }) {
            self.workspace.operation_busy = true;
            self.workspace.operation_context = None;
            self.workspace.removal = Some(RemovalState {
                plan: None,
                discarded: Vec::new(),
                save_as: None,
                permanent: false,
            });
            self.pending = Some(Pending::Removal(RemovalPrompt::Busy));
            self.workspace
                .set_terminal_focus(self.workspace.layout.terminal);
            self.message = "Preparing Move to Trash… Terminal input remains available.".into();
            tx.blur();
        }
    }
    pub(super) fn finish_prepare_removal(
        &mut self,
        id: u64,
        result: Result<RemovalPlan, String>,
        tx: &mut Tx,
    ) {
        if id != self.workspace.operation_id || self.workspace.removal.is_none() {
            return;
        }
        self.workspace.operation_busy = false;
        match result {
            Ok(plan) => {
                self.workspace.removal.as_mut().unwrap().plan = Some(plan);
                if self.config.explorer.confirm_deletion {
                    self.pending = Some(Pending::Removal(RemovalPrompt::Confirm));
                    tx.focus_when_shown("removal-cancel", false);
                } else {
                    self.advance_removal(false, tx);
                }
            }
            Err(error) => self.removal_failed(error),
        }
    }
    fn advance_removal(&mut self, permanent: bool, tx: &mut Tx) {
        if permanent && let Some(removal) = &mut self.workspace.removal {
            removal.permanent = true;
        }
        let Some(removal) = &self.workspace.removal else {
            return;
        };
        let permanent = removal.permanent;
        let Some(plan) = &removal.plan else {
            return;
        };
        if let Some(tab) = self.workspace.tabs.iter().find(|t| {
            t.document.path.starts_with(&plan.path)
                && t.dirty()
                && !removal.discarded.contains(&(t.id, t.revision))
        }) {
            self.pending = Some(Pending::Removal(RemovalPrompt::Dirty(tab.id)));
            tx.focus_when_shown("removal-cancel", false);
            return;
        }
        if self.file_io_pending() {
            self.message = "Wait for open files and saves to finish.".into();
            return;
        }
        let plan = plan.clone();
        self.workspace.operation_id += 1;
        if self.io(Job::Remove {
            id: self.workspace.operation_id,
            plan,
            permanent,
        }) {
            self.workspace.operation_busy = true;
            self.pending = Some(Pending::Removal(RemovalPrompt::Busy));
            self.workspace
                .set_terminal_focus(self.workspace.layout.terminal);
            self.message = if permanent {
                "Deleting permanently…"
            } else {
                "Moving to Trash…"
            }
            .into();
            tx.blur();
        }
    }
    fn discard_removal_buffer(&mut self, id: u64, tx: &mut Tx) {
        if let Some(tab) = self.workspace.tabs.iter().find(|t| t.id == id)
            && let Some(removal) = self.workspace.removal.as_mut()
        {
            // Record consent; do not destroy the text before successful removal.
            removal.discarded.push((id, tab.revision));
        }
        self.advance_removal(false, tx);
    }
    fn begin_removal_save_as(&mut self, id: u64, tx: &mut Tx) {
        let Some(tab) = self.workspace.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        let Some(plan) = self
            .workspace
            .removal
            .as_ref()
            .and_then(|r| r.plan.as_ref())
        else {
            return;
        };
        let path = plan
            .path
            .parent()
            .unwrap_or(&self.directory)
            .join(tab.document.path.file_name().unwrap_or_default());
        self.workspace.operation_destination = TextEdit::new(path.to_string_lossy());
        self.workspace.operation_error = None;
        self.pending = Some(Pending::Removal(RemovalPrompt::SaveAs(id)));
        tx.focus_when_shown("removal-save-as-path", true);
    }
    fn submit_removal_save_as(&mut self, tx: &mut Tx) {
        use crate::files::operations::{FileOperation, OperationRequest};
        let Some(Pending::Removal(RemovalPrompt::SaveAs(id))) = self.pending else {
            return;
        };
        let Some(tab) = self.workspace.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        let Some(removal) = &mut self.workspace.removal else {
            return;
        };
        let Some(plan) = &removal.plan else {
            return;
        };
        let path = PathBuf::from(&self.workspace.operation_destination.text);
        let target = if path.is_absolute() {
            path
        } else {
            self.workspace
                .directory
                .as_ref()
                .unwrap_or(&self.directory)
                .join(path)
        };
        let request = OperationRequest {
            target,
            operation: FileOperation::SaveAs {
                document: tab.document.clone(),
                text: tab.edit.text.clone(),
                removing: plan.path.clone(),
                protected: self
                    .workspace
                    .tabs
                    .iter()
                    .map(|t| t.document.path.clone())
                    .collect(),
            },
        };
        removal.save_as = Some(id);
        self.start_file_operation(request, None, tx);
        if self.workspace.operation_busy {
            self.pending = Some(Pending::Removal(RemovalPrompt::Busy));
        }
    }
    pub(super) fn finish_removal_save_as(&mut self, document: crate::files::Document, tx: &mut Tx) {
        let Some(removal) = &mut self.workspace.removal else {
            return;
        };
        let Some(id) = removal.save_as.take() else {
            return;
        };
        if let Some(tab) = self.workspace.tabs.iter_mut().find(|t| t.id == id) {
            // Preserve newer text/undo history while updating only the saved baseline.
            tab.document = document;
            tab.preview = false;
            tab.external = None;
            tab.disk_error = None;
            tab.dismissed = false;
            tab.baseline += 1;
            tab.revision += 1;
        }
        self.persist_file_references();
        self.invalidate_diff(true);
        self.advance_removal(false, tx);
    }
    pub(super) fn removal_save_as_failed(&mut self, error: String, tx: &mut Tx) {
        let id = self
            .workspace
            .removal
            .as_mut()
            .and_then(|r| r.save_as.take());
        if let Some(id) = id {
            self.workspace.operation_error = Some(error.clone());
            self.pending = Some(Pending::Removal(RemovalPrompt::SaveAs(id)));
            self.message = format!("Could not preserve work: {error}");
            tx.focus_when_shown("removal-save-as-path", true);
        }
    }
    pub(super) fn cancel_removal(&mut self, tx: &mut Tx) {
        if self.workspace.operation_busy {
            return;
        }
        if let Some(path) = self
            .workspace
            .removal
            .as_ref()
            .and_then(|r| r.plan.as_ref())
            .map(|p| &p.path)
        {
            for tab in &mut self.workspace.tabs {
                if tab.document.path.starts_with(path) {
                    tab.autosave.suspend(tab.revision);
                }
            }
        }
        self.workspace.removal = None;
        self.pending = None;
        self.workspace.operation_error = None;
        self.message = "Deletion cancelled. Unsaved work was retained.".into();
        tx.blur();
    }
    fn removal_failed(&mut self, error: String) {
        self.workspace.removal = None;
        self.pending = None;
        self.message = format!("Removal failed: {error}");
    }
    pub(super) fn finish_removal(
        &mut self,
        id: u64,
        result: Result<RemovalOutcome, String>,
        tx: &mut Tx,
    ) {
        if id != self.workspace.operation_id || self.workspace.removal.is_none() {
            return;
        }
        self.workspace.operation_busy = false;
        match result {
            Ok(RemovalOutcome::Removed { path, permanent }) => {
                self.workspace.removal = None;
                self.pending = None;
                self.remove_file_references(&path);
                self.message = format!(
                    "{} {}",
                    if permanent {
                        "Permanently deleted"
                    } else {
                        "Moved to Trash"
                    },
                    path.display()
                );
                tx.blur();
            }
            Ok(RemovalOutcome::TrashFailed { plan, error }) => {
                self.workspace.removal.as_mut().unwrap().plan = Some(plan);
                self.pending = Some(Pending::Removal(RemovalPrompt::Permanent(error)));
                tx.focus_when_shown("removal-cancel", false);
            }
            Err(error) => self.removal_failed(error),
        }
    }
}

pub(super) fn view(s: &State, a: &Appearance) -> Option<El> {
    let Some(Pending::Removal(prompt)) = &s.pending else {
        return None;
    };
    if matches!(prompt, RemovalPrompt::Busy) {
        return None;
    }
    let path = s
        .workspace
        .removal
        .as_ref()
        .and_then(|r| r.plan.as_ref())
        .map(|p| p.path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut content = col()
        .key("removal-dialog")
        .modal_tx(true, |s: &mut State, tx| s.cancel_removal(tx))
        .w_full()
        .max_w(520.0)
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
    let mut buttons = row().w_full().gap(8.0).child(el().grow(1.0));
    if !matches!(prompt, RemovalPrompt::Busy) {
        buttons = buttons.child(button(a, "removal-cancel", "Cancel", |s, tx| {
            s.cancel_removal(tx)
        }));
    }
    let mut fields: Option<El> = None;
    let (title, description) = match prompt {
        RemovalPrompt::Busy => (
            "Working on files…",
            "Wait for the host file operation to finish.".into(),
        ),
        RemovalPrompt::Confirm => {
            buttons = buttons.child(
                button(a, "removal-confirm", "Move to Trash", |s, tx| {
                    s.advance_removal(false, tx)
                })
                .border(1.0, a.danger()),
            );
            (
                "Move this item to Trash?",
                "Files and folders can be recovered from the host Trash.".into(),
            )
        }
        RemovalPrompt::Dirty(id) => {
            let id = *id;
            buttons = buttons
                .child(button(a, "removal-discard", "Discard", move |s, tx| {
                    s.discard_removal_buffer(id, tx)
                }))
                .child(button(a, "removal-save-as", "Save As…", move |s, tx| {
                    s.begin_removal_save_as(id, tx)
                }));
            ("Preserve unsaved work before deletion?", s.workspace.tabs.iter().find(|t|t.id==id).map(|t|format!("{} has unsaved changes. Discard applies only after successful removal.",t.document.path.display())).unwrap_or_default())
        }
        RemovalPrompt::SaveAs(_) => {
            let mut field = col().w_full().gap(8.0).child(
                text_input(&s.workspace.operation_destination, ui_label())
                    .key("removal-save-as-path")
                    .label("Preserve work at path")
                    .single_line()
                    .w_full()
                    .h((ui_label().size * 1.5 + 12.0).max(34.0))
                    .color(a.ink())
                    .fill(a.bg())
                    .border(1.0, a.hover_bg())
                    .radius(5.0)
                    .px(8.0)
                    .caret(a.accent(), a.selection())
                    .on_edit(|s: &mut State| &mut s.workspace.operation_destination)
                    .on_submit(|s: &mut State, tx| s.submit_removal_save_as(tx)),
            );
            if let Some(error) = &s.workspace.operation_error {
                field = field.child(text(error).font(ui_label()).color(a.danger()).wrap());
            }
            fields = Some(field);
            buttons = buttons.child(button(a, "removal-save-as-submit", "Save As", |s, tx| {
                s.submit_removal_save_as(tx)
            }));
            ("Save work before deletion", "Choose a file outside the item being deleted. Existing files require a separate collision decision.".into())
        }
        RemovalPrompt::Permanent(error) => {
            buttons = buttons.child(
                button(a, "removal-permanent", "Delete permanently", |s, tx| {
                    s.advance_removal(true, tx)
                })
                .border(1.0, a.danger()),
            );
            (
                "Trash could not remove this item",
                format!(
                    "The item is still at its original location. {error}\nDelete permanently only if you accept that this cannot be undone."
                ),
            )
        }
    };
    content = content
        .child(text(title).font(ui_style(18.0, 600.0)).color(a.ink()))
        .child(text(path).font(ui_label()).color(a.muted()).wrap_anywhere())
        .child(
            text(description)
                .font(ui_label())
                .color(a.muted())
                .wrap_anywhere(),
        );
    if let Some(fields) = fields {
        content = content.child(fields);
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
