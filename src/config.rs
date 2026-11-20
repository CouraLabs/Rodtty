use crate::paths::Canonical as _;
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
    RosePine,
    Kanagawa,
    Everforest,
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
        Self::RosePine,
        Self::Kanagawa,
        Self::Everforest,
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
            Self::RosePine => "Rosé Pine",
            Self::Kanagawa => "Kanagawa",
            Self::Everforest => "Everforest",
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
            Self::RosePine => "rosepine",
            Self::Kanagawa => "kanagawa",
            Self::Everforest => "everforest",
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutosavePolicy {
    #[default]
    Explicit,
    AfterDelay,
    EditorFocusLoss,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AutosavePreferences {
    pub policy: AutosavePolicy,
    pub delay_ms: u64,
}
impl Default for AutosavePreferences {
    fn default() -> Self {
        Self {
            policy: AutosavePolicy::Explicit,
            delay_ms: 1000,
        }
    }
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
    pub autosave: AutosavePreferences,
    pub theme_family: ThemeFamily,
    pub appearance: Appearance,
    pub disabled: Vec<String>,
    pub scrollback: usize,
    pub history: bool,
    pub single_click_keeps_open: bool,
    pub fonts: FontPreferences,
    /// Stable action IDs mapped to owner bindings; an empty string unbinds an action.
    pub shortcuts: std::collections::BTreeMap<String, String>,
    pub explorer: ExplorerPreferences,
    pub behavior: BehaviorPreferences,
    pub terminal_clipboard: TerminalClipboardPreferences,
    /// Applies live: running Rodtty shells pick it up at their next prompt.
    pub prompt: crate::prompt::PromptPreferences,
    /// The strip along the window's bottom with the app name, folder and messages.
    pub show_footer: bool,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalRightClick {
    #[default]
    ContextMenu,
    Paste,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct TerminalClipboardPreferences {
    pub right_click: TerminalRightClick,
    /// On unless the owner saved `false`: a finished, nonempty terminal
    /// selection is copied to the clipboard.
    pub copy_on_selection: bool,
}
impl Default for TerminalClipboardPreferences {
    fn default() -> Self {
        Self {
            right_click: TerminalRightClick::default(),
            copy_on_selection: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            autosave: AutosavePreferences::default(),
            theme_family: ThemeFamily::default(),
            appearance: Appearance::default(),
            disabled: Vec::new(),
            scrollback: 10_000,
            history: true,
            single_click_keeps_open: false,
            fonts: FontPreferences::default(),
            shortcuts: Default::default(),
            explorer: ExplorerPreferences::default(),
            behavior: BehaviorPreferences::default(),
            terminal_clipboard: TerminalClipboardPreferences::default(),
            prompt: Default::default(),
            show_footer: true,
            extra: Default::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if !(100..=60_000).contains(&self.autosave.delay_ms) {
            bail!("autosave delay must be between 100 and 60000 milliseconds");
        }
        if !(100..=100_000).contains(&self.scrollback) {
            bail!("scrollback must be between 100 and 100000 lines");
        }
        self.fonts.validate()?;
        for binding in self.shortcuts.values() {
            validate_shortcut(binding)?;
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
        let path = path.canonical().context("invalid startup directory")?;
        if !path.is_dir() {
            bail!("startup path is not a directory: {}", path.display());
        }
        return Ok(path);
    }
    // Launchers start apps in `/` (Finder) or the install folder (Start menu), so
    // the first terminal opens in the home folder unless `--cwd` names another.
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_owned())
        .filter(|home| home.is_dir())
        .map_or_else(std::env::current_dir, Ok)
        .context("no usable startup directory")
}

/// The Startup folder setting's choice, when it names one. `Err` explains why
/// the first terminal falls back to the home folder instead.
pub fn configured_startup_directory(
    behavior: &BehaviorPreferences,
) -> Option<std::result::Result<PathBuf, String>> {
    if behavior.startup_directory != StartupDirectory::Specified {
        return None;
    }
    let path = &behavior.startup_specified_directory;
    Some(if path.as_os_str().is_empty() {
        Err(
            "No startup folder is specified in General settings. Opened the home folder instead."
                .into(),
        )
    } else if path.is_absolute() && path.is_dir() {
        Ok(path.clone())
    } else {
        Err(format!(
            "Startup folder is unavailable: {}. Opened the home folder instead.",
            path.display()
        ))
    })
}

pub fn shell_executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("RODTTY_SHELL") {
        return PathBuf::from(path)
            .canonical()
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

/// The allowed terminal line height, as multiples of the font size.
pub const TERMINAL_LINE_HEIGHT: (f32, f32) = (1.0, 2.5);
/// The allowed terminal letter spacing, in points per cell.
pub const TERMINAL_LETTER_SPACING: (f32, f32) = (-2.0, 10.0);

/// Owner typography. The editor uses the terminal family with its own size.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct FontPreferences {
    pub ui_family: String,
    pub terminal_family: String,
    pub ui_size: f32,
    pub terminal_size: f32,
    pub editor_size: f32,
    /// Terminal row height as a multiple of the terminal font size.
    pub terminal_line_height: f32,
    /// Points added to each terminal cell's width; negative values tighten it.
    pub terminal_letter_spacing: f32,
}
impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            ui_family: "Geist".into(),
            terminal_family: "Geist Mono".into(),
            ui_size: 13.0,
            terminal_size: 14.0,
            editor_size: 14.0,
            terminal_line_height: 1.2,
            terminal_letter_spacing: 0.0,
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
        if !self.terminal_line_height.is_finite()
            || !(TERMINAL_LINE_HEIGHT.0..=TERMINAL_LINE_HEIGHT.1)
                .contains(&self.terminal_line_height)
        {
            bail!(
                "terminal line height must be between {} and {}",
                TERMINAL_LINE_HEIGHT.0,
                TERMINAL_LINE_HEIGHT.1
            );
        }
        if !self.terminal_letter_spacing.is_finite()
            || !(TERMINAL_LETTER_SPACING.0..=TERMINAL_LETTER_SPACING.1)
                .contains(&self.terminal_letter_spacing)
        {
            bail!(
                "terminal letter spacing must be between {} and {} points",
                TERMINAL_LETTER_SPACING.0,
                TERMINAL_LETTER_SPACING.1
            );
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

/// The preference format is shared with backend builds, without a desktop dependency.
fn validate_shortcut(binding: &str) -> Result<()> {
    if binding.is_empty() {
        return Ok(());
    }
    let parts: Vec<_> = binding.split('+').map(str::trim).collect();
    let key = parts.last().unwrap().to_ascii_lowercase();
    let named = [
        "enter",
        "tab",
        "escape",
        "backspace",
        "delete",
        "insert",
        "home",
        "end",
        "pageup",
        "pagedown",
        "left",
        "right",
        "up",
        "down",
        "space",
        "plus",
        "comma",
        "minus",
        "contextmenu",
        "printscreen",
        "scrolllock",
        "pause",
        "mousemiddle",
        "mouseback",
        "mouseforward",
    ];
    let function = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    let mut seen = std::collections::BTreeSet::new();
    let invalid_modifier = parts[..parts.len() - 1].iter().any(|m| {
        let modifier = m.to_ascii_lowercase();
        let modifier = if modifier == "cmd" {
            "super"
        } else {
            modifier.as_str()
        };
        !["ctrl", "alt", "shift", "super", "mod"].contains(&modifier)
            || !seen.insert(modifier.to_owned())
    });
    let modified_mouse = key.starts_with("mouse") && parts.len() != 1;
    if !(key.chars().count() == 1 && !key.chars().any(char::is_control)
        || named.contains(&key.as_str())
        || function)
        || invalid_modifier
        || modified_mouse
    {
        bail!("invalid shortcut: {binding}");
    }
    Ok(())
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
    /// Where the first terminal opens; `--cwd` overrides it.
    pub startup_directory: StartupDirectory,
    /// The folder for `StartupDirectory::Specified`.
    pub startup_specified_directory: PathBuf,
    pub terminal_startup: TerminalStartup,
    /// Preview multiline terminal pastes and wait for Paste or Cancel.
    pub confirm_multiline_paste: bool,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupDirectory {
    #[default]
    Home,
    Specified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStartup {
    #[default]
    RestoreWorkspace,
    SingleFresh,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn load(json: &str) -> Config {
        let root = crate::test_support::tempdir().unwrap();
        std::fs::write(root.path().join("settings.json"), json).unwrap();
        Config::load(root.path()).unwrap()
    }

    #[test]
    fn first_terminal_defaults_to_home_and_honors_explicit_cwd() {
        let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
        assert_eq!(initial_directory(None).unwrap(), home);
        let explicit = crate::test_support::tempdir().unwrap();
        assert_eq!(
            initial_directory(Some(explicit.path().to_owned())).unwrap(),
            explicit.path().canonical().unwrap()
        );
    }

    #[test]
    fn startup_folder_setting_names_a_folder_or_explains_the_home_fallback() {
        let folder = crate::test_support::tempdir().unwrap();
        let mut behavior = BehaviorPreferences::default();
        assert_eq!(behavior.startup_directory, StartupDirectory::Home);
        assert_eq!(configured_startup_directory(&behavior), None);
        behavior.startup_directory = StartupDirectory::Specified;
        assert!(configured_startup_directory(&behavior).unwrap().is_err());
        behavior.startup_specified_directory = folder.path().into();
        assert_eq!(
            configured_startup_directory(&behavior),
            Some(Ok(folder.path().to_owned()))
        );
        behavior.startup_specified_directory = folder.path().join("missing");
        assert!(
            configured_startup_directory(&behavior)
                .unwrap()
                .unwrap_err()
                .contains("missing")
        );
        behavior.startup_specified_directory = "relative".into();
        assert!(configured_startup_directory(&behavior).unwrap().is_err());
        let saved: BehaviorPreferences = serde_json::from_str(
            r#"{"startup_directory":"specified","startup_specified_directory":"/tmp"}"#,
        )
        .unwrap();
        assert_eq!(saved.startup_directory, StartupDirectory::Specified);
    }

    #[test]
    fn copy_on_selection_defaults_on_and_honors_saved_choices() {
        let root = crate::test_support::tempdir().unwrap();
        assert!(
            Config::load(root.path())
                .unwrap()
                .terminal_clipboard
                .copy_on_selection,
            "no settings.json"
        );
        assert!(load("{}").terminal_clipboard.copy_on_selection);
        assert!(
            load(r#"{"terminal_clipboard":{"right_click":"paste"}}"#)
                .terminal_clipboard
                .copy_on_selection,
            "the preference itself is absent"
        );
        assert!(
            !load(r#"{"terminal_clipboard":{"copy_on_selection":false}}"#)
                .terminal_clipboard
                .copy_on_selection
        );
        assert!(
            load(r#"{"terminal_clipboard":{"copy_on_selection":true}}"#)
                .terminal_clipboard
                .copy_on_selection
        );
    }
}
