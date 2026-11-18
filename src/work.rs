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
    /// Where the window sat when it was last saved, so relaunching reopens it
    /// there instead of at the host's default placement. `None` before the
    /// first save on a build that records it.
    pub window: Option<WindowGeometry>,
}

/// A window's place on the desktop, saved so a relaunch reopens it there.
///
/// Every field is physical pixels, as the host reports them: `x`/`y` is the
/// frame's top-left and `width`/`height` the content's size, matching
/// [`rune::WindowOptions::position`] and `physical_size`. Points would move the
/// window when the display's scale or the app's zoom changed between runs.
///
/// `display` records the bounds of the display the window was on. A monitor
/// that has since been unplugged, rearranged or rescaled leaves the saved
/// origin outside every display, so the restore drops it and lets the host
/// place the window instead of reopening it off-screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// The display's origin and size, physical pixels, at save time.
    pub display: Option<((i32, i32), (u32, u32))>,
}

impl WindowGeometry {
    /// A saved geometry, if it is small enough to be a real window and big
    /// enough to stay usable. A corrupt or truncated record is dropped rather
    /// than reopening the window at an unusable size.
    pub fn sanitized(self) -> Option<Self> {
        let (min_w, min_h) = (640u32, 360u32);
        (self.width >= min_w
            && self.height >= min_h
            && self.width <= 16_384
            && self.height <= 16_384)
            .then_some(self)
    }

    /// Whether the saved origin still lands on the display it was saved on.
    /// The window's top-left is enough: a window wider than its display still
    /// opens where the owner left it.
    pub fn origin_on_display(self) -> bool {
        let Some(((dx, dy), (dw, dh))) = self.display else {
            return true;
        };
        let (dw, dh) = (dw as i64, dh as i64);
        let (x, y) = (self.x as i64, self.y as i64);
        let (dx, dy) = (dx as i64, dy as i64);
        x >= dx && y >= dy && x < dx + dw && y < dy + dh
    }
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
            window: None,
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
                                        size: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_placement_is_dropped_when_it_no_longer_lands_on_its_display() {
        // On its display: reopened where the owner left it.
        let on = WindowGeometry {
            x: 100,
            y: 100,
            width: 1200,
            height: 800,
            display: Some(((0, 0), (2560, 1440))),
        };
        assert!(on.origin_on_display());
        assert_eq!(on.sanitized(), Some(on));

        // A monitor that has been unplugged or rearranged: the saved origin is
        // outside every current display, so the host places the window instead.
        let off = WindowGeometry {
            x: 3000,
            y: 100,
            ..on
        };
        assert!(!off.origin_on_display());
    }

    #[test]
    fn a_negative_origin_still_counts_as_on_display() {
        // A display to the left of the primary one has a negative origin, and a
        // window there is legitimate.
        let left = WindowGeometry {
            x: -1500,
            y: 40,
            width: 1000,
            height: 700,
            display: Some(((-1920, 0), (1920, 1080))),
        };
        assert!(left.origin_on_display());
    }

    #[test]
    fn an_unusable_size_is_dropped_rather_than_reopened() {
        let base = WindowGeometry {
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
            display: None,
        };
        assert_eq!(base.sanitized(), Some(base));
        // Below the window's own minimum: the host would clamp it, so don't
        // start from it.
        assert_eq!(WindowGeometry { width: 200, ..base }.sanitized(), None);
        // Absurd: a corrupt record.
        assert_eq!(
            WindowGeometry {
                width: 99_999,
                ..base
            }
            .sanitized(),
            None
        );
        // A record with no display recorded still restores: it came from a host
        // that hides positions, and the origin is trusted.
        assert!(base.origin_on_display());
    }

    #[test]
    fn a_layout_without_a_window_record_still_loads() {
        // The field is new: a workspace saved before it existed must not fail.
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(
            root.path().join("workspace-state.json"),
            br#"{"explorer":true,"explorer_width":300.0,"terminals":["/tmp"]}"#,
        )
        .unwrap();
        let layout = Layout::load(root.path()).unwrap();
        assert!(layout.explorer);
        assert_eq!(layout.window, None);
    }

    #[test]
    fn a_saved_placement_round_trips_through_the_layout_file() {
        let root = crate::test_support::tempdir().unwrap();
        let layout = Layout {
            window: Some(WindowGeometry {
                x: 429,
                y: 276,
                width: 1080,
                height: 720,
                display: Some(((0, 0), (4112, 2658))),
            }),
            ..Layout::default()
        };
        layout.save(root.path()).unwrap();
        let reloaded = Layout::load(root.path()).unwrap();
        assert_eq!(reloaded.window, layout.window);
    }
}
