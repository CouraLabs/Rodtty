//! Preview scenes and native checks run by `--scene` and the `--*-check` flags.
mod clipboard;
mod demos;
mod native;
mod restoration;
mod scenes;
mod trash;
pub(crate) use demos::*;
pub(crate) use native::*;
pub(crate) use scenes::*;
