//! Gives a page the entire viewport and restores the window state when it leaves fullscreen.

use super::*;

actions!(fullscreen, [ExitPageFullscreen]);

// Keep element fullscreen separate from the currently displayed window presentation.
#[derive(Default)]
pub(super) struct FullscreenPages {
    pages: HashSet<u64>,
    exiting: HashSet<u64>,
}

impl FullscreenPages {
    fn observe(&mut self, id: u64, active: bool) {
        if active {
            self.pages.insert(id);
        } else {
            self.pages.remove(&id);
            self.exiting.remove(&id);
        }
    }

    fn exit_target(&self, shown: Option<u64>) -> Option<u64> {
        shown.filter(|id| self.pages.contains(id))
    }

    fn presentation(&self, shown: Option<u64>) -> Option<u64> {
        self.exit_target(shown)
            .filter(|id| !self.exiting.contains(id))
    }

    fn request_exit(&mut self, shown: Option<u64>) -> Option<BrowserCommand> {
        let id = self.exit_target(shown)?;
        self.exiting.insert(id);
        Some(BrowserCommand::ExitFullscreen { id })
    }
}

impl BrowserView {
    pub(super) fn fullscreen_page_failed(&mut self, id: u64, window: &mut Window) {
        self.fullscreen_pages.observe(id, false);
        if self.fullscreen.is_some_and(|(owner, _)| owner == id) {
            self.restore_fullscreen(window);
        }
    }

    pub(super) fn page_fullscreen(&mut self, id: u64, active: bool, window: &mut Window) {
        self.fullscreen_pages.observe(id, active);
        self.restore_fullscreen_if_hidden(window);
    }

    pub(super) fn fullscreen_shortcut(&mut self, window: &mut Window) {
        if let Some(command) = self
            .fullscreen_pages
            .request_exit(self.shown.map(TabId::get))
        {
            // Restore chrome even if queuing or executing the native exit fails.
            self.restore_fullscreen(window);
            self.resize_pages(window);
            self.page_command(command);
        } else {
            window.toggle_fullscreen();
        }
    }

    pub(super) fn restore_fullscreen_if_hidden(&mut self, window: &mut Window) {
        self.fullscreen_pages
            .pages
            .retain(|id| self.model.tabs.iter().any(|tab| tab.id.get() == *id));
        self.fullscreen_pages
            .exiting
            .retain(|id| self.fullscreen_pages.pages.contains(id));
        let target = self
            .fullscreen_pages
            .presentation(self.shown.map(TabId::get));
        if self.fullscreen.is_some_and(|(id, _)| Some(id) != target) {
            self.restore_fullscreen(window);
        }
        if let Some(id) = target
            && self.fullscreen.is_none()
        {
            let was_fullscreen = window.is_fullscreen();
            self.fullscreen = Some((id, was_fullscreen));
            if !was_fullscreen {
                window.toggle_fullscreen();
            }
        }
        self.resize_pages(window);
    }

    fn restore_fullscreen(&mut self, window: &mut Window) {
        if let Some((_, was_fullscreen)) = self.fullscreen.take()
            && window.is_fullscreen() != was_fullscreen
        {
            window.toggle_fullscreen();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_tabs_restores_each_pages_fullscreen_presentation() {
        let mut state = FullscreenPages::default();
        state.observe(7, true);
        assert_eq!(state.presentation(Some(7)), Some(7));
        assert_eq!(state.presentation(Some(8)), None);
        assert_eq!(state.presentation(Some(7)), Some(7));
        state.observe(8, true);
        assert_eq!(state.presentation(Some(8)), Some(8));
        state.observe(7, false);
        assert_eq!(state.presentation(Some(7)), None);
    }

    #[test]
    fn failed_exit_restores_chrome_and_keeps_a_native_retry_target() {
        let mut state = FullscreenPages::default();
        state.observe(7, true);
        for _ in 0..2 {
            assert!(matches!(
                state.request_exit(Some(7)),
                Some(BrowserCommand::ExitFullscreen { id: 7 })
            ));
            // No successful native callback: chrome still stays restored, and F11 retries exit.
            assert_eq!(state.presentation(Some(7)), None);
        }
        state.observe(7, false);
        assert!(state.request_exit(Some(7)).is_none());
    }
}
