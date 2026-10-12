//! Gives a page the entire viewport and restores the window state when it leaves fullscreen.

use super::*;

impl BrowserView {
    pub(super) fn page_fullscreen(&mut self, id: u64, active: bool, window: &mut Window) {
        if active && self.shown.is_some_and(|shown| shown.get() == id) && self.fullscreen.is_none()
        {
            let was_fullscreen = window.is_fullscreen();
            self.fullscreen = Some((id, was_fullscreen));
            if !was_fullscreen {
                window.toggle_fullscreen();
            }
        } else if !active && self.fullscreen.is_some_and(|(owner, _)| owner == id) {
            self.restore_fullscreen(window);
        }
        self.resize_pages(window);
    }

    pub(super) fn fullscreen_shortcut(&mut self, window: &mut Window) {
        if let Some((id, _)) = self.fullscreen {
            // Escape goes through the native page input path; no page script is executed.
            self.page_command(BrowserCommand::ExitFullscreen { id });
        } else {
            window.toggle_fullscreen();
        }
    }

    pub(super) fn restore_fullscreen_if_hidden(&mut self, window: &mut Window) {
        if self
            .fullscreen
            .is_some_and(|(id, _)| self.shown.map(TabId::get) != Some(id))
        {
            self.restore_fullscreen(window);
        }
    }

    fn restore_fullscreen(&mut self, window: &mut Window) {
        if let Some((_, was_fullscreen)) = self.fullscreen.take()
            && window.is_fullscreen() != was_fullscreen
        {
            window.toggle_fullscreen();
        }
    }
}
