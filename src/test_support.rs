//! Helpers shared by unit tests.

/// A temporary directory whose path is already canonical.
///
/// Rodtty resolves host paths (explorer roots, terminal working directories,
/// opened files) to their canonical form. On macOS the system temporary
/// directory sits behind the `/var` -> `/private/var` symlink, so fixtures made
/// with `tempfile::tempdir()` would never compare equal to the paths Rodtty
/// reports. Creating them inside the canonical temporary directory keeps the
/// tests' expectations in the same form the host uses.
pub(crate) fn tempdir() -> std::io::Result<tempfile::TempDir> {
    let base = std::env::temp_dir().canonicalize()?;
    tempfile::tempdir_in(base)
}

/// The `rodtty-shell` companion binary Cargo built beside the test binary,
/// with the platform's executable suffix (`.exe` on Windows).
pub(crate) fn shell_binary() -> Option<std::path::PathBuf> {
    let name = format!("rodtty-shell{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()?
        .ancestors()
        .map(|p| p.join(&name))
        .find(|p| p.is_file())
}
