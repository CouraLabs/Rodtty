//! The window: root view, custom frame, header, footer, workspace menu and
//! command palette.
mod footer;
pub(crate) mod frame;
mod menu;
pub(crate) mod palette;
mod view;

pub(crate) use footer::*;
pub(crate) use menu::*;
pub(crate) use view::*;
