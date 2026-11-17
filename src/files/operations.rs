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
    Move { source: PathBuf },
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
    source_version: Option<u64>,
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
        let mut case_alias = false;
        if let FileOperation::Move { source } = &self.operation {
            let original = source
                .parent()
                .context("Source has no parent")?
                .canonicalize()?
                .join(source.file_name().context("Source has no name")?);
            if original == target {
                fs::symlink_metadata(source).context("Read source")?;
                return Ok(OperationOutcome::Completed {
                    request: self,
                    path: target,
                });
            }
            case_alias = same_entry_case_alias(&original, &target)?;
        }
        let source_version = match &self.operation {
            FileOperation::Move { source } => fingerprint(source)?,
            _ => None,
        };
        let directory = match &self.operation {
            FileOperation::CreateFolder => true,
            FileOperation::CreateFile => false,
            FileOperation::Copy { source } | FileOperation::Move { source } => {
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
        // A proven case alias names the source entry itself. Quarantine removes
        // that alias before no-replace publication creates the requested spelling.
        let existing = if case_alias {
            None
        } else {
            fingerprint(&target)?
        };
        let mut replace = false;
        if let Some(version) = existing {
            match decision {
                Some((collision, choice))
                    if collision.request.target == self.target
                        && collision.version == version
                        && collision.source_version == source_version =>
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
                        source_version,
                    }));
                }
            }
        }
        if replace
            && let FileOperation::Copy { source } | FileOperation::Move { source } = &self.operation
        {
            let target_metadata = fs::symlink_metadata(&target)?;
            if target_metadata.is_dir() && source.canonicalize()?.starts_with(&target) {
                bail!("A copy cannot replace a folder containing its source. Choose Keep Both.");
            }
        }
        // Prefer the host's exclusive rename. This preserves object identity and
        // handles read-only source folders without needing to alter their mode.
        // Filesystems without this primitive retain the staged-copy fallback.
        if !replace
            && !case_alias
            && let FileOperation::Move { source } = &self.operation
        {
            if fingerprint(source)? != source_version {
                bail!("Source changed while preparing the move. Try again.");
            }
            if rename_exclusive(source, &target)? {
                return Ok(OperationOutcome::Completed {
                    request: self,
                    path: target,
                });
            }
        }
        // Finish the potentially expensive/fallible copy before touching a destination.
        let stage = OperationStage(Some(
            tempfile::Builder::new()
                .prefix(".rodtty-operation-")
                .tempdir_in(&parent)?,
        ));
        let prepared = stage.path().join("item");
        match &self.operation {
            FileOperation::CreateFile => {
                fs::File::create(&prepared)?;
            }
            FileOperation::CreateFolder => fs::create_dir(&prepared)?,
            FileOperation::Copy { source } | FileOperation::Move { source } => {
                copy_item(source, &prepared)?
            }
        }
        // Quarantine a move's original only after preparation succeeds. This rename
        // stays on the source filesystem; publication can therefore cross volumes.
        // The guard restores the original on every failure before publication.
        let mut moved = if let FileOperation::Move { source } = &self.operation {
            if fingerprint(source)? != source_version {
                bail!("Source changed while preparing the move. Try again.");
            }
            Some(MoveSource::take(source, source_version)?)
        } else {
            None
        };
        let publication = (|| -> Result<()> {
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
                    if fs::symlink_metadata(&target).is_ok()
                        || fs::rename(&backup, &target).is_err()
                    {
                        let recovery = stage.keep();
                        bail!(
                            "{error:#}. Previous destination preserved at {}",
                            recovery.join("previous").display()
                        );
                    }
                }
                return Err(error);
            }
            Ok(())
        })();
        if let Err(error) = publication {
            if let Some(moved) = &mut moved {
                moved.restore().with_context(|| format!("{error:#}"))?;
            }
            return Err(error);
        }
        if let Some(moved) = &mut moved {
            moved.committed = true;
        }
        Ok(OperationOutcome::Completed {
            request: self,
            path: target,
        })
    }
}

/// Prove a case-only destination is an alias for the selected directory entry,
/// rather than a distinct hardlink or another case-sensitive entry. Never infer
/// case sensitivity from the operating system; volumes can differ on one host.
fn same_entry_case_alias(source: &Path, target: &Path) -> Result<bool> {
    if source.parent() != target.parent() {
        return Ok(false);
    }
    let Some((source_name, target_name)) = source
        .file_name()
        .and_then(|n| n.to_str())
        .zip(target.file_name().and_then(|n| n.to_str()))
    else {
        return Ok(false);
    };
    if source_name == target_name || !source_name.eq_ignore_ascii_case(target_name) {
        return Ok(false);
    }
    let target_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let source_metadata = fs::symlink_metadata(source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if source_metadata.dev() != target_metadata.dev()
            || source_metadata.ino() != target_metadata.ino()
        {
            return Ok(false);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (source_metadata, target_metadata);
        if source.canonicalize()? != target.canonicalize()? {
            return Ok(false);
        }
    }
    let mut exact_source = false;
    let mut equivalents = 0;
    for entry in fs::read_dir(source.parent().context("Source has no parent")?)? {
        let name = entry?.file_name();
        if name == target_name {
            return Ok(false);
        }
        if let Some(name) = name.to_str()
            && name.eq_ignore_ascii_case(source_name)
        {
            exact_source |= name == source_name;
            equivalents += 1;
        }
    }
    Ok(exact_source && equivalents == 1)
}

/// Return false when the host/filesystem needs the staged move fallback.
fn rename_exclusive(source: &Path, target: &Path) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        use nix::libc;
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let source = CString::new(source.as_os_str().as_bytes())?;
        let target = CString::new(target.as_os_str().as_bytes())?;
        // Both C strings stay alive for the syscall; no pointer escapes it.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP | libc::EXDEV)
        ) {
            return Ok(false);
        }
        Err(error).context("Move source without replacing destination")
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (source, target);
        Ok(false)
    }
}

/// A quarantined source remains recoverable until publication succeeds.
struct MoveSource {
    source: PathBuf,
    stage: Option<OperationStage>,
    committed: bool,
}
impl MoveSource {
    fn take(source: &Path, version: Option<u64>) -> Result<Self> {
        let stage = OperationStage(Some(
            tempfile::Builder::new()
                .prefix(".rodtty-move-")
                .tempdir_in(source.parent().context("Source has no parent")?)?,
        ));
        fs::rename(source, stage.path().join("item")).context("Preserve move source")?;
        let mut moved = Self {
            source: source.to_owned(),
            stage: Some(stage),
            committed: false,
        };
        let checked = fingerprint(&moved.path());
        if !matches!(&checked, Ok(current) if *current == version) {
            moved
                .restore()
                .context("Source changed while preparing the move")?;
            checked?;
            bail!("Source changed while preparing the move. Try again.");
        }
        Ok(moved)
    }
    fn path(&self) -> PathBuf {
        self.stage.as_ref().unwrap().path().join("item")
    }
}
impl MoveSource {
    fn restore(&mut self) -> Result<()> {
        let path = self.path();
        let restored = (|| -> Result<()> {
            if !rename_exclusive(&path, &self.source)? {
                publish(&path, &self.source)?;
            }
            Ok(())
        })();
        self.committed = true;
        if let Err(error) = restored {
            let recovery = self.stage.take().unwrap().keep();
            bail!(
                "Could not restore move source: {error:#}. Original preserved at {}",
                recovery.join("item").display()
            );
        }
        Ok(())
    }
}
impl Drop for MoveSource {
    fn drop(&mut self) {
        if !self.committed
            && let Err(error) = self.restore()
        {
            eprintln!("{error:#}");
        }
    }
}

/// Owns only disposable prepared items and previous destinations. Published items
/// have left this directory, and a failed rollback explicitly keeps it for recovery.
struct OperationStage(Option<tempfile::TempDir>);

impl OperationStage {
    fn path(&self) -> &Path {
        self.0.as_ref().unwrap().path()
    }

    fn keep(mut self) -> PathBuf {
        self.0.take().unwrap().keep()
    }
}

impl Drop for OperationStage {
    fn drop(&mut self) {
        if let Some(stage) = &self.0 {
            // TempDir's recursive removal cannot unlink children of read-only
            // directories copied from the source or moved here as a backup.
            let _ = make_disposable_removable(stage.path());
        }
    }
}

fn make_disposable_removable(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(());
    }
    #[cfg(unix)]
    if metadata.is_dir() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
        )?;
    }
    #[cfg(windows)]
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        // Windows has a read-only attribute; this block never changes Unix modes.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    if metadata.is_dir() {
        // Continue restoring siblings if an entry cannot be accessed. TempDir
        // still attempts removal, without touching anything outside its root.
        for entry in fs::read_dir(path)?.flatten() {
            let _ = make_disposable_removable(&entry.path());
        }
    }
    Ok(())
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
                let prepared_child = entry.path();
                let published_child = target.join(entry.file_name());
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    let metadata = fs::symlink_metadata(&prepared_child)?;
                    if metadata.is_dir() {
                        // A cross-parent directory rename needs access to update its
                        // parent entry. Change only this private staged copy, then
                        // restore its original permissions after moving it.
                        let permissions = metadata.permissions();
                        fs::set_permissions(
                            &prepared_child,
                            fs::Permissions::from_mode(permissions.mode() | 0o300),
                        )?;
                        Some(permissions)
                    } else {
                        None
                    }
                };
                fs::rename(&prepared_child, &published_child)?;
                #[cfg(unix)]
                if let Some(permissions) = permissions {
                    fs::set_permissions(&published_child, permissions)?;
                }
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode().hash(&mut hash);
    }
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
