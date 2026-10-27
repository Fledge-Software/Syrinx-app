//! The window's side of page-text capture (#54): the owner's setting and excluded sites, the
//! policy the page host reads by, and which captured text the session worker keeps (D35, D40).

use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use syrinx_core::{PageTextPolicy, PolicyError, TabId, TabModelSnapshot, excluded_site};
use syrinx_platform::browser::{BrowserCommand, CapturedText, PageTextOutcome};

use super::BrowserView;
use crate::session::{CapturedPage, PageTextSetting, SessionCommand};

/// The capture setting as the window holds it, and the tabs whose next page must not be read.
#[derive(Default)]
pub(super) struct PageTextCapture {
    setting: PageTextSetting,
    /// Set when the stored list could not be read; capture stays off until switched on again.
    unreadable: bool,
    /// The policy the page host was last sent, rebuilt whenever the setting changes.
    policy: Option<PageTextPolicy>,
    /// Tabs whose next page puts back an earlier one: restored at start-up, reopened or
    /// duplicated. Taint is not saved, so such a page is never read until the owner
    /// navigates the tab (P1-25's production recommendation).
    reopened: HashSet<TabId>,
}

impl PageTextCapture {
    /// Takes the stored setting, keeping what is known about the tabs.
    pub(super) fn restore(&mut self, settings: &BTreeMap<String, String>) {
        let restored = PageTextSetting::restored(settings);
        self.setting = restored.setting;
        self.unreadable = restored.unreadable;
        self.rebuild_policy();
    }

    pub(super) fn is_on(&self) -> bool {
        self.setting.on
    }

    pub(super) fn excluded(&self) -> &[String] {
        &self.setting.excluded
    }

    /// Whether the stored list could not be read, which keeps capture off.
    pub(super) fn unreadable(&self) -> bool {
        self.unreadable
    }

    /// The policy for the page host: `None` while capture is off.
    pub(super) fn policy(&self) -> Option<PageTextPolicy> {
        self.policy.clone()
    }

    /// Switches capture on or off; returns the setting to save.
    pub(super) fn switch(&mut self, on: bool) -> PageTextSetting {
        self.setting.on = on;
        self.unreadable = false;
        self.rebuild_policy();
        self.setting.clone()
    }

    /// Adds a site to the exclusion list; returns the setting to save, or why it was refused.
    pub(super) fn add_exclusion(&mut self, entry: &str) -> Result<PageTextSetting, &'static str> {
        let site = excluded_site(entry)
            .map_err(|_| "Enter a site such as example.com, or paste its address.")?;
        if self.setting.excluded.contains(&site) {
            return Err("That site is already excluded.");
        }
        self.setting.excluded.push(site);
        self.rebuild_policy();
        Ok(self.setting.clone())
    }

    /// Takes a site off the exclusion list; returns the setting to save.
    pub(super) fn remove_exclusion(&mut self, site: &str) -> PageTextSetting {
        self.setting.excluded.retain(|excluded| excluded != site);
        self.rebuild_policy();
        self.setting.clone()
    }

    /// Keeps the tab marks level with a new model: at start-up every restored tab is marked
    /// reopened, later only the tabs the worker reports as reopened or duplicated.
    pub(super) fn tabs_changed(
        &mut self,
        model: &TabModelSnapshot,
        initial: bool,
        reopened: &[TabId],
    ) {
        if initial {
            self.reopened.extend(model.tabs.iter().map(|tab| tab.id));
        }
        self.reopened.extend(reopened.iter().copied());
        self.reopened
            .retain(|id| model.tabs.iter().any(|tab| tab.id == *id));
    }

    /// The owner chose an address for the tab, so its next page is Syrinx's own navigation.
    pub(super) fn owner_navigated(&mut self, id: TabId) {
        self.reopened.remove(&id);
    }

    /// The command that creates `id`'s page: reopened tabs start tainted (D35).
    pub(super) fn create_command(&self, id: TabId, url: String) -> BrowserCommand {
        if self.reopened.contains(&id) {
            BrowserCommand::Reopen { id: id.get(), url }
        } else {
            BrowserCommand::Create { id: id.get(), url }
        }
    }

    /// The page now exists, and the page host carries its taint from here.
    pub(super) fn page_created(&mut self, id: TabId) {
        self.reopened.remove(&id);
    }

    /// The text to keep from a capture, judged by the setting now rather than when the
    /// snapshot was taken: one already parsing when capture was switched off, or when the
    /// owner excluded its site, still arrives and is dropped here. A capture whose visit time
    /// is unknown is dropped too, since no clear could be matched against it.
    pub(super) fn keepable(
        &self,
        captured: CapturedText,
        title: Option<&str>,
    ) -> Option<CapturedPage> {
        let policy = self.policy.as_ref()?;
        if policy.address_allowed(&captured.address).is_err() {
            return None;
        }
        if captured.text.trim().is_empty() {
            return None;
        }
        let visited_at = unix_seconds(captured.visited_at?)?;
        let address = without_fragment(&captured.address).to_owned();
        let title = title.unwrap_or(&address).to_owned();
        Some(CapturedPage {
            address,
            title,
            text: captured.text,
            visited_at,
        })
    }

    fn rebuild_policy(&mut self) {
        if !self.setting.on {
            self.policy = None;
            return;
        }
        // Every listed site was normalized when added, so this should not fail; if it does,
        // capture goes off and says so rather than read without the owner's list.
        match PageTextPolicy::new(&self.setting.excluded) {
            Ok(policy) => self.policy = Some(policy),
            Err(PolicyError) => {
                self.setting.on = false;
                self.unreadable = true;
                self.policy = None;
            }
        }
    }
}

/// Whole seconds since the Unix epoch, as the index dates documents.
fn unix_seconds(time: SystemTime) -> Option<i64> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok()
}

/// The address the index keys a page by: a fragment names a place in the same page.
fn without_fragment(address: &str) -> &str {
    address.split_once('#').map_or(address, |(page, _)| page)
}

impl BrowserView {
    /// Reads the stored setting before the first state arrives, and tells the page host.
    pub(super) fn restore_page_text(&mut self, settings: &BTreeMap<String, String>) {
        self.page_text.restore(settings);
        if self.browser.is_some() {
            self.send_page_text_policy();
        }
    }

    /// Sends the page host the current policy. Commands keep their order, so every page
    /// created after this is observed, or not, by it.
    pub(super) fn send_page_text_policy(&mut self) {
        let policy = self.page_text.policy();
        self.page_command(BrowserCommand::SetPageTextPolicy(policy));
    }

    /// Saves and applies a changed setting; the worker erases what it no longer allows.
    pub(super) fn page_text_setting_changed(&mut self, setting: PageTextSetting) {
        self.send_page_text_policy();
        self.command(SessionCommand::SavePageTextSetting(setting));
    }

    /// Hands a capture the setting still allows to the worker; every other outcome is only
    /// P1-25's evidence and is dropped.
    pub(super) fn page_text_arrived(&mut self, id: u64, outcome: PageTextOutcome) {
        let PageTextOutcome::Captured(captured) = outcome else {
            return;
        };
        let title = self.captured_page_title(id, &captured.address);
        if let Some(page) = self.page_text.keepable(captured, title.as_deref()) {
            self.command(SessionCommand::StorePageText(page));
        }
    }

    /// The tab's title, only while the tab still shows the captured page.
    fn captured_page_title(&self, id: u64, address: &str) -> Option<String> {
        let tab = self.model.tabs.iter().find(|tab| tab.id.get() == id)?;
        let showing = without_fragment(&tab.metadata.address) == without_fragment(address);
        (showing && !tab.metadata.title.trim().is_empty()).then(|| tab.metadata.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    /// When every synthetic page below was visited.
    const VISITED_AT: i64 = 1_700_000_000;

    fn captured(address: &str, text: &str) -> CapturedText {
        CapturedText {
            address: address.to_owned(),
            text: text.to_owned(),
            documents: 1,
            truncated: false,
            response_bytes: text.len(),
            call_time: Duration::ZERO,
            parse_time: Duration::ZERO,
            visited_at: Some(UNIX_EPOCH + Duration::from_secs(VISITED_AT as u64)),
        }
    }

    fn switched_on() -> PageTextCapture {
        let mut capture = PageTextCapture::default();
        capture.switch(true);
        capture
    }

    #[test]
    fn capture_is_off_until_the_owner_switches_it_on() {
        let mut capture = PageTextCapture::default();
        capture.restore(&BTreeMap::new());
        assert!(!capture.is_on());
        assert!(capture.policy().is_none());
        let saved = capture.switch(true);
        capture.restore(&saved.entries().into_iter().collect());
        assert!(capture.is_on());
        assert!(capture.policy().is_some());
    }

    #[test]
    fn an_unreadable_site_list_keeps_capture_off() {
        let mut capture = PageTextCapture::default();
        let stored = PageTextSetting {
            on: true,
            excluded: vec!["owner.test".to_owned(), "not/a/site".to_owned()],
        };
        capture.restore(&stored.entries().into_iter().collect());
        assert!(capture.unreadable());
        assert!(!capture.is_on());
        assert!(capture.policy().is_none());
        assert_eq!(capture.excluded(), ["owner.test"]);
    }

    #[test]
    fn exclusions_are_normalized_and_refused_twice() {
        let mut capture = switched_on();
        capture
            .add_exclusion("https://Owner.test/account")
            .expect("synthetic site");
        let saved = capture
            .add_exclusion("second.example.test")
            .expect("second site");
        assert_eq!(saved.excluded, ["owner.test", "second.example.test"]);
        assert!(
            capture.add_exclusion("owner.test").is_err(),
            "no duplicate entries"
        );
        assert!(capture.add_exclusion("has space.test").is_err());
        let saved = capture.remove_exclusion("owner.test");
        assert_eq!(saved.excluded, ["second.example.test"]);
    }

    #[test]
    fn restoring_the_setting_keeps_the_reopened_tabs() {
        let (model, ids) = model_with(&["https://a.example.test/"]);
        let mut capture = switched_on();
        capture.tabs_changed(&model.snapshot(), true, &[]);
        capture.restore(&BTreeMap::new());
        assert!(reopens(&capture, ids[0]));
    }

    #[test]
    fn capture_off_keeps_nothing_even_if_a_snapshot_was_already_parsing() {
        let mut capture = switched_on();
        capture.switch(false);
        let page = capture.keepable(
            captured("https://page.example.test/", "synthetic words"),
            None,
        );
        assert!(page.is_none());
    }

    #[test]
    fn excluded_and_built_in_sites_are_rechecked_when_text_arrives() {
        let mut capture = switched_on();
        capture.add_exclusion("owner.test").expect("synthetic site");
        for address in [
            "https://www.owner.test/",
            "https://mail.google.com/",
            "https://www.youtube.com/",
            "about:blank",
        ] {
            assert!(
                capture
                    .keepable(captured(address, "synthetic words"), None)
                    .is_none(),
                "{address}"
            );
        }
    }

    #[test]
    fn an_allowed_page_is_kept_by_its_address_without_a_fragment_and_dated_by_its_visit() {
        let capture = switched_on();
        let page = capture
            .keepable(
                captured("https://page.example.test/a?b=1#section", "synthetic words"),
                Some("Synthetic title"),
            )
            .expect("an allowed synthetic page");
        assert_eq!(page.address, "https://page.example.test/a?b=1");
        assert_eq!(page.title, "Synthetic title");
        assert_eq!(page.text, "synthetic words");
        assert_eq!(page.visited_at, VISITED_AT);
        let untitled = capture
            .keepable(captured("https://page.example.test/", "words"), None)
            .expect("an untitled synthetic page");
        assert_eq!(untitled.title, "https://page.example.test/");
        assert!(
            capture
                .keepable(captured("https://page.example.test/", " \n "), None)
                .is_none()
        );
    }

    #[test]
    fn a_capture_with_no_visit_time_is_not_kept() {
        let capture = switched_on();
        let mut undated = captured("https://page.example.test/", "synthetic words");
        undated.visited_at = None;
        let page = capture.keepable(undated, None);
        assert!(page.is_none(), "no visit time to match a clear against");
    }

    fn model_with(addresses: &[&str]) -> (TabModel, Vec<TabId>) {
        let mut model = TabModel::new();
        let ids = addresses
            .iter()
            .map(|address| {
                model
                    .open(
                        TabKind::Web,
                        TabMetadata {
                            address: (*address).to_owned(),
                            ..TabMetadata::default()
                        },
                        Activation::Foreground,
                    )
                    .expect("open synthetic tab")
            })
            .collect();
        (model, ids)
    }

    fn reopens(capture: &PageTextCapture, id: TabId) -> bool {
        matches!(
            capture.create_command(id, "https://page.example.test/".into()),
            BrowserCommand::Reopen { .. }
        )
    }

    #[test]
    fn restored_tabs_are_reopened_until_the_owner_navigates_them() {
        let (mut model, ids) = model_with(&["https://a.example.test/", "https://b.example.test/"]);
        let mut capture = switched_on();
        capture.tabs_changed(&model.snapshot(), true, &[]);
        assert!(ids.iter().all(|id| reopens(&capture, *id)));
        capture.owner_navigated(ids[0]);
        assert!(!reopens(&capture, ids[0]));
        capture.page_created(ids[1]);
        assert!(
            !reopens(&capture, ids[1]),
            "the page host carries taint once created"
        );
        // A tab opened later is the owner's own choice; one reported reopened is not.
        let fresh = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open synthetic tab");
        let reopened = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open synthetic tab");
        capture.tabs_changed(&model.snapshot(), false, &[reopened]);
        assert!(!reopens(&capture, fresh));
        assert!(reopens(&capture, reopened));
        // A closed tab's mark goes with it.
        model.close(reopened).expect("close synthetic tab");
        capture.tabs_changed(&model.snapshot(), false, &[]);
        assert!(capture.reopened.is_empty());
    }
}
