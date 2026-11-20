//! Recovery drafts for unsaved editor buffers: `<app root>/drafts/`, one file
//! per buffer, replaced atomically. A draft holds the buffer's text and the
//! saved version it was edited from (its exact bytes, so encoding, BOM and
//! line endings come back unchanged and a disk change is detected as a
//! conflict). Drafts stay on this machine and out of diagnostics.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    version: u32,
    path: PathBuf,
    existed: bool,
    saved_at_ms: u64,
    saved_len: usize,
    text_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    /// The file the buffer edits.
    pub path: PathBuf,
    /// Whether the file existed when the buffer opened.
    pub existed: bool,
    /// The bytes the buffer was opened or last saved from.
    pub saved: Vec<u8>,
    pub text: String,
    /// When the snapshot was taken, in milliseconds since the Unix epoch.
    pub saved_at_ms: u64,
}

/// What the disk holds now, against the draft's saved version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskState {
    /// The file still has the bytes the draft was edited from.
    Unchanged,
    /// The file was changed by something else since.
    Changed,
    /// The file is gone (deleted or moved by something else).
    Missing,
}

pub fn directory(app_root: &Path) -> PathBuf {
    app_root.join("drafts")
}

/// A stable file name for `path`'s draft (FNV-1a of the path).
pub fn key(path: &Path) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.draft")
}

fn encode(draft: &Draft) -> Result<Vec<u8>> {
    let header = Header {
        version: VERSION,
        path: draft.path.clone(),
        existed: draft.existed,
        saved_at_ms: draft.saved_at_ms,
        saved_len: draft.saved.len(),
        text_len: draft.text.len(),
    };
    let mut out = serde_json::to_vec(&header)?;
    out.push(b'\n');
    out.extend_from_slice(&draft.saved);
    out.extend_from_slice(draft.text.as_bytes());
    Ok(out)
}

fn decode(bytes: &[u8]) -> Result<Draft> {
    let newline = bytes
        .iter()
        .position(|b| *b == b'\n')
        .context("no header")?;
    let header: Header = serde_json::from_slice(&bytes[..newline]).context("unreadable header")?;
    if header.version != VERSION {
        bail!("draft version {} is not supported", header.version);
    }
    let body = &bytes[newline + 1..];
    if body.len() != header.saved_len + header.text_len {
        bail!("the draft is truncated");
    }
    let (saved, text) = body.split_at(header.saved_len);
    Ok(Draft {
        path: header.path,
        existed: header.existed,
        saved: saved.to_vec(),
        text: String::from_utf8(text.to_vec()).context("the draft text is not UTF-8")?,
        saved_at_ms: header.saved_at_ms,
    })
}

/// Write `draft`, replacing any earlier one for its file atomically.
pub fn write(app_root: &Path, draft: &Draft) -> Result<()> {
    let dir = directory(app_root);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    file.write_all(&encode(draft)?)?;
    file.as_file().sync_all()?;
    file.persist(dir.join(key(&draft.path)))?;
    Ok(())
}

/// Remove `path`'s draft, if any.
pub fn remove(app_root: &Path, path: &Path) {
    let _ = std::fs::remove_file(directory(app_root).join(key(path)));
}

/// Every pending draft, and the draft files that couldn't be read (with the
/// reason). One bad file never hides the others.
pub fn load(app_root: &Path) -> (Vec<Draft>, Vec<(PathBuf, String)>) {
    let mut drafts = Vec::new();
    let mut bad = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory(app_root)) else {
        return (drafts, bad);
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "draft"))
        .collect();
    paths.sort();
    for file in paths {
        match std::fs::read(&file)
            .map_err(anyhow::Error::from)
            .and_then(|b| decode(&b))
        {
            Ok(draft) => drafts.push(draft),
            Err(e) => bad.push((file, format!("{e:#}"))),
        }
    }
    drafts.sort_by_key(|d| std::cmp::Reverse(d.saved_at_ms));
    (drafts, bad)
}

impl Draft {
    pub fn disk_state(&self) -> DiskState {
        match std::fs::read(&self.path) {
            Ok(bytes) if self.existed && bytes == self.saved => DiskState::Unchanged,
            Ok(_) => DiskState::Changed,
            Err(_) if !self.existed => DiskState::Unchanged,
            Err(_) => DiskState::Missing,
        }
    }
}

/// A sibling path for saving a recovered draft as a copy: `name (recovered).ext`,
/// numbered when that exists. Never an existing file.
pub fn copy_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    (1..)
        .map(|n| {
            let suffix = if n == 1 {
                " (recovered)".to_owned()
            } else {
                format!(" (recovered {n})")
            };
            parent.join(format!("{stem}{suffix}{ext}"))
        })
        .find(|p| !p.exists())
        .expect("an unused name")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_round_trip_and_a_corrupt_one_leaves_the_others() {
        let root = crate::test_support::tempdir().unwrap();
        let file = root.path().join("a.txt");
        std::fs::write(&file, b"\xef\xbb\xbfone\r\n").unwrap();
        let draft = Draft {
            path: file.clone(),
            existed: true,
            saved: std::fs::read(&file).unwrap(),
            text: "one\ntwo ✓\n".into(),
            saved_at_ms: now_ms(),
        };
        write(root.path(), &draft).unwrap();
        let other = Draft {
            path: root.path().join("b.txt"),
            existed: false,
            saved: Vec::new(),
            text: "new".into(),
            saved_at_ms: 1,
        };
        write(root.path(), &other).unwrap();
        std::fs::write(
            directory(root.path()).join("zzzz.draft"),
            b"{\"version\":1}\nxx",
        )
        .unwrap();
        let (drafts, bad) = load(root.path());
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0], draft);
        assert_eq!(bad.len(), 1);
        assert_eq!(drafts[0].disk_state(), DiskState::Unchanged);
        assert_eq!(
            drafts[1].disk_state(),
            DiskState::Unchanged,
            "a new file not yet on disk"
        );
        std::fs::write(&file, "changed").unwrap();
        assert_eq!(drafts[0].disk_state(), DiskState::Changed);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(drafts[0].disk_state(), DiskState::Missing);
        remove(root.path(), &file);
        assert_eq!(load(root.path()).0.len(), 1);
        assert_eq!(
            copy_path(&root.path().join("x.rs")),
            root.path().join("x (recovered).rs")
        );
    }
}
