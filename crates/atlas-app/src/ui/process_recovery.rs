//! Displays controller failures without closing, reopening or reordering the session's tabs.

use super::*;

#[derive(Default)]
pub(super) struct CrashState {
    pages: HashSet<u64>,
    sequence: u64,
    recovering: bool,
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
        true
    }
    pub(super) fn finished(&mut self, sequence: u64) -> bool {
        if self.sequence != sequence || !self.recovering {
            return false;
        }
        self.recovering = false;
        true
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
    pub(super) fn page_crashed(&mut self, id: u64, window: &mut Window) {
        if self.closing || !self.crashes.mark(id, &self.model) {
            return;
        }
        self.fullscreen_page_failed(id, window);
        self.forget_find(id, false);
        self.forget_permissions(id);
        self.connections.remove(&id);
        self.zoom_levels.remove(&id);
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
            .child(div().text_2xl().child(if self.crashes.recovering {
                "Recovering page engine…"
            } else {
                "Page crashed"
            }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::{Activation, TabKind, TabMetadata, TabModel};

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
        assert!(state.finished(1));
        assert!(!state.finished(1));
        assert!(state.pages.contains(&second.get()));
        assert_eq!(model.snapshot(), before);

        model.close(second).expect("close a failed tab");
        state.retain(&model.snapshot());
        assert!(!state.mark(second.get(), &model.snapshot()));
        assert!(state.pages.is_empty());
        assert!(state.start(2));
        assert!(!state.finished(1));
        assert!(state.finished(2));
    }
}
