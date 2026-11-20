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

/// Points a utility launcher at the shell binary.
///
/// On Unix the launcher is a symbolic link, so every exec resolves to the
/// shell binary's own, single name. Hard links give that one file a name in
/// every session's profile; macOS then can report an exec under another
/// session's name, and when that session has already removed its profile,
/// Gatekeeper's scan of the vanished path fails and the system SIGKILLs the
/// command. Windows keeps hard links: symbolic links there need privileges.
fn link_launcher(shell: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let shell = std::path::absolute(shell)?;
        std::os::unix::fs::symlink(shell, target)
    }
    #[cfg(not(unix))]
    {
        std::fs::hard_link(shell, target)
    }
}

pub struct PreparedProfile {
    pub directory: tempfile::TempDir,
    pub rc: PathBuf,
    pub path: OsString,
    pub disabled: String,
    pub jobs: PathBuf,
    pub job_pids: PathBuf,
    pub activity_tag: String,
    /// Prompt data file read by the shell's prompt hook at each prompt.
    pub prompt: PathBuf,
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
            if link_launcher(shell, &target).is_err() {
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
        // Rodtty's prompt is assigned before the owner's startup file so an
        // owner-defined PS1 there wins and ends Rodtty's prompt management.
        let prompt = directory.path().join(crate::prompt::DATA_FILE);
        config.prompt.publish(directory.path())?;
        script.push_str(&crate::prompt::shell_hook(&prompt)?);
        script.push_str("__rodtty_prompt_update\n");
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
        let history_sync = if config.history {
            // Brush implements append, but not Bash's incremental import (-n).
            // New sessions load the shared file during shell initialization.
            "history -a; "
        } else {
            ""
        };
        // The private profile path uniquely identifies this owning session. This
        // tag separates our activity from public/nested shell integration; it is
        // not a security boundary against programs that can read the profile.
        let activity_tag: String = directory
            .path()
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        // Preserve the owner's pre-command prompt and every PROMPT_COMMAND
        // array entry. Invisible OSC markers describe command activity without
        // matching prompt text, so builtins and host programs behave alike.
        script.push_str(&format!(
            "PS0='\\e]777;rodtty;{activity_tag};C\\a'\"${{PS0-}}\"\n"
        ));
        let jobs = directory.path().join("jobs");
        let job_pids = directory.path().join("job-pids");
        // `jobs` in command substitution runs in a subshell and loses Brush's
        // internal async jobs. Capture directly in private session files instead.
        script.push_str(&format!(
            "__rodtty_prompt_activity() {{\n jobs -r > {jobs}; jobs -s >> {jobs}\n jobs -pr > {pids}; jobs -ps >> {pids}\n printf '\\033]777;rodtty;{activity_tag};A;%s\\007' \"${{__rodtty_prompt_exit:-0}}\"\n}}\n",
            jobs = quote(&jobs)?, pids = quote(&job_pids)?,
        ));
        // The status entry runs before every owner entry and returns `$?`
        // unchanged, so exit-status-aware prompts and owner hooks agree.
        script.push_str("PROMPT_COMMAND=('__rodtty_prompt_status' \"${PROMPT_COMMAND[@]}\")\n");
        script.push_str(&format!(
            "PROMPT_COMMAND+=('{history_sync}printf \"\\033]7;file://localhost%s\\007\" \"$PWD\"; __rodtty_prompt_activity; __rodtty_prompt_update')\n"
        ));
        std::fs::write(&rc, script)?;
        Ok(Self {
            directory,
            rc,
            path,
            disabled: config.disabled.join(","),
            jobs,
            job_pids,
            activity_tag,
            prompt,
        })
    }
}
