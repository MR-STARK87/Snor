//! Dim Mode: one action that lowers the screen while agents keep running.
//!
//! The manager only ever touches display brightness through the caller-fed
//! backend (`crate::brightness` in the app, fakes in tests). It never looks
//! at tabs, ptys or child processes, so toggling cannot suspend, close or
//! otherwise disturb a running terminal or agent.
//!
//! Save/restore discipline:
//!
//! * The pre-dim level is saved exactly once, on the transition from
//!   inactive to active. Re-activating while already dimmed is a no-op, so a
//!   stray second toggle cannot overwrite the restore value.
//! * While active the saved value is never re-read, so if the user changes
//!   the brightness by hand (laptop keys, Settings) the original restore
//!   value survives intact.
//! * Deactivating restores the saved level verbatim. If the restore fails,
//!   the manager stays active with the value intact so retrying can still
//!   bring the screen back, and the error waits in [`DimManager::notice`].

use crate::brightness::DEFAULT_DIM_LEVEL;

pub struct DimManager {
    dim_level: u8,
    saved: Option<u8>,
    notice: Option<String>,
}

impl DimManager {
    pub fn new() -> Self {
        Self {
            dim_level: DEFAULT_DIM_LEVEL,
            saved: None,
            notice: None,
        }
    }

    /// Whether the screen is currently dimmed by us.
    pub fn is_active(&self) -> bool {
        self.saved.is_some()
    }

    /// The level applied while dimmed.
    pub fn dim_level(&self) -> u8 {
        self.dim_level
    }

    /// A non-blocking status message from the last failed backend call, if
    /// any. The UI shows it in the status bar; it clears on the next toggle.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// Flip Dim Mode. Backend failures leave a [`DimManager::notice`] and
    /// never panic, so the app and its terminals keep working.
    pub fn toggle(
        &mut self,
        get: impl FnOnce() -> Result<u8, String>,
        set: impl Fn(u8) -> Result<(), String>,
    ) {
        self.clear_notice();
        if self.is_active() {
            self.deactivate(&set);
        } else {
            self.activate(get, &set);
        }
    }

    /// Dim the screen, remembering the current level for the way back.
    /// A second call while active does nothing: the saved level is the one
    /// that was live *before* Dim Mode, not a dimmed re-read of it.
    pub fn activate(
        &mut self,
        get: impl FnOnce() -> Result<u8, String>,
        set: impl Fn(u8) -> Result<(), String>,
    ) {
        if self.is_active() {
            return;
        }
        let current = match get() {
            Ok(level) => level,
            Err(e) => {
                self.notice = Some(e);
                return;
            }
        };
        if let Err(e) = set(self.dim_level) {
            self.notice = Some(e);
            return;
        }
        self.saved = Some(current);
    }

    /// Bring the screen back to the level saved on activation.
    pub fn deactivate(&mut self, set: impl Fn(u8) -> Result<(), String>) {
        let Some(original) = self.saved else {
            return;
        };
        match set(original) {
            Ok(()) => self.saved = None,
            Err(e) => self.notice = Some(e),
        }
    }

    /// Best-effort restore for shutdown (app exit while dimmed). Always
    /// clears the saved value so a second call is a no-op, and never fails:
    /// shutdown must not panic or hang on display control.
    pub fn restore_on_exit(&mut self, set: impl Fn(u8) -> Result<(), String>) {
        if let Some(original) = self.saved.take() {
            let _ = set(original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A scripted backend: `levels` are what `get` returns in order, `sets`
    /// records every `set`, and `fail_on` makes those set values fail.
    struct Fake {
        levels: RefCell<Vec<u8>>,
        sets: RefCell<Vec<u8>>,
        fail_on: Vec<u8>,
    }

    impl Fake {
        fn new(levels: &[u8]) -> Self {
            Self {
                levels: RefCell::new(levels.iter().rev().copied().collect()),
                sets: RefCell::new(Vec::new()),
                fail_on: Vec::new(),
            }
        }

        fn get(&self) -> Result<u8, String> {
            self.levels
                .borrow_mut()
                .pop()
                .ok_or_else(|| "no reading".to_string())
        }

        fn set(&self, level: u8) -> Result<(), String> {
            self.sets.borrow_mut().push(level);
            if self.fail_on.contains(&level) {
                return Err(format!("backend refused {level}"));
            }
            Ok(())
        }
    }

    #[test]
    fn toggling_on_dims_and_toggling_off_restores_exactly() {
        let fake = Fake::new(&[80]);
        let mut dim = DimManager::new();
        dim.toggle(|| fake.get(), |v| fake.set(v));
        assert!(dim.is_active());
        assert_eq!(*fake.sets.borrow(), vec![10]);
        dim.toggle(|| fake.get(), |v| fake.set(v));
        assert!(!dim.is_active());
        assert_eq!(*fake.sets.borrow(), vec![10, 80]);
    }

    #[test]
    fn activating_while_active_keeps_the_original_restore_value() {
        let fake = Fake::new(&[80, 50]);
        let mut dim = DimManager::new();
        dim.activate(|| fake.get(), |v| fake.set(v));
        // A stray second activation (e.g. double-pressed shortcut) must not
        // re-read the dimmed screen as the new "original".
        dim.activate(|| fake.get(), |v| fake.set(v));
        assert_eq!(*fake.sets.borrow(), vec![10]);
        dim.deactivate(|v| fake.set(v));
        assert_eq!(*fake.sets.borrow(), vec![10, 80]);
    }

    #[test]
    fn repeated_cycles_save_fresh_each_time() {
        let fake = Fake::new(&[80, 60]);
        let mut dim = DimManager::new();
        dim.toggle(|| fake.get(), |v| fake.set(v));
        dim.toggle(|| fake.get(), |v| fake.set(v));
        dim.toggle(|| fake.get(), |v| fake.set(v));
        dim.toggle(|| fake.get(), |v| fake.set(v));
        assert_eq!(*fake.sets.borrow(), vec![10, 80, 10, 60]);
        assert!(!dim.is_active());
    }

    #[test]
    fn user_brightness_change_while_dimmed_does_not_corrupt_restore() {
        let fake = Fake::new(&[80]);
        let mut dim = DimManager::new();
        dim.activate(|| fake.get(), |v| fake.set(v));
        // The user lowers the brightness by hand mid-session; the manager
        // never re-reads while active, so the pre-dim value survives.
        dim.activate(|| Ok::<u8, String>(30), |v| fake.set(v));
        dim.deactivate(|v| fake.set(v));
        assert_eq!(*fake.sets.borrow(), vec![10, 80]);
    }

    #[test]
    fn restore_on_exit_bring_back_the_screen_and_clears() {
        let fake = Fake::new(&[80]);
        let mut dim = DimManager::new();
        dim.activate(|| fake.get(), |v| fake.set(v));
        dim.restore_on_exit(|v| fake.set(v));
        assert!(!dim.is_active());
        assert_eq!(*fake.sets.borrow(), vec![10, 80]);
        // A second call (Drop after on_exit, say) is a harmless no-op.
        dim.restore_on_exit(|v| fake.set(v));
        assert_eq!(*fake.sets.borrow(), vec![10, 80]);
    }

    #[test]
    fn failed_query_leaves_everything_untouched_and_reports() {
        let fake = Fake::new(&[]);
        let mut dim = DimManager::new();
        dim.toggle(|| fake.get(), |v| fake.set(v));
        assert!(!dim.is_active());
        assert!(fake.sets.borrow().is_empty());
        assert!(dim.notice().is_some());
    }

    #[test]
    fn failed_dim_set_does_not_claim_to_be_active() {
        let mut fake = Fake::new(&[80]);
        fake.fail_on = vec![10];
        let mut dim = DimManager::new();
        dim.toggle(|| fake.get(), |v| fake.set(v));
        assert!(!dim.is_active(), "a failed dim must not look active");
        assert!(dim.notice().is_some());
    }

    #[test]
    fn failed_restore_stays_active_so_retry_can_recover() {
        let mut fake = Fake::new(&[80]);
        fake.fail_on = vec![80];
        let mut dim = DimManager::new();
        dim.activate(|| fake.get(), |v| fake.set(v));
        dim.deactivate(|v| fake.set(v));
        assert!(dim.is_active());
        assert!(dim.notice().is_some());
    }

    #[test]
    fn deactivating_while_idle_is_a_no_op() {
        let fake = Fake::new(&[]);
        let mut dim = DimManager::new();
        dim.deactivate(|v| fake.set(v));
        assert!(fake.sets.borrow().is_empty());
        assert!(dim.notice().is_none());
    }
}
