//! Split terminals: a terminal tab shows up to two independent sessions,
//! side by side or stacked, with a draggable divider. The focused pane's
//! session is the active one, so keyboard, paste, mouse reporting,
//! selection and scrollback go only to it; the other pane draws its
//! session and takes focus when pressed. Each pane resizes its own PTY from
//! its own grid. Narrow windows show the focused pane with a switcher.
use super::*;

/// Narrower than this, a split tab shows one pane at a time.
pub(crate) const COMPACT_WIDTH: f32 = 560.0;
/// The smallest share of the tab either pane gets.
const MIN_SHARE: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Split {
    /// The session whose tab holds the split, by identity.
    pub first: usize,
    /// The second pane's session.
    pub second: usize,
    /// Side by side (a vertical divider) rather than stacked.
    pub side_by_side: bool,
    /// The first pane's share of the tab.
    pub share: f32,
}

impl Split {
    pub fn partner(&self, id: usize) -> Option<usize> {
        if id == self.first {
            Some(self.second)
        } else if id == self.second {
            Some(self.first)
        } else {
            None
        }
    }
}

impl State {
    /// The split holding session `id`.
    pub(crate) fn split_of(&self, id: usize) -> Option<Split> {
        self.splits
            .iter()
            .copied()
            .find(|s| s.partner(id).is_some())
    }

    /// Whether session `id` is a second pane, drawn inside another tab.
    pub(crate) fn is_second_pane(&self, id: usize) -> bool {
        self.splits.iter().any(|s| s.second == id)
    }

    /// Split the active tab, starting a fresh session in the new pane with
    /// the new-terminal directory policy. A tab holds at most two panes.
    pub(crate) fn split_active(&mut self, side_by_side: bool) {
        let Some(current) = self.active().map(Session::id) else {
            self.message = "Open a terminal to split.".into();
            return;
        };
        if self.split_of(current).is_some() {
            self.message = "This tab already has two panes.".into();
            return;
        }
        let before = self.sessions.len();
        self.new_tab();
        if self.sessions.len() == before {
            return;
        }
        let second = self.sessions[self.sessions.len() - 1].id();
        self.splits.push(Split {
            first: current,
            second,
            side_by_side,
            share: 0.5,
        });
        self.message = "Split the terminal. Terminal: Focus Other Pane switches panes.".into();
    }

    /// Move keyboard focus to the other pane of the active tab.
    pub(crate) fn focus_other_pane(&mut self) {
        let Some(id) = self.active().map(Session::id) else {
            return;
        };
        let Some(other) = self.split_of(id).and_then(|s| s.partner(id)) else {
            self.message = "This tab has one pane.".into();
            return;
        };
        self.focus_session(other);
    }

    pub(crate) fn focus_session(&mut self, id: usize) {
        if let Some(index) = self.sessions.iter().position(|s| s.id() == id)
            && index != self.active
        {
            self.active = index;
            self.selection = None;
            self.workspace.note_session_switch();
        }
        self.workspace.layout.terminal = true;
        self.workspace.set_terminal_focus(true);
    }

    /// Drop splits whose sessions are gone; the survivor fills its tab.
    pub(crate) fn prune_splits(&mut self) {
        let live: Vec<usize> = self.sessions.iter().map(Session::id).collect();
        self.splits
            .retain(|s| live.contains(&s.first) && live.contains(&s.second));
    }

    /// The session ids of the tab holding `id`: one, or both panes.
    pub(crate) fn tab_sessions(&self, id: usize) -> Vec<usize> {
        match self.split_of(id) {
            Some(s) => vec![s.first, s.second],
            None => vec![id],
        }
    }

    pub(crate) fn set_split_share(&mut self, first: usize, share: f32) {
        if let Some(split) = self.splits.iter_mut().find(|s| s.first == first) {
            split.share = share.clamp(MIN_SHARE, 1.0 - MIN_SHARE);
        }
    }

    /// Splits as saved layouts keep them, by tab position.
    pub(crate) fn saved_splits(&self) -> Vec<crate::work::SavedSplit> {
        let index = |id: usize| self.sessions.iter().position(|s| s.id() == id);
        self.splits
            .iter()
            .filter_map(|s| {
                Some(crate::work::SavedSplit {
                    first: index(s.first)?,
                    second: index(s.second)?,
                    side_by_side: s.side_by_side,
                    share: s.share,
                })
            })
            .collect()
    }

    /// Rebuild saved splits over freshly started sessions. When the session
    /// count doesn't match what was saved, positions can't be trusted and
    /// the splits are dropped.
    pub(crate) fn restore_splits(&mut self, saved: &[crate::work::SavedSplit], expected: usize) {
        if self.sessions.len() != expected {
            return;
        }
        for s in saved {
            let (Some(a), Some(b)) = (self.sessions.get(s.first), self.sessions.get(s.second))
            else {
                continue;
            };
            let (first, second) = (a.id(), b.id());
            if first == second || self.split_of(first).is_some() || self.split_of(second).is_some()
            {
                continue;
            }
            self.splits.push(Split {
                first,
                second,
                side_by_side: s.side_by_side,
                share: s.share.clamp(MIN_SHARE, 1.0 - MIN_SHARE),
            });
        }
    }

    /// Resize session `id`'s PTY to fit `rect`, as its pane laid out.
    pub(crate) fn resize_pane(&mut self, id: usize, rect: [f32; 4]) {
        let (rows, cols) = fonts::Metrics::measure().dimensions(rect);
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id() == id)
            && session.size() != (rows, cols)
            && let Err(error) = session.resize(rows, cols)
        {
            self.message = error.to_string();
        }
    }
}

/// The terminal area for the active tab: one grid, or two panes and a
/// divider. `grid` draws the focused pane.
pub(crate) fn panes(s: &State, a: &Appearance, width: f32) -> El {
    let focused = |content: El| {
        col()
            .grow(1.0)
            .min_h(0.0)
            .min_w(0.0)
            .w_full()
            .h_full()
            .px(16.0)
            .py(12.0)
            .child(content)
    };
    let Some(active) = s.active().map(Session::id) else {
        return focused(grid(s, a));
    };
    let Some(split) = s.split_of(active) else {
        return focused(grid(s, a));
    };
    let other = split.partner(active).unwrap_or(active);
    let first_focused = active == split.first;
    if width < COMPACT_WIDTH {
        let position = if first_focused { 1 } else { 2 };
        let bar = row()
            .key("split-compact")
            .w_full()
            .px(12.0)
            .py(4.0)
            .gap(8.0)
            .center_y()
            .fill(a.panel())
            .child(
                text(format!("Pane {position} of 2"))
                    .font(ui_label())
                    .color(a.muted())
                    .grow(1.0),
            )
            .child(button(a, "split-switch", "Other Pane", |s, _| {
                s.focus_other_pane()
            }));
        return col()
            .key("terminal-split")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .child(bar)
            .child(focused(grid(s, a)));
    }
    let pane = |focused_pane: bool, id: usize, share: f32| {
        let content = if focused_pane {
            grid(s, a)
        } else {
            terminal::view::passive_grid(s, a, id)
        };
        let mut pane = col()
            .key(format!("terminal-pane-{id}"))
            .role("group")
            .label(if focused_pane {
                "Terminal pane (focused)"
            } else {
                "Terminal pane"
            })
            .grow(share)
            .min_w(0.0)
            .min_h(0.0)
            .px(12.0)
            .py(8.0)
            .border(
                1.5,
                if focused_pane {
                    a.accent()
                } else {
                    Color::TRANSPARENT
                },
            )
            .radius(4.0)
            .child(content);
        if split.side_by_side {
            pane = pane.h_full();
        } else {
            pane = pane.w_full();
        }
        pane
    };
    let first = pane(first_focused, split.first, split.share);
    let second = pane(
        !first_focused,
        if first_focused { other } else { active },
        1.0 - split.share,
    );
    let first_id = split.first;
    let side = split.side_by_side;
    let divider = el()
        .key("split-divider")
        .role("separator")
        .label("Pane divider")
        .center()
        .hover(a.accent().alpha(0.3))
        .child(if side {
            el().w(1.0).h_full().fill(a.hover_bg())
        } else {
            el().h(1.0).w_full().fill(a.hover_bg())
        })
        .on_drag(move |s: &mut State, d, _| {
            if d.phase == Phase::Down {
                s.split_drag = s.split_of(first_id).map_or(0.5, |x| x.share);
            }
            let extent = s
                .split_rect
                .map_or(1.0, |r| if side { r[2] } else { r[3] })
                .max(1.0);
            let delta = if side { d.dx } else { d.dy };
            s.set_split_share(first_id, s.split_drag + delta / extent);
        });
    let container = if side {
        row()
            .child(first)
            .child(divider.w(5.0).h_full())
            .child(second)
    } else {
        col()
            .child(first)
            .child(divider.h(5.0).w_full())
            .child(second)
    };
    container
        .key("terminal-split")
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .on_layout(|s: &mut State, r| s.split_rect = Some(r))
}
