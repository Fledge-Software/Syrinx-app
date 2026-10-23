//! Remembers each site's zoom level and reapplies it, as Chrome, Edge and Firefox do (#90).
//! WebView2's zoom belongs to a page's controller, so Syrinx follows each page's host itself.

use std::collections::{BTreeMap, HashMap};

use syrinx_store::is_default_zoom;

/// The saved level for each site, and what each live page shows. Hosts are browsing data,
/// so this type has no `Debug` and never reaches a log.
#[derive(Default)]
pub(super) struct SiteZoom {
    /// Saved levels away from 100%, by host; an absent host is at 100%.
    saved: HashMap<String, f64>,
    /// The factor each live page last reported.
    current: HashMap<u64, f64>,
    /// The host each live page shows, once its address is known.
    hosts: HashMap<u64, String>,
    /// A level Syrinx asked a page to take; the page's reports until it arrives are not the
    /// owner's choice, so they are not saved.
    applying: HashMap<u64, f64>,
}

/// What to do after a page reported a new zoom level. Holds a host, so `Debug` is for tests.
#[cfg_attr(test, derive(Debug))]
#[derive(Default, PartialEq)]
pub(super) struct ZoomChoice {
    /// The site to save: its host and its level, `None` at 100%.
    pub(super) save: Option<(String, Option<f64>)>,
    /// Other open pages of that site, and the level they should take to match.
    pub(super) follow: Vec<(u64, f64)>,
}

impl SiteZoom {
    /// Takes the levels saved by earlier sessions, sent once at start-up.
    pub(super) fn restore(&mut self, saved: BTreeMap<String, f64>) {
        self.saved = saved.into_iter().collect();
    }

    /// Replaces the saved levels after a clear. Returns the open pages whose site lost its
    /// level and that should go back to 100%, now marked as applying it.
    pub(super) fn replace_saved(&mut self, saved: BTreeMap<String, f64>) -> Vec<(u64, f64)> {
        self.saved = saved.into_iter().collect();
        let mut reset: Vec<(u64, f64)> = self
            .hosts
            .iter()
            .filter(|(id, host)| {
                let shown = self.current.get(id).copied().unwrap_or(1.);
                !same_level(shown, self.level(host))
            })
            .map(|(id, host)| (*id, self.level(host)))
            .collect();
        reset.sort_by_key(|(id, _)| *id);
        for (id, level) in &reset {
            self.applying.insert(*id, *level);
        }
        reset
    }

    /// Syrinx could not apply a level to page `id`, so its next report is the owner's again.
    pub(super) fn abandon_apply(&mut self, id: u64) {
        self.applying.remove(&id);
    }

    /// The level page `id` shows; 100% until it reports one.
    pub(super) fn factor(&self, id: u64) -> f64 {
        self.current.get(&id).copied().unwrap_or(1.)
    }

    /// Page `id` now shows `address`. Returns the level to apply when it moved to another
    /// site whose saved level differs from what the page shows.
    pub(super) fn page_address(&mut self, id: u64, address: &str) -> Option<f64> {
        // Pages without a host (blank, files, Syrinx's own) keep their level and save nothing.
        let Some(host) = site_host(address) else {
            self.hosts.remove(&id);
            return None;
        };
        if self.hosts.get(&id) == Some(&host) {
            return None;
        }
        let wanted = self.level(&host);
        self.hosts.insert(id, host);
        if same_level(self.factor(id), wanted) {
            self.applying.remove(&id);
            return None;
        }
        self.applying.insert(id, wanted);
        Some(wanted)
    }

    /// Page `id` reports `factor`. A change the owner made is saved for its site and copied
    /// to the site's other open pages; Syrinx's own reapplied levels are not saved again.
    pub(super) fn page_reported(&mut self, id: u64, factor: f64) -> ZoomChoice {
        self.current.insert(id, factor);
        if let Some(&wanted) = self.applying.get(&id) {
            if same_level(factor, wanted) {
                self.applying.remove(&id);
            }
            return ZoomChoice::default();
        }
        let Some(host) = self.hosts.get(&id).cloned() else {
            return ZoomChoice::default();
        };
        if same_level(factor, self.level(&host)) {
            return ZoomChoice::default();
        }
        let stored = (!is_default_zoom(factor)).then_some(factor);
        match stored {
            Some(level) => self.saved.insert(host.clone(), level),
            None => self.saved.remove(&host),
        };
        let follow = self.follow_site(id, &host, factor);
        ZoomChoice {
            save: Some((host, stored)),
            follow,
        }
    }

    /// Page `id` is gone or was recreated at 100%; its next address applies afresh.
    pub(super) fn forget_page(&mut self, id: u64) {
        self.current.remove(&id);
        self.hosts.remove(&id);
        self.applying.remove(&id);
    }

    /// Keeps only pages that are still tabs.
    pub(super) fn retain_pages(&mut self, live: impl Fn(u64) -> bool) {
        self.current.retain(|id, _| live(*id));
        self.hosts.retain(|id, _| live(*id));
        self.applying.retain(|id, _| live(*id));
    }

    /// The saved level for `host`, or 100%.
    fn level(&self, host: &str) -> f64 {
        self.saved.get(host).copied().unwrap_or(1.)
    }

    /// The other open pages of `host` not yet at `factor`, now marked as applying it.
    fn follow_site(&mut self, changed: u64, host: &str, factor: f64) -> Vec<(u64, f64)> {
        let mut follow: Vec<(u64, f64)> = self
            .hosts
            .iter()
            .filter(|(id, page_host)| **id != changed && page_host.as_str() == host)
            .filter(|(id, _)| !same_level(self.current.get(id).copied().unwrap_or(1.), factor))
            .map(|(id, _)| (*id, factor))
            .collect();
        follow.sort_by_key(|(id, _)| *id);
        for (id, level) in &follow {
            self.applying.insert(*id, *level);
        }
        follow
    }
}

/// The site a page address belongs to: its lowercase host, for web pages only.
pub(super) fn site_host(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether two levels are the same zoom step, ignoring WebView2's rounding.
fn same_level(first: f64, second: f64) -> bool {
    (first - second).abs() < 0.001
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(entries: &[(&str, f64)]) -> SiteZoom {
        let mut zoom = SiteZoom::default();
        zoom.restore(
            entries
                .iter()
                .map(|(host, level)| ((*host).to_owned(), *level))
                .collect(),
        );
        zoom
    }

    #[test]
    fn a_site_is_its_lowercase_web_host_without_scheme_port_or_path() {
        assert_eq!(
            site_host("https://Docs.Example.test:8443/private?q=1#top").as_deref(),
            Some("docs.example.test")
        );
        assert_eq!(
            site_host("http://docs.example.test/").as_deref(),
            Some("docs.example.test")
        );
        assert_eq!(
            site_host("https://user:secret@login.example.test/").as_deref(),
            Some("login.example.test")
        );
        for address in [
            "about:blank",
            "file:///C:/synthetic/page.html",
            "data:text/html,x",
            "",
        ] {
            assert_eq!(site_host(address), None, "{address}");
        }
    }

    #[test]
    fn a_page_takes_its_sites_saved_level_when_it_arrives() {
        let mut zoom = saved(&[("zoomed.example.test", 1.25)]);
        zoom.page_reported(7, 1.);
        assert_eq!(
            zoom.page_address(7, "https://zoomed.example.test/a"),
            Some(1.25)
        );
        // Another page on the same site keeps the level without asking again.
        assert_eq!(zoom.page_address(7, "https://zoomed.example.test/b"), None);
        // The acknowledgement is Syrinx's own change, not the owner's: nothing is saved.
        assert_eq!(zoom.page_reported(7, 1.25), ZoomChoice::default());
        assert_eq!(zoom.factor(7), 1.25);
        // Moving to an unsaved site returns the page to 100%.
        assert_eq!(
            zoom.page_address(7, "https://plain.example.test/"),
            Some(1.)
        );
    }

    #[test]
    fn an_owner_change_is_saved_for_the_site_and_copied_to_its_other_pages() {
        let mut zoom = SiteZoom::default();
        for id in [1, 2, 3] {
            zoom.page_reported(id, 1.);
        }
        assert_eq!(zoom.page_address(1, "https://site.example.test/one"), None);
        assert_eq!(zoom.page_address(2, "https://site.example.test/two"), None);
        assert_eq!(zoom.page_address(3, "https://other.example.test/"), None);
        assert_eq!(
            zoom.page_reported(1, 1.1),
            ZoomChoice {
                save: Some(("site.example.test".to_owned(), Some(1.1))),
                follow: vec![(2, 1.1)],
            }
        );
        // Page 2's acknowledgement is not saved again.
        assert_eq!(zoom.page_reported(2, 1.1), ZoomChoice::default());
        // A new page on the site opens at the saved level.
        zoom.page_reported(4, 1.);
        assert_eq!(
            zoom.page_address(4, "https://site.example.test/"),
            Some(1.1)
        );
    }

    #[test]
    fn a_reset_to_one_hundred_percent_forgets_the_site() {
        let mut zoom = saved(&[("reset.example.test", 1.5)]);
        zoom.page_reported(5, 1.5);
        assert_eq!(zoom.page_address(5, "https://reset.example.test/"), None);
        assert_eq!(
            zoom.page_reported(5, 1.),
            ZoomChoice {
                save: Some(("reset.example.test".to_owned(), None)),
                follow: Vec::new(),
            }
        );
        zoom.page_reported(6, 1.);
        assert_eq!(zoom.page_address(6, "https://reset.example.test/"), None);
    }

    #[test]
    fn pages_without_a_site_and_repeated_reports_save_nothing() {
        let mut zoom = SiteZoom::default();
        zoom.page_reported(9, 1.);
        assert_eq!(zoom.page_address(9, "about:blank"), None);
        assert_eq!(zoom.page_reported(9, 1.25), ZoomChoice::default());
        assert_eq!(
            zoom.page_address(9, "https://quiet.example.test/"),
            Some(1.)
        );
        assert_eq!(zoom.page_reported(9, 1.), ZoomChoice::default());
        // The same level reported twice (WebView2's event and Syrinx's acknowledgement).
        zoom.page_reported(9, 1.1);
        assert_eq!(zoom.page_reported(9, 1.1), ZoomChoice::default());
    }

    #[test]
    fn a_recreated_page_reapplies_its_sites_level() {
        let mut zoom = saved(&[("crash.example.test", 1.75)]);
        zoom.page_reported(3, 1.);
        assert_eq!(
            zoom.page_address(3, "https://crash.example.test/"),
            Some(1.75)
        );
        zoom.page_reported(3, 1.75);
        zoom.forget_page(3);
        zoom.page_reported(3, 1.);
        assert_eq!(
            zoom.page_address(3, "https://crash.example.test/"),
            Some(1.75)
        );
    }

    #[test]
    fn a_level_that_could_not_be_applied_does_not_block_later_saves() {
        let mut zoom = saved(&[("stuck.example.test", 1.25)]);
        zoom.page_reported(4, 1.);
        assert_eq!(
            zoom.page_address(4, "https://stuck.example.test/"),
            Some(1.25)
        );
        // The page never took 1.25, so the owner's next change must still be saved.
        zoom.abandon_apply(4);
        assert_eq!(
            zoom.page_reported(4, 1.1),
            ZoomChoice {
                save: Some(("stuck.example.test".to_owned(), Some(1.1))),
                follow: Vec::new(),
            }
        );
    }

    #[test]
    fn a_clear_returns_open_pages_of_cleared_sites_to_one_hundred_percent() {
        let mut zoom = saved(&[("cleared.example.test", 1.5), ("kept.example.test", 1.25)]);
        for (id, address, level) in [
            (1, "https://cleared.example.test/", 1.5),
            (2, "https://kept.example.test/", 1.25),
        ] {
            zoom.page_reported(id, 1.);
            zoom.page_address(id, address);
            zoom.page_reported(id, level);
        }
        let remaining = BTreeMap::from([("kept.example.test".to_owned(), 1.25)]);
        assert_eq!(zoom.replace_saved(remaining), vec![(1, 1.)]);
        // The reset's acknowledgement is not saved as the owner's choice.
        assert_eq!(zoom.page_reported(1, 1.), ZoomChoice::default());
    }

    #[test]
    fn closed_tabs_are_forgotten_but_saved_sites_are_kept() {
        let mut zoom = saved(&[("kept.example.test", 1.25)]);
        zoom.page_reported(1, 1.);
        zoom.page_address(1, "https://kept.example.test/");
        zoom.retain_pages(|id| id != 1);
        assert_eq!(zoom.factor(1), 1.);
        zoom.page_reported(2, 1.);
        assert_eq!(
            zoom.page_address(2, "https://kept.example.test/"),
            Some(1.25)
        );
    }
}
