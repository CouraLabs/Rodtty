use crate::desktop::workspace::*;
use crate::desktop::*;
use crate::{files::Entry, git::Change};
use std::time::{Duration, Instant};

impl State {
    pub(crate) fn toggle_files(&mut self) {
        let minimum_width = explorer_minimum_width(self);
        if self.workspace.body_width < minimum_width {
            self.workspace.explorer_revealed = !self.workspace.explorer_revealed;
            self.workspace.layout.explorer = self.workspace.explorer_revealed;
        } else {
            self.workspace.explorer_revealed = false;
            self.workspace.layout.explorer = !self.workspace.layout.explorer;
        }
        if self.workspace.layout.explorer && self.workspace.directory.is_none() {
            self.navigate(
                self.active()
                    .map(Session::cwd)
                    .unwrap_or_else(|| self.directory.clone()),
            );
        }
    }
    /// Files is drawn as the compact overlay above the terminal and editor.
    pub(crate) fn explorer_overlay_open(&self) -> bool {
        let (shown, _, _) = pane_visibility(self, self.workspace.body_width);
        shown && self.workspace.body_width < explorer_minimum_width(self)
    }
    /// The Files toggle: also moves keyboard focus into the compact overlay as
    /// it opens, and back where it was as it closes.
    pub(crate) fn toggle_files_with_focus(&mut self, tx: &mut Tx) {
        let was_overlay = self.explorer_overlay_open();
        let return_to = if self.workspace.terminal_focus() {
            FocusReturn::Terminal
        } else {
            FocusReturn::Editor(
                self.workspace
                    .tabs
                    .get(self.workspace.active)
                    .filter(|tab| !self.settings && !self.search_tab && tab.mode == View::Edit)
                    .map(|tab| tab.id),
            )
        };
        self.workspace.explorer_menu = false;
        self.toggle_files();
        let overlay = self.explorer_overlay_open();
        if overlay && !was_overlay {
            self.workspace.overlay_return = Some(return_to);
            tx.focus_when_shown("file-list", false);
        } else if was_overlay && !overlay {
            self.restore_overlay_focus(tx);
        }
    }
    pub(crate) fn restore_overlay_focus(&mut self, tx: &mut Tx) {
        match self.workspace.overlay_return.take() {
            Some(FocusReturn::Terminal) => {
                self.workspace.set_terminal_focus(true);
                tx.blur();
            }
            Some(FocusReturn::Editor(Some(id)))
                if self.workspace.tabs.iter().any(|tab| tab.id == id) =>
            {
                self.workspace.set_terminal_focus(false);
                if let Some(tab) = self.workspace.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.edit.focused = true;
                }
                tx.focus_when_shown(format!("file-editor-{id}"), true);
            }
            Some(FocusReturn::Editor(_)) => {
                self.workspace.set_terminal_focus(false);
                tx.blur();
            }
            None => {
                if self.workspace.terminal_focus() {
                    tx.blur();
                }
            }
        }
    }
    /// Escape over the compact overlay closes it, as the Files button does.
    pub(crate) fn dismiss_explorer_overlay(&mut self, tx: &mut Tx) -> bool {
        if !self.explorer_overlay_open() {
            return false;
        }
        self.toggle_files_with_focus(tx);
        true
    }
    pub(crate) fn navigate(&mut self, path: PathBuf) {
        self.workspace.directory = Some(path.clone());
        self.workspace.entries.clear();
        self.workspace.tree_listed.clear();
        self.workspace.tree.clear();
        self.workspace.tree_dirty = true;
        self.workspace.layout.expanded_folders.clear();
        self.workspace.layout.selected_path = None;
        self.workspace.explorer_error = None;
        self.workspace.outside_root = None;
        self.workspace.pending_reveal = None;
        self.workspace.revealed_directory = None;
        self.workspace.last_follow = None;
        self.workspace.repo = None;
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }
    pub(crate) fn reveal_terminal(&mut self, tx: &mut Tx) {
        let cwd = self
            .active()
            .map(Session::cwd)
            .unwrap_or_else(|| self.directory.clone());
        self.workspace.last_follow = Some(cwd.clone());
        let Some(root) = self.workspace.directory.as_ref() else {
            self.navigate(cwd);
            return;
        };
        if !cwd.starts_with(root) {
            self.workspace.outside_root = Some(cwd);
            return;
        }
        self.workspace.outside_root = None;
        self.workspace.revealed_directory = Some(cwd.clone());
        self.workspace.tree_dirty = true;
        let mut ancestor = cwd.as_path();
        while ancestor != root {
            if !self
                .workspace
                .layout
                .expanded_folders
                .iter()
                .any(|p| p == ancestor)
            {
                self.workspace
                    .layout
                    .expanded_folders
                    .push(ancestor.to_owned());
            }
            self.workspace
                .tree
                .set_expanded(&ancestor.to_string_lossy(), true);
            let Some(parent) = ancestor.parent() else {
                break;
            };
            ancestor = parent;
        }
        if cwd != *root {
            self.workspace.pending_reveal = Some(cwd.clone());
            self.workspace.tree.reveal(&cwd.to_string_lossy(), tx);
        }
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(crate) fn reveal_file_result(&mut self, path: PathBuf, tx: &mut Tx) {
        if let Some(root) = self.workspace.directory.clone()
            && path.starts_with(&root)
        {
            let mut parent = path.parent();
            while let Some(folder) =
                parent.filter(|folder| *folder != root && folder.starts_with(&root))
            {
                if !self
                    .workspace
                    .layout
                    .expanded_folders
                    .iter()
                    .any(|p| p == folder)
                {
                    self.workspace
                        .layout
                        .expanded_folders
                        .push(folder.to_owned());
                }
                self.workspace
                    .tree
                    .set_expanded(&folder.to_string_lossy(), true);
                parent = folder.parent();
            }
            self.workspace.revealed_directory = Some(path.clone());
            self.workspace.pending_reveal = Some(path.clone());
            self.workspace.tree.reveal(&path.to_string_lossy(), tx);
        }
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(crate) fn collapse_tree(&mut self) {
        self.workspace.tree.collapse_all();
        // Collapse-all would otherwise close the synthetic root row the tree
        // shows its contents through; the workspace root stays open by design.
        if let Some(root) = self.workspace.directory.as_ref() {
            self.workspace
                .tree
                .set_expanded(&root.to_string_lossy(), Self::WORKSPACE_ROOT_EXPANDED);
        }
        self.workspace.tree_dirty = true;
        self.workspace.layout.expanded_folders.clear();
        self.workspace.scan_token += 1;
        self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
    }

    pub(crate) fn open_tree_selection(&mut self) {
        let entry = self
            .workspace
            .tree
            .cursor()
            .and_then(|id| self.workspace.tree.get(id))
            .and_then(|n| n.entry.clone());
        if let Some(entry) = entry {
            if entry.directory {
                self.workspace.tree.toggle(&entry.path.to_string_lossy());
                self.sync_tree_expansion();
            } else {
                self.open_file(entry.path);
            }
        }
    }

    pub(crate) fn rebuild_tree(&mut self) {
        let workspace = &mut self.workspace;
        workspace.tree_dirty = false;
        let selected = workspace.tree.cursor().map(str::to_owned).or_else(|| {
            workspace
                .layout
                .selected_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
        });
        workspace.tree.clear();
        let Some(root) = workspace.directory.as_ref() else {
            return;
        };
        // The tree reads as a rooted folder: the first row is the workspace folder
        // itself, named as the host folder is, with every other row under it.
        let root_key = root.to_string_lossy().into_owned();
        let root_label = root
            .file_name()
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .into_owned();
        workspace.tree.insert(
            None,
            &root_key,
            TreeEntry {
                entry: None,
                label: root_label,
                badge: String::new(),
                change: None,
                lines: None,
                root: true,
            },
        );
        let root_expanded = Self::WORKSPACE_ROOT_EXPANDED;
        workspace.tree.set_expanded(&root_key, root_expanded);
        // First the whole parent-child structure: a parent precedes its
        // children (a stable sort by depth keeps each folder's sibling order),
        // so no entry is dropped for arriving before its folder and every
        // folder knows its real children before any status row is decided.
        let mut entries: Vec<&Entry> = workspace.entries.iter().collect();
        entries.sort_by_key(|entry| entry.path.components().count());
        for entry in entries {
            let id = entry.path.to_string_lossy().into_owned();
            // A nested entry stays under its own folder; only the root's direct
            // children hang from the synthetic root row.
            let parent = entry
                .path
                .parent()
                .filter(|p| *p != root)
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| root_expanded.then(|| root_key.clone()));
            workspace.tree.insert(
                parent.as_deref(),
                &id,
                TreeEntry {
                    entry: Some(entry.clone()),
                    root: false,
                    label: entry.name.clone(),
                    badge: workspace
                        .repo
                        .as_ref()
                        .map(|r| r.badge(&entry.path))
                        .unwrap_or_default(),
                    change: workspace
                        .repo
                        .as_ref()
                        .filter(|_| !entry.directory)
                        .and_then(|r| r.change(&entry.path))
                        .or(entry.missing.then_some(Change::Deleted)),
                    lines: workspace
                        .repo
                        .as_ref()
                        .filter(|_| !entry.directory)
                        .and_then(|r| r.line_changes(&entry.path)),
                },
            );
            workspace
                .tree
                .set_expanded(&id, workspace.layout.expanded_folders.contains(&entry.path));
        }
        // Then one status row per childless folder. It also gives an unloaded
        // folder its disclosure affordance before its children arrive.
        for entry in &workspace.entries {
            let id = entry.path.to_string_lossy();
            if entry.directory && !workspace.tree.has_children(&id) {
                let label = folder_status(
                    workspace
                        .tree_errors
                        .iter()
                        .find(|(p, _)| p == &entry.path)
                        .map(|(_, e)| e.as_str()),
                    workspace.tree_listed.contains(&entry.path),
                );
                workspace.tree.insert(
                    Some(&id),
                    format!("{id}/::folder-state"),
                    TreeEntry {
                        entry: None,
                        root: false,
                        label,
                        badge: String::new(),
                        change: None,
                        lines: None,
                    },
                );
            }
        }
        if let Some(id) = selected {
            workspace.tree.select(&id);
        }
    }

    pub(crate) fn sync_tree_expansion(&mut self) -> bool {
        let mut changed = false;
        for entry in &self.workspace.entries {
            if !entry.directory {
                continue;
            }
            let expanded = self
                .workspace
                .tree
                .is_expanded(&entry.path.to_string_lossy());
            let saved = self.workspace.layout.expanded_folders.contains(&entry.path);
            if expanded != saved {
                self.workspace
                    .layout
                    .expanded_folders
                    .retain(|p| p != &entry.path);
                if expanded {
                    self.workspace
                        .layout
                        .expanded_folders
                        .push(entry.path.clone());
                }
                changed = true;
            }
        }
        if changed {
            self.workspace.tree_dirty = true;
            self.workspace.scan_token += 1;
            self.workspace.last_scan = Instant::now() - Duration::from_secs(3);
        }
        changed
    }

    pub(crate) fn preview_tree_selection(&mut self) {
        if let Some(entry) = self.selected_entry().filter(|e| !e.directory) {
            self.open_file_with_preview(entry.path, !self.config.single_click_keeps_open);
        }
    }
}
