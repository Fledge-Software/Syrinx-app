//! The test host's answers to a plugin's `syrinx:plugin` imports, with
//! synthetic state: in-memory settings, views as records, a fixed clock.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, Outcome, PluginError};
use crate::limits;
use crate::validate;

use super::Declarations;

/// A fixed clock, so tests never depend on the real time.
pub const SYNTHETIC_NOW: i64 = 1_700_000_000_000;

/// One open plugin view, as Syrinx's tab model would hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewRecord {
    pub kind: String,
    pub address: String,
    pub title: String,
    pub badge: Option<String>,
    /// Envelopes sent to the view's interface, oldest first.
    pub events: Vec<String>,
}

/// A call the host defers until the current export returns.
#[derive(Debug, Clone)]
pub(super) enum Deferred {
    Opened(u64),
    Message(ViewRequest),
    Deliver(u64, Outcome),
}

impl Deferred {
    pub(super) fn request(&self) -> Option<u64> {
        match self {
            Self::Opened(_) => None,
            Self::Message(request) => Some(request.request_id),
            Self::Deliver(request, _) => Some(*request),
        }
    }
}

/// Everything the test host tracks for one plugin.
pub struct SyntheticHost {
    pub(super) declarations: Declarations,
    pub(super) logs: Vec<(LogLevel, String)>,
    settings: BTreeMap<String, Vec<u8>>,
    pub(super) views: BTreeMap<u64, ViewRecord>,
    next_view: u64,
    pub(super) deferred: VecDeque<Deferred>,
    pub(super) cancelled: BTreeSet<u64>,
    pub(super) changed_feeds: Vec<String>,
}

impl SyntheticHost {
    pub(super) fn new(declarations: Declarations) -> Self {
        Self {
            declarations,
            logs: Vec::new(),
            settings: BTreeMap::new(),
            views: BTreeMap::new(),
            next_view: 1,
            deferred: VecDeque::new(),
            cancelled: BTreeSet::new(),
            changed_feeds: Vec::new(),
        }
    }

    /// The host's own notes go to the same bounded log as the plugin's.
    pub(super) fn note(&mut self, level: LogLevel, message: String) {
        if self.logs.len() == limits::MAX_LOG_LINES {
            return;
        }
        let bounded: String = message.chars().take(limits::MAX_LOG_CHARS).collect();
        self.logs.push((level, bounded));
    }

    /// Drop anything still queued for a cancelled request.
    pub(super) fn cancel_request(&mut self, request: u64) {
        self.cancelled.insert(request);
        self.deferred
            .retain(|deferred| deferred.request() != Some(request));
    }

    fn view_mut(&mut self, view: u64) -> Result<&mut ViewRecord, PluginError> {
        self.views
            .get_mut(&view)
            .ok_or_else(|| PluginError::NotFound(format!("no open view {view}")))
    }
}

fn check_settings_key(key: &str) -> Result<(), PluginError> {
    validate::bytes("settings key", key, limits::MAX_SETTINGS_KEY_BYTES)?;
    validate::id("settings key", key)
}

impl types::Host for SyntheticHost {}

impl host::Host for SyntheticHost {
    fn log(&mut self, level: LogLevel, message: String) {
        self.note(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        check_settings_key(&key)?;
        Ok(self.settings.get(&key).cloned())
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        check_settings_key(&key)?;
        if value.len() > limits::MAX_SETTINGS_VALUE_BYTES {
            return Err(PluginError::InvalidInput("the value is too large".into()));
        }
        let is_new = !self.settings.contains_key(&key);
        if is_new && self.settings.len() == limits::MAX_SETTINGS_KEYS {
            return Err(PluginError::InvalidInput("too many settings keys".into()));
        }
        self.settings.insert(key, value);
        Ok(())
    }

    /// Open a declared view kind; `views.opened` follows once the current export returns.
    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        if !self.declarations.view_kinds.contains(&kind) {
            return Err(PluginError::InvalidInput(format!(
                "view kind {kind} is not declared"
            )));
        }
        validate::bytes("view address", &address, limits::MAX_ADDRESS_BYTES)?;
        let view = self.next_view;
        self.next_view += 1;
        let record = ViewRecord {
            title: kind.clone(),
            kind,
            address,
            badge: None,
            events: Vec::new(),
        };
        self.views.insert(view, record);
        self.deferred.push_back(Deferred::Opened(view));
        Ok(view)
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        validate::chars("view title", &title, limits::MAX_TITLE_CHARS)?;
        if let Some(badge) = &badge {
            validate::chars("view badge", badge, 8)?;
        }
        let record = self.view_mut(view)?;
        record.title = title;
        record.badge = badge;
        Ok(())
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        validate::id("event topic", &topic)?;
        validate::json("event body", &json)?;
        let envelope =
            format!("{{\"v\":1,\"kind\":\"event\",\"topic\":\"{topic}\",\"body\":{json}}}");
        self.view_mut(view)?.events.push(envelope);
        Ok(())
    }

    fn feed_changed(&mut self, feed: String) {
        if self.declarations.feeds.contains(&feed) {
            self.changed_feeds.push(feed);
            return;
        }
        self.note(
            LogLevel::Warn,
            format!("feed-changed for undeclared feed {feed:?}"),
        );
    }

    /// Gated `@unstable(feature = home-widgets)`; never linked for a plugin.
    fn widget_changed(&mut self, _widget: String) {}

    fn cancel(&mut self, request: u64) {
        self.cancel_request(request);
    }

    fn now(&mut self) -> i64 {
        SYNTHETIC_NOW
    }
}
