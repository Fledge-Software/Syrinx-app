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
/// Set only in the window's copy of the settings, never saved: the database's list or the copy
/// kept beside it (#199) could not be read, so capture stays off.
const SITES_UNREADABLE_KEY: &str = "page_text.sites_unreadable";

/// The copy of the excluded sites kept beside the database, outside its backups (#199), as
/// read at start-up.
pub enum KeptSites {
    /// Never saved yet, as before the first start of a build that keeps one.
    Absent,
    Read(Vec<u8>),
    Unreadable,
}

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
    /// The database and the kept copy disagree or the copy is missing, so both are saved again.
    pub resave: bool,
}

impl PageTextSetting {
    /// The stored setting; only the exact stored `on` with a readable site list is on.
    pub fn restored(settings: &BTreeMap<String, String>) -> RestoredSetting {
        let stored_on = settings.get(CAPTURE_KEY).map(String::as_str) == Some(CAPTURE_ON);
        let (excluded, unreadable_list) = settings
            .get(EXCLUDED_SITES_KEY)
            .map_or((Vec::new(), false), |stored| parse_sites(stored));
        let unreadable = unreadable_list || settings.contains_key(SITES_UNREADABLE_KEY);
        RestoredSetting {
            setting: Self {
                on: stored_on && !unreadable,
                excluded,
            },
            unreadable,
            ever_set: settings.contains_key(CAPTURE_KEY),
            resave: false,
        }
    }

    /// The stored setting with every site the kept copy excludes added to the database's.
    ///
    /// A restored backup can hold an older, shorter list; the kept copy is never restored, so
    /// the union still excludes every site the owner had excluded (#199). A copy that cannot
    /// be read keeps capture off, as an unreadable database list does.
    pub(super) fn restored_with(
        settings: &BTreeMap<String, String>,
        kept: &KeptSites,
    ) -> RestoredSetting {
        let mut restored = Self::restored(settings);
        // An unreadable database list keeps capture off until the owner saves anew; saving
        // now would store that switch-off as the owner's choice. Nor is a setting the owner
        // never made saved: that would record capture as switched off.
        let may_resave = !restored.unreadable && restored.ever_set;
        let kept_sites = match kept {
            KeptSites::Absent => {
                restored.resave = may_resave;
                return restored;
            }
            KeptSites::Unreadable => None,
            // Every readable name still counts; a damaged byte only spoils its own entry.
            KeptSites::Read(bytes) => Some(parse_sites(&String::from_utf8_lossy(bytes))),
        };
        let Some((kept_sites, unreadable)) = kept_sites else {
            restored.keep_off_unreadable();
            return restored;
        };
        let database_sites = restored.setting.excluded.clone();
        for site in &kept_sites {
            if !restored.setting.excluded.contains(site) {
                restored.setting.excluded.push(site.clone());
            }
        }
        if unreadable {
            restored.keep_off_unreadable();
            return restored;
        }
        restored.resave = may_resave
            && (restored.setting.excluded != database_sites
                || restored.setting.excluded != kept_sites);
        restored
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
    /// Writes the merged setting into the settings the window reads at start-up.
    pub(super) fn show_in(&self, settings: &mut BTreeMap<String, String>) {
        // A fresh profile stores no list, and the window is sent none.
        if settings.contains_key(EXCLUDED_SITES_KEY) || !self.setting.excluded.is_empty() {
            settings.insert(
                EXCLUDED_SITES_KEY.to_owned(),
                self.setting.excluded.join(" "),
            );
        }
        if self.unreadable {
            settings.insert(SITES_UNREADABLE_KEY.to_owned(), String::new());
        }
    }

    /// Nothing the kept copy says can be trusted, and nothing overwrites it until the owner
    /// saves a new setting.
    fn keep_off_unreadable(&mut self) {
        self.unreadable = true;
        self.setting.on = false;
        self.resave = false;
    }

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
    /// The setting read at start-up; `resave` saves it again at the next idle step.
    pub(super) fn restored(setting: PageTextSetting, resave: bool) -> Self {
        Self {
            setting,
            unsaved: resave,
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
        self.save(store, page_text)
    }

    /// The setting in force now, saved or not.
    pub(super) fn setting(&self) -> &PageTextSetting {
        &self.setting
    }

    /// Saves again a setting the database refused; the idle step and shutdown call this.
    pub(super) fn retry(
        &mut self,
        store: &Store,
        page_text: &PageText,
    ) -> Result<(), SessionError> {
        if !self.unsaved {
            return Ok(());
        }
        self.save(store, page_text)
    }

    /// Kept until it is saved, so a switch-off the database refused cannot come back on at
    /// the next start while the owner believes capture is off.
    ///
    /// The kept copy is saved first and the database regardless: if only one takes the new
    /// list, the start-up union can only exclude more, never less (#199).
    fn save(&mut self, store: &Store, page_text: &PageText) -> Result<(), SessionError> {
        let kept = page_text.save_excluded_sites(&self.setting.excluded);
        let stored = store
            .save_settings(self.setting.entries())
            .map_err(SessionError::Store);
        let saved = kept.and(stored);
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

    fn kept(sites: &str) -> KeptSites {
        KeptSites::Read(sites.as_bytes().to_vec())
    }

    #[test]
    fn a_site_only_the_kept_copy_excludes_stays_excluded() {
        // A restored backup's list lacks the site the owner excluded after it was taken.
        let stored = settings(&[
            (CAPTURE_KEY, CAPTURE_ON),
            (EXCLUDED_SITES_KEY, "older.example.test"),
        ]);
        let kept_copy = kept("older.example.test newer.example.test");
        let restored = PageTextSetting::restored_with(&stored, &kept_copy);
        assert!(restored.setting.on);
        assert_eq!(
            restored.setting.excluded,
            ["older.example.test", "newer.example.test"]
        );
        assert!(restored.resave, "the database takes the merged list back");
    }

    #[test]
    fn agreeing_lists_need_no_save_and_a_missing_copy_is_written() {
        let stored = settings(&[
            (CAPTURE_KEY, CAPTURE_ON),
            (EXCLUDED_SITES_KEY, "owner.example.test"),
        ]);
        let agreeing = PageTextSetting::restored_with(&stored, &kept("owner.example.test"));
        assert!(!agreeing.resave);
        let missing = PageTextSetting::restored_with(&stored, &KeptSites::Absent);
        assert!(missing.setting.on);
        assert!(missing.resave);
    }

    #[test]
    fn an_unreadable_kept_copy_keeps_capture_off_and_is_not_overwritten() {
        let stored = settings(&[
            (CAPTURE_KEY, CAPTURE_ON),
            (EXCLUDED_SITES_KEY, "owner.example.test"),
        ]);
        for copy in [
            KeptSites::Unreadable,
            KeptSites::Read(vec![0xff, 0xfe]),
            kept("other.example.test not/a/site"),
        ] {
            let restored = PageTextSetting::restored_with(&stored, &copy);
            assert!(restored.unreadable);
            assert!(!restored.setting.on);
            assert!(!restored.resave);
            assert!(restored.leaves_text_to_erase());
        }
    }

    #[test]
    fn a_setting_the_owner_never_made_is_not_saved() {
        let never = PageTextSetting::restored_with(&BTreeMap::new(), &KeptSites::Absent);
        assert!(!never.resave, "saving would record capture as switched off");
        assert!(!never.leaves_text_to_erase());
    }

    #[test]
    fn a_damaged_byte_spoils_only_its_own_entry() {
        let stored = settings(&[(CAPTURE_KEY, CAPTURE_ON)]);
        let mut bytes = b"owner.example.test ".to_vec();
        bytes.extend_from_slice(&[0xff, b'x']);
        let restored = PageTextSetting::restored_with(&stored, &KeptSites::Read(bytes));
        assert_eq!(restored.setting.excluded, ["owner.example.test"]);
        assert!(restored.unreadable);
        assert!(!restored.setting.on);
    }

    #[test]
    fn an_unreadable_database_list_is_not_saved_back_as_off() {
        let stored = settings(&[
            (CAPTURE_KEY, CAPTURE_ON),
            (EXCLUDED_SITES_KEY, "owner.example.test not/a/site"),
        ]);
        let restored = PageTextSetting::restored_with(&stored, &kept("other.example.test"));
        assert!(!restored.setting.on);
        assert!(!restored.resave);
        let missing = PageTextSetting::restored_with(&stored, &KeptSites::Absent);
        assert!(!missing.resave);
    }

    #[test]
    fn the_window_is_shown_the_merged_list_and_an_unreadable_copy() {
        let stored = settings(&[(CAPTURE_KEY, CAPTURE_ON)]);
        let mut shown = stored.clone();
        PageTextSetting::restored_with(&stored, &kept("owner.example.test")).show_in(&mut shown);
        let window = PageTextSetting::restored(&shown);
        assert!(window.setting.on);
        assert_eq!(window.setting.excluded, ["owner.example.test"]);

        let mut shown = stored.clone();
        PageTextSetting::restored_with(&stored, &KeptSites::Unreadable).show_in(&mut shown);
        let window = PageTextSetting::restored(&shown);
        assert!(window.unreadable);
        assert!(!window.setting.on);
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
