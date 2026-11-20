//! The owner's page-text setting (#54), stored and read in one place for the window and the
//! session worker.

use std::{collections::BTreeMap, fmt};

use syrinx_core::excluded_site;
use syrinx_store::Store;

use super::SessionError;
use super::page_text::PageText;

/// Whether page text is captured: `on`, or anything else for off.
const CAPTURE_KEY: &str = "page_text.capture";
/// The one stored value that switches capture on.
const CAPTURE_ON: &str = "on";
const CAPTURE_OFF: &str = "off";
/// The owner's excluded sites: normalized host names, separated by spaces, in the order added.
const EXCLUDED_SITES_KEY: &str = "page_text.excluded_sites";

/// Whether capture is on and which sites the owner excluded (D35).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PageTextSetting {
    /// Off until the owner switches it on.
    pub on: bool,
    /// Normalized host names, in the order the owner added them.
    pub excluded: Vec<String>,
}

/// The stored setting as read at start-up.
pub struct RestoredSetting {
    pub setting: PageTextSetting,
    /// Some stored site could not be read, so capture is kept off rather than read a site
    /// the owner may have excluded.
    pub unreadable: bool,
    /// Capture was set at some point, so text may have been kept.
    pub ever_set: bool,
}

impl PageTextSetting {
    /// The stored setting; only the exact stored `on` with a readable site list is on.
    pub fn restored(settings: &BTreeMap<String, String>) -> RestoredSetting {
        let stored_on = settings.get(CAPTURE_KEY).map(String::as_str) == Some(CAPTURE_ON);
        let (excluded, unreadable) = settings
            .get(EXCLUDED_SITES_KEY)
            .map_or((Vec::new(), false), |stored| parse_sites(stored));
        RestoredSetting {
            setting: Self {
                on: stored_on && !unreadable,
                excluded,
            },
            unreadable,
            ever_set: settings.contains_key(CAPTURE_KEY),
        }
    }

    /// The settings rows that store this setting.
    pub fn entries(&self) -> Vec<(String, String)> {
        let on = if self.on { CAPTURE_ON } else { CAPTURE_OFF };
        vec![
            (CAPTURE_KEY.to_owned(), on.to_owned()),
            (EXCLUDED_SITES_KEY.to_owned(), self.excluded.join(" ")),
        ]
    }
}

impl RestoredSetting {
    /// Capture was switched on once and is off now, so any text left behind must go: an
    /// erase that failed or was cut short, or a site list that can no longer be read.
    pub fn leaves_text_to_erase(&self) -> bool {
        self.ever_set && !self.setting.on
    }
}

impl fmt::Debug for PageTextSetting {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The excluded sites are the owner's browsing; only their number reaches a log.
        formatter
            .debug_struct("PageTextSetting")
            .field("on", &self.on)
            .field("excluded_sites", &self.excluded.len())
            .finish()
    }
}

/// The setting as the worker last applied it, and whether the database has yet to take it.
#[derive(Default)]
pub(super) struct AppliedSetting {
    setting: PageTextSetting,
    unsaved: bool,
}

impl AppliedSetting {
    pub(super) fn restored(setting: PageTextSetting) -> Self {
        Self {
            setting,
            unsaved: false,
        }
    }

    /// Applies the owner's new setting: erases at once what it no longer allows, since erasing
    /// is the safe direction, then saves it.
    pub(super) fn apply(
        &mut self,
        next: PageTextSetting,
        page_text: &mut PageText,
        store: &Store,
        now: i64,
    ) -> Result<(), SessionError> {
        // While capture is off every save erases again, which retries an erase that failed.
        if next.on {
            page_text.erase_sites(&added_sites(&self.setting, &next), now);
        } else {
            page_text.erase(now);
        }
        self.setting = next;
        self.save(store)
    }

    /// Saves again a setting the database refused; the idle step and shutdown call this.
    pub(super) fn retry(&mut self, store: &Store) -> Result<(), SessionError> {
        if !self.unsaved {
            return Ok(());
        }
        self.save(store)
    }

    /// Kept until it is saved, so a switch-off the database refused cannot come back on at
    /// the next start while the owner believes capture is off.
    fn save(&mut self, store: &Store) -> Result<(), SessionError> {
        let saved = store
            .save_settings(self.setting.entries())
            .map_err(SessionError::Store);
        self.unsaved = saved.is_err();
        saved
    }

    #[cfg(test)]
    pub(super) fn unsaved(&self) -> bool {
        self.unsaved
    }
}

/// The sites `next` excludes that `previous` did not.
fn added_sites(previous: &PageTextSetting, next: &PageTextSetting) -> Vec<String> {
    next.excluded
        .iter()
        .filter(|site| !previous.excluded.contains(site))
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn only_the_exact_stored_value_switches_capture_on() {
        let never = PageTextSetting::restored(&BTreeMap::new());
        assert!(!never.setting.on);
        assert!(!never.leaves_text_to_erase(), "nothing was ever captured");
        let odd = PageTextSetting::restored(&settings(&[(CAPTURE_KEY, "yes")]));
        assert!(!odd.setting.on);
        assert!(odd.leaves_text_to_erase());
        let on = PageTextSetting::restored(&settings(&[(CAPTURE_KEY, CAPTURE_ON)]));
        assert!(on.setting.on);
        assert!(!on.leaves_text_to_erase());
    }

    #[test]
    fn an_unreadable_site_list_keeps_capture_off_and_its_text_is_erased() {
        let stored = settings(&[
            (CAPTURE_KEY, CAPTURE_ON),
            (EXCLUDED_SITES_KEY, "owner.test not/a/site"),
        ]);
        let restored = PageTextSetting::restored(&stored);
        assert!(restored.unreadable);
        assert!(!restored.setting.on);
        assert_eq!(restored.setting.excluded, ["owner.test"]);
        assert!(restored.leaves_text_to_erase());
    }

    #[test]
    fn the_setting_round_trips_through_its_entries() {
        let setting = PageTextSetting {
            on: true,
            excluded: vec!["owner.test".to_owned(), "second.example.test".to_owned()],
        };
        let restored = PageTextSetting::restored(&setting.entries().into_iter().collect());
        assert!(!restored.unreadable);
        assert_eq!(restored.setting, setting);
        let off = PageTextSetting {
            on: false,
            ..setting
        };
        let restored = PageTextSetting::restored(&off.entries().into_iter().collect());
        assert_eq!(restored.setting, off);
    }

    #[test]
    fn debug_output_names_no_excluded_site() {
        let setting = PageTextSetting {
            on: true,
            excluded: vec!["debug-sentinel.test".to_owned()],
        };
        assert!(!format!("{setting:?}").contains("sentinel"));
    }
}
