//! The test host's services behind a plugin's `syrinx:plugin` imports, with
//! synthetic state: in-memory settings, the shared view bookkeeping and a fixed clock.
//! The contract's own rules (grants, declared kinds and feeds, the open-view limit and the
//! settings bounds) are the `Mediator`'s, in front of these services.

use std::collections::BTreeMap;

use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, PluginError};
use crate::views::ViewBook;
use crate::{PluginHost, limits};

/// A fixed clock, so tests never depend on the real time.
pub const SYNTHETIC_NOW: i64 = 1_700_000_000_000;

/// Everything the test host's services keep for one plugin.
#[derive(Default)]
pub struct SyntheticHost {
    settings: BTreeMap<String, Vec<u8>>,
    pub(super) book: ViewBook,
}

impl SyntheticHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the plugin and the host logged, oldest first.
    pub fn logs(&self) -> &[(LogLevel, String)] {
        self.book.logs()
    }
}

impl PluginHost for SyntheticHost {
    fn view_closed(&mut self, view: u64) {
        self.book.view_closed(view);
    }
}

impl types::Host for SyntheticHost {}

impl host::Host for SyntheticHost {
    fn log(&mut self, level: LogLevel, message: String) {
        self.book.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        Ok(self.settings.get(&key).cloned())
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        let is_new = !self.settings.contains_key(&key);
        if is_new && self.settings.len() == limits::MAX_SETTINGS_KEYS {
            return Err(PluginError::InvalidInput("too many settings keys".into()));
        }
        self.settings.insert(key, value);
        Ok(())
    }

    fn open_view(&mut self, kind: String, _address: String) -> Result<u64, PluginError> {
        Ok(self.book.open_view(kind))
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        self.book.set_view_metadata(view, title, badge)
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        self.book.post_view_event(view, topic, json)
    }

    fn feed_changed(&mut self, feed: String) {
        self.book.feed_changed(feed);
    }

    /// Gated `@unstable(feature = home-widgets)`; never linked for a plugin.
    fn widget_changed(&mut self, _widget: String) {}

    /// Synthetic services start no work of their own, so there is nothing to abandon.
    fn cancel(&mut self, _request: u64) {}

    fn now(&mut self) -> i64 {
        SYNTHETIC_NOW
    }
}
