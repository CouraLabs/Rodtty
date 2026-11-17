//! Pane-local tab interactions share the application's action dispatcher.
use super::*;
use std::{cell::Cell, collections::HashMap, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    Terminal(usize),
    Editor(u64),
    Settings,
}

pub(super) struct Tabs {
    pub settings_position: usize,
    pub preview_root: Option<tempfile::TempDir>,
    terminal_scroll: Rc<Cell<f32>>,
    editor_scroll: Rc<Cell<f32>>,
    rects: HashMap<Target, [f32; 4]>,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            preview_root: None,
            settings_position: usize::MAX,
            rects: HashMap::new(),
            terminal_scroll: Default::default(),
            editor_scroll: Default::default(),
        }
    }
}

pub(super) fn scroll_strip(strip: El, state: &State, terminal: bool) -> El {
    let offset = if terminal {
        &state.tab_strips.terminal_scroll
    } else {
        &state.tab_strips.editor_scroll
    }
    .clone();
    let wheel_offset = offset.clone();
    let key = if terminal {
        "terminal-tabs"
    } else {
        "editor-tabs"
    };
    strip
        .on_scroll_offset(move |at| offset.set(at.offset[0]))
        .on_wheel(move |_: &mut State, event, tx| {
            if event.dx.abs() >= event.dy.abs() {
                return false;
            }
            tx.scroll_to_x(key, (wheel_offset.get() - event.dy).max(0.0));
            true
        })
}

pub(super) fn session_id(session: &Session) -> usize {
    session.id()
}

impl State {
    pub(super) fn move_tab(&mut self, target: Target, to: usize) {
        match target {
            Target::Terminal(id) => {
                let Some(from) = self.sessions.iter().position(|s| session_id(s) == id) else {
                    return;
                };
                let active = self.active().map(session_id);
                let to = to.min(self.sessions.len() - 1);
                let session = self.sessions.remove(from);
                self.sessions.insert(to, session);
                if from < self.generations.len() && to < self.generations.len() {
                    let generation = self.generations.remove(from);
                    self.generations.insert(to, generation);
                } else {
                    self.generations.clear();
                }
                self.active = self
                    .sessions
                    .iter()
                    .position(|s| Some(session_id(s)) == active)
                    .unwrap_or(0);
            }
            Target::Editor(_) | Target::Settings => {
                let mut order = self.editor_tab_order();
                let Some(from) = order.iter().position(|t| *t == target) else {
                    return;
                };
                let to = to.min(order.len() - 1);
                let moved = order.remove(from);
                order.insert(to, moved);
                self.tab_strips.settings_position = order
                    .iter()
                    .position(|t| *t == Target::Settings)
                    .unwrap_or(usize::MAX);
                let active = self.workspace.tabs.get(self.workspace.active).map(|t| t.id);
                self.workspace
                    .tabs
                    .sort_by_key(|tab| order.iter().position(|t| *t == Target::Editor(tab.id)));
                self.workspace.active = self
                    .workspace
                    .tabs
                    .iter()
                    .position(|t| Some(t.id) == active)
                    .unwrap_or(0);
            }
        }
    }

    pub(super) fn editor_tab_order(&self) -> Vec<Target> {
        let mut order: Vec<_> = self
            .workspace
            .tabs
            .iter()
            .map(|t| Target::Editor(t.id))
            .collect();
        if self.settings_open {
            order.insert(
                self.tab_strips.settings_position.min(order.len()),
                Target::Settings,
            );
        }
        order
    }

    fn tab_index(&self, target: Target) -> Option<(usize, usize)> {
        match target {
            Target::Terminal(id) => self
                .sessions
                .iter()
                .position(|s| session_id(s) == id)
                .map(|i| (i, self.sessions.len())),
            Target::Editor(_) | Target::Settings => {
                let order = self.editor_tab_order();
                order
                    .iter()
                    .position(|t| *t == target)
                    .map(|i| (i, order.len()))
            }
        }
    }
}

fn close(target: Target) -> AppAction {
    match target {
        Target::Terminal(id) => AppAction::CloseTerminal(id),
        Target::Editor(id) => AppAction::CloseEditor(Some(id)),
        Target::Settings => AppAction::CloseSettings,
    }
}

pub(super) fn interactive(tab: El, target: Target) -> El {
    tab.on_drawn_layout(move |s: &mut State, rect| {
        s.tab_strips.rects.insert(target, rect);
    })
    .on_middle_press(move |s: &mut State, tx| s.dispatch(close(target), tx))
    .on_drag(move |s: &mut State, drag, tx| {
        if drag.phase != Phase::Up || drag.cancelled() || drag.dx.abs() < 4.0 {
            return;
        }
        let [x, y] = drag.window_point();
        let mut candidates: Vec<_> = s
            .tab_strips
            .rects
            .iter()
            .filter(|(t, r)| {
                matches!(t, Target::Terminal(_)) == matches!(target, Target::Terminal(_))
                    && y >= r[1]
                    && y <= r[1] + r[3]
                    && s.tab_index(**t).is_some()
            })
            .map(|(t, r)| (*t, *r))
            .collect();
        candidates.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
        if let Some((other, _)) = candidates.iter().min_by(|a, b| {
            (x - (a.1[0] + a.1[2] / 2.0))
                .abs()
                .total_cmp(&(x - (b.1[0] + b.1[2] / 2.0)).abs())
        }) && let Some((to, _)) = s.tab_index(*other)
        {
            s.dispatch(AppAction::MoveTab(target, to), tx);
        }
    })
    .on_secondary_press(move |s: &mut State, point, tx| {
        let Some((index, count)) = s.tab_index(target) else {
            return;
        };
        let mut menu = rune::PopupMenu::new();
        if matches!(target, Target::Terminal(_)) {
            menu = menu.item(rune::PopupItem::new("new", "New terminal tab"));
        } else if matches!(target, Target::Editor(_)) {
            menu = menu.item(rune::PopupItem::new("save", "Save file"));
        }
        menu = menu
            .item(rune::PopupItem::new("left", "Move tab left").enabled(index > 0))
            .item(rune::PopupItem::new("right", "Move tab right").enabled(index + 1 < count))
            .separator()
            .item(rune::PopupItem::new("close", "Close tab"));
        tx.popup_menu(menu, point, move |s: &mut State, chosen, tx| {
            let action = match chosen {
                Some("new") => Some(AppAction::NewTerminal),
                Some("save") => match target {
                    Target::Editor(id) => Some(AppAction::SaveEditor(Some(id))),
                    _ => None,
                },
                Some("close") => Some(close(target)),
                Some("left" | "right") => s.tab_index(target).map(|(i, n)| {
                    AppAction::MoveTab(
                        target,
                        if chosen == Some("left") {
                            i.saturating_sub(1)
                        } else {
                            (i + 1).min(n - 1)
                        },
                    )
                }),
                _ => None,
            };
            if let Some(action) = action {
                s.dispatch(action, tx);
            }
        });
    })
}
