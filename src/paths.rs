//! Canonical host paths in the form people and shells use.

use std::{
    io,
    path::{Path, PathBuf},
};

/// `canonicalize` that keeps ordinary Windows paths ordinary.
///
/// On Windows, `std::fs::canonicalize` returns verbatim paths (`\\?\C:\…`).
/// Shells, Git and people don't use that form, and a relative `cd` from a
/// verbatim working directory fails. `dunce` returns the plain path whenever
/// it means the same file, and is `std::fs::canonicalize` everywhere else.
pub trait Canonical {
    fn canonical(&self) -> io::Result<PathBuf>;
}

impl Canonical for Path {
    fn canonical(&self) -> io::Result<PathBuf> {
        dunce::canonicalize(self)
    }
}
