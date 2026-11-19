//! Owner preferences shown as a dedicated editor tab.
use super::*;
use std::time::{Duration, Instant};

mod appearance;
mod catalog;
mod commands;
mod controls;
mod general;
mod layout;
mod model;
use appearance::*;
use catalog::*;
use commands::*;
use controls::*;
use general::*;
use layout::*;

/// The Settings page's tabs. Appearance holds everything that changes how
/// Rodtty looks; General holds the rest, grouped into fieldsets. A search
/// looks through every tab at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    General,
    Appearance,
    Shortcuts,
    Commands,
}

impl Tab {
    pub(super) const ALL: [Tab; 4] = [Tab::General, Tab::Appearance, Tab::Shortcuts, Tab::Commands];
    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Shortcuts => "Shortcuts",
            Tab::Commands => "Commands",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::Appearance => "appearance",
            Tab::Shortcuts => "shortcuts",
            Tab::Commands => "commands",
        }
    }
    /// Other words a search finds the tab by, such as the sections' former names.
    fn aliases(self) -> &'static str {
        match self {
            Tab::General => "Behavior",
            Tab::Appearance => "Theme",
            Tab::Shortcuts => "Keyboard Shortcuts",
            Tab::Commands => "Bundled Commands",
        }
    }
}

fn query(s: &State) -> String {
    s.settings_query.text.trim().to_lowercase()
}

/// Whether the search query occurs in the tab's names followed by `words`.
fn matches(s: &State, tab: Tab, words: &str) -> bool {
    format!("{} {} {words}", tab.label(), tab.aliases())
        .to_lowercase()
        .contains(&query(s))
}

type Build = fn(&State, &Appearance, &mut Vec<El>) -> El;

/// One setting: its name and explanation on the left, its control on the
/// right, and optionally content under both (a preview or a follow-up field).
struct Setting {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    /// What a search matches besides the tab's names, fieldset and label.
    words: &'static str,
    control: Build,
    below: Option<Build>,
}

/// A titled group of settings within a tab.
struct Fieldset {
    tab: Tab,
    key: &'static str,
    title: &'static str,
    settings: &'static [Setting],
    /// Rows built from data, already filtered by the search, for the lists
    /// of shortcuts and bundled commands.
    rows: Option<fn(&State, &Appearance) -> Vec<El>>,
}

impl Fieldset {
    fn shown_settings(&self, s: &State) -> Vec<&Setting> {
        self.settings
            .iter()
            .filter(|setting| {
                if query(s).is_empty() {
                    s.settings_tab == self.tab
                } else {
                    matches(
                        s,
                        self.tab,
                        &format!("{} {} {}", self.title, setting.label, setting.words),
                    )
                }
            })
            .collect()
    }
    fn shown_rows(&self, s: &State, a: &Appearance) -> Vec<El> {
        match self.rows {
            Some(rows) if !query(s).is_empty() || s.settings_tab == self.tab => rows(s, a),
            _ => Vec::new(),
        }
    }
}

const fn setting(key: &'static str, label: &'static str, control: Build) -> Setting {
    Setting {
        key,
        label,
        description: "",
        words: "",
        control,
        below: None,
    }
}

pub(super) fn content(s: &State, a: &Appearance) -> El {
    page(s, a, &mut Vec::new())
}

/// The open dropdown's or picker's list, drawn above the page.
pub(super) fn overlays(s: &State, a: &Appearance, width: f32) -> Vec<El> {
    let mut layers = Vec::new();
    if s.settings
        && !s.application_overlay_open()
        && pane_visibility(s, width).2
        && (s.settings_select.is_some()
            || s.theme_family_picker.is_open()
            || s.font_ui.is_open()
            || s.font_terminal.is_open())
    {
        let _ = page(s, a, &mut layers);
    }
    layers
}

/// Tests: restores every setting on the tab keyed `tab` (`general`,
/// `appearance`, …) through its Restore Defaults button and confirmation.
#[cfg(test)]
pub(super) fn restore_tab_defaults(rt: &mut rune::Runtime<State>, tab: &str) {
    for key in [
        format!("settings-page-tab-{tab}"),
        format!("reset-tab-{tab}"),
        "confirm-reset-tab".to_owned(),
    ] {
        let (x, y) = rt
            .find(&key)
            .unwrap_or_else(|| panic!("missing control {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
    }
}

/// Tests: opens the dropdown keyed `key` and chooses its option `label`, as a
/// pointer would.
#[cfg(test)]
pub(super) fn choose(rt: &mut rune::Runtime<State>, key: &str, label: &str) {
    let press = |rt: &mut rune::Runtime<State>, (x, y): (f32, f32)| {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
        }
    };
    let trigger = rt
        .find(&format!("{key}-trigger"))
        .unwrap_or_else(|| panic!("missing dropdown {key}"));
    press(rt, trigger);
    let option = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "option" && e.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("dropdown {key} has no option {label}"))
        .rect;
    press(
        rt,
        (option[0] + option[2] * 0.5, option[1] + option[3] * 0.5),
    );
}

#[cfg(test)]
mod tests;
