use crate::config::Config;
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const DISABLED_ENV: &str = "RODTTY_DISABLED";

pub fn disabled_from_env() -> HashSet<String> {
    std::env::var(DISABLED_ENV)
        .unwrap_or_default()
        .split(',')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn utility_names() -> Vec<String> {
    let mut names: Vec<_> = brush_coreutils_builtins::bundled_commands()
        .into_keys()
        .collect();
    names.sort();
    names
}

pub fn cleanup_stale(root: &Path) {
    // Called only after acquiring the host-instance lock, never by a second
    // launch or a headless renderer sharing a live owner's configuration.
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(suffix) = name.strip_prefix("session-")
                && suffix.len() == 6
                && suffix.bytes().all(|c| c.is_ascii_alphanumeric())
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

pub struct PreparedProfile {
    pub directory: tempfile::TempDir,
    pub rc: PathBuf,
    pub path: OsString,
    pub disabled: String,
    pub jobs: PathBuf,
    pub job_pids: PathBuf,
}

pub fn quote(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .context("startup paths must be valid Unicode in this prototype")?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

impl PreparedProfile {
    pub fn new(shell: &Path, root: &Path, config: &Config) -> Result<Self> {
        config.validate()?;
        std::fs::create_dir_all(root)?;
        // Each session owns immutable launcher files, retained until its PTY exits.
        let directory = tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(root)?;
        let bin = directory.path().join("bin");
        std::fs::create_dir(&bin)?;
        for name in utility_names() {
            let target = bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if std::fs::hard_link(shell, &target).is_err() {
                std::fs::copy(shell, &target)
                    .with_context(|| format!("create launcher {}", target.display()))?;
            }
        }
        let mut paths = vec![bin.clone()];
        if let Some(inherited) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&inherited));
        }
        let path = std::env::join_paths(paths)?;
        let rc = directory.path().join("startup.sh");
        let user_rc = root.join("startup.sh");
        let mut script = String::new();
        if user_rc.is_file() {
            script.push_str(&format!("source {}\n", quote(&user_rc)?));
        }
        // Prepend after the owner's startup file so its PATH edits retain the
        // selected utilities. Builtin guards independently protect disabled names.
        let separator = if cfg!(windows) { ';' } else { ':' };
        script.push_str(&format!(
            "export PATH={}\"{separator}$PATH\"\nhash -r\n",
            quote(&bin)?
        ));
        let history = if config.history {
            root.join("history")
        } else {
            PathBuf::new()
        };
        if config.history {
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(&history)
                .context("create owner history file")?;
        }
        script.push_str(&format!(
            "export HISTFILE={}\nHISTSIZE=10000\nHISTFILESIZE=10000\n",
            quote(&history)?
        ));
        script.push_str("PS1='rodtty:\\w\\$ '\n");
        let history_sync = if config.history {
            // Brush implements append, but not Bash's incremental import (-n).
            // New sessions load the shared file during shell initialization.
            "history -a; "
        } else {
            ""
        };
        // Preserve the owner's pre-command prompt and every PROMPT_COMMAND
        // array entry. Invisible OSC markers describe command activity without
        // matching prompt text, so builtins and host programs behave alike.
        script.push_str("PS0='\\e]133;C\\a'\"${PS0-}\"\n");
        let jobs = directory.path().join("jobs");
        let job_pids = directory.path().join("job-pids");
        // `jobs` in command substitution runs in a subshell and loses Brush's
        // internal async jobs. Capture directly in private session files instead.
        script.push_str(&format!(
            "__rodtty_prompt_activity() {{\n jobs -r > {jobs}; jobs -s >> {jobs}\n jobs -pr > {pids}; jobs -ps >> {pids}\n printf '\\033]133;A\\007'\n}}\n",
            jobs = quote(&jobs)?, pids = quote(&job_pids)?,
        ));
        script.push_str(&format!(
            "PROMPT_COMMAND+=('{history_sync}printf \"\\033]7;file://localhost%s\\007\" \"$PWD\"; __rodtty_prompt_activity')\n"
        ));
        std::fs::write(&rc, script)?;
        Ok(Self {
            directory,
            rc,
            path,
            disabled: config.disabled.join(","),
            jobs,
            job_pids,
        })
    }
}
