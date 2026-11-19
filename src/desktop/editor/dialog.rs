use crate::desktop::workspace::*;
use crate::desktop::*;

pub(crate) fn file_dialog(s: &State, a: &Appearance) -> Option<El> {
    let Some(Pending::File(prompt)) = &s.pending else {
        return None;
    };
    let (id, title) = match prompt {
        FilePrompt::Unsaved { id, .. } => (*id, "Save unsaved changes?"),
        FilePrompt::Conflict { id, .. } => (*id, "File changed on disk"),
    };
    let tab = s.workspace.tabs.iter().find(|t| t.id == id)?;
    let message = if matches!(prompt, FilePrompt::Conflict { .. }) {
        "Saving would replace changes made outside this editor. Reload discards your editor changes."
    } else {
        "Choose Save, Discard, or Cancel to continue."
    };
    let mut buttons = row()
        .gap(8.0)
        .child(button(a, "file-cancel", "Cancel", |s, _| {
            s.cancel_file_action()
        }));
    match *prompt {
        FilePrompt::Unsaved { id, action } => {
            buttons = buttons
                .child(button(a, "file-discard", "Discard", move |s, tx| {
                    s.discard_for_action(id, action, tx)
                }))
                .child(button(a, "file-save-confirm", "Save", move |s, _| {
                    s.save_file(id, false, Some(action))
                }));
        }
        FilePrompt::Conflict { id, after } => {
            buttons = buttons
                .child(button(a, "file-reload-confirm", "Reload", move |s, _| {
                    s.reload_external(id)
                }))
                .child(button(a, "file-overwrite", "Overwrite", move |s, _| {
                    s.save_file(id, true, after)
                }));
        }
    }
    Some(motion::modal(
        a,
        el().center().child(
            col()
                .key("file-dialog")
                .role("alertdialog")
                .label(title)
                .description(format!("{} {message}", tab.document.path.display()))
                .modal(false, |s: &mut State| s.cancel_file_action())
                .w(470.0)
                .px(24.0)
                .py(24.0)
                .gap(16.0)
                .radius(10.0)
                .fill(a.panel())
                .child(text(title).font(ui_style(17.0, 600.0)).color(a.ink()))
                .child(
                    text(tab.document.path.to_string_lossy())
                        .font(ui_label())
                        .color(a.muted())
                        .wrap_anywhere(),
                )
                .child(text(message).font(ui_label()).color(a.muted()).wrap())
                .child(buttons),
        ),
        Entrance::Dialog,
    ))
}
