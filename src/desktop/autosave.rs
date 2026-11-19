//! Autosave schedules use the same version-protected worker as explicit Save.
use super::*;
use config::AutosavePolicy;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Schedule {
    revision: u64,
    edited_at: Option<Instant>,
    attempted: Option<u64>,
    focus_due: bool,
}
impl Schedule {
    pub(super) fn remap_revision(&mut self, previous: u64, revision: u64) {
        // Moving a file invalidates scan/diff results without changing the text
        // or consuming its waiting autosave deadline/focus-loss transition.
        if self.revision == previous {
            self.revision = revision;
            if self.attempted == Some(previous) {
                self.attempted = Some(revision);
            }
        }
    }

    pub(super) fn suspend(&mut self, revision: u64) {
        self.revision = revision;
        self.attempted = Some(revision);
        self.focus_due = false;
    }
}

impl State {
    pub(super) fn poll_autosave(&mut self, tx: &mut Tx) -> bool {
        let preferences = self.config.autosave.clone();
        let policy_changed = self.workspace.autosave_preferences != preferences;
        self.workspace.autosave_preferences = preferences.clone();
        let now = Instant::now();
        // Inspecting this file's diff or using its toolbar/find controls stays
        // inside the same editor context. Raw widget blur is not a save action.
        let context = (!self.settings
            && !self.search_tab
            && self.workspace.layout.editor
            && !self.workspace.terminal_focus())
        .then(|| {
            self.workspace
                .tabs
                .get(self.workspace.active)
                .map(|tab| tab.id)
        })
        .flatten();
        let previous = self.workspace.autosave_context;
        self.workspace.autosave_context = context;
        for tab in &mut self.workspace.tabs {
            let schedule = &mut tab.autosave;
            if policy_changed || schedule.revision != tab.revision {
                schedule.revision = tab.revision;
                schedule.edited_at = Some(now);
                schedule.attempted = None;
                schedule.focus_due = false;
            }
            if !policy_changed && previous == Some(tab.id) && context != previous {
                schedule.focus_due = true;
            }
            if !tab.dirty() {
                tab.autosave.edited_at = None;
                tab.autosave.focus_due = false;
            }
        }
        // File operations and owner decisions retain their authority over saves.
        if self.workspace.operation_busy
            || self.workspace.removal.is_some()
            || self.pending.is_some()
            || self.shutdown_saved
        {
            return false;
        }
        let due: Vec<_> = self
            .workspace
            .tabs
            .iter()
            .filter(|tab| {
                tab.dirty()
                    && !tab.saving
                    && tab.autosave.attempted != Some(tab.revision)
                    && match preferences.policy {
                        AutosavePolicy::Explicit => false,
                        AutosavePolicy::AfterDelay => tab.autosave.edited_at.is_some_and(|at| {
                            let remaining = Duration::from_millis(preferences.delay_ms)
                                .saturating_sub(now.duration_since(at));
                            if !remaining.is_zero() {
                                tx.wake_after(remaining);
                            }
                            remaining.is_zero()
                        }),
                        AutosavePolicy::EditorFocusLoss => tab.autosave.focus_due,
                    }
            })
            .map(|tab| tab.id)
            .collect();
        let mut changed = false;
        for id in due {
            self.save_file(id, false, None);
            if let Some(tab) = self.workspace.tabs.iter_mut().find(|tab| tab.id == id)
                && tab.saving
            {
                tab.autosave.attempted = Some(tab.revision);
                tab.autosave.focus_due = false;
                changed = true;
            }
        }
        changed
    }
}
