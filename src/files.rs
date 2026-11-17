use anyhow::{Context, Result, bail};
use similar::{ChangeTag, TextDiff};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ending {
    Lf,
    CrLf,
    Cr,
}
impl Ending {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
            Self::Cr => b"\r",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextFile {
    pub raw: Vec<u8>,
    pub text: String,
    endings: Vec<Ending>,
    pub preferred: Ending,
    pub bom: bool,
}

impl TextFile {
    pub fn decode(raw: Vec<u8>) -> Result<Self> {
        if raw.len() as u64 > MAX_FILE_BYTES {
            bail!("Files over 2 MiB are not supported by this editor yet.");
        }
        if raw.contains(&0) {
            bail!("Binary files cannot be opened as editable text.");
        }
        let bom = raw.starts_with(&[0xef, 0xbb, 0xbf]);
        let source = std::str::from_utf8(&raw[if bom { 3 } else { 0 }..])
            .context("This file is not UTF-8 text.")?;
        let mut text = String::with_capacity(source.len());
        let mut endings = Vec::new();
        let mut chars = source.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                endings.push(if chars.peek() == Some(&'\n') {
                    chars.next();
                    Ending::CrLf
                } else {
                    Ending::Cr
                });
                text.push('\n');
            } else {
                if c == '\n' {
                    endings.push(Ending::Lf);
                }
                text.push(c);
            }
        }
        let preferred = endings.first().copied().unwrap_or(Ending::Lf);
        Ok(Self {
            raw,
            text,
            endings,
            preferred,
            bom,
        })
    }

    pub fn encode(&self, text: &str) -> Vec<u8> {
        if text == self.text {
            return self.raw.clone();
        }
        let mut out = if self.bom {
            vec![0xef, 0xbb, 0xbf]
        } else {
            Vec::new()
        };
        // Keep the individual endings of unchanged lines, including mixed-EOL
        // files. Replacement lines keep the corresponding old line ending;
        // additional lines use the file's original style.
        let diff = TextDiff::configure()
            .timeout(Duration::from_secs(1))
            .diff_lines(self.text.as_str(), text);
        let mut replaced = std::collections::VecDeque::new();
        for change in diff.iter_all_changes() {
            if change.tag() == ChangeTag::Delete {
                replaced.push_back(
                    change
                        .old_index()
                        .and_then(|i| self.endings.get(i))
                        .copied(),
                );
                continue;
            }
            if change.tag() == ChangeTag::Equal {
                replaced.clear();
            }
            let value = change.value();
            if let Some(line) = value.strip_suffix('\n') {
                out.extend_from_slice(line.as_bytes());
                let ending = if change.tag() == ChangeTag::Equal {
                    change
                        .old_index()
                        .and_then(|i| self.endings.get(i))
                        .copied()
                        .unwrap_or(self.preferred)
                } else {
                    replaced.pop_front().flatten().unwrap_or(self.preferred)
                };
                out.extend_from_slice(ending.bytes());
            } else {
                out.extend_from_slice(value.as_bytes());
            }
        }
        out
    }
}

pub fn read_text(path: &Path) -> Result<TextFile> {
    let file = fs::File::open(path).with_context(|| format!("Open {}", path.display()))?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        bail!("Files over 2 MiB are not supported by this editor yet.");
    }
    let mut raw = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut raw)?;
    TextFile::decode(raw)
}

#[derive(Clone, Debug)]
pub struct Document {
    pub path: PathBuf,
    pub saved: TextFile,
    pub existed: bool,
}

#[derive(Debug)]
pub enum SaveFailure {
    Conflict,
    Io(String),
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("Open {}", path.display()))?;
        let saved = read_text(&path)?;
        Ok(Self {
            path,
            saved,
            existed: true,
        })
    }
    pub fn missing(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .context("file has no parent directory")?
            .canonicalize()?;
        let path = parent.join(path.file_name().context("missing file name")?);
        Ok(Self {
            path,
            saved: TextFile::decode(Vec::new())?,
            existed: false,
        })
    }
    pub fn dirty(&self, text: &str) -> bool {
        text != self.saved.text
    }

    pub fn save(&mut self, text: &str, overwrite: bool) -> std::result::Result<(), SaveFailure> {
        let mut committed = false;
        let result = (|| -> Result<()> {
            if fs::metadata(&self.path).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
                bail!("The disk file now exceeds the 2 MiB editor limit.");
            }
            let before = match fs::read(&self.path) {
                Ok(raw) => Some(raw),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if !overwrite && before.as_deref() != self.existed.then_some(self.saved.raw.as_slice())
            {
                return Ok(());
            }
            if self.existed
                && before.as_deref() == Some(self.saved.raw.as_slice())
                && text == self.saved.text
            {
                committed = true;
                return Ok(());
            }
            let bytes = self.saved.encode(text);
            if bytes.len() as u64 > MAX_FILE_BYTES {
                bail!("The edited file exceeds the 2 MiB editor limit.");
            }
            let parent = self.path.parent().context("file has no parent directory")?;
            let mut output = tempfile::NamedTempFile::new_in(parent)?;
            if let Ok(metadata) = fs::metadata(&self.path) {
                if metadata.permissions().readonly() {
                    bail!("This file is read-only. Change its permissions before saving.");
                }
                output.as_file().set_permissions(metadata.permissions())?;
            }
            output.write_all(&bytes)?;
            output.as_file().sync_all()?;
            // Detect a second writer that changed the file while it was being prepared.
            let now = match fs::read(&self.path) {
                Ok(raw) => Some(raw),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if now != before {
                return Ok(());
            }
            output.persist(&self.path)?;
            self.saved = TextFile::decode(bytes)?;
            self.existed = true;
            committed = true;
            Ok(())
        })();
        result.map_err(|e| SaveFailure::Io(format!("{e:#}")))?;
        if !committed {
            return Err(SaveFailure::Conflict);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub missing: bool,
}

pub fn list_directory(path: &Path, hidden: bool) -> Result<(PathBuf, Vec<Entry>)> {
    let root = path.canonicalize()?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        entries.push(Entry {
            directory: path.is_dir(),
            path,
            name,
            missing: false,
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(a.name.cmp(&b.name))
    });
    Ok((root, entries))
}

/// A host tree snapshot. Only expanded folders are read; individual folder errors
/// retain the rest of the tree so a disappearing child cannot blank its siblings.
#[derive(Debug)]
pub struct TreeListing {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub errors: Vec<(PathBuf, String)>,
}

pub fn list_tree(
    path: &Path,
    expanded: &[PathBuf],
    hidden: bool,
    ignored: &[PathBuf],
    reveal: Option<&Path>,
) -> Result<TreeListing> {
    let (root, entries) = list_directory(path, true)?;
    let mut listing = TreeListing {
        root,
        entries: Vec::new(),
        errors: Vec::new(),
    };
    let mut stack: Vec<_> = entries.into_iter().rev().collect();
    while let Some(entry) = stack.pop() {
        let revealed = reveal.is_some_and(|p| p.starts_with(&entry.path));
        if !revealed
            && ((!hidden && entry.name.starts_with('.'))
                || ignored.iter().any(|p| entry.path.starts_with(p)))
        {
            continue;
        }
        if entry.directory && expanded.contains(&entry.path) {
            match list_directory(&entry.path, true) {
                Ok((_, children)) => stack.extend(children.into_iter().rev().map(|mut child| {
                    // Preserve a symlink folder's position under the chosen root.
                    if let Some(name) = child.path.file_name() {
                        child.path = entry.path.join(name);
                    }
                    child
                })),
                Err(error) => listing
                    .errors
                    .push((entry.path.clone(), format!("{error:#}"))),
            }
        }
        listing.entries.push(entry);
    }
    Ok(listing)
}
