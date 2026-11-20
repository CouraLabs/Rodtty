//! Owner-defined project tasks: named shell commands such as "Dev server"
//! or "Watch tests", stored per project in `<app root>/tasks/<workspace>.json`,
//! never in the repository and never in the saved layout. Tasks run only
//! when the owner (or an approved automation client) runs them.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTask {
    pub name: String,
    /// A command line for the session's shell (Bash syntax).
    pub command: String,
    /// Relative to the project root, or an absolute local folder. Empty is the root.
    #[serde(default)]
    pub cwd: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFile {
    #[serde(default)]
    pub tasks: Vec<ProjectTask>,
}

pub fn path(app_root: &Path, workspace: &str) -> PathBuf {
    app_root.join("tasks").join(format!("{workspace}.json"))
}

pub const TEMPLATE: &str = r#"{
  "tasks": [
    { "name": "Build", "command": "cargo build", "cwd": "" }
  ]
}
"#;

impl TaskFile {
    pub fn load(app_root: &Path, workspace: &str) -> Result<Self> {
        match std::fs::read(path(app_root, workspace)) {
            Ok(bytes) => {
                let file: Self = serde_json::from_slice(&bytes).context("read project tasks")?;
                file.validate()?;
                Ok(file)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).context("read project tasks"),
        }
    }

    pub fn validate(&self) -> Result<()> {
        for (i, task) in self.tasks.iter().enumerate() {
            if task.name.trim().is_empty() {
                bail!("task {} has no name", i + 1);
            }
            if task.command.trim().is_empty() {
                bail!("task {:?} has no command", task.name);
            }
            if self.tasks[..i].iter().any(|t| t.name == task.name) {
                bail!("two tasks are named {:?}", task.name);
            }
        }
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<&ProjectTask> {
        self.tasks.iter().find(|t| t.name == name)
    }
}

impl ProjectTask {
    /// The folder the task runs in, which must exist.
    pub fn directory(&self, root: &Path) -> Result<PathBuf> {
        let dir = if self.cwd.is_empty() {
            root.to_owned()
        } else {
            root.join(&self.cwd)
        };
        if !dir.is_dir() {
            bail!(
                "task {:?}: folder {} does not exist",
                self.name,
                dir.display()
            );
        }
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_are_per_project_and_validated() {
        let home = crate::test_support::tempdir().unwrap();
        let file = path(home.path(), "ws_a");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"tasks":[{"name":"Test","command":"cargo test","cwd":"sub"}]}"#,
        )
        .unwrap();
        std::fs::write(
            path(home.path(), "ws_b"),
            r#"{"tasks":[{"name":"Test","command":"npm test"}]}"#,
        )
        .unwrap();
        let a = TaskFile::load(home.path(), "ws_a").unwrap();
        let b = TaskFile::load(home.path(), "ws_b").unwrap();
        assert_eq!(a.find("Test").unwrap().command, "cargo test");
        assert_eq!(b.find("Test").unwrap().command, "npm test");
        assert!(
            a.find("Test").unwrap().directory(home.path()).is_err(),
            "a missing folder is an error"
        );
        assert_eq!(
            TaskFile::load(home.path(), "ws_none").unwrap(),
            TaskFile::default()
        );
        std::fs::write(
            path(home.path(), "ws_c"),
            r#"{"tasks":[{"name":"x","command":""}]}"#,
        )
        .unwrap();
        assert!(TaskFile::load(home.path(), "ws_c").is_err());
        let template: TaskFile = serde_json::from_str(TEMPLATE).unwrap();
        assert!(template.validate().is_ok());
    }
}
