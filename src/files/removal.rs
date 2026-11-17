//! Recoverable host removal. Every permanent deletion requires a separate caller decision.
use super::operations::fingerprint;
use anyhow::{Context, Result, bail};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug)]
pub struct RemovalPlan {
    pub path: PathBuf,
    version: u64,
}

#[derive(Debug)]
pub enum RemovalOutcome {
    Removed { path: PathBuf, permanent: bool },
    TrashFailed { plan: RemovalPlan, error: String },
}

impl RemovalPlan {
    /// Snapshot on the file worker before showing the owner a decision.
    pub fn prepare(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .context("Cannot remove a filesystem root")?
            .canonicalize()
            .context("Open removal parent folder")?;
        let name = path
            .file_name()
            .context("Cannot remove a filesystem root")?;
        // Resolve parent aliases without following a selected symbolic link.
        let path = parent.join(name);
        let version = fingerprint(&path)?.context("The selected item no longer exists")?;
        Ok(Self { path, version })
    }

    fn check(&self) -> Result<()> {
        if fingerprint(&self.path)? != Some(self.version) {
            bail!(
                "The selected item changed since you reviewed it. Nothing was removed. Try again to review the new version."
            );
        }
        Ok(())
    }

    /// Use native APIs on Windows/macOS and the Freedesktop specification on Linux.
    /// Failure never falls back to permanent removal.
    pub fn trash(self) -> Result<RemovalOutcome> {
        self.check()?;
        #[allow(unused_mut)]
        let mut context = trash::TrashContext::new();
        #[cfg(target_os = "macos")]
        {
            use trash::macos::{DeleteMethod, TrashContextExtMacos};
            context.set_delete_method(DeleteMethod::NsFileManager);
        }
        match context.delete(&self.path) {
            Ok(()) => Ok(RemovalOutcome::Removed {
                path: self.path,
                permanent: false,
            }),
            Err(error) => {
                // A platform error is not evidence of successful removal. If the source
                // changed, retain buffers and report the uncertain outcome explicitly.
                if fingerprint(&self.path)? != Some(self.version) {
                    bail!(
                        "Trash reported an error: {error}. The source changed or is no longer at {}. Editor buffers were retained; inspect the host Trash and source before retrying.",
                        self.path.display()
                    );
                }
                Ok(RemovalOutcome::TrashFailed {
                    plan: self,
                    error: error.to_string(),
                })
            }
        }
    }

    /// Call only after explicit permanent-deletion consent, even if confirmations are off.
    pub fn permanently_delete(self) -> Result<RemovalOutcome> {
        self.check()?;
        let metadata = fs::symlink_metadata(&self.path)?;
        let result = if metadata.is_dir() && !metadata.is_symlink() {
            fs::remove_dir_all(&self.path)
        } else {
            fs::remove_file(&self.path)
        };
        result.with_context(|| format!("Permanent deletion failed for {}; some folder contents may have been removed. Editor buffers were retained", self.path.display()))?;
        Ok(RemovalOutcome::Removed {
            path: self.path,
            permanent: true,
        })
    }
}
