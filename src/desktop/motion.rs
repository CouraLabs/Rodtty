//! Rodtty's interface motion, on Rune's own presence transitions and springs.
//!
//! Drawers, menus, dialogs, the tab indicator and action feedback enter and
//! leave over [`ENTER`] and [`EXIT`] seconds on critically damped springs
//! (Rune's `Spring::new(duration, 0.0)`: an ease-out with no overshoot). Under
//! Reduce Motion, the system's or Rune's runtime override, every one of them
//! becomes [`Transition::none`] and [`Spring::NONE`], which Rune lands in the
//! frame it starts rather than stretching into its reduced-motion fade.
//!
//! Typing, terminal rendering, selection, divider drags and focus changes are
//! not given motion here; their containers opt out with `instant_changes()`.
use super::appearance::Appearance;
use rune::{El, Spring, SpringPair, Transition, Vec2, el};

/// The perceptual duration of an entrance or a change, in seconds.
pub(super) const ENTER: f32 = 0.14;
/// The perceptual duration of an exit, in seconds: a little quicker than the
/// entrance, as Rune's own exits are.
pub(super) const EXIT: f32 = 0.12;

/// How an element enters and leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Entrance {
    /// A transparent layer that only positions what it holds: it shows whole,
    /// and stays while what it holds plays its exit.
    Holder,
    /// A menu: a fade and slight scale from `anchor`, the corner nearest its
    /// trigger, in fractions of its size.
    Menu(Vec2),
    /// A dialog: a fade and slight scale about its centre.
    Dialog,
    /// A fade alone: the dimmed backdrop behind a dialog, the tab indicator.
    Fade,
    /// A drawer or floating panel sliding in a short way from `(x, y)`.
    Drawer(f32, f32),
    /// Feedback for an action: a status message rising slightly as it fades in.
    Feedback,
}

/// The motion one view uses, resolved from whether this commit reduces motion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Motion {
    reduced: bool,
}

impl Motion {
    pub fn new(reduced: bool) -> Self {
        Self { reduced }
    }
    /// The spring a change (a colour, the tab indicator's travel) takes.
    pub fn spring(self) -> Spring {
        if self.reduced {
            Spring::NONE
        } else {
            Spring::new(ENTER, 0.0)
        }
    }
    /// The enter and exit springs every entrance runs on.
    pub fn springs(self) -> SpringPair {
        if self.reduced {
            SpringPair::NONE
        } else {
            SpringPair::new(Spring::new(ENTER, 0.0), Spring::new(EXIT, 0.0))
        }
    }
    /// Rune's transition for `entrance`: [`Transition::none`] under Reduce Motion.
    pub fn transition(self, entrance: Entrance) -> Transition {
        if self.reduced {
            return Transition::none();
        }
        match entrance {
            Entrance::Holder => Transition::identity(),
            Entrance::Menu(anchor) => Transition::scale_anchor(0.96, anchor),
            Entrance::Dialog => Transition::scale(0.96),
            Entrance::Fade => Transition::opacity(),
            Entrance::Drawer(x, y) => Transition::offset(x, y).combined(Transition::opacity()),
            Entrance::Feedback => Transition::offset(0.0, 4.0).combined(Transition::opacity()),
        }
        .springs(self.springs())
    }
    /// `el` entering and leaving as `entrance`. Under Reduce Motion it and
    /// everything it holds land at once: a layer without an effect of its own
    /// would otherwise let each child play its default fade.
    pub fn present(self, el: El, entrance: Entrance) -> El {
        let el = el.transition(self.transition(entrance));
        if self.reduced {
            el.instant_changes()
        } else {
            el
        }
    }
}

/// [`Motion::present`] as a builder step, under an appearance's motion.
pub(super) trait Enters {
    fn enters(self, a: &Appearance, entrance: Entrance) -> El;
    /// [`Self::enters`] for an element inside a container whose changes land
    /// at once (the header, the workspace body): it keeps its own motion there.
    fn enters_animated(self, a: &Appearance, entrance: Entrance) -> El;
}

impl Enters for El {
    fn enters(self, a: &Appearance, entrance: Entrance) -> El {
        a.motion().present(self, entrance)
    }
    fn enters_animated(self, a: &Appearance, entrance: Entrance) -> El {
        let motion = a.motion();
        if motion.reduced {
            motion.present(self, entrance)
        } else {
            motion.present(self.animated_changes(), entrance)
        }
    }
}

/// Where a menu below its trigger's trailing edge grows from.
pub(super) const FROM_TOP_TRAILING: Vec2 = Vec2::new(1.0, 0.0);
/// Where a menu opened at the pointer grows from.
pub(super) const FROM_TOP_LEADING: Vec2 = Vec2::new(0.0, 0.0);

/// A modal layer over the window: a dimmed backdrop that fades, and `placed`,
/// a window-sized layer that positions the dialog, entering as `entrance`.
pub(super) fn modal(a: &Appearance, placed: El, entrance: Entrance) -> El {
    let motion = a.motion();
    motion.present(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(motion.present(
                el().absolute(0.0, 0.0).w_full().h_full().fill(a.scrim()),
                Entrance::Fade,
            ))
            .child(motion.present(placed.absolute(0.0, 0.0).w_full().h_full(), entrance)),
        Entrance::Holder,
    )
}

#[cfg(test)]
mod tests;
