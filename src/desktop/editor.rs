//! The editor: open files, saving and diffs (model), its pane (view) and
//! file prompts (dialog).
mod dialog;
mod model;
mod view;
pub(crate) use dialog::*;
pub(crate) use view::*;
