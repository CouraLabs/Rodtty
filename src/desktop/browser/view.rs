//! The owner's Browser view: toolbar, address field and the page surface,
//! plus the top-bar Open Browser icon.
use super::*;

pub(crate) static GLOBE: rune::Icon = rune::Icon {
    name: "rodtty-globe",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6.25" stroke="currentColor" fill="none"/><path d="M1.75 8H14.25M8 1.75C9.9 3.5 10.6 5.7 10.6 8C10.6 10.3 9.9 12.5 8 14.25C6.1 12.5 5.4 10.3 5.4 8C5.4 5.7 6.1 3.5 8 1.75Z" stroke="currentColor" fill="none" stroke-linejoin="round"/>"#,
};

/// The top-bar icon, placed right after Search and before File Explorer.
pub(crate) fn open_browser_button(s: &State, a: &Appearance) -> El {
    icon_button(
        a,
        "open-browser",
        &GLOBE,
        s.shortcut_help("open-browser", "Open Browser"),
        |s, tx| s.dispatch(AppAction::OpenBrowser, tx),
    )
    .label("Open Browser")
    .toggle_state(a, s.browsers.view_open)
}

impl State {
    pub(crate) fn browser_view_visible(&self) -> bool {
        self.browsers.view_open
    }

    pub(crate) fn shown_browser(&self) -> Option<&BrowserTarget> {
        let id = self.browsers.shown.as_deref()?;
        self.browsers.targets.iter().find(|t| t.id == id)
    }

    /// Open Browser: open or focus the one reusable Browser view. A repeat
    /// focuses the address field and keeps the page.
    pub(crate) fn open_browser_view(&mut self, tx: &mut Tx) {
        if self.browsers.view_open && self.shown_browser().is_some() {
            self.browsers.page_focus = false;
            tx.focus("browser-address", true);
            return;
        }
        self.browsers.view_open = true;
        if self.shown_browser().is_none() {
            let owner = self
                .browsers
                .targets
                .iter()
                .find(|t| t.client.is_none())
                .map(|t| t.id.clone());
            match owner {
                Some(id) => self.browsers.shown = Some(id),
                None => {
                    let (w, h) = self
                        .browsers
                        .view_size
                        .map_or((960, 640), |(w, h)| (w as u32, h as u32));
                    let scale = if self.browsers.view_scale > 0.0 {
                        self.browsers.view_scale
                    } else {
                        2.0
                    };
                    match self.create_browser(None, None, (w.max(200), h.max(200)), scale) {
                        Ok(id) => self.browsers.shown = Some(id),
                        Err(error) => {
                            self.browsers.status =
                                Some(format!("{error}. The terminal and editor are unaffected."))
                        }
                    }
                }
            }
        }
        let url = self
            .shown_browser()
            .map(|t| t.shared.lock().url.clone())
            .unwrap_or_default();
        self.browsers.address = Some(TextEdit::new(if url == "about:blank" {
            String::new()
        } else {
            url
        }));
        self.browsers.page_focus = false;
        self.workspace.set_terminal_focus(false);
        tx.focus("browser-address", true);
    }

    /// Close the view and release the owner's browser; agent targets keep running.
    pub(crate) fn close_browser_view(&mut self, tx: &mut Tx) {
        self.browsers.view_open = false;
        self.browsers.page_focus = false;
        if let Some(id) = self.browsers.shown.take()
            && self
                .browsers
                .targets
                .iter()
                .any(|t| t.id == id && t.client.is_none())
        {
            self.close_browser_target(&id);
        }
        self.workspace.set_terminal_focus(true);
        tx.blur();
    }

    pub(crate) fn browser_go(&mut self) {
        let input = self
            .browsers
            .address
            .as_ref()
            .map(|a| a.text.clone())
            .unwrap_or_default();
        match address::normalize(&input) {
            Ok(url) => {
                if let Some(target) = self.shown_browser() {
                    target.navigate(&url);
                    target.shared.lock().blocked = None;
                    self.browsers.page_focus = true;
                }
                self.browsers.status = None;
            }
            Err(error) => self.browsers.status = Some(error),
        }
    }

    fn owner_browser_input(&mut self) {
        if let Some(id) = self.browsers.shown.clone() {
            self.owner_input(&id);
        }
    }

    /// The owner's keys while the page has focus.
    pub(crate) fn browser_owner_key(&mut self, event: &KeyEvent) -> bool {
        if !self.browsers.view_open || !self.browsers.page_focus {
            return false;
        }
        self.owner_browser_input();
        let Some(target) = self.shown_browser() else {
            return false;
        };
        let name = match &event.key {
            Key::Text(t) => t.clone(),
            Key::Command(c) => c.to_string(),
            Key::Enter => "Enter".into(),
            Key::Backspace => "Backspace".into(),
            Key::Delete => "Delete".into(),
            Key::Escape => "Escape".into(),
            Key::Tab => "Tab".into(),
            Key::Up => "Up".into(),
            Key::Down => "Down".into(),
            Key::Left => "Left".into(),
            Key::Right => "Right".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            _ => return false,
        };
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && !event.mods.ctrl
            && !event.mods.cmd
            && !c.is_ascii()
        {
            for e in char_events(c) {
                target.key(e);
            }
            return true;
        }
        let Some(key) = crate::automation::keys::key_name(&name) else {
            return false;
        };
        let chord = crate::automation::keys::Chord {
            key,
            mods: crate::automation::keys::Modifiers {
                shift: event.mods.shift,
                ctrl: event.mods.ctrl,
                alt: event.mods.alt,
                meta: event.mods.logo || (cfg!(target_os = "macos") && event.mods.cmd),
            },
        };
        for e in key_events(&chord, true, true) {
            target.key(e);
        }
        true
    }

    pub(crate) fn browser_owner_text(&mut self, text: &str) -> bool {
        if !self.browsers.view_open || !self.browsers.page_focus {
            return false;
        }
        self.owner_browser_input();
        if let Some(target) = self.shown_browser() {
            for c in text.chars() {
                for e in char_events(c) {
                    target.key(e);
                }
            }
        }
        true
    }
}

fn toolbar_button(
    a: &Appearance,
    key: &'static str,
    glyph: &'static rune::Icon,
    label: &'static str,
    enabled: bool,
    action: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    icon_button(a, key, glyph, label, move |s, tx| {
        if enabled {
            action(s, tx)
        }
    })
    .label(label)
    .opacity(if enabled { 1.0 } else { 0.4 })
}

pub(crate) fn browser_pane(s: &State, a: &Appearance) -> El {
    let target = s.shown_browser();
    let page = target.map(|t| {
        let p = t.shared.lock();
        (
            p.url.clone(),
            p.loading,
            p.can_back,
            p.can_forward,
            p.error.clone(),
            p.crashed.clone(),
            p.blocked.clone(),
            t.recording.is_some(),
        )
    });
    let (_, loading, back, forward, error, crashed, blocked, recording) =
        page.clone()
            .unwrap_or((String::new(), false, false, false, None, None, None, false));
    let mut bar = row()
        .key("browser-toolbar")
        .w_full()
        .h(36.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .fill(a.panel())
        .child(toolbar_button(
            a,
            "browser-back",
            &rune_kit::glyphs::ARROW_LEFT,
            "Back",
            back,
            |s, _| {
                if let Some(t) = s.shown_browser() {
                    t.browser.go_back();
                }
            },
        ))
        .child(toolbar_button(
            a,
            "browser-forward",
            &rune_kit::glyphs::ARROW_RIGHT,
            "Forward",
            forward,
            |s, _| {
                if let Some(t) = s.shown_browser() {
                    t.browser.go_forward();
                }
            },
        ))
        .child(if loading {
            toolbar_button(
                a,
                "browser-stop",
                &rune_kit::glyphs::CLOSE,
                "Stop",
                true,
                |s, _| {
                    if let Some(t) = s.shown_browser() {
                        t.browser.stop_load();
                    }
                },
            )
        } else {
            toolbar_button(
                a,
                "browser-reload",
                &rune_kit::glyphs::REFRESH,
                "Reload",
                target.is_some(),
                |s, _| {
                    if let Some(t) = s.shown_browser() {
                        t.shared.lock().crashed = None;
                        t.browser.reload();
                    }
                },
            )
        });
    if let Some(address) = &s.browsers.address {
        bar = bar.child(
            text_input(address, ui_label())
                .key("browser-address")
                .label("Address")
                .placeholder("localhost:4200 or https://…", a.muted())
                .single_line()
                .no_window_drag()
                .color(a.ink())
                .caret(a.accent(), a.selection())
                .grow(1.0)
                .min_w(0.0)
                .h(26.0)
                .px(8.0)
                .radius(6.0)
                .fill(a.bg())
                .border(1.0, a.hover_bg())
                .on_edit(|s: &mut State| {
                    s.browsers.address.get_or_insert_with(|| TextEdit::new(""))
                })
                .on_input_key(|s: &mut State, event, _| {
                    s.browsers.page_focus = false;
                    if event.key == Key::Enter {
                        s.browser_go();
                        return true;
                    }
                    false
                }),
        );
    } else {
        bar = bar.child(el().grow(1.0));
    }
    if recording {
        bar = bar.child(
            row()
                .key("browser-recording")
                .gap(4.0)
                .center_y()
                .child(
                    text("● REC")
                        .key("browser-recording-label")
                        .font(ui_label())
                        .color(a.danger()),
                )
                .child(toolbar_button(
                    a,
                    "browser-recording-stop",
                    &rune_kit::glyphs::CLOSE,
                    "Stop recording",
                    true,
                    |s, _| s.stop_shown_recording(),
                )),
        );
    }
    bar = bar.child(
        icon_button(
            a,
            "browser-close",
            &rune_kit::glyphs::CLOSE,
            "Close Browser",
            |s, tx| s.close_browser_view(tx),
        )
        .label("Close Browser"),
    );
    let notice = s.browsers.status.clone().or(crashed).or(error).or(blocked);
    let mut pane = col()
        .key("browser-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .child(bar);
    if let Some(notice) = notice {
        pane = pane.child(
            row()
                .key("browser-notice")
                .w_full()
                .px(10.0)
                .py(4.0)
                .fill(a.warning_bg())
                .child(
                    text(notice)
                        .key("browser-notice-text")
                        .font(ui_label())
                        .color(a.ink())
                        .truncate()
                        .min_w(0.0)
                        .shrink(),
                ),
        );
    }
    let content = match target {
        Some(target) => el()
            .key("browser-surface")
            .label("Web page")
            .focusable(true)
            .grow(1.0)
            .w_full()
            .min_h(0.0)
            .fill(rune::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            })
            .native_surface(&target.shared.surface, rune::SurfaceFit::Fill)
            .on_layout(|s: &mut State, r| {
                s.browsers.view_size = Some((r[2], r[3]));
                let scale = s
                    .window
                    .as_ref()
                    .and_then(|w| w.with_winit_window(|w| w.scale_factor() as f32).ok())
                    .unwrap_or(2.0);
                s.browsers.view_scale = scale;
                if let Some(t) = s.shown_browser() {
                    t.resize(r[2].max(1.0) as u32, r[3].max(1.0) as u32, scale);
                }
            })
            .on_pointer_move_local(|s: &mut State, p, _| {
                if let Some(t) = s.shown_browser() {
                    let held = s.browsers.owner_button.map_or(0, |_| MOD_LEFT);
                    t.mouse_move(p.x, p.y, held);
                }
            })
            .on_drag(|s: &mut State, d, tx| {
                s.owner_browser_input();
                s.browsers.page_focus = true;
                tx.focus("browser-surface", false);
                if let Some(t) = s.shown_browser() {
                    match d.phase {
                        Phase::Down => {
                            t.mouse_button(
                                d.x,
                                d.y,
                                MouseButtonType::LEFT,
                                false,
                                d.clicks as i32,
                                MOD_LEFT,
                            );
                        }
                        Phase::Move => t.mouse_move(d.x, d.y, MOD_LEFT),
                        _ => t.mouse_button(
                            d.x,
                            d.y,
                            MouseButtonType::LEFT,
                            true,
                            d.clicks as i32,
                            0,
                        ),
                    }
                }
                s.browsers.owner_button =
                    matches!(d.phase, Phase::Down | Phase::Move).then_some(MouseButtonType::LEFT);
            })
            .on_wheel(|s: &mut State, e, _| {
                s.owner_browser_input();
                if let Some(t) = s.shown_browser() {
                    t.wheel(e.x, e.y, e.dx, e.dy, 0);
                }
                true
            }),
        None => col()
            .key("browser-empty")
            .grow(1.0)
            .w_full()
            .center()
            .child(
                text(
                    s.browsers
                        .status
                        .clone()
                        .unwrap_or_else(|| "The browser is starting…".into()),
                )
                .font(ui_label())
                .color(a.muted()),
            ),
    };
    pane.child(content)
}

impl State {
    pub(crate) fn stop_shown_recording(&mut self) {
        let Some(id) = self.browsers.shown.clone() else {
            return;
        };
        if let Some(index) = self.browser_index(&id)
            && let Some(mut recording) = self.browsers.targets[index].recording.take()
        {
            self.browsers.targets[index].shared.lock().feed = None;
            let status = recording.stop("stopped by the owner", false);
            self.emit("recording", Some(&recording.id), serde_json::json!(status));
            // Finalization continues on the encoder thread.
            self.browsers.finalizing.push(recording);
        }
    }
}
