//! Command execution on Windows: as the stubs, except for `exec`.

pub use crate::sys::stubs::commands::*;

/// Windows can't replace a process's image, so this runs `command` to its end
/// and exits with its status, as the replaced shell would have. Returns only
/// if the command can't be started, with the reason why.
///
/// # Arguments
///
/// * `command` - The process to replace the current one with.
pub(crate) fn exec(mut command: std::process::Command) -> std::io::Error {
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(err) => err,
    }
}
