use super::*;

pub(super) fn page(s: &State, a: &Appearance, layers: &mut Vec<El>) -> El {
    let searching = !query(s).is_empty();
    let mut page = col()
        .key("settings-page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .child(
            text("Settings")
                .role("heading")
                .level(1)
                .font(ui_style(18.0, 600.0))
                .color(a.ink()),
        )
        .child(
            row()
                .key("settings-toolbar")
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text_input(&s.settings_query, ui_label())
                        .caret(a.accent(), a.selection())
                        .key("settings-search")
                        .label("Search settings")
                        .single_line()
                        .w_full()
                        .min_w(0.0)
                        .grow(1.0)
                        .px(8.0)
                        .py(5.0)
                        .color(a.ink())
                        .placeholder("Search settings", a.muted())
                        // A thin border marks the field's bounds against the page.
                        .border(1.0, a.kit().c.border_base)
                        .radius(6.0)
                        .on_edit(|s: &mut State| &mut s.settings_query),
                )
                .child(
                    button(a, "open-settings-json", "Open Settings JSON", |s, tx| {
                        s.dispatch(AppAction::OpenSettingsJson, tx)
                    })
                    .shrink(),
                ),
        )
        .child(tab_strip(s, a, searching));
    if let Some(error) = &s.settings_error {
        page = page.child(
            text(error)
                .font(ui_label())
                .color(a.danger())
                .wrap_anywhere(),
        );
    }
    let mut list = col()
        .key("settings-list")
        .instant_changes()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(16.0)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    let mut count = 0;
    for tab in Tab::ALL {
        let mut fieldsets = Vec::new();
        for fieldset in FIELDSETS.iter().filter(|f| f.tab == tab) {
            let mut rows: Vec<El> = fieldset
                .shown_settings(s)
                .into_iter()
                .map(|setting| setting_row(s, a, setting, layers))
                .collect();
            rows.extend(fieldset.shown_rows(s, a));
            if !rows.is_empty() {
                fieldsets.push(fieldset_box(a, fieldset, rows));
            }
        }
        if fieldsets.is_empty() {
            continue;
        }
        count += 1;
        let mut section = col()
            .key(format!("settings-section-{}", tab.label()))
            .role("group")
            .label(tab.label())
            .w_full()
            .gap(16.0);
        // The selected tab names its own page; results from a search are
        // headed by the tab they belong to.
        if searching {
            section = section.child(
                text(tab.label())
                    .role("heading")
                    .level(2)
                    .font(ui_style(13.0, 600.0))
                    .color(a.accent()),
            );
        }
        if !searching {
            section = section.child(restore_defaults(s, a, tab));
        }
        section = section.children(fieldsets);
        list = list.child(section);
    }
    if count == 0 {
        list = list.child(
            text("No settings match your search.")
                .font(ui_label())
                .color(a.muted()),
        );
    }
    page.child(list)
}

/// The row of tabs under the search field. While a search shows results from
/// every tab, none is selected; choosing one clears the search.
pub(super) fn tab_strip(s: &State, a: &Appearance, searching: bool) -> El {
    let mut strip = row()
        .key("settings-tabs")
        .role("tablist")
        .label("Settings sections")
        .w_full()
        .gap(4.0)
        .padding(3.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.kit().c.border_base)
        // Wrap onto a second line in a narrow pane so no tab is out of view.
        .flex_wrap();
    for (position, tab) in Tab::ALL.into_iter().enumerate() {
        let selected = !searching && s.settings_tab == tab;
        let key = format!("settings-page-tab-{}", tab.key());
        pressable(&key);
        strip = strip.child(
            el().key(key)
                .role("tab")
                .label(tab.label())
                .focus_selected(selected)
                .position_in_set(position + 1, Tab::ALL.len())
                .focusable(true)
                .no_window_drag()
                .focus_ring(2.0, 1.0, a.accent())
                .h(26.0_f32.max(ui_label().size * 1.5 + 6.0))
                .px(12.0)
                .center()
                .radius(6.0)
                .fill(if selected {
                    a.hover_bg()
                } else {
                    Color::TRANSPARENT
                })
                .border(
                    1.0,
                    if selected {
                        a.button_border()
                    } else {
                        Color::TRANSPARENT
                    },
                )
                .hover(a.hover_bg())
                .press(a.pressed())
                .on_press_tx(move |s: &mut State, tx| {
                    tx.blur();
                    s.settings_tab = tab;
                    s.settings_select = None;
                    s.settings_reset_confirm = None;
                    s.settings_query.replace_text("");
                })
                .child(
                    text(tab.label())
                        .font(if selected {
                            ui_style(ui_label().size, 600.0)
                        } else {
                            ui_label()
                        })
                        .color(if selected { a.ink() } else { a.muted() })
                        .a11y_hidden(),
                ),
        );
    }
    strip
}

/// A fieldset: its title above a bordered box of rows divided by thin rules.
pub(super) fn fieldset_box(a: &Appearance, fieldset: &Fieldset, rows: Vec<El>) -> El {
    let divider = || {
        el().w_full()
            .h(1.0)
            .fill(a.kit().c.border_base.mix(a.panel(), 0.5))
            .a11y_hidden()
    };
    let mut body = col()
        .w_full()
        .px(14.0)
        .py(2.0)
        .radius(8.0)
        .fill(a.panel())
        .border(1.0, a.kit().c.border_base);
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(divider());
        }
        body = body.child(row);
    }
    col()
        .key(format!("settings-group-{}", fieldset.key))
        .role("group")
        .label(fieldset.title)
        .w_full()
        .gap(6.0)
        .child(
            text(fieldset.title)
                .role("heading")
                .level(3)
                .font(ui_style(12.0, 600.0))
                .color(a.muted())
                .pl(2.0),
        )
        .child(body)
}

/// A setting's line: label and description on the left, its control on the
/// right, wrapping below them when the pane is too narrow for both.
pub(super) fn setting_row(
    s: &State,
    a: &Appearance,
    setting: &Setting,
    layers: &mut Vec<El>,
) -> El {
    let mut name = col()
        .grow(1.0)
        .shrink()
        .min_w(160.0)
        .gap(2.0)
        .child(text(setting.label).font(ui_label()).color(a.ink()).wrap());
    if !setting.description.is_empty() {
        name = name.child(
            text(setting.description)
                .font(ui_style(12.0, 400.0))
                .color(a.muted())
                .wrap(),
        );
    }
    let mut line = col()
        .key(format!("{}-setting", setting.key))
        .w_full()
        .py(10.0)
        .gap(8.0)
        .child(
            row()
                .key(format!("{}-row", setting.key))
                .w_full()
                .gap(12.0)
                .flex_wrap()
                .center_y()
                .child(name)
                .child((setting.control)(s, a, layers)),
        );
    if let Some(below) = setting.below {
        line = line.child(below(s, a, layers));
    }
    line
}

/// A row for data-driven lists, such as one bundled command.
pub(super) fn plain_row(a: &Appearance, key: String, label: &str, control: El) -> El {
    row()
        .key(key)
        .w_full()
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(
            text(label)
                .font(ui_label())
                .color(a.ink())
                .grow(1.0)
                .shrink()
                .min_w(0.0),
        )
        .child(control)
}

/// The tab's one reset, above its fieldsets: a button that asks before
/// restoring every setting on the tab to its default.
pub(super) fn restore_defaults(s: &State, a: &Appearance, tab: Tab) -> El {
    let mut line = row()
        .key(format!("restore-defaults-{}", tab.key()))
        .w_full()
        .gap(8.0)
        .flex_wrap()
        .center_y();
    if s.settings_reset_confirm == Some(tab) {
        line = line
            .child(
                text(format!(
                    "Restore every {} setting to its default?",
                    tab.label()
                ))
                .font(ui_label())
                .color(a.ink())
                .wrap()
                .shrink()
                .min_w(0.0),
            )
            .child(
                button(a, "confirm-reset-tab", "Restore Defaults", move |s, tx| {
                    s.settings_reset_confirm = None;
                    s.reset_settings_tab(tab, tx);
                })
                .label(format!("Restore {} defaults", tab.label())),
            )
            .child(button(a, "cancel-reset-tab", "Cancel", |s, _| {
                s.settings_reset_confirm = None;
            }));
    } else {
        line = line.child(el().grow(1.0)).child(
            button(
                a,
                format!("reset-tab-{}", tab.key()),
                "Restore Defaults",
                move |s, _| s.settings_reset_confirm = Some(tab),
            )
            .label(format!("Restore {} defaults", tab.label()))
            .help(format!(
                "Restore every {} setting to its default",
                tab.label()
            )),
        );
    }
    line
}
