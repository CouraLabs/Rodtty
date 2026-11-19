use crate::desktop::*;

/// A single-line text field with the shared caret, width, inset and text
/// color. Callers add the frame (padding, border, fill) their context needs.
pub(crate) fn text_field(
    a: &Appearance,
    edit: &TextEdit,
    key: impl Into<String>,
    label: impl Into<String>,
) -> El {
    text_input(edit, ui_label())
        .caret(a.accent(), a.selection())
        .key(key)
        .label(label)
        .single_line()
        .w_full()
        .px(8.0)
        .color(a.ink())
}
