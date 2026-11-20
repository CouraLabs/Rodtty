//! Cold startup reconstructs views and fresh shells; window reopen retains State.
use super::*;

impl State {
    pub(super) fn restore_startup(&mut self) {
        self.workspace_setup();
        self.load_pending_drafts();
        let directories = self.workspace.layout.terminals.clone();
        let active = self.workspace.layout.active_terminal;
        if directories.is_empty() {
            self.start_tab(self.directory.clone());
        } else if self.config.behavior.terminal_startup
            == crate::config::TerminalStartup::SingleFresh
        {
            let directory = match self.config.behavior.new_terminal_directory {
                crate::config::NewTerminalDirectory::Active => directories.get(active).cloned(),
                crate::config::NewTerminalDirectory::Explorer => self.workspace.directory.clone(),
                crate::config::NewTerminalDirectory::Specified => {
                    Some(self.config.behavior.specified_directory.clone())
                }
            }
            .unwrap_or_else(|| self.directory.clone());
            self.start_restored_terminal(directory);
        } else {
            let count = directories.len();
            for directory in directories {
                self.start_restored_terminal(directory);
            }
            let splits = self.workspace.layout.splits.clone();
            self.restore_splits(&splits, count);
            self.active = active.min(self.sessions.len().saturating_sub(1));
        }
        self.workspace
            .set_terminal_focus(self.workspace.layout.terminal_focus);
        if self.workspace_restoration_ready() && !self.workspace.restoration_warnings.is_empty() {
            self.message = self.workspace.restoration_warnings.join("\n");
        }
    }

    fn start_restored_terminal(&mut self, directory: PathBuf) {
        if directory.is_absolute() && directory.is_dir() {
            self.start_tab(directory);
        } else {
            self.workspace.restoration_warnings.push(format!(
                "Saved terminal folder is unavailable: {}. Opened the startup folder instead.",
                directory.display()
            ));
            self.start_tab(self.directory.clone());
        }
    }
}

#[cfg(test)]
mod tests;
