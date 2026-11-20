//! Terminal sessions in the window: key encoding, tabs, grid, search, palette
//! and clipboard.
pub(crate) mod clipboard;
pub(crate) mod filter;
mod keys;
pub(crate) mod mouse;
pub(crate) mod palette;
pub(crate) mod references;
pub(crate) mod search;
mod tabs;
pub(crate) mod view;

pub use keys::encode_key;
pub(crate) use tabs::*;
