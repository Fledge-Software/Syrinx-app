//! Synthetic services for the worker tests: the test host's, plus a journal the test keeps
//! after the instance is gone, and optionally a view service whose dependency never answers.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use syrinx_plugin_host::PluginHost;
use syrinx_plugin_host::bindings::syrinx::plugin::host::{self, LogLevel};
use syrinx_plugin_host::bindings::syrinx::plugin::types::{self, PluginError};
use syrinx_plugin_host::deadline::CallDeadline;
use syrinx_plugin_host::manifest::PluginManifest;
use syrinx_plugin_host::test_host::SyntheticHost;

/// What the plugin logged and which requests the service was told to cancel, in order.
#[derive(Clone, Default)]
pub struct Journal(Arc<Mutex<Vec<String>>>);

impl Journal {
    fn push(&self, line: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many lines start with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.lines()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }
}

pub struct TestServices {
    inner: SyntheticHost,
    journal: Journal,
    /// Whether opening a view waits on a dependency that never answers.
    stall_views: bool,
    /// The running export's deadline; without one a stalled service would wait for ever.
    deadline: Option<CallDeadline>,
    dependency: Receiver<u64>,
    _never_answers: Sender<u64>,
}

impl TestServices {
    /// A factory for services that only keep `journal`.
    pub fn recording(
        journal: &Journal,
    ) -> impl Fn(&PluginManifest) -> Self + Send + Sync + 'static {
        let journal = journal.clone();
        move |_manifest| Self::new(journal.clone(), false)
    }

    /// A factory for services whose view service never gets an answer from its dependency.
    pub fn stalling(journal: &Journal) -> impl Fn(&PluginManifest) -> Self + Send + Sync + 'static {
        let journal = journal.clone();
        move |_manifest| Self::new(journal.clone(), true)
    }

    fn new(journal: Journal, stall_views: bool) -> Self {
        let (never_answers, dependency) = mpsc::channel();
        Self {
            inner: SyntheticHost::new(),
            journal,
            stall_views,
            deadline: None,
            dependency,
            _never_answers: never_answers,
        }
    }

    /// Wait for the dependency only as long as the deadline allows, then answer `unavailable`.
    fn stall(&self) -> Result<u64, PluginError> {
        let answer = match self.deadline {
            Some(deadline) => self.dependency.recv_timeout(deadline.service_budget()).ok(),
            None => self.dependency.recv().ok(),
        };
        answer.ok_or_else(|| PluginError::Unavailable("the view service did not answer".into()))
    }
}

impl PluginHost for TestServices {
    fn begin_export(&mut self, deadline: CallDeadline) {
        self.deadline = Some(deadline);
    }

    fn view_closed(&mut self, view: u64) {
        self.journal.push(format!("view-closed {view}"));
        self.inner.view_closed(view);
    }
}

impl types::Host for TestServices {}

impl host::Host for TestServices {
    fn log(&mut self, level: LogLevel, message: String) {
        self.journal.push(message.clone());
        self.inner.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.inner.settings_get(key)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.inner.settings_set(key, value)
    }

    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        if self.stall_views {
            return self.stall();
        }
        self.inner.open_view(kind, address)
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
        self.journal.push(format!("cancel {request}"));
        self.inner.cancel(request);
    }

    fn now(&mut self) -> i64 {
        self.inner.now()
    }
}
