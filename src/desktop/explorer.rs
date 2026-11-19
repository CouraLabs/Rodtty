//! The file explorer: tree state and navigation (model), its pane (view),
//! file icons, and the file operations and removal it starts.
mod icon_catalog;
pub(crate) mod icons;
mod model;
pub(crate) mod operations;
pub(crate) mod removal;
mod view;

pub(crate) use view::*;
