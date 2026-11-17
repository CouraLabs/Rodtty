//! Owner preferences shown as a dedicated editor tab.
use super::*;
use std::time::{Duration, Instant};

impl State {
    pub(super) fn open_settings(&mut self, tx: &mut Tx) {
        self.workspace.explorer_revealed = false;
        self.settings_open = true;
        self.settings = true;
        self.workspace.layout.editor = true;
        self.workspace.terminal_focus = false;
        tx.focus_when_shown("settings-search", true);
    }

    pub(super) fn open_settings_json(&mut self) {
        let path = self.root.join("settings.json");
        if !path.exists()
            && let Err(error) = self.config.save(&self.root)
        {
            self.message = format!("Could not create Settings JSON: {error:#}");
            return;
        }
        self.open_file(path);
    }

    pub(super) fn poll_settings(&mut self) -> bool {
        if self.settings_poll.elapsed() < Duration::from_millis(500) {
            return false;
        }
        self.settings_poll = Instant::now();
        let next = match std::fs::read(self.root.join("settings.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                self.settings_error = Some(format!("Could not read Settings JSON: {error}"));
                return true;
            }
        };
        if next == self.settings_seen {
            return false;
        }
        self.settings_seen = next;
        match Config::load(&self.root) {
            Ok(config) => {
                self.config = config.clone();
                self.last_valid_config = config;
                self.config_valid = true;
                self.settings_error = None;
                self.message =
                    "Settings JSON updated. Terminal configuration applies to new sessions.".into();
            }
            Err(error) => {
                let error =
                    format!("Invalid Settings JSON; keeping last valid preferences: {error:#}");
                self.message = error.clone();
                self.settings_error = Some(error);
            }
        }
        true
    }
}

fn matches(s: &State, section: &str, name: &str) -> bool {
    let query = s.settings_query.text.trim().to_lowercase();
    format!("{section} {name}").to_lowercase().contains(&query)
}

pub(super) fn content(s: &State, a: &Appearance) -> El {
    let mut page = col()
        .key("settings-page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(12.0)
        .py(12.0)
        .gap(10.0)
        .child(
            text("Settings")
                .font(TextStyle::new(18.0, 600.0))
                .color(a.ink()),
        )
        .child(
            text_input(&s.settings_query, LABEL)
                .caret(a.accent(), a.selection())
                .key("settings-search")
                .label("Search settings")
                .w_full()
                .h(30.0)
                .color(a.ink())
                .placeholder("Search settings", a.muted())
                .on_edit(|s: &mut State| &mut s.settings_query),
        )
        .child(button(
            a,
            "open-settings-json",
            "Open Settings JSON",
            |s, tx| s.dispatch(AppAction::OpenSettingsJson, tx),
        ));
    if let Some(error) = &s.settings_error {
        page = page.child(text(error).font(LABEL).color(a.danger()).wrap_anywhere());
    }
    let mut list = col()
        .key("settings-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(12.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    let mut count = 0;
    for section in [
        "Appearance",
        "Behavior",
        "Keyboard Shortcuts",
        "Terminal",
        "Editor",
        "Files",
        "Bundled Commands",
    ] {
        let visible = if section == "Appearance" {
            matches(
                s,
                section,
                "theme family Catppuccin Latte Mocha Light Dark System palette",
            )
        } else if section == "Terminal" {
            matches(s, section, "history save shell commands")
                || matches(s, section, "scrollback retained lines")
        } else if section == "Files" {
            matches(
                s,
                section,
                "explorer follow terminal show hidden files respect ignore rules",
            )
        } else if section == "Bundled Commands" {
            profile::utility_names()
                .iter()
                .any(|name| matches(s, section, name))
        } else {
            matches(s, section, "")
        };
        if !visible {
            continue;
        }
        count += 1;
        let mut group = col()
            .key(format!("settings-section-{section}"))
            .w_full()
            .gap(6.0)
            .child(
                text(section)
                    .font(TextStyle::new(13.0, 600.0))
                    .color(a.accent()),
            );
        match section {
            "Appearance" => {
                use crate::config::{Appearance as Mode, ThemeFamily};
                let mut families = row().gap(6.0).scroll_x();
                for &family in ThemeFamily::ALL {
                    families = families.child(
                        button(
                            a,
                            format!("theme-family-{}", family.label()),
                            family.label(),
                            move |s, _| s.save_config(|c| c.theme_family = family),
                        )
                        .role("radio")
                        .checked(s.config.theme_family == family)
                        .border(
                            1.0,
                            if s.config.theme_family == family {
                                a.accent()
                            } else {
                                a.panel()
                            },
                        ),
                    );
                }
                group = group
                    .child(text("Theme family").font(LABEL).color(a.muted()))
                    .child(families)
                    .child(button(a, "reset-theme-family", "Reset family", |s, _| {
                        s.save_config(|c| c.theme_family = Config::default().theme_family);
                    }));
                let mut choices = row().gap(6.0).scroll_x();
                for (key, label, mode) in [
                    ("light", "Light", Mode::Light),
                    ("dark", "Dark", Mode::Dark),
                    ("system", "System", Mode::System),
                ] {
                    choices = choices.child(
                        button(a, format!("appearance-{key}"), label, move |s, _| {
                            s.save_config(|c| c.appearance = mode)
                        })
                        .role("radio")
                        .checked(s.config.appearance == mode)
                        .border(
                            1.0,
                            if s.config.appearance == mode {
                                a.accent()
                            } else {
                                a.panel()
                            },
                        ),
                    );
                }
                group = group.child(choices)
                    .child(text(format!("{} · applies immediately", a.name)).font(LABEL).color(a.muted()).wrap())
                    .child(palette_preview(s.config.theme_family, Mode::Light))
                    .child(palette_preview(s.config.theme_family, Mode::Dark))
                    .child(button(a, "reset-appearance", "Reset appearance", |s, _| {
                        s.save_config(|c| c.appearance = Config::default().appearance);
                    }))
                    .child(text("Catppuccin palette © 2021 Catppuccin · MIT · github.com/catppuccin/palette")
                        .font(LABEL).color(a.muted()).wrap_anywhere());
            }
            "Terminal" => {
                if matches(s, section, "history save shell commands") {
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(button(
                                a,
                                "history",
                                format!("History: {}", if s.config.history { "on" } else { "off" }),
                                |s, _| {
                                    s.save_config(|config| config.history = !config.history);
                                },
                            ))
                            .child(button(a, "reset-history", "Reset", |s, _| {
                                s.save_config(|config| config.history = Config::default().history);
                            })),
                    );
                }
                if matches(s, section, "scrollback retained lines") {
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .scroll_x()
                            .child(
                                text(format!("Scrollback: {} lines", s.config.scrollback))
                                    .font(LABEL)
                                    .color(a.ink()),
                            )
                            .child(button(a, "less-scrollback", "− 1000", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback =
                                        config.scrollback.saturating_sub(1000).max(100)
                                });
                            }))
                            .child(button(a, "more-scrollback", "+ 1000", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback = (config.scrollback + 1000).min(100_000)
                                });
                            }))
                            .child(button(a, "reset-scrollback", "Reset", |s, _| {
                                s.save_config(|config| {
                                    config.scrollback = Config::default().scrollback
                                });
                            })),
                    );
                }
                group = group.child(
                    text(
                        "Command, history, and scrollback choices apply to new terminal sessions.",
                    )
                    .font(LABEL)
                    .color(a.muted())
                    .wrap(),
                );
            }
            "Files" => {
                for (key, title, enabled, action) in [
                    (
                        "settings-explorer-follow",
                        "Follow Terminal",
                        s.config.explorer.follow_terminal,
                        AppAction::ToggleFollow,
                    ),
                    (
                        "settings-explorer-hidden",
                        "Show hidden files",
                        s.config.explorer.show_hidden,
                        AppAction::ToggleHidden,
                    ),
                    (
                        "settings-explorer-ignore",
                        "Respect Git ignore rules",
                        s.config.explorer.respect_ignore,
                        AppAction::ToggleIgnore,
                    ),
                ] {
                    if matches(s, section, title) {
                        group = group.child(button(
                            a,
                            key,
                            format!("{title}: {}", if enabled { "on" } else { "off" }),
                            move |s, tx| s.dispatch(action, tx),
                        ));
                    }
                }
                group = group.child(button(
                    a,
                    "reset-explorer-preferences",
                    "Reset file preferences",
                    |s, _| {
                        s.save_config(|config| config.explorer = Default::default());
                    },
                ));
            }
            "Bundled Commands" => {
                for name in profile::utility_names() {
                    if !matches(s, section, &name) {
                        continue;
                    }
                    let enabled = !s.config.disabled.contains(&name);
                    let reset = name.clone();
                    group = group.child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(button(
                                a,
                                format!("utility-{name}"),
                                format!("{} {name}", if enabled { "✓" } else { "−" }),
                                move |s, _| {
                                    s.save_config(|config| {
                                        if config.disabled.contains(&name) {
                                            config.disabled.retain(|n| n != &name);
                                        } else {
                                            config.disabled.push(name.clone());
                                        }
                                    });
                                },
                            ))
                            .child(button(
                                a,
                                format!("reset-utility-{reset}"),
                                "Reset",
                                move |s, _| {
                                    s.save_config(|config| config.disabled.retain(|n| n != &reset));
                                },
                            )),
                    );
                }
            }
            _ => {
                group = group.child(
                    text("No configurable preferences in this section yet.")
                        .font(LABEL)
                        .color(a.muted())
                        .wrap(),
                );
            }
        }
        list = list.child(group);
    }
    if count == 0 {
        list = list.child(
            text("No settings match your search.")
                .font(LABEL)
                .color(a.muted()),
        );
    }
    page.child(list)
}

fn palette_preview(family: crate::config::ThemeFamily, mode: crate::config::Appearance) -> El {
    let p = Appearance::resolve(family, mode, false);
    let mut swatches = row().gap(3.0);
    for i in 0..8 {
        swatches = swatches.child(
            el().size(16.0, 10.0)
                .radius(2.0)
                .fill(p.terminal(vt100::Color::Idx(i + 1), true)),
        );
    }
    col()
        .key(format!(
            "palette-preview-{}",
            if p.dark { "dark" } else { "light" }
        ))
        .w_full()
        .min_w(0.0)
        .px(10.0)
        .py(8.0)
        .gap(4.0)
        .radius(6.0)
        .fill(p.bg())
        .border(1.0, p.kit().c.border_base)
        .child(text(p.name).font(LABEL).color(p.ink()).wrap())
        .child(
            text("rodtty:~/projects $ cargo build")
                .font(LABEL)
                .color(p.accent())
                .wrap(),
        )
        .child(swatches)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime(root: &std::path::Path) -> rune::Runtime<State> {
        let mut state = State::with_root(root.to_owned());
        state.workspace_setup();
        let mut runtime = rune::Runtime::new(
            state,
            view,
            (1200.0, 800.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        runtime.frame(0.0);
        runtime
    }
    fn click(runtime: &mut rune::Runtime<State>, key: &str) {
        let (x, y) = runtime
            .find(key)
            .unwrap_or_else(|| panic!("missing control {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
    }
    fn assert_canvas(runtime: &rune::Runtime<State>, hex: &str) {
        let c = rune::theme::hex(hex).unwrap();
        let actual = runtime.clear_color();
        assert!(
            actual
                .iter()
                .zip([c.r, c.g, c.b, c.a])
                .all(|(a, b)| (a - b).abs() < 0.0001),
            "canvas {actual:?}, expected {hex}"
        );
    }
    #[test]
    fn system_tracks_live_host_changes_while_explicit_appearance_stays_fixed() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        assert_canvas(&runtime, "#1e1e2e");
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-dark");
        runtime.set_system_dark(true);
        runtime.frame(0.0);
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#1e1e2e");
        click(&mut runtime, "appearance-light");
        runtime.set_system_dark(true);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "reset-appearance");
        assert_canvas(&runtime, "#1e1e2e");
        runtime.set_system_dark(false);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        assert_eq!(
            Config::load(root.path()).unwrap().appearance,
            crate::config::Appearance::System
        );
        runtime.update(|s, _| s.settings_query.replace_text("latte"));
        runtime.frame(0.0);
        assert!(runtime.find("appearance-system").is_some());
    }
    #[test]
    fn switching_appearance_recolors_ansi_but_preserves_extended_output_and_selection() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        runtime.update(|s, _| {
            let mut parser = vt100::Parser::new(8, 30, 0);
            parser.process(b"\x1b[41m \x1b[48;5;196m \x1b[48;2;12;34;56m \x1b[0m output stays");
            s.preview = Some(parser.screen().clone());
            s.selection = Some(((1, 0), (1, 5)));
        });
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        let cell_background = |runtime: &rune::Runtime<State>, col: usize| {
            let r = runtime.presented("terminal").unwrap().0;
            let x = r[0] + (col as f32 + 0.5) * CW;
            runtime
                .instances
                .iter()
                .find(|i| {
                    (i.rect[1] - r[1]).abs() < 0.01
                        && i.rect[3] == CH
                        && i.rect[0] <= x
                        && x < i.rect[0] + i.rect[2]
                        && i.rect[2] < 2.0 * CW
                })
                .expect("terminal cell background")
                .fill
        };
        let rgb = |hex: &str| {
            let c = rune::theme::hex(hex).unwrap();
            [c.r, c.g, c.b, c.a]
        };
        assert_eq!(cell_background(&runtime, 0), rgb("#f38ba8"));
        assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
        assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-light");
        assert_eq!(cell_background(&runtime, 0), rgb("#d20f39"));
        assert_eq!(cell_background(&runtime, 1), rgb("#ff0000"));
        assert_eq!(cell_background(&runtime, 2), rgb("#0c2238"));
        assert!(
            runtime
                .state
                .with_screen(|screen| screen.contents().contains("output stays"))
        );
        assert_eq!(runtime.state.selection, Some(((1, 0), (1, 5))));
    }
    #[test]
    fn owner_changes_appearance_live_and_reopens_with_saved_choice() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = self::runtime(root.path());
        click(&mut runtime, "settings");
        click(&mut runtime, "appearance-light");
        assert_canvas(&runtime, "#eff1f5");
        click(&mut runtime, "appearance-dark");
        assert_canvas(&runtime, "#1e1e2e");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["appearance"], "dark");
        assert_eq!(saved["theme_family"], "catppuccin");
        let mut reopened = self::runtime(root.path());
        reopened.set_system_dark(false);
        for _ in 0..45 {
            reopened.frame(1.0 / 60.0);
        }
        assert_canvas(&reopened, "#1e1e2e");
    }
    #[test]
    fn owner_searches_resets_and_edits_actual_json_beside_the_terminal() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        assert!(runtime.find("editor-pane").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        assert!(runtime.find("settings-tab").is_some());
        runtime.key(KeyEvent {
            key: Key::Text("history".into()),
            mods: Default::default(),
        });
        runtime.frame(0.0);
        assert!(runtime.find("history").is_some());
        assert!(runtime.find("utility-cat").is_none());
        click(&mut runtime, "history");
        assert!(
            !Config::load(root.path()).unwrap().history,
            "{}; active history {}",
            runtime.state.message,
            runtime.state.config.history
        );
        click(&mut runtime, "reset-history");
        assert!(Config::load(root.path()).unwrap().history);
        runtime.update(|s, _| s.settings_query.replace_text("cat"));
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "utility-cat");
        assert_eq!(Config::load(root.path()).unwrap().disabled, ["cat"]);
        click(&mut runtime, "reset-utility-cat");
        assert!(Config::load(root.path()).unwrap().disabled.is_empty());
        runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "more-scrollback");
        assert_eq!(Config::load(root.path()).unwrap().scrollback, 11_000);
        click(&mut runtime, "reset-scrollback");
        assert_eq!(Config::load(root.path()).unwrap().scrollback, 10_000);
        click(&mut runtime, "open-settings-json");
        let start = Instant::now();
        while runtime.state.workspace.tabs.is_empty() && start.elapsed() < Duration::from_secs(5) {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(0.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            runtime.state.workspace.tabs[0].document.path,
            root.path().join("settings.json")
        );
        assert!(runtime.find("file-editor-1").is_some());
        assert!(runtime.find("terminal-pane").is_some());
        runtime.update(|_, tx| tx.focus("file-editor-1", true));
        runtime.frame(1.0 / 60.0);
        runtime.key(KeyEvent {
            key: Key::Command('a'),
            mods: rune::Mods {
                cmd: true,
                ctrl: true,
                ..Default::default()
            },
        });
        runtime.key(KeyEvent {
            key: Key::Text(r#"{"history":false,"scrollback":3000}"#.into()),
            mods: Default::default(),
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "file-save");
        let start = Instant::now();
        while runtime.state.workspace.tabs[0].saving && start.elapsed() < Duration::from_secs(5) {
            runtime.update(|s, tx| {
                s.workspace_poll(tx);
            });
            runtime.frame(1.0 / 60.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        assert!(!Config::load(root.path()).unwrap().history);
        assert!(!runtime.state.config.history);
        assert_eq!(runtime.state.config.scrollback, 3000);
    }
    #[test]
    fn external_json_updates_preferences_but_invalid_edits_keep_the_last_valid_choices() {
        let root = tempfile::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        let path = root.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"appearance":"light","history":false,"scrollback":2000,"future":{"choice":7}}"#,
        )
        .unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        assert!(!runtime.state.config.history);
        assert_eq!(runtime.state.config.scrollback, 2000);
        for _ in 0..45 {
            runtime.frame(1.0 / 60.0);
        }
        assert_canvas(&runtime, "#eff1f5");
        std::fs::write(&path, "{invalid JSON").unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        assert!(!runtime.state.config.history);
        assert!(
            runtime
                .state
                .message
                .contains("keeping last valid preferences")
        );
        assert_canvas(&runtime, "#eff1f5");
        // A UI action must not destroy the invalid JSON being repaired externally.
        runtime.update(|s, _| {
            s.settings_query.replace_text("history");
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "reset-history");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{invalid JSON");
        assert!(!runtime.state.config.history);
        std::fs::write(&path, r#"{"history":false,"future":{"choice":7}}"#).unwrap();
        runtime.update(|s, tx| {
            s.settings_poll = Instant::now() - Duration::from_secs(1);
            poll(s, 0.0, tx);
        });
        runtime.frame(1.0 / 60.0);
        click(&mut runtime, "reset-history");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["history"], true);
        assert_eq!(saved["future"]["choice"], 7);
        assert!(runtime.state.settings_error.is_none());
    }
    #[test]
    fn a_ui_edit_preserves_unrelated_external_changes_before_the_next_poll() {
        let root = tempfile::tempdir().unwrap();
        Config::default().save(root.path()).unwrap();
        let mut runtime = runtime(root.path());
        click(&mut runtime, "settings");
        runtime.update(|s, _| s.settings_query.replace_text("scrollback"));
        runtime.frame(1.0 / 60.0);
        std::fs::write(
            root.path().join("settings.json"),
            r#"{"history":false,"scrollback":10000}"#,
        )
        .unwrap();
        click(&mut runtime, "more-scrollback");
        let saved = Config::load(root.path()).unwrap();
        assert!(!saved.history);
        assert_eq!(saved.scrollback, 11000);
    }
}
