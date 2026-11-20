//! Read-only host-Git inspection. Arguments are passed directly, never through a shell.
use crate::files::{MAX_FILE_BYTES, TextFile};
use crate::paths::Canonical as _;
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
/// How a file differs from HEAD, as the explorer marks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
    Deleted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, Status>,
    /// Lines added and removed between HEAD and the working tree, per tracked
    /// file. Empty before the first commit; binary files are left out.
    pub lines: BTreeMap<PathBuf, (u32, u32)>,
}
impl Repository {
    /// The file's change against HEAD, or `None` when it is unchanged.
    pub fn change(&self, path: &Path) -> Option<Change> {
        if self.untracked(path) {
            return Some(Change::Added);
        }
        let code = &self.files.get(path)?.code;
        Some(if code.contains('D') {
            Change::Deleted
        } else if code.starts_with('A') {
            Change::Added
        } else {
            Change::Modified
        })
    }
    /// Lines added and removed in a changed tracked file.
    pub fn line_changes(&self, path: &Path) -> Option<(u32, u32)> {
        self.lines.get(path).copied()
    }
    /// Whether the comparison would show a change for this file. `Current`
    /// compares the editor with HEAD, so it also applies to unsaved edits,
    /// which the caller knows about.
    pub fn compares(&self, path: &Path, comparison: Comparison) -> bool {
        if self.untracked(path) {
            return comparison != Comparison::Staged;
        }
        let Some(status) = self.files.get(path) else {
            return false;
        };
        let mut code = status.code.chars();
        let (index, worktree) = (code.next().unwrap_or(' '), code.next().unwrap_or(' '));
        match comparison {
            Comparison::Current => true,
            Comparison::Unstaged => worktree != ' ',
            Comparison::Staged => index != ' ' && index != '?',
        }
    }
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
    // A GUI process has no console, so Windows would open one for each git call.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
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
    let root = bytes_path(raw).canonical()?;
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
    let lines = if files.is_empty() {
        BTreeMap::new()
    } else {
        line_changes(&root)?
    };
    Ok(Some(Repository { root, files, lines }))
}

/// Lines added and removed between HEAD and the working tree. A repository
/// without commits has no HEAD to count from, so it reports none.
fn line_changes(root: &Path) -> Result<BTreeMap<PathBuf, (u32, u32)>> {
    let output = run(
        root,
        [
            "diff",
            "HEAD",
            "--numstat",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--",
        ],
    )?;
    let mut lines = BTreeMap::new();
    if !output.status.success() {
        let head = run(root, ["rev-parse", "--verify", "--quiet", "HEAD"])?;
        if head.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        return Ok(lines);
    }
    for record in output.stdout.split(|b| *b == 0) {
        let mut fields = record.splitn(3, |b| *b == b'\t');
        let (Some(added), Some(removed), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // Binary files report `-` for both counts.
        let count = |raw: &[u8]| std::str::from_utf8(raw).ok()?.parse::<u32>().ok();
        if let (Some(added), Some(removed)) = (count(added), count(removed)) {
            lines.insert(root.join(bytes_path(path)), (added, removed));
        }
    }
    Ok(lines)
}

/// One changed file in one group of [`Changes`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ChangedFile {
    pub path: PathBuf,
    /// The path under the repository root, `/`-separated.
    pub relative: String,
    /// The file name, with enough of its folder to tell it apart from
    /// another changed file of the same name.
    pub label: String,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `C` copied, `T`
    /// type changed, `U` unmerged, `?` untracked.
    pub kind: char,
    /// A rename's or copy's original path.
    pub original: Option<PathBuf>,
}

/// A repository's changes, grouped as `git status` groups them. A file
/// staged and then edited again appears in both `staged` and `unstaged`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Changes {
    pub root: PathBuf,
    /// Whether `root` is a linked worktree rather than the main one.
    pub linked_worktree: bool,
    /// Whether HEAD names a commit yet.
    pub has_commits: bool,
    pub staged: Vec<ChangedFile>,
    pub unstaged: Vec<ChangedFile>,
    pub untracked: Vec<ChangedFile>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty()
    }
    /// Every row in display order: staged, then unstaged, then untracked.
    pub fn rows(&self) -> impl Iterator<Item = (Comparison, &ChangedFile)> {
        self.staged
            .iter()
            .map(|f| (Comparison::Staged, f))
            .chain(self.unstaged.iter().map(|f| (Comparison::Unstaged, f)))
            .chain(self.untracked.iter().map(|f| (Comparison::Unstaged, f)))
    }
}

/// The changes in the repository or worktree containing `directory`, or
/// `None` outside one. Read-only: never touches the index or working tree.
pub fn changes(directory: &Path) -> Result<Option<Changes>> {
    let Some(repo) = inspect(directory)? else {
        return Ok(None);
    };
    let relative = |path: &Path| {
        path.strip_prefix(&repo.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    for (path, status) in &repo.files {
        let mut code = status.code.chars();
        let (index, worktree) = (code.next().unwrap_or(' '), code.next().unwrap_or(' '));
        let file = |kind: char| ChangedFile {
            path: path.clone(),
            relative: relative(path),
            label: String::new(),
            kind,
            original: status.original.clone(),
        };
        if status.code == "??" {
            untracked.push(file('?'));
            continue;
        }
        if index == 'U' || worktree == 'U' || (index == 'A' && worktree == 'A') || (index == 'D' && worktree == 'D') {
            unstaged.push(file('U'));
            continue;
        }
        if index != ' ' {
            staged.push(file(index));
        }
        if worktree != ' ' {
            unstaged.push(ChangedFile { original: None, ..file(worktree) });
        }
    }
    // Label each file by its name, adding parent folders until no two
    // different paths share a label.
    let all: Vec<String> = staged
        .iter()
        .chain(&unstaged)
        .chain(&untracked)
        .map(|f| f.relative.clone())
        .collect();
    let label = |relative: &str| {
        let parts: Vec<&str> = relative.split('/').collect();
        for take in 1..=parts.len() {
            let tail = parts[parts.len() - take..].join("/");
            let clash = all.iter().any(|other| {
                other != relative && (other == &tail || other.ends_with(&format!("/{tail}")))
            });
            if !clash {
                return tail;
            }
        }
        relative.to_owned()
    };
    for file in staged.iter_mut().chain(unstaged.iter_mut()).chain(untracked.iter_mut()) {
        file.label = label(&file.relative);
    }
    let linked_worktree = repo.root.join(".git").is_file();
    let has_commits = run(&repo.root, ["rev-parse", "--verify", "--quiet", "HEAD"])?.status.success();
    Ok(Some(Changes {
        root: repo.root,
        linked_worktree,
        has_commits,
        staged,
        unstaged,
        untracked,
    }))
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
        path.canonical()?
    } else {
        path.parent()
            .context("file has no parent")?
            .canonical()?
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
