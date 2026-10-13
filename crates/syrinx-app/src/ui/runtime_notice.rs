//! Shows D36's notice when the WebView2 runtime changes, with an explicit open-in-Edge action (P1-35).

use std::collections::BTreeMap;

use gpui::{
    Context, Div, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, div, px,
};
use syrinx_core::web_address;
use syrinx_platform::edge::open_in_edge;

use super::{BrowserView, button};
use crate::{session::SessionCommand, theme};

/// The setting holding the last runtime version Syrinx saw (`docs/p1-34-35-instance-runtime.md`).
pub(super) const RUNTIME_VERSION_SETTING: &str = "webview2.runtime_version";

/// The notice strip's height; the page card moves down by this much while it shows.
pub(super) const NOTICE_HEIGHT: f32 = 36.;

/// A runtime update between the last start-up and this one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VersionChange {
    pub(super) previous: String,
    pub(super) current: String,
}

/// Compares the stored and running versions once both are known.
#[derive(Default)]
pub(super) struct RuntimeNotice {
    /// `None` until settings load; then the stored version, if any.
    stored: Option<Option<String>>,
    running: Option<String>,
    compared: bool,
    unsaved: Option<String>,
    shown: Option<VersionChange>,
}

impl RuntimeNotice {
    pub(super) fn settings_loaded(&mut self, settings: &BTreeMap<String, String>) {
        self.stored = Some(settings.get(RUNTIME_VERSION_SETTING).cloned());
        self.compare();
    }

    /// Records the running version; returns false when it cannot be stored or compared.
    pub(super) fn observed(&mut self, version: Option<String>) -> bool {
        let Some(version) = version.filter(|version| fits_setting(version)) else {
            return false;
        };
        self.running = Some(version);
        self.compare();
        true
    }

    /// The version setting to save, once, after a first run or a change.
    pub(super) fn take_save(&mut self) -> Option<(String, String)> {
        let version = self.unsaved.take()?;
        Some((RUNTIME_VERSION_SETTING.to_owned(), version))
    }

    pub(super) fn change(&self) -> Option<&VersionChange> {
        self.shown.as_ref()
    }

    pub(super) fn dismiss(&mut self) {
        self.shown = None;
    }

    fn compare(&mut self) {
        if self.compared {
            return;
        }
        let (Some(stored), Some(running)) = (&self.stored, &self.running) else {
            return;
        };
        self.compared = true;
        // A first run has nothing to compare with, so it is remembered without a notice.
        if stored.as_ref() == Some(running) {
            return;
        }
        if let Some(previous) = stored {
            self.shown = Some(VersionChange {
                previous: previous.clone(),
                current: running.clone(),
            });
        }
        self.unsaved = Some(running.clone());
    }
}

fn fits_setting(version: &str) -> bool {
    !version.is_empty() && version.len() <= syrinx_store::MAX_SETTING_VALUE_BYTES
}

impl BrowserView {
    /// Compares the stored version as soon as settings arrive at start-up.
    pub(super) fn runtime_settings_loaded(&mut self, settings: &BTreeMap<String, String>) {
        self.runtime.settings_loaded(settings);
        self.save_runtime_version();
    }

    pub(super) fn runtime_version_observed(&mut self, version: Option<String>) {
        if !self.runtime.observed(version) {
            self.message = "Syrinx could not read the WebView2 version, so it cannot tell you \
                            when an update might affect sign-in."
                .to_owned();
            return;
        }
        self.save_runtime_version();
    }

    /// Saves the running version once the session accepts settings; until then it waits.
    pub(super) fn save_runtime_version(&mut self) {
        if self.session_revision.is_none() || self.closing {
            return;
        }
        if let Some(setting) = self.runtime.take_save() {
            self.command(SessionCommand::SaveSettings(vec![setting]));
        }
    }

    pub(super) fn notice_height(&self) -> f32 {
        if self.runtime.change().is_some() {
            NOTICE_HEIGHT
        } else {
            0.
        }
    }

    /// The one-line notice under the toolbar, or nothing when the runtime is unchanged.
    pub(super) fn runtime_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let change = self.runtime.change()?;
        let text = notice_text(change);
        let mut banner = div()
            .h(px(NOTICE_HEIGHT))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .bg(theme::panel())
            .text_xs()
            .child(
                // Named so a screen reader can read the whole notice, not just its buttons.
                div()
                    .id("runtime-notice-text")
                    .role(gpui::Role::Label)
                    .aria_label(text.clone())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(text),
            );
        // Edge is offered only for a web page, and opens only when the owner clicks.
        if self.active_web_address().is_some() {
            banner = banner.child(button("open-in-edge", "Open in Edge").on_click(cx.listener(
                |this, _, _, cx| {
                    this.open_active_in_edge();
                    cx.notify();
                },
            )));
        }
        Some(banner.child(
            button("dismiss-runtime-notice", "Dismiss").on_click(cx.listener(|this, _, _, cx| {
                this.runtime.dismiss();
                cx.notify();
            })),
        ))
    }

    fn active_web_address(&self) -> Option<String> {
        let tab = self
            .model
            .tabs
            .iter()
            .find(|tab| Some(tab.id) == self.model.active_id)?;
        // Home and about:blank tabs have no web address to hand over.
        web_address(&tab.metadata.address).ok()
    }

    fn open_active_in_edge(&mut self) {
        let Some(address) = self.active_web_address() else {
            self.message = "Open a web page first, then send it to Edge.".to_owned();
            return;
        };
        self.message = match open_in_edge(&address) {
            Ok(()) => "Opened this page in Microsoft Edge.".to_owned(),
            Err(error) => error.to_string(),
        };
    }
}

fn notice_text(change: &VersionChange) -> String {
    format!(
        "WebView2 updated from {} to {}. Google sign-in can change after a runtime update; \
         if it stops working, open the page in Edge.",
        change.previous, change.current
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(version: Option<&str>) -> BTreeMap<String, String> {
        version
            .map(|version| (RUNTIME_VERSION_SETTING.to_owned(), version.to_owned()))
            .into_iter()
            .collect()
    }

    fn saved(version: &str) -> Option<(String, String)> {
        Some((RUNTIME_VERSION_SETTING.to_owned(), version.to_owned()))
    }

    #[test]
    fn a_first_run_remembers_the_version_without_a_notice() {
        let mut notice = RuntimeNotice::default();
        assert!(notice.observed(Some("140.0.1.2".to_owned())));
        assert_eq!(notice.take_save(), None, "settings have not loaded yet");
        notice.settings_loaded(&settings(None));
        assert_eq!(notice.change(), None);
        assert_eq!(notice.take_save(), saved("140.0.1.2"));
        assert_eq!(notice.take_save(), None, "saved once");
    }

    #[test]
    fn an_unchanged_runtime_shows_and_saves_nothing() {
        let mut notice = RuntimeNotice::default();
        notice.settings_loaded(&settings(Some("140.0.1.2")));
        assert!(notice.observed(Some("140.0.1.2".to_owned())));
        assert_eq!(notice.change(), None);
        assert_eq!(notice.take_save(), None);
    }

    #[test]
    fn a_changed_runtime_shows_the_notice_until_dismissed() {
        for settings_first in [true, false] {
            let mut notice = RuntimeNotice::default();
            if settings_first {
                notice.settings_loaded(&settings(Some("140.0.1.2")));
                assert!(notice.observed(Some("141.0.3.4".to_owned())));
            } else {
                assert!(notice.observed(Some("141.0.3.4".to_owned())));
                notice.settings_loaded(&settings(Some("140.0.1.2")));
            }
            let change = notice.change().expect("notice shown").clone();
            assert_eq!(change.previous, "140.0.1.2");
            assert_eq!(change.current, "141.0.3.4");
            assert!(notice_text(&change).contains("open the page in Edge"));
            assert_eq!(notice.take_save(), saved("141.0.3.4"));
            notice.dismiss();
            assert_eq!(notice.change(), None);
        }
    }

    #[test]
    fn an_unreadable_version_is_neither_compared_nor_saved() {
        let mut notice = RuntimeNotice::default();
        notice.settings_loaded(&settings(Some("140.0.1.2")));
        assert!(!notice.observed(None));
        assert!(!notice.observed(Some(String::new())));
        assert!(!notice.observed(Some("9".repeat(syrinx_store::MAX_SETTING_VALUE_BYTES + 1))));
        assert_eq!(notice.change(), None);
        assert_eq!(notice.take_save(), None);
    }
}
