//! Synthetic services whose view service waits on a dependency that never answers, bounded by
//! the call's deadline, and that otherwise behave as the test host's.

use std::sync::mpsc::{self, Receiver, Sender};

use syrinx_plugin_host::PluginHost;
use syrinx_plugin_host::bindings::syrinx::plugin::host::{self, LogLevel};
use syrinx_plugin_host::bindings::syrinx::plugin::types::{self, PluginError};
use syrinx_plugin_host::deadline::CallDeadline;
use syrinx_plugin_host::manifest::PluginManifest;
use syrinx_plugin_host::test_host::SyntheticHost;

pub struct StallingHost {
    inner: SyntheticHost,
    /// The running export's deadline; without one the service would wait for ever.
    deadline: Option<CallDeadline>,
    /// The dependency's answers; its sender is held and never used.
    dependency: Receiver<u64>,
    _never_answers: Sender<u64>,
}

impl StallingHost {
    pub fn new(manifest: &PluginManifest) -> Self {
        let (never_answers, dependency) = mpsc::channel();
        Self {
            inner: SyntheticHost::new(manifest.into()),
            deadline: None,
            dependency,
            _never_answers: never_answers,
        }
    }
}

impl PluginHost for StallingHost {
    fn begin_export(&mut self, deadline: CallDeadline) {
        self.deadline = Some(deadline);
    }

    fn view_closed(&mut self, view: u64) {
        self.inner.view_closed(view);
    }
}

impl types::Host for StallingHost {}

impl host::Host for StallingHost {
    fn log(&mut self, level: LogLevel, message: String) {
        self.inner.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.inner.settings_get(key)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.inner.settings_set(key, value)
    }

    /// Wait for the dependency only as long as the deadline allows, then answer `unavailable`.
    fn open_view(&mut self, _kind: String, _address: String) -> Result<u64, PluginError> {
        let answer = match self.deadline {
            Some(deadline) => self.dependency.recv_timeout(deadline.service_budget()).ok(),
            None => self.dependency.recv().ok(),
        };
        answer.ok_or_else(|| PluginError::Unavailable("the view service did not answer".into()))
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        self.inner.set_view_metadata(view, title, badge)
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        self.inner.post_view_event(view, topic, json)
    }

    fn feed_changed(&mut self, feed: String) {
        self.inner.feed_changed(feed);
    }

    fn widget_changed(&mut self, widget: String) {
        self.inner.widget_changed(widget);
    }

    fn cancel(&mut self, request: u64) {
        self.inner.cancel(request);
    }

    fn now(&mut self) -> i64 {
        self.inner.now()
    }
}
