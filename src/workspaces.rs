//! Named project workspaces: a label for a project root folder, with that
//! project's saved layout. The registry lives in `workspaces.json`; which one
//! is selected lives in `workspace-selection.json`, apart from preferences.
use crate::{paths::Canonical as _, work::Layout};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const REGISTRY_FILE: &str = "workspaces.json";
pub const SELECTION_FILE: &str = "workspace-selection.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedWorkspace {
    /// Opaque and stable: derived from the canonical root, so the same
    /// project folder keeps its identity across launches and renames.
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    /// The project's arrangement when it was last switched away from or saved.
    #[serde(default)]
    pub layout: Option<Layout>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub workspaces: Vec<NamedWorkspace>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Selection {
    selected: Option<String>,
}

/// The identity of a project root. Equivalent paths (symlinks, `..`, a
/// trailing separator) resolve to one root; each linked Git worktree is its
/// own folder and so its own identity.
pub fn workspace_id(root: &Path) -> String {
    // FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("ws_{hash:016x}")
}

impl Registry {
    pub fn load(app_root: &Path) -> Result<Self> {
        match std::fs::read(app_root.join(REGISTRY_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("read workspaces.json"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("read workspaces.json"),
        }
    }

    pub fn save(&self, app_root: &Path) -> Result<()> {
        write_atomic(
            &app_root.join(REGISTRY_FILE),
            &serde_json::to_vec_pretty(self)?,
        )
    }

    pub fn get(&self, id: &str) -> Option<&NamedWorkspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut NamedWorkspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }

    /// The workspace whose root is `folder` or contains it most closely.
    pub fn containing(&self, folder: &Path) -> Option<&NamedWorkspace> {
        let folder = folder.canonical().unwrap_or_else(|_| folder.to_owned());
        self.workspaces
            .iter()
            .filter(|w| folder.starts_with(&w.root))
            .max_by_key(|w| w.root.components().count())
    }

    /// Register `folder` as a workspace named `name`. A folder that is
    /// already a workspace keeps its existing entry and name.
    pub fn create(&mut self, name: &str, folder: &Path) -> Result<NamedWorkspace> {
        let root = folder
            .canonical()
            .with_context(|| format!("open project folder {}", folder.display()))?;
        if !root.is_dir() {
            bail!("{} is not a folder", root.display());
        }
        let id = workspace_id(&root);
        if let Some(existing) = self.get(&id) {
            return Ok(existing.clone());
        }
        let name = match name.trim() {
            "" => root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
            name => name.to_owned(),
        };
        let workspace = NamedWorkspace {
            id,
            name,
            root,
            layout: None,
        };
        self.workspaces.push(workspace.clone());
        Ok(workspace)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("A workspace name can't be empty");
        }
        self.get_mut(id).context("no such workspace")?.name = name.to_owned();
        Ok(())
    }

    /// Forget a workspace. Its project folder is never touched.
    pub fn remove(&mut self, id: &str) -> Option<NamedWorkspace> {
        let index = self.workspaces.iter().position(|w| w.id == id)?;
        Some(self.workspaces.remove(index))
    }

    /// The label shown for `id`: its name, plus its root when another
    /// workspace has the same name.
    pub fn label(&self, id: &str) -> String {
        let Some(workspace) = self.get(id) else {
            return String::new();
        };
        let collides = self
            .workspaces
            .iter()
            .any(|w| w.id != id && w.name == workspace.name);
        if collides {
            format!("{} — {}", workspace.name, workspace.root.display())
        } else {
            workspace.name.clone()
        }
    }

    /// The one-time migration of the single saved layout from before named
    /// workspaces: it becomes the first workspace, rooted at its explorer folder.
    pub fn migrate_single_layout(&mut self, layout: &Layout, fallback: &Path) -> Option<String> {
        if !self.workspaces.is_empty() {
            return None;
        }
        let folder = layout
            .explorer_directory
            .clone()
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| fallback.to_owned());
        let mut workspace = self.create("", &folder).ok()?;
        workspace.layout = Some(layout.clone());
        if let Some(saved) = self.get_mut(&workspace.id) {
            saved.layout = Some(layout.clone());
        }
        Some(workspace.id)
    }
}

pub fn load_selection(app_root: &Path) -> Option<String> {
    let bytes = std::fs::read(app_root.join(SELECTION_FILE)).ok()?;
    serde_json::from_slice::<Selection>(&bytes).ok()?.selected
}

pub fn save_selection(app_root: &Path, id: Option<&str>) -> Result<()> {
    write_atomic(
        &app_root.join(SELECTION_FILE),
        &serde_json::to_vec(&Selection {
            selected: id.map(str::to_owned),
        })?,
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("workspace file has no folder")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equivalent_paths_share_an_identity_and_names_collide_visibly() {
        let home = crate::test_support::tempdir().unwrap();
        let a = home.path().join("a");
        let b = home.path().join("b");
        std::fs::create_dir_all(a.join("src")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut registry = Registry::default();
        let first = registry.create("Project", &a).unwrap();
        let again = registry.create("Other", &a.join("src/..")).unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(registry.workspaces.len(), 1);
        let second = registry.create("Project", &b).unwrap();
        assert_ne!(first.id, second.id);
        assert!(registry.label(&first.id).contains(" — "));
        registry.rename(&second.id, "B").unwrap();
        assert_eq!(registry.label(&first.id), "Project");
        assert_eq!(
            registry.containing(&a.join("src")).map(|w| w.id.clone()),
            Some(first.id.clone())
        );
        registry.save(home.path()).unwrap();
        assert_eq!(Registry::load(home.path()).unwrap(), registry);
        assert!(registry.remove(&first.id).is_some());
        assert!(
            a.join("src").is_dir(),
            "removing a workspace keeps its files"
        );
    }

    #[test]
    fn the_single_saved_layout_migrates_once() {
        let home = crate::test_support::tempdir().unwrap();
        let project = home.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        let layout = Layout {
            explorer_directory: Some(project.clone()),
            ..Layout::default()
        };
        let mut registry = Registry::default();
        let id = registry
            .migrate_single_layout(&layout, home.path())
            .unwrap();
        assert_eq!(registry.get(&id).unwrap().layout, Some(layout.clone()));
        assert_eq!(registry.migrate_single_layout(&layout, home.path()), None);
        save_selection(home.path(), Some(&id)).unwrap();
        assert_eq!(load_selection(home.path()), Some(id));
    }
}
