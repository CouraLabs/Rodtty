use super::*;

pub(super) struct Palette {
    pub selected: usize,
    pub return_focus: Option<String>,
}

impl State {
    pub(super) fn open_palette(&mut self, tx: &mut Tx) {
        self.palette_query = TextEdit::new("");
        self.palette = Some(Palette {
            selected: 0,
            return_focus: tx.focused().map(str::to_owned),
        });
        tx.focus_when_shown("command-query", true);
    }

    fn palette_matches(&self) -> Vec<usize> {
        let query = self.palette_query.text.to_lowercase();
        self.commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                let title = command.title.to_lowercase();
                query.split_whitespace().all(|word| title.contains(word))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub(super) fn dismiss_palette(&mut self, tx: &mut Tx) {
        let previous = self.palette.take().and_then(|palette| palette.return_focus);
        tx.blur();
        if let Some(key) = previous {
            tx.return_focus(key, true);
        }
    }

    fn execute_palette(&mut self, index: usize, tx: &mut Tx) {
        let action = self.commands[index].action;
        if self.action_disabled(action).is_some() {
            return;
        }
        self.dismiss_palette(tx);
        self.dispatch(action, tx);
    }

    pub(super) fn palette_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        let matches = self.palette_matches();
        let selected = self.palette.as_ref().map_or(0, |p| p.selected);
        match event.key {
            Key::Escape => self.dismiss_palette(tx),
            Key::Enter => {
                if let Some(index) = matches.get(selected) {
                    self.execute_palette(*index, tx);
                }
            }
            Key::Down | Key::Up => {
                let count = matches.len();
                if count > 0 {
                    self.palette.as_mut().unwrap().selected = if event.key == Key::Down {
                        (selected + 1) % count
                    } else {
                        (selected + count - 1) % count
                    };
                }
            }
            _ => return false,
        }
        true
    }
}

pub(super) fn view(state: &State, a: &Appearance) -> Option<El> {
    let palette = state.palette.as_ref()?;
    let mut content = col()
        .key("command-palette")
        .modal_tx(true, |s: &mut State, tx| s.dismiss_palette(tx))
        .w_full()
        .max_w(560.0)
        .px(16.0)
        .py(16.0)
        .gap(8.0)
        .radius(10.0)
        .fill(a.panel())
        .child(text("Command Palette").font(ui_label()).color(a.ink()))
        .child(
            text_input(&state.palette_query, ui_label())
                .caret(a.accent(), a.selection())
                .key("command-query")
                .label("Search application actions")
                .placeholder("Search actions…", a.muted())
                .single_line()
                .w_full()
                .h(32.0)
                .color(a.ink())
                .on_input_key(|s: &mut State, event, tx| s.palette_key(event, tx))
                .on_edit(|s: &mut State| &mut s.palette_query)
                .on_input_change(|s: &mut State, _| {
                    if let Some(palette) = &mut s.palette {
                        palette.selected = 0;
                    }
                }),
        );
    let matches = state.palette_matches();
    if matches.is_empty() {
        content = content.child(
            text("No matching actions")
                .font(ui_label())
                .color(a.muted()),
        );
    }
    let count = matches.len();
    let first = palette.selected.saturating_sub(3);
    for (position, index) in matches.into_iter().enumerate().skip(first).take(4) {
        let command = &state.commands[index];
        let disabled = state.action_disabled(command.action);
        let mut item = col()
            .key(format!("command-{}", command.id))
            .w_full()
            .h(40.0)
            .px(8.0)
            .py(3.0)
            .radius(5.0)
            .fill(if position == palette.selected {
                a.hover_bg()
            } else {
                a.panel()
            })
            .border(
                1.0,
                if position == palette.selected {
                    a.accent()
                } else {
                    a.panel()
                },
            )
            .disabled(disabled.is_some())
            .on_press_tx(move |s: &mut State, tx| s.execute_palette(index, tx))
            .child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(
                        text(command.title)
                            .font(ui_label())
                            .color(if disabled.is_some() {
                                a.muted()
                            } else {
                                a.ink()
                            })
                            .truncate()
                            .shrink()
                            .grow(1.0),
                    )
                    .child(
                        text(state.shortcut_label(command.id))
                            .font(ui_style(10.0, 400.0))
                            .color(a.muted()),
                    ),
            );
        if let Some(reason) = disabled {
            item = item.child(
                text(reason)
                    .font(ui_style(10.0, 400.0))
                    .color(a.muted())
                    .truncate(),
            );
        }
        content = content.child(item);
    }
    content = content.child(
        text(format!(
            "{} of {count} · ↑ ↓ select · Enter run · Esc dismiss",
            if count == 0 { 0 } else { palette.selected + 1 }
        ))
        .font(ui_style(10.0, 400.0))
        .color(a.muted()),
    );
    Some(motion::modal(
        a,
        col().items_center().px(16.0).pt(16.0).child(content),
        Entrance::Drawer(0.0, -8.0),
    ))
}
