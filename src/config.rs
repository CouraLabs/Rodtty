use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Color family and brightness are independent owner preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFamily {
    #[default]
    Catppuccin,
    Dracula,
    TokyoNight,
    Nord,
    Gruvbox,
    OneDarkPro,
    NightOwl,
    Ayu,
    Solarized,
}
impl ThemeFamily {
    pub const ALL: &[Self] = &[
        Self::Catppuccin,
        Self::Dracula,
        Self::TokyoNight,
        Self::Nord,
        Self::Gruvbox,
        Self::OneDarkPro,
        Self::NightOwl,
        Self::Ayu,
        Self::Solarized,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Catppuccin => "Catppuccin",
            Self::Dracula => "Dracula",
            Self::TokyoNight => "Tokyo Night",
            Self::Nord => "Nord",
            Self::Gruvbox => "Gruvbox",
            Self::OneDarkPro => "One Dark Pro",
            Self::NightOwl => "Night Owl",
            Self::Ayu => "Ayu",
            Self::Solarized => "Solarized",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Catppuccin => "catppuccin",
            Self::Dracula => "dracula",
            Self::TokyoNight => "tokyonight",
            Self::Nord => "nord",
            Self::Gruvbox => "gruvbox",
            Self::OneDarkPro => "onedarkpro",
            Self::NightOwl => "nightowl",
            Self::Ayu => "ayu",
            Self::Solarized => "solarized",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}
impl Appearance {
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::System => system_dark,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub theme_family: ThemeFamily,
    pub appearance: Appearance,
    pub disabled: Vec<String>,
    pub scrollback: usize,
    pub history: bool,
    pub single_click_keeps_open: bool,
    pub fonts: FontPreferences,
    pub explorer: ExplorerPreferences,
    pub behavior: BehaviorPreferences,
    /// Preserve preferences owned by other feature modules and future versions.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExplorerPreferences {
    pub follow_terminal: bool,
    pub show_hidden: bool,
    pub respect_ignore: bool,
    pub confirm_deletion: bool,
}
impl Default for ExplorerPreferences {
    fn default() -> Self {
        Self {
            follow_terminal: false,
            show_hidden: false,
            respect_ignore: true,
            confirm_deletion: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme_family: ThemeFamily::default(),
            appearance: Appearance::default(),
            disabled: Vec::new(),
            scrollback: 10_000,
            history: true,
            single_click_keeps_open: false,
            fonts: FontPreferences::default(),
            explorer: ExplorerPreferences::default(),
            behavior: BehaviorPreferences::default(),
            extra: Default::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if !(100..=100_000).contains(&self.scrollback) {
            bail!("scrollback must be between 100 and 100000 lines");
        }
        self.fonts.validate()?;
        let utilities = brush_coreutils_builtins::bundled_commands();
        for name in &self.disabled {
            if !utilities.contains_key(name) {
                bail!("unknown bundled utility: {name}");
            }
        }
        Ok(())
    }

    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("settings.json");
        if !path.exists() && !root.join(".storage-migrated-v1").exists() {
            let legacy = root.join("config.toml");
            if legacy.exists() {
                let config: Self = toml::from_str(&std::fs::read_to_string(&legacy)?)
                    .with_context(|| format!("invalid {}", legacy.display()))?;
                config.validate()?;
                write_json(&path, &config, false)?;
            }
        }
        let config: Self = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?)
                .with_context(|| format!("invalid {}", path.display()))?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        write_json(&root.join("settings.json"), self, true)
    }
}

/// Atomically publish a complete JSON document. Migration uses no-clobber
/// persistence so an existing owner destination always wins, including races.
pub(crate) fn write_json(path: &Path, value: &impl Serialize, replace: bool) -> Result<()> {
    use std::io::Write;
    let root = path.parent().context("settings path has no parent")?;
    std::fs::create_dir_all(root)?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    if replace {
        file.persist(path)?;
    } else if let Err(error) = file.persist_noclobber(path)
        && error.error.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(error.into());
    }
    Ok(())
}

pub fn app_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("RODTTY_HOME") {
        return Ok(PathBuf::from(root));
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".rodtty"))
        .context("could not find the owner's home directory")
}

/// The previous coordination location must remain locked for compatibility
/// with a surviving host from before the storage migration.
pub fn legacy_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("RODTTY_HOME") {
        return Ok(PathBuf::from(root));
    }
    directories::ProjectDirs::from("dev", "CouraLabs", "Rodtty")
        .map(|dirs| dirs.config_dir().to_owned())
        .context("could not find the owner's configuration directory")
}

/// Called after acquiring both owner locks. Originals and existing destinations
/// are retained; session directories are deliberately never copied.
pub fn migrate_storage(root: &Path, legacy: &Path) -> Result<()> {
    let marker = root.join(".storage-migrated-v1");
    if marker.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    if root != legacy {
        for name in ["config.toml", "workspace.toml", "startup.sh", "history"] {
            let source = legacy.join(name);
            let destination = root.join(name);
            if source.is_file() && !destination.exists() {
                let mut file = tempfile::NamedTempFile::new_in(root)?;
                std::io::copy(&mut std::fs::File::open(&source)?, &mut file)?;
                file.as_file().sync_all()?;
                if let Err(error) = file.persist_noclobber(&destination)
                    && error.error.kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(error.into());
                }
            }
        }
    }
    if !root.join("settings.json").exists() {
        Config::load(root)?;
    }
    if !root.join("workspace-state.json").exists() {
        crate::work::Layout::load(root)?;
    }
    write_json(&marker, &1, false)
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

/// Owner typography. The editor uses the terminal family with its own size.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct FontPreferences {
    pub ui_family: String,
    pub terminal_family: String,
    pub ui_size: f32,
    pub terminal_size: f32,
    pub editor_size: f32,
}
impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            ui_family: "Geist".into(),
            terminal_family: "Geist Mono".into(),
            ui_size: 13.0,
            terminal_size: 14.0,
            editor_size: 14.0,
        }
    }
}
impl FontPreferences {
    fn validate(&self) -> Result<()> {
        for (name, size) in [
            ("UI", self.ui_size),
            ("terminal", self.terminal_size),
            ("editor", self.editor_size),
        ] {
            if !size.is_finite() || !(8.0..=32.0).contains(&size) {
                bail!("{name} font size must be between 8 and 32 pixels");
            }
        }
        for family in [&self.ui_family, &self.terminal_family] {
            if family.trim().is_empty()
                || family.len() > 256
                || family.chars().any(char::is_control)
            {
                bail!("font family must be a nonempty name of at most 256 bytes");
            }
        }
        Ok(())
    }
    pub fn families(&self) -> (String, String) {
        let list = |family: &str, fallback: &str| {
            let quoted = serde_json::to_string(family).expect("font name is a string");
            format!("{quoted}, \"{fallback}\"")
        };
        (
            list(&self.ui_family, "Geist"),
            list(&self.terminal_family, "Geist Mono"),
        )
    }
}

/// Session lifecycle choices apply immediately; session command configuration
/// remains an immutable snapshot taken when the session starts.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BehaviorPreferences {
    pub window_close: WindowClosePolicy,
    pub terminal_tab_close: TerminalClosePolicy,
    pub new_terminal_directory: NewTerminalDirectory,
    pub specified_directory: PathBuf,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowClosePolicy {
    #[default]
    KeepSessionsRunning,
    AskEachTime,
    QuitRodtty,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalClosePolicy {
    #[default]
    ConfirmRunning,
    AlwaysConfirm,
    CloseImmediately,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NewTerminalDirectory {
    #[default]
    Active,
    Explorer,
    Specified,
}
