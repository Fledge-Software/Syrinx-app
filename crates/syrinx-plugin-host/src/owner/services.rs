//! The services behind one installed plugin's imports in the application: its settings in
//! Syrinx's store, under its own id only, and in-memory views until plugin tabs exist (#29).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use syrinx_store::{Store, StoreError};

use crate::PluginHost;
use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, PluginError};
use crate::deadline::CallDeadline;
use crate::views::{ViewBook, ViewRecord};

/// Made for one plugin: no call can name another plugin's namespace.
pub struct StoreServices {
    plugin: String,
    store: Arc<Store>,
    book: ViewBook,
    deadline: Option<CallDeadline>,
}

impl StoreServices {
    pub fn new(plugin: &str, store: Arc<Store>) -> Self {
        Self {
            plugin: plugin.to_owned(),
            store,
            book: ViewBook::new(),
            deadline: None,
        }
    }

    /// What the plugin logged, oldest first; kept in memory and bounded.
    pub fn logs(&self) -> &[(LogLevel, String)] {
        self.book.logs()
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.book.view(view)
    }

    /// How long a store call may wait (`CallDeadline::service_budget`): an epoch deadline
    /// cannot interrupt a host function. Every export sets its deadline first.
    fn budget(&self) -> Duration {
        self.deadline
            .map_or(Duration::ZERO, CallDeadline::service_budget)
    }
}

/// A store failure as the plugin sees it: typed, and without the store's details.
fn plugin_error(error: StoreError) -> PluginError {
    match error {
        StoreError::Busy => PluginError::Unavailable("Syrinx's store is busy".into()),
        StoreError::TooManyRecords => PluginError::InvalidInput("too many settings keys".into()),
        StoreError::InvalidEncoding(_) => {
            PluginError::InvalidInput("the setting was refused".into())
        }
        _ => PluginError::Unavailable("Syrinx's store could not keep the setting".into()),
    }
}

impl PluginHost for StoreServices {
    fn begin_export(&mut self, deadline: CallDeadline) {
        self.deadline = Some(deadline);
    }

    fn view_closed(&mut self, view: u64) {
        self.book.view_closed(view);
    }
}

impl types::Host for StoreServices {}

impl host::Host for StoreServices {
    fn log(&mut self, level: LogLevel, message: String) {
        self.book.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.store
            .plugin_setting_within(&self.plugin, &key, self.budget())
            .map_err(plugin_error)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        let budget = self.budget();
        self.store
            .save_plugin_setting_within(self.plugin.clone(), key, value, budget)
            .map_err(plugin_error)
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

    /// These services start no work of their own, so there is nothing to abandon.
    fn cancel(&mut self, _request: u64) {}

    /// Unix milliseconds; a clock set before 1970 reads as 0.
    fn now(&mut self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            })
    }
}
