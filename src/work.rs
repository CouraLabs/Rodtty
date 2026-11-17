use crate::{
    files::{self, Document, Entry, SaveFailure, TextFile},
    git::{self, Comparison, Repository},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub explorer: bool,
    pub explorer_directory: Option<PathBuf>,
    pub follow: bool,
    pub hidden: bool,
    pub explorer_width: f32,
    pub editor_share: f32,
    pub terminal: bool,
    pub editor: bool,
    pub files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            explorer: false,
            explorer_directory: None,
            follow: false,
            hidden: false,
            explorer_width: 240.0,
            editor_share: 0.42,
            terminal: true,
            editor: true,
            files: Vec::new(),
            active_file: None,
        }
    }
}
impl Layout {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("workspace-state.json");
        if !path.exists()
            && !root.join(".storage-migrated-v1").exists()
            && root.join("workspace.toml").exists()
        {
            let layout: Self =
                toml::from_str(&std::fs::read_to_string(root.join("workspace.toml"))?)?;
            crate::config::write_json(&path, &layout, false)?;
        }
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut layout: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        layout.explorer_width = if layout.explorer_width.is_finite() {
            layout.explorer_width.clamp(160.0, 600.0)
        } else {
            240.0
        };
        layout.editor_share = if layout.editor_share.is_finite() {
            layout.editor_share.clamp(0.2, 0.8)
        } else {
            0.52
        };
        Ok(layout)
    }
    pub fn save(&self, root: &Path) -> anyhow::Result<()> {
        crate::config::write_json(&root.join("workspace-state.json"), self, true)
    }
}

pub enum Job {
    Reload {
        id: u64,
        revision: u64,
        path: PathBuf,
    },
    Open(PathBuf),
    Save {
        id: u64,
        document: Document,
        text: String,
        overwrite: bool,
    },
    Scan {
        token: u64,
        directory: Option<PathBuf>,
        hidden: bool,
        files: Vec<(u64, u64, PathBuf)>,
    },
    Diff {
        token: u64,
        path: PathBuf,
        text: String,
        mode: Comparison,
    },
    Layout {
        root: PathBuf,
        layout: Layout,
    },
}
pub enum Event {
    Reload {
        id: u64,
        revision: u64,
        result: Result<TextFile, String>,
    },
    Open {
        path: PathBuf,
        result: Result<Document, String>,
    },
    Save {
        id: u64,
        result: Result<Document, SaveFailure>,
    },
    Scan {
        token: u64,
        listing: Option<Result<(PathBuf, Vec<Entry>), String>>,
        git: Result<Option<Repository>, String>,
        files: Vec<(u64, u64, Result<TextFile, String>)>,
    },
    Diff {
        token: u64,
        result: Result<git::Diff, String>,
    },
    Error(String),
}
pub struct Worker {
    tx: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<Event>,
}
impl Worker {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, jobs) = mpsc::sync_channel(64);
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for job in jobs {
                let event = match job {
                    Job::Reload { id, revision, path } => Event::Reload {
                        id,
                        revision,
                        result: files::read_text(&path).map_err(|e| format!("{e:#}")),
                    },
                    Job::Open(path) => {
                        let result = Document::open(&path)
                            .or_else(|error| {
                                if !path.exists()
                                    && git::inspect(path.parent().unwrap_or(Path::new(".")))?
                                        .is_some_and(|repo| repo.files.contains_key(&path))
                                {
                                    Document::missing(&path)
                                } else {
                                    Err(error)
                                }
                            })
                            .map_err(|e| format!("{e:#}"));
                        Event::Open { path, result }
                    }
                    Job::Save {
                        id,
                        mut document,
                        text,
                        overwrite,
                    } => {
                        let result = document.save(&text, overwrite).map(|()| document);
                        Event::Save { id, result }
                    }
                    Job::Scan {
                        token,
                        directory,
                        hidden,
                        files,
                    } => {
                        let mut listing = directory.as_ref().map(|p| {
                            files::list_directory(p, hidden).map_err(|e| format!("{e:#}"))
                        });
                        let git = directory
                            .as_ref()
                            .map(|p| git::inspect(p).map_err(|e| format!("{e:#}")))
                            .unwrap_or(Ok(None));
                        if let (Some(Ok((directory, entries))), Ok(Some(repo))) =
                            (&mut listing, &git)
                        {
                            for (path, status) in &repo.files {
                                if status.code.contains('D')
                                    && path.parent() == Some(directory.as_path())
                                    && !path.exists()
                                {
                                    entries.push(Entry {
                                        path: path.clone(),
                                        name: path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned(),
                                        directory: false,
                                        missing: true,
                                    });
                                }
                            }
                        }
                        let files = files
                            .into_iter()
                            .map(|(id, baseline, path)| {
                                (
                                    id,
                                    baseline,
                                    files::read_text(&path).map_err(|e| format!("{e:#}")),
                                )
                            })
                            .collect();
                        Event::Scan {
                            token,
                            listing,
                            git,
                            files,
                        }
                    }
                    Job::Diff {
                        token,
                        path,
                        text,
                        mode,
                    } => Event::Diff {
                        token,
                        result: git::diff(&path, &text, mode).map_err(|e| format!("{e:#}")),
                    },
                    Job::Layout { root, layout } => {
                        if let Err(e) = layout.save(&root) {
                            let _ = events.send(Event::Error(format!("Save workspace: {e:#}")));
                            wake();
                        }
                        continue;
                    }
                };
                if events.send(event).is_err() {
                    break;
                }
                wake();
            }
        });
        Self { tx, rx }
    }
    pub fn send(&self, job: Job) -> Result<(), String> {
        self.tx
            .try_send(job)
            .map_err(|_| "File worker is busy; try again shortly.".into())
    }
    pub fn drain(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
}
