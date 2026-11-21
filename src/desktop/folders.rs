//! Recent folders: the distinct local directories terminals reported, most
//! recent first, kept in memory only (never saved). The command palette
//! lists them as `Folder: Open Terminal in …` and `Folder: Change Directory
//! to …`; searching matches the folder name and full path.
use super::*;

/// How many folders the history keeps.
pub(crate) const FOLDER_HISTORY: usize = 50;
/// How many appear in the palette.
const LISTED: usize = 20;

/// A POSIX single-quoted path, as Brush reads it on every platform.
pub(crate) fn shell_path(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

impl State {
    /// Record a folder a terminal reported. Only existing local folders
    /// count; a repeat visit moves the folder to the front.
    pub(crate) fn note_folder(&mut self, folder: PathBuf) {
        if self.folder_history.first() == Some(&folder) || !folder.is_absolute() || !folder.is_dir()
        {
            return;
        }
        self.folder_history.retain(|f| *f != folder);
        self.folder_history.insert(0, folder);
        self.folder_history.truncate(FOLDER_HISTORY);
    }

    /// Rebuild the palette entries for recent folders.
    pub(crate) fn refresh_folder_commands(&mut self) {
        self.commands.retain(|c| !c.id.starts_with("folder-"));
        let folders: Vec<PathBuf> = self.folder_history.iter().take(LISTED).cloned().collect();
        for (index, folder) in folders.iter().enumerate() {
            let name = folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.display().to_string());
            // The full path tells two folders of the same name apart, such
            // as two Git worktrees.
            let place = window::tilde_path(folder);
            for (kind, title, action) in [
                (
                    "open",
                    format!("Folder: Open Terminal in {name} — {place}"),
                    AppAction::FolderTerminal(index as u16),
                ),
                (
                    "cd",
                    format!("Folder: Change Directory to {name} — {place}"),
                    AppAction::FolderChange(index as u16),
                ),
            ] {
                self.commands.push(actions::Command {
                    id: Box::leak(format!("folder-{kind}-{index}").into_boxed_str()),
                    title: Box::leak(title.into_boxed_str()),
                    shortcut: "",
                    action,
                    context: actions::ShortcutContext::Global,
                });
            }
        }
    }

    fn history_folder(&mut self, index: usize) -> Option<PathBuf> {
        let folder = self.folder_history.get(index).cloned()?;
        if !folder.is_dir() {
            self.folder_history.retain(|f| *f != folder);
            self.message = format!(
                "{} no longer exists; removed it from recent folders.",
                folder.display()
            );
            return None;
        }
        Some(folder)
    }

    pub(crate) fn folder_open_terminal(&mut self, index: usize) {
        if let Some(folder) = self.history_folder(index) {
            self.workspace.layout.terminal = true;
            self.start_tab(folder);
        }
    }

    /// `cd` the current shell to the folder: only an idle Rodtty shell, so
    /// the text can't land in a running program.
    pub(crate) fn folder_change_directory(&mut self, index: usize) {
        let Some(folder) = self.history_folder(index) else {
            return;
        };
        let Some(session) = self.active() else {
            self.message = "No terminal to change directory in.".into();
            return;
        };
        if session.has_running_commands() {
            self.message =
                "This terminal is running a command; use Open Terminal in instead.".into();
            return;
        }
        let line = format!("cd -- {}\r", shell_path(&folder));
        self.send(line.into_bytes());
        self.message = format!("Changed directory to {}.", folder.display());
    }
}

#[cfg(test)]
impl State {
    fn dispatch_folder_clear(&mut self) {
        self.folder_history.clear();
        self.refresh_folder_commands();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_distinct_recent_first_and_jumps_into_tricky_paths() {
        let root = crate::test_support::tempdir().unwrap();
        let mut state = State::with_root(root.path().into());
        state.shell = Some(crate::test_support::shell_binary().unwrap());
        let tricky = root.path().join("a b $(touch x) 'q' & ;");
        let other = root.path().join("other");
        std::fs::create_dir_all(&tricky).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        state.note_folder(other.clone());
        state.note_folder(tricky.clone());
        state.note_folder(other.clone());
        assert_eq!(state.folder_history, [other.clone(), tricky.clone()]);
        state.note_folder(root.path().join("missing"));
        assert_eq!(state.folder_history.len(), 2, "missing folders are skipped");
        state.start_tab(root.path().into());
        state
            .active()
            .unwrap()
            .wait_for("rodtty:", std::time::Duration::from_secs(15))
            .unwrap();
        state.folder_change_directory(1);
        state.send(b"pwd > here.txt\r".to_vec());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !tricky.join("here.txt").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the shell never reached the folder"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            !tricky.join("x").exists() && !root.path().join("x").exists(),
            "nothing in the path ran"
        );
        // A busy terminal receives nothing.
        state.send(b"sleep 5\r".to_vec());
        std::thread::sleep(std::time::Duration::from_millis(400));
        let before = state.active().unwrap().state.lock().unwrap().output.end();
        state.folder_change_directory(0);
        assert!(state.message.contains("running a command"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(
            state.active().unwrap().state.lock().unwrap().output.end(),
            before
        );
        state.dispatch_folder_clear();
        assert!(state.folder_history.is_empty());
    }
}
