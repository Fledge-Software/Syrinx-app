//! The services behind one installed plugin's imports in the application: its settings in
//! Syrinx's store, under its own id only, and synthetic views until plugin tabs exist (#29).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use syrinx_store::StoreError;

use super::records::{RecordsError, RecordsHandle};
use crate::PluginHost;
use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, PluginError};
use crate::deadline::CallDeadline;
use crate::test_host::SyntheticHost;

/// Made for one plugin: no call can name another plugin's namespace.
pub struct StoreServices {
    plugin: String,
    records: RecordsHandle,
    views: SyntheticHost,
    deadline: Option<CallDeadline>,
}

impl StoreServices {
    pub(super) fn new(plugin: &str, records: RecordsHandle) -> Self {
        Self {
            plugin: plugin.to_owned(),
            records,
            views: SyntheticHost::new(),
            deadline: None,
        }
    }

    /// What the plugin logged, oldest first; kept in memory and bounded.
    pub fn logs(&self) -> &[(LogLevel, String)] {
        self.views.logs()
    }

    /// Half the running export's time left; every export sets its deadline first.
    fn budget(&self) -> Duration {
        self.deadline
            .map_or(Duration::ZERO, CallDeadline::service_budget)
    }
}

/// A store failure as the plugin sees it: typed, and without the store's details.
fn plugin_error(error: RecordsError) -> PluginError {
    match error {
        RecordsError::Busy => PluginError::Unavailable("Syrinx's store is busy".into()),
        RecordsError::Stopped => PluginError::Unavailable("Syrinx's store is closing".into()),
        RecordsError::Store(StoreError::TooManyRecords) => {
            PluginError::InvalidInput("too many settings keys".into())
        }
        RecordsError::Store(StoreError::InvalidEncoding(_)) => {
            PluginError::InvalidInput("the setting was refused".into())
        }
        RecordsError::Store(_) => {
            PluginError::Unavailable("Syrinx's store could not keep the setting".into())
        }
    }
}

impl PluginHost for StoreServices {
    fn begin_export(&mut self, deadline: CallDeadline) {
        self.deadline = Some(deadline);
    }

    fn view_closed(&mut self, view: u64) {
        self.views.view_closed(view);
    }
}

impl types::Host for StoreServices {}

impl host::Host for StoreServices {
    fn log(&mut self, level: LogLevel, message: String) {
        self.views.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        let plugin = self.plugin.clone();
        self.records
            .run(self.budget(), move |store| {
                store.plugin_setting(&plugin, &key)
            })
            .map_err(plugin_error)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        let plugin = self.plugin.clone();
        self.records
            .run(self.budget(), move |store| {
                store.save_plugin_setting(plugin, key, value)
            })
            .map_err(plugin_error)
    }

    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        self.views.open_view(kind, address)
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        self.views.set_view_metadata(view, title, badge)
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        self.views.post_view_event(view, topic, json)
    }

    fn feed_changed(&mut self, feed: String) {
        self.views.feed_changed(feed);
    }

    fn widget_changed(&mut self, widget: String) {
        self.views.widget_changed(widget);
    }

    fn cancel(&mut self, request: u64) {
        self.views.cancel(request);
    }

    /// Unix milliseconds; a clock set before 1970 reads as 0.
    fn now(&mut self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            })
    }
}
