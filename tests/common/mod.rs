//! Helpers shared by the integration tests.

/// A temporary directory whose path is already canonical.
///
/// Rodtty reports host paths in canonical form. On macOS the system temporary
/// directory sits behind the `/var` -> `/private/var` symlink, so fixtures made
/// with `tempfile::tempdir()` would never compare equal to the paths Rodtty
/// returns. Creating them inside the canonical temporary directory keeps the
/// tests' expectations in the same form the host uses.
pub fn tempdir() -> std::io::Result<tempfile::TempDir> {
    let base = rodtty::paths::Canonical::canonical(std::env::temp_dir().as_path())?;
    tempfile::tempdir_in(base)
}
