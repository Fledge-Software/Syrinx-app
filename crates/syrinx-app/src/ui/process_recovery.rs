//! Displays controller failures without closing, reopening or reordering the session's tabs.

use super::*;

#[derive(Default)]
pub(super) struct CrashState {
    pages: HashSet<u64>,
    sequence: u64,
    recovering: bool,
    engine_stopped: bool,
}

impl CrashState {
    fn mark(&mut self, id: u64, model: &TabModelSnapshot) -> bool {
        model.tabs.iter().any(|tab| tab.id.get() == id) && self.pages.insert(id)
    }
    fn start(&mut self, sequence: u64) -> bool {
        if sequence <= self.sequence {
            return false;
        }
        self.sequence = sequence;
        self.recovering = true;
        self.engine_stopped = false;
        true
    }
    pub(super) fn finished(&mut self, sequence: u64, stopped: bool) -> bool {
        if self.sequence != sequence || !self.recovering {
            return false;
        }
        self.recovering = false;
        self.engine_stopped = stopped;
        true
    }

    fn heading(&self) -> &'static str {
        if self.recovering {
            "Recovering page engine…"
        } else if self.engine_stopped {
            "Browser engine stopped"
        } else {
            "Page crashed"
        }
    }
    /// Whether this page shows the crash card instead of its web view.
    pub(super) fn is_crashed(&self, id: u64) -> bool {
        self.pages.contains(&id)
    }
    pub(super) fn restored(&mut self, id: u64) {
        self.pages.remove(&id);
    }
    pub(super) fn retain(&mut self, model: &TabModelSnapshot) {
        self.pages
            .retain(|id| model.tabs.iter().any(|tab| tab.id.get() == *id));
    }
}

impl BrowserView {
    pub(super) fn recovery_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let active = self.shown.map(TabId::get);
        let page = recovery_focus_target(
            active,
            active.is_some_and(|id| self.crashes.pages.contains(&id)),
            window.is_window_active(),
            self.palette.is_some() || self.find.is_some(),
        );
        if let Some(id) = page {
            self.page_command(BrowserCommand::Focus { id });
            return;
        }
        // Keep logical focus in GPUI without activating a Syrinx window in the background.
        if self.palette.is_some() || self.find.is_some() {
            return;
        }
        window.focus(&self.focus, cx);
        if window.is_window_active() {
            self.move_keys_to_chrome();
        }
    }

    pub(super) fn page_crashed(&mut self, id: u64, window: &mut Window) {
        if self.closing || !self.crashes.mark(id, &self.model) {
            return;
        }
        self.fullscreen_page_failed(id, window);
        self.forget_find(id, false);
        self.forget_permissions(id);
        self.connections.remove(&id);
        self.site_zoom.forget_page(id);
        if let Ok(id) = TabId::new(id) {
            self.navigation_status.forget(id);
        }
    }

    pub(super) fn browser_recovering(
        &mut self,
        sequence: u64,
        tabs: Vec<u64>,
        window: &mut Window,
    ) {
        if self.closing || !self.crashes.start(sequence) {
            return;
        }
        self.usage
            .recorder
            .error(syrinx_core::usage::ErrorCategory::BrowserProcessExited);
        for id in tabs {
            self.page_crashed(id, window);
        }
        self.message = "Recovering the page engine. Your tabs are retained.".into();
    }

    pub(super) fn crashed_page(&self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.model.active_id?.get();
        if !self.crashes.pages.contains(&id) {
            return None;
        }
        let mut card = div()
            .flex_1()
            .h_full()
            .bg(rgb(0x141519))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(div().text_2xl().child(self.crashes.heading()))
            .child("Your tab is still open. Reload to try again.");
        if !self.crashes.recovering {
            card = card.child(
                button("reload-crashed-page", "Reload").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if this
                            .model
                            .active_id
                            .is_some_and(|active| active.get() == id)
                            && this.crashes.pages.contains(&id)
                        {
                            this.page_command(BrowserCommand::Reload { id });
                            cx.notify();
                        }
                    },
                )),
            );
        }
        Some(card)
    }
}

// Recovery can restore page focus only when that page and its window can receive input.
fn recovery_focus_target(
    active: Option<u64>,
    crashed: bool,
    window_active: bool,
    overlay: bool,
) -> Option<u64> {
    active.filter(|_| !crashed && window_active && !overlay)
}

pub(super) fn smoke_process_failure(smoke: bool, event: &BrowserEvent, outcome: &AtomicU8) -> bool {
    if !smoke
        || !matches!(
            event,
            BrowserEvent::ProcessFailed { .. }
                | BrowserEvent::PageCrashed { .. }
                | BrowserEvent::RecoveryStarted { .. }
        )
    {
        return false;
    }
    outcome.store(2, Ordering::Release);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};

    #[test]
    fn recovery_notifications_preserve_session_identity_order_selection_and_saved_associations() {
        let mut model = TabModel::new();
        let first = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("synthetic first tab");
        model.save_page(first).expect("synthetic saved row");
        let second = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("synthetic second tab");
        let before = model.snapshot();
        let mut state = CrashState::default();
        assert!(state.mark(first.get(), &before));
        assert!(!state.mark(first.get(), &before));
        assert!(!state.pages.contains(&second.get()));
        assert!(state.start(1));
        assert!(!state.start(1));
        assert!(state.mark(second.get(), &before));
        state.restored(first.get());
        assert!(state.finished(1, false));
        assert!(!state.finished(1, false));
        assert!(state.pages.contains(&second.get()));
        assert_eq!(model.snapshot(), before);

        model.close(second).expect("close a failed tab");
        state.retain(&model.snapshot());
        assert!(!state.mark(second.get(), &model.snapshot()));
        assert!(state.pages.is_empty());
        assert!(state.start(2));
        assert!(!state.finished(1, false));
        assert!(state.finished(2, false));
    }

    #[test]
    fn capped_recovery_shows_stopped_until_owner_retry_starts() {
        let mut state = CrashState::default();
        assert!(state.start(1));
        assert!(state.finished(1, true));
        assert_eq!(state.heading(), "Browser engine stopped");
        assert!(!state.recovering, "Reload must remain available");
        assert!(!state.finished(0, false));
        assert_eq!(state.heading(), "Browser engine stopped");
        assert!(state.start(2));
        assert_eq!(state.heading(), "Recovering page engine…");
        assert!(state.finished(2, false));
        assert!(!state.engine_stopped);
    }

    #[test]
    fn every_process_failure_preserves_the_smoke_failure_outcome() {
        for kind in 0..=10 {
            let outcome = AtomicU8::new(0);
            let event = BrowserEvent::ProcessFailed { id: 7, kind };
            assert!(!smoke_process_failure(false, &event, &outcome));
            assert_eq!(outcome.load(Ordering::Acquire), 0);
            assert!(smoke_process_failure(true, &event, &outcome));
            assert_eq!(outcome.load(Ordering::Acquire), 2);
        }
        assert!(!smoke_process_failure(
            true,
            &BrowserEvent::Ready,
            &AtomicU8::new(0)
        ));
    }

    #[test]
    fn recovery_focus_requires_a_live_active_page_and_active_window() {
        assert_eq!(recovery_focus_target(Some(7), false, true, false), Some(7));
        assert_eq!(recovery_focus_target(Some(7), true, true, false), None);
        assert_eq!(recovery_focus_target(Some(7), false, false, false), None);
        assert_eq!(recovery_focus_target(None, false, true, false), None);
        assert_eq!(recovery_focus_target(Some(7), false, true, true), None);
    }
}
