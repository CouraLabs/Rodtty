use crate::{
    files::{self, Document, Entry, SaveFailure, TextFile, TreeListing},
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
    pub explorer_revealed: bool,
    pub explorer_directory: Option<PathBuf>,
    pub expanded_folders: Vec<PathBuf>,
    #[serde(skip_serializing)]
    pub follow: bool,
    #[serde(skip_serializing)]
    pub hidden: bool,
    pub explorer_width: f32,
    pub editor_share: f32,
    pub terminal: bool,
    pub editor: bool,
    pub empty_editor: bool,
    pub files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    /// Ordered last directories, never commands, labels, or process identities.
    pub terminals: Vec<PathBuf>,
    pub active_terminal: usize,
    pub terminal_focus: bool,
    pub selected_path: Option<PathBuf>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            explorer: false,
            explorer_revealed: false,
            explorer_directory: None,
            expanded_folders: Vec::new(),
            follow: false,
            hidden: false,
            explorer_width: 240.0,
            editor_share: 0.42,
            terminal: true,
            editor: true,
            empty_editor: false,
            files: Vec::new(),
            active_file: None,
            terminals: Vec::new(),
            active_terminal: 0,
            terminal_focus: true,
            selected_path: None,
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
    PrepareRemoval {
        id: u64,
        path: PathBuf,
    },
    Remove {
        id: u64,
        plan: files::removal::RemovalPlan,
        permanent: bool,
    },
    FileOperation {
        id: u64,
        request: files::operations::OperationRequest,
        decision: Option<
            Box<(
                files::operations::Collision,
                files::operations::CollisionChoice,
            )>,
        >,
    },
    Reload {
        id: u64,
        revision: u64,
        path: PathBuf,
    },
    Open(PathBuf),
    Save {
        id: u64,
        token: u64,
        document: Document,
        text: String,
        overwrite: bool,
    },
    Scan {
        token: u64,
        directory: Option<PathBuf>,
        hidden: bool,
        expanded: Vec<PathBuf>,
        respect_ignore: bool,
        reveal: Option<PathBuf>,
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
    PrepareRemoval {
        id: u64,
        result: Result<files::removal::RemovalPlan, String>,
    },
    Remove {
        id: u64,
        result: Result<files::removal::RemovalOutcome, String>,
    },
    FileOperation {
        id: u64,
        result: Result<files::operations::OperationOutcome, String>,
    },
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
        token: u64,
        result: Result<Document, SaveFailure>,
    },
    Scan {
        token: u64,
        listing: Option<Result<TreeListing, String>>,
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
                    Job::PrepareRemoval { id, path } => Event::PrepareRemoval {
                        id,
                        result: files::removal::RemovalPlan::prepare(path)
                            .map_err(|e| format!("{e:#}")),
                    },
                    Job::Remove {
                        id,
                        plan,
                        permanent,
                    } => Event::Remove {
                        id,
                        result: (if permanent {
                            plan.permanently_delete()
                        } else {
                            plan.trash()
                        })
                        .map_err(|e| format!("{e:#}")),
                    },
                    Job::FileOperation {
                        id,
                        request,
                        decision,
                    } => Event::FileOperation {
                        id,
                        result: request
                            .execute(decision.map(|d| *d))
                            .map_err(|e| format!("{e:#}")),
                    },
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
                        token,
                        mut document,
                        text,
                        overwrite,
                    } => {
                        let result = document.save(&text, overwrite).map(|()| document);
                        Event::Save { id, token, result }
                    }
                    Job::Scan {
                        token,
                        directory,
                        hidden,
                        expanded,
                        respect_ignore,
                        reveal,
                        files,
                    } => {
                        let mut git = directory
                            .as_ref()
                            .map(|p| git::inspect(p).map_err(|e| format!("{e:#}")))
                            .unwrap_or(Ok(None));
                        let ignored = if respect_ignore {
                            match &git {
                                Ok(Some(repo)) => match git::ignored(repo) {
                                    Ok(paths) => paths,
                                    Err(error) => {
                                        git = Err(format!("Ignore rules: {error:#}"));
                                        Vec::new()
                                    }
                                },
                                _ => Vec::new(),
                            }
                        } else {
                            Vec::new()
                        };
                        let mut listing = directory.as_ref().map(|p| {
                            files::list_tree(p, &expanded, hidden, &ignored, reveal.as_deref())
                                .map_err(|e| format!("{e:#}"))
                        });
                        if let (Some(Ok(listing)), Ok(Some(repo))) = (&mut listing, &git) {
                            for (path, status) in &repo.files {
                                if status.code.contains('D')
                                    && (hidden
                                        || !path.file_name().is_some_and(|name| {
                                            name.to_string_lossy().starts_with('.')
                                        }))
                                    && path.parent().is_some_and(|parent| {
                                        parent == listing.root
                                            || (expanded.iter().any(|p| p == parent)
                                                && listing.entries.iter().any(|entry| {
                                                    entry.directory && entry.path == parent
                                                }))
                                    })
                                    && !path.exists()
                                {
                                    listing.entries.push(Entry {
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
