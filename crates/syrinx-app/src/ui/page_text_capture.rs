//! The window's side of page-text capture (#54): the owner's setting and excluded sites, the
//! policy the page host reads by, and which captured text the session worker keeps (D35, D40).

use std::collections::{BTreeMap, HashSet};

use syrinx_core::{PageTextPolicy, PolicyError, TabId, TabModelSnapshot, excluded_site};
use syrinx_platform::browser::{BrowserCommand, CapturedText, PageTextOutcome};

use super::BrowserView;
use crate::session::{CapturedPage, SessionCommand};

/// Whether capture is on: `on`, or anything else for off, so an unreadable value reads nothing.
pub(super) const CAPTURE_KEY: &str = "page_text.capture";
/// The owner's excluded sites: normalized host names, separated by spaces, in the order added.
pub(super) const EXCLUDED_SITES_KEY: &str = "page_text.excluded_sites";

const ON: &str = "on";
const OFF: &str = "off";

/// The capture setting as the window holds it, and the tabs whose next page must not be read.
#[derive(Default)]
pub(super) struct PageTextCapture {
    /// Off until the owner switches it on (D35).
    on: bool,
    excluded: Vec<String>,
    /// The policy the page host was last sent, rebuilt whenever the setting changes.
    policy: Option<PageTextPolicy>,
    /// Tabs whose next page puts back an earlier one: restored at start-up, reopened or
    /// duplicated. Taint is not saved, so such a page is never read until the owner
    /// navigates the tab (P1-25's production recommendation).
    reopened: HashSet<TabId>,
    /// Set when the stored list could not be read; capture stays off until switched on again.
    pub(super) unreadable: bool,
}

impl PageTextCapture {
    /// The stored setting; an unreadable site list keeps capture off rather than read a site
    /// the owner may have excluded.
    pub(super) fn restored(settings: &BTreeMap<String, String>) -> Self {
        let on = settings.get(CAPTURE_KEY).is_some_and(|value| value == ON);
        let (excluded, unreadable) = match settings.get(EXCLUDED_SITES_KEY) {
            None => (Vec::new(), false),
            Some(stored) => parse_sites(stored),
        };
        let mut capture = Self {
            on: on && !unreadable,
            excluded,
            unreadable,
            ..Self::default()
        };
        capture.rebuild_policy();
        capture
    }

    pub(super) fn is_on(&self) -> bool {
        self.on
    }

    pub(super) fn excluded(&self) -> &[String] {
        &self.excluded
    }

    /// The policy for the page host: `None` while capture is off.
    pub(super) fn policy(&self) -> Option<PageTextPolicy> {
        self.policy.clone()
    }

    /// Switches capture on or off; returns the settings to save.
    pub(super) fn switch(&mut self, on: bool) -> Vec<(String, String)> {
        self.on = on;
        self.unreadable = false;
        self.rebuild_policy();
        self.stored()
    }

    /// Adds a site to the exclusion list; returns the settings to save, or why it was refused.
    pub(super) fn add_exclusion(
        &mut self,
        entry: &str,
    ) -> Result<Vec<(String, String)>, &'static str> {
        let site = excluded_site(entry)
            .map_err(|_| "Enter a site such as example.com, or paste its address.")?;
        if self.excluded.contains(&site) {
            return Err("That site is already excluded.");
        }
        self.excluded.push(site);
        self.rebuild_policy();
        Ok(self.stored())
    }

    /// Takes a site off the exclusion list; returns the settings to save.
    pub(super) fn remove_exclusion(&mut self, site: &str) -> Vec<(String, String)> {
        self.excluded.retain(|excluded| excluded != site);
        self.rebuild_policy();
        self.stored()
    }

    /// Keeps the reopened marks level with a new model: at start-up every restored tab is
    /// marked, later only the tabs the worker reports as reopened or duplicated.
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
    /// owner excluded its site, still arrives and is dropped here.
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
        let address = without_fragment(&captured.address).to_owned();
        let title = title.unwrap_or(&address).to_owned();
        Some(CapturedPage {
            address,
            title,
            text: captured.text,
        })
    }

    fn rebuild_policy(&mut self) {
        if !self.on {
            self.policy = None;
            return;
        }
        // Every listed site was normalized when added, so this should not fail; if it does,
        // capture goes off and says so rather than read without the owner's list.
        match PageTextPolicy::new(&self.excluded) {
            Ok(policy) => self.policy = Some(policy),
            Err(PolicyError) => {
                self.on = false;
                self.unreadable = true;
                self.policy = None;
            }
        }
    }

    fn stored(&self) -> Vec<(String, String)> {
        let on = if self.on { ON } else { OFF };
        vec![
            (CAPTURE_KEY.to_owned(), on.to_owned()),
            (EXCLUDED_SITES_KEY.to_owned(), self.excluded.join(" ")),
        ]
    }
}

/// The stored sites, and whether any entry could not be read.
fn parse_sites(stored: &str) -> (Vec<String>, bool) {
    let mut sites = Vec::new();
    let mut unreadable = false;
    for entry in stored.split_whitespace() {
        match excluded_site(entry) {
            Ok(site) if !sites.contains(&site) => sites.push(site),
            Ok(_) => {}
            Err(_) => unreadable = true,
        }
    }
    (sites, unreadable)
}

/// The address the index keys a page by: a fragment names a place in the same page.
fn without_fragment(address: &str) -> &str {
    address.split_once('#').map_or(address, |(page, _)| page)
}

impl BrowserView {
    /// Reads the stored setting before the first state arrives, and tells the page host.
    pub(super) fn restore_page_text(&mut self, settings: &BTreeMap<String, String>) {
        let reopened = std::mem::take(&mut self.page_text.reopened);
        self.page_text = PageTextCapture {
            reopened,
            ..PageTextCapture::restored(settings)
        };
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

    /// Saves and applies a changed setting.
    pub(super) fn page_text_setting_changed(&mut self, entries: Vec<(String, String)>) {
        self.send_page_text_policy();
        self.command(SessionCommand::SaveSettings(entries));
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

    fn settings(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn captured(address: &str, text: &str) -> CapturedText {
        CapturedText {
            address: address.to_owned(),
            text: text.to_owned(),
            documents: 1,
            truncated: false,
            response_bytes: text.len(),
            call_time: Duration::ZERO,
            parse_time: Duration::ZERO,
        }
    }

    fn switched_on() -> PageTextCapture {
        let mut capture = PageTextCapture::default();
        capture.switch(true);
        capture
    }

    #[test]
    fn capture_is_off_until_the_owner_switches_it_on() {
        let fresh = PageTextCapture::restored(&BTreeMap::new());
        assert!(!fresh.is_on());
        assert!(fresh.policy().is_none());
        let odd = PageTextCapture::restored(&settings(&[(CAPTURE_KEY, "yes")]));
        assert!(
            !odd.is_on(),
            "only the exact stored value switches capture on"
        );
        let on = PageTextCapture::restored(&settings(&[(CAPTURE_KEY, ON)]));
        assert!(on.is_on());
        assert!(on.policy().is_some());
    }

    #[test]
    fn an_unreadable_site_list_keeps_capture_off() {
        let stored = settings(&[
            (CAPTURE_KEY, ON),
            (EXCLUDED_SITES_KEY, "owner.test not/a/site"),
        ]);
        let capture = PageTextCapture::restored(&stored);
        assert!(capture.unreadable);
        assert!(!capture.is_on());
        assert!(capture.policy().is_none());
        assert_eq!(capture.excluded(), ["owner.test"]);
    }

    #[test]
    fn the_setting_round_trips_through_the_stored_entries() {
        let mut capture = switched_on();
        capture
            .add_exclusion("https://Owner.test/account")
            .expect("synthetic site");
        let saved = capture
            .add_exclusion("second.example.test")
            .expect("second site");
        let restored = PageTextCapture::restored(&saved.into_iter().collect());
        assert!(restored.is_on());
        assert_eq!(restored.excluded(), ["owner.test", "second.example.test"]);
        assert!(
            capture.add_exclusion("owner.test").is_err(),
            "no duplicate entries"
        );
        assert!(capture.add_exclusion("has space.test").is_err());
        let saved = capture.remove_exclusion("owner.test");
        let restored = PageTextCapture::restored(&saved.into_iter().collect());
        assert_eq!(restored.excluded(), ["second.example.test"]);
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
    fn an_allowed_page_is_kept_by_its_address_without_a_fragment() {
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
