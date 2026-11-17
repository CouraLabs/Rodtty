//! Read-only host-Git inspection. Arguments are passed directly, never through a shell.
use crate::files::{MAX_FILE_BYTES, TextFile};
use anyhow::{Context, Result, bail};
use similar::TextDiff;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Current,
    Unstaged,
    Staged,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub code: String,
    pub original: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, Status>,
}
impl Repository {
    pub fn untracked(&self, path: &Path) -> bool {
        self.files
            .iter()
            .any(|(p, s)| s.code == "??" && path.starts_with(p))
    }
    pub fn badge(&self, path: &Path) -> String {
        if self.untracked(path) {
            return "??".into();
        }
        self.files
            .get(path)
            .map(|s| s.code.trim().to_owned())
            .or_else(|| {
                self.files
                    .keys()
                    .any(|p| p.starts_with(path))
                    .then(|| "M".into())
            })
            .unwrap_or_default()
    }
}

fn command(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-C",
        ])
        .arg(directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    command
}

fn run(directory: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<Output> {
    let mut child = command(directory)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Git is unavailable. Install Git and make it available on the host PATH.")?;
    let stdout = child.stdout.take().context("Git stdout unavailable")?;
    let stderr = child.stderr.take().context("Git stderr unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut raw = Vec::new();
        stdout
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut raw)
            .map(|_| raw)
    });
    let errors = std::thread::spawn(move || {
        let mut raw = Vec::new();
        stderr.take(64 * 1024).read_to_end(&mut raw).map(|_| raw)
    });
    let status = match child.wait_timeout(Duration::from_secs(5))? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Git inspection timed out.");
        }
    };
    let stdout = reader
        .join()
        .map_err(|_| anyhow::anyhow!("Git reader stopped"))??;
    let stderr = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Git error reader stopped"))??;
    if stdout.len() as u64 > MAX_FILE_BYTES {
        bail!("Git result exceeds the 2 MiB editor limit.");
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn bytes_path(raw: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(raw.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(raw).as_ref())
    }
}

pub fn inspect(directory: &Path) -> Result<Option<Repository>> {
    let root = run(directory, ["rev-parse", "--show-toplevel"])?;
    if !root.status.success() {
        let error = String::from_utf8_lossy(&root.stderr);
        if error.contains("not a git repository") {
            return Ok(None);
        }
        bail!("{}", error.trim());
    }
    let mut raw = root.stdout.as_slice();
    if raw.ends_with(b"\n") {
        raw = &raw[..raw.len() - 1];
    }
    if raw.ends_with(b"\r") {
        raw = &raw[..raw.len() - 1];
    }
    let root = bytes_path(raw).canonicalize()?;
    let output = run(
        &root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let mut records = output.stdout.split(|b| *b == 0);
    let mut files = BTreeMap::new();
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let code = String::from_utf8_lossy(&record[..2]).into_owned();
        let original = if code.contains(['R', 'C']) {
            records.next().map(|raw| root.join(bytes_path(raw)))
        } else {
            None
        };
        files.insert(
            root.join(bytes_path(&record[3..])),
            Status { code, original },
        );
    }
    Ok(Some(Repository { root, files }))
}

/// Git's own exclude engine honors nested .gitignore, info/exclude, global
/// excludes and negations, including repositories opened through linked worktrees.
pub fn ignored(repo: &Repository) -> Result<Vec<PathBuf>> {
    let output = run(
        &repo.root,
        [
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--full-name",
            "-z",
        ],
    )?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| repo.root.join(bytes_path(p)))
        .collect())
}

fn blob(repo: &Repository, path: &Path, revision: &str) -> Result<String> {
    let relative = path
        .strip_prefix(&repo.root)
        .context("file is outside repository")?;
    let mut spec = if revision.is_empty() {
        OsString::from(":0:")
    } else {
        let mut value = OsString::from(revision);
        value.push(":");
        value
    };
    spec.push(relative);
    let output = run(
        &repo.root,
        [
            OsString::from("show"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            spec,
        ],
    )?;
    if !output.status.success() {
        // A missing entry (new file, deleted index entry, or unborn HEAD) is empty.
        // Verify through the object tree instead of treating every Git failure as empty.
        if revision.is_empty() {
            let check = run(
                &repo.root,
                [
                    OsString::from("ls-files"),
                    OsString::from("--stage"),
                    OsString::from("-z"),
                    OsString::from("--"),
                    relative.as_os_str().to_owned(),
                ],
            )?;
            if check.status.success() && check.stdout.is_empty() {
                return Ok(String::new());
            }
        } else {
            let head = run(&repo.root, ["rev-parse", "--verify", "HEAD"])?;
            if !head.status.success() {
                return Ok(String::new());
            }
            let check = run(
                &repo.root,
                [
                    OsString::from("ls-tree"),
                    OsString::from("-z"),
                    OsString::from("HEAD"),
                    OsString::from("--"),
                    relative.as_os_str().to_owned(),
                ],
            )?;
            if check.status.success() && check.stdout.is_empty() {
                return Ok(String::new());
            }
        }
        bail!(
            "Cannot read Git baseline: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(TextFile::decode(output.stdout)?.text)
}

#[derive(Debug)]
pub struct Diff {
    pub patch: String,
    pub label: String,
    pub untracked: bool,
    pub changed: bool,
}

pub fn diff(path: &Path, buffer: &str, comparison: Comparison) -> Result<Diff> {
    let normalized = if path.exists() {
        path.canonicalize()?
    } else {
        path.parent()
            .context("file has no parent")?
            .canonicalize()?
            .join(path.file_name().context("file has no name")?)
    };
    let path = normalized.as_path();
    let repo = inspect(path.parent().context("file has no parent")?)?
        .context("This file is outside a Git working tree.")?;
    let relative = path
        .strip_prefix(&repo.root)?
        .to_string_lossy()
        .replace('\\', "/");
    let status = repo.files.get(path);
    let untracked = repo.untracked(path);
    let head_path = status.and_then(|s| s.original.as_deref()).unwrap_or(path);
    let (old, new, label) = match comparison {
        Comparison::Current => (
            blob(&repo, head_path, "HEAD")?,
            buffer.to_owned(),
            "Editor ↔ HEAD · includes unsaved edits",
        ),
        Comparison::Unstaged => (
            blob(&repo, path, "")?,
            match crate::files::read_text(path) {
                Ok(file) => file.text,
                Err(_) if !path.exists() => String::new(),
                Err(e) => return Err(e),
            },
            "Disk ↔ Index · unstaged changes",
        ),
        Comparison::Staged => (
            blob(&repo, head_path, "HEAD")?,
            blob(&repo, path, "")?,
            "Index ↔ HEAD · staged changes",
        ),
    };
    let changed = old != new;
    let old_name = if untracked && old.is_empty() {
        "/dev/null".to_owned()
    } else {
        format!("a/{relative}")
    };
    let patch = if changed {
        TextDiff::configure()
            .timeout(Duration::from_secs(1))
            .diff_lines(&old, &new)
            .unified_diff()
            .context_radius(3)
            .header(&old_name, &format!("b/{relative}"))
            .to_string()
    } else {
        String::new()
    };
    Ok(Diff {
        patch,
        label: label.into(),
        untracked,
        changed,
    })
}
