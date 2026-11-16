use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub disabled: Vec<String>,
    pub scrollback: usize,
    pub history: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            scrollback: 10_000,
            history: true,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if !(100..=100_000).contains(&self.scrollback) {
            bail!("scrollback must be between 100 and 100000 lines");
        }
        let utilities = brush_coreutils_builtins::bundled_commands();
        for name in &self.disabled {
            if !utilities.contains_key(name) {
                bail!("unknown bundled utility: {name}");
            }
        }
        Ok(())
    }

    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.toml");
        let config = if path.exists() {
            toml::from_str(&std::fs::read_to_string(&path)?)
                .with_context(|| format!("invalid {}", path.display()))?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        use std::io::Write;
        self.validate()?;
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(root.join("config.toml"))?;
        Ok(())
    }
}

pub fn app_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("RODTTY_HOME") {
        return Ok(PathBuf::from(root));
    }
    directories::ProjectDirs::from("dev", "CouraLabs", "Rodtty")
        .map(|dirs| dirs.config_dir().to_owned())
        .context("could not find the owner's configuration directory")
}

pub fn initial_directory(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let path = path.canonicalize().context("invalid startup directory")?;
        if !path.is_dir() {
            bail!("startup path is not a directory: {}", path.display());
        }
        return Ok(path);
    }
    std::env::current_dir()
        .or_else(|_| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.home_dir().to_owned())
                .ok_or_else(|| std::io::Error::other("home directory unavailable"))
        })
        .context("no usable startup directory")
}

pub fn shell_executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("RODTTY_SHELL") {
        return PathBuf::from(path)
            .canonicalize()
            .context("invalid RODTTY_SHELL");
    }
    let path = std::env::current_exe()?
        .with_file_name(format!("rodtty-shell{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        bail!(
            "{} is missing; run cargo build --bins first",
            path.display()
        );
    }
    Ok(path)
}
