//! The editor: open files, saving and diffs (model); its pane (view), tab
//! strip and status line; and file prompts (dialog).
mod dialog;
mod model;
mod status;
mod tab_strip;
mod view;
pub(crate) use dialog::*;
pub(crate) use status::*;
pub(crate) use tab_strip::*;
pub(crate) use view::*;
