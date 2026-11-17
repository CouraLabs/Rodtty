//! Background filesystem operations with explicit, version-checked collision decisions.
use anyhow::{Context, Result, bail};
use std::{
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum FileOperation {
    CreateFile,
    CreateFolder,
    Copy { source: PathBuf },
}
#[derive(Clone, Debug)]
pub struct OperationRequest {
    pub operation: FileOperation,
    pub target: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionChoice {
    Replace,
    KeepBoth,
    Cancel,
}
#[derive(Clone, Debug)]
pub struct Collision {
    pub request: OperationRequest,
    version: u64,
}
#[derive(Debug)]
pub enum OperationOutcome {
    Completed {
        request: OperationRequest,
        path: PathBuf,
    },
    Collision(Collision),
    Cancelled,
}

impl OperationRequest {
    /// Run on the file worker. A decision is valid only for the destination version shown.
    pub fn execute(
        self,
        decision: Option<(Collision, CollisionChoice)>,
    ) -> Result<OperationOutcome> {
        if decision
            .as_ref()
            .is_some_and(|(_, choice)| *choice == CollisionChoice::Cancel)
        {
            return Ok(OperationOutcome::Cancelled);
        }
        let parent = self
            .target
            .parent()
            .context("Choose a destination folder")?
            .canonicalize()
            .context("Open destination folder")?;
        let name = self
            .target
            .file_name()
            .context("Enter a file or folder name")?;
        let mut target = parent.join(name);
        let directory = match &self.operation {
            FileOperation::CreateFolder => true,
            FileOperation::CreateFile => false,
            FileOperation::Copy { source } => {
                let metadata = fs::symlink_metadata(source).context("Read source")?;
                if metadata.is_dir()
                    && parent.starts_with(source.canonicalize().context("Open source folder")?)
                {
                    bail!("A folder cannot be copied into itself or one of its descendants.");
                }
                let source_path = source
                    .parent()
                    .and_then(|p| p.canonicalize().ok())
                    .zip(source.file_name())
                    .map(|(parent, name)| parent.join(name));
                if source_path.as_ref() == Some(&target) {
                    // Keep Both is safe for a same-folder duplicate; Replace is never safe.
                    if decision
                        .as_ref()
                        .is_some_and(|(_, c)| *c == CollisionChoice::Replace)
                    {
                        bail!("Source and destination are the same item. Choose Keep Both.");
                    }
                }
                metadata.is_dir()
            }
        };
        let existing = fingerprint(&target)?;
        let mut replace = false;
        if let Some(version) = existing {
            match decision {
                Some((collision, choice))
                    if collision.request.target == self.target && collision.version == version =>
                {
                    match choice {
                        CollisionChoice::Replace => replace = true,
                        CollisionChoice::KeepBoth => target = available_name(&target, directory)?,
                        CollisionChoice::Cancel => unreachable!(),
                    }
                }
                _ => {
                    return Ok(OperationOutcome::Collision(Collision {
                        request: self,
                        version,
                    }));
                }
            }
        }
        if replace && let FileOperation::Copy { source } = &self.operation {
            let target_metadata = fs::symlink_metadata(&target)?;
            if target_metadata.is_dir() && source.canonicalize()?.starts_with(&target) {
                bail!("A copy cannot replace a folder containing its source. Choose Keep Both.");
            }
        }
        // Finish the potentially expensive/fallible copy before touching a destination.
        let stage = tempfile::Builder::new()
            .prefix(".rodtty-operation-")
            .tempdir_in(&parent)?;
        let prepared = stage.path().join("item");
        match &self.operation {
            FileOperation::CreateFile => {
                fs::File::create(&prepared)?;
            }
            FileOperation::CreateFolder => fs::create_dir(&prepared)?,
            FileOperation::Copy { source } => copy_item(source, &prepared)?,
        }
        let backup = stage.path().join("previous");
        if replace {
            // Recheck after staging as another process may have changed the target meanwhile.
            if fingerprint(&target)? != existing {
                bail!(
                    "Destination changed while preparing the operation. Try again to review the new version."
                );
            }
            fs::rename(&target, &backup).context("Preserve existing destination")?;
        }
        if let Err(error) = publish(&prepared, &target) {
            if replace {
                // Never overwrite an item another process created while publication failed.
                if fs::symlink_metadata(&target).is_ok() || fs::rename(&backup, &target).is_err() {
                    let recovery = stage.keep();
                    bail!(
                        "{error:#}. Previous destination preserved at {}",
                        recovery.join("previous").display()
                    );
                }
            }
            return Err(error);
        }
        Ok(OperationOutcome::Completed {
            request: self,
            path: target,
        })
    }
}

fn available_name(target: &Path, directory: bool) -> Result<PathBuf> {
    let stem = if directory {
        target.file_name()
    } else {
        target.file_stem()
    }
    .context("Invalid destination name")?
    .to_string_lossy();
    let extension = if directory { None } else { target.extension() };
    for number in 1..100_000 {
        let suffix = if number == 1 {
            " copy".into()
        } else {
            format!(" copy {number}")
        };
        let mut name = format!("{stem}{suffix}");
        if let Some(extension) = extension {
            name.push('.');
            name.push_str(&extension.to_string_lossy());
        }
        let candidate = target.with_file_name(name);
        if fs::symlink_metadata(&candidate).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(candidate);
        }
    }
    bail!("Could not find an available destination name")
}

fn copy_item(source: &Path, target: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(source).with_context(|| format!("Read {}", source.display()))?;
    if metadata.is_symlink() {
        let link = fs::read_link(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(link, target)?;
        #[cfg(windows)]
        {
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(link, target)?;
            } else {
                std::os::windows::fs::symlink_file(link, target)?;
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = link;
            bail!("This host cannot copy symbolic links");
        }
    } else if metadata.is_dir() {
        fs::create_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_item(&entry.path(), &target.join(entry.file_name()))?;
        }
        fs::set_permissions(target, metadata.permissions())?;
    } else if metadata.is_file() {
        fs::copy(source, target).with_context(|| format!("Copy {}", source.display()))?;
    } else {
        bail!("Cannot copy this special file: {}", source.display());
    }
    Ok(())
}

/// Reserve the destination exclusively. Existing items are never silently overwritten.
fn publish(prepared: &Path, target: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(prepared)?;
    if metadata.is_dir() {
        // The prepared folder may carry read-only source permissions. Moving its children
        // requires write permission on this disposable staging folder, never on the source.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                prepared,
                fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
            )?;
        }
        fs::create_dir(target).context("Create destination folder")?;
        let result = (|| -> Result<()> {
            for entry in fs::read_dir(prepared)? {
                let entry = entry?;
                fs::rename(entry.path(), target.join(entry.file_name()))?;
            }
            fs::set_permissions(target, metadata.permissions())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(target);
        }
        result
    } else if metadata.is_symlink() {
        // symlink creation itself is exclusive, including dangling links.
        copy_item(prepared, target)
    } else {
        // Prefer the host's exclusive rename, which also works on filesystems without
        // hard links. Tempfile falls back to a hard link on older Unix hosts; if
        // neither is supported, reserve a new file before copying any bytes.
        let path = tempfile::TempPath::try_from_path(prepared)?;
        match path.persist_noclobber(target) {
            Ok(()) => {
                // Windows persistence normalizes attributes, including read-only.
                #[cfg(windows)]
                if let Err(error) = fs::set_permissions(target, metadata.permissions()) {
                    let _ = fs::remove_file(target);
                    return Err(error).context("Preserve destination permissions");
                }
                Ok(())
            }
            Err(prepared) => {
                let mut source = fs::File::open(&prepared.path)?;
                let mut destination = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .context("Create destination file")?;
                let result = (|| -> Result<()> {
                    std::io::copy(&mut source, &mut destination).context("Copy prepared file")?;
                    destination
                        .set_permissions(metadata.permissions())
                        .context("Preserve destination permissions")?;
                    Ok(())
                })();
                if result.is_err() {
                    drop(destination);
                    let _ = fs::remove_file(target);
                }
                result
            }
        }
    }
}

fn fingerprint(path: &Path) -> Result<Option<u64>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    metadata.len().hash(&mut hash);
    metadata.modified().ok().hash(&mut hash);
    metadata.permissions().readonly().hash(&mut hash);
    if metadata.is_symlink() {
        fs::read_link(path)?.hash(&mut hash);
    } else if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            entry.file_name().hash(&mut hash);
            fingerprint(&entry.path())?.hash(&mut hash);
        }
    } else if metadata.is_file() {
        let mut file = fs::File::open(path)?;
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            buffer[..count].hash(&mut hash);
        }
    } else {
        bail!("Destination is a special file: {}", path.display());
    }
    Ok(Some(hash.finish()))
}
