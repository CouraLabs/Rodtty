//! Project tasks in the desktop: palette entries per task, a dedicated
//! titled terminal per run, and focus instead of a second copy while one runs.
use super::*;
use crate::tasks::{ProjectTask, TaskFile};
use std::path::Path;

impl State {
    /// Rebuild the `Task: Run …` palette entries for the active workspace.
    pub(crate) fn refresh_task_commands(&mut self) {
        self.commands.retain(|c| !c.id.starts_with("task-run-"));
        let (workspace, _) = self.active_workspace();
        let Ok(file) = TaskFile::load(&self.root, &workspace) else {
            return;
        };
        for (index, task) in file.tasks.iter().enumerate().take(200) {
            // Titles live as long as the palette's other command titles.
            let id: &'static str = Box::leak(format!("task-run-{index}").into_boxed_str());
            let title: &'static str =
                Box::leak(format!("Task: Run {}", task.name).into_boxed_str());
            self.commands.push(actions::Command {
                id,
                title,
                shortcut: "",
                action: AppAction::RunTask(index as u16),
                context: actions::ShortcutContext::Global,
            });
        }
    }

    /// Tasks: Edit Project Tasks — the active project's task file, created
    /// from a template the first time. Saving it runs nothing.
    pub(crate) fn edit_tasks(&mut self) {
        let (workspace, root) = self.active_workspace();
        let path = crate::tasks::path(&self.root, &workspace);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(error) = std::fs::write(&path, crate::tasks::TEMPLATE) {
                self.message = format!("Could not create the task file: {error}");
                return;
            }
        }
        self.message = format!(
            "Tasks for {} — directories are relative to that folder.",
            root.display()
        );
        self.open_file(path);
    }

    pub(crate) fn run_task_index(&mut self, index: usize) {
        let (workspace, root) = self.active_workspace();
        let task = match TaskFile::load(&self.root, &workspace) {
            Ok(file) => file.tasks.get(index).cloned(),
            Err(error) => {
                self.message = format!("The project's task file is invalid: {error:#}");
                return;
            }
        };
        let Some(task) = task else {
            self.message = "That task no longer exists.".into();
            return;
        };
        match self.run_task(&workspace, &root, &task, None) {
            Ok((index, started)) => {
                self.active = index;
                self.workspace.set_terminal_focus(true);
                self.workspace.layout.terminal = true;
                self.workspace.note_session_switch();
                self.selection = None;
                self.message = if started {
                    format!(
                        "Running {}: {} in {}",
                        task.name,
                        task.command,
                        task.directory(&root)
                            .map(|d| d.display().to_string())
                            .unwrap_or_default()
                    )
                } else {
                    format!("{} is already running; showing its terminal.", task.name)
                };
            }
            Err(error) => self.message = error,
        }
    }

    /// Start `task` in its own terminal, or return the terminal where it is
    /// still running. Returns the session index and whether it started now.
    pub(crate) fn run_task(
        &mut self,
        workspace: &str,
        root: &Path,
        task: &ProjectTask,
        client: Option<&str>,
    ) -> Result<(usize, bool), String> {
        let key = (workspace.to_owned(), task.name.clone());
        if let Some(id) = self.task_sessions.get(&key)
            && let Some(index) = self.sessions.iter().position(|s| s.id() == *id)
            && self.sessions[index].has_running_commands()
        {
            return Ok((index, false));
        }
        let dir = task.directory(root).map_err(|e| format!("{e:#}"))?;
        let index = self.spawn_session(dir)?;
        let session = &self.sessions[index];
        let line = format!(
            "printf '\\033]0;%s\\007' {}; {}\r",
            automation::terminal_quote(&task.name),
            task.command
        );
        session
            .send(line.into_bytes())
            .map_err(|e| format!("{e:#}"))?;
        let id = session.id();
        self.task_sessions.insert(key, id);
        self.automation.owners.insert(
            id,
            automation::SessionOwner {
                workspace: workspace.to_owned(),
                root: root.to_owned(),
                client: client.map(str::to_owned),
            },
        );
        Ok((index, true))
    }
}
