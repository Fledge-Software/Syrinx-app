//! In-memory bookkeeping for a plugin's views, logs and changed feeds, shared by the test
//! host and the application's services until plugin tabs exist (#29). The contract's rules
//! (declared kinds and feeds, the open-view limit) are the `Mediator`'s, in front of this.

use std::collections::BTreeMap;

use crate::bindings::syrinx::plugin::host::LogLevel;
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::{limits, validate};

/// What the tab model shows for one open plugin view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewRecord {
    pub title: String,
    pub badge: Option<String>,
    /// Envelopes sent to the view's interface, oldest first.
    pub events: Vec<String>,
}

/// One plugin's views, bounded log and changed feeds.
#[derive(Default)]
pub struct ViewBook {
    logs: Vec<(LogLevel, String)>,
    views: BTreeMap<u64, ViewRecord>,
    next_view: u64,
    /// Feeds with a change to pull, each listed once however often it was signalled.
    changed_feeds: Vec<String>,
}

impl ViewBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the plugin and the host logged, oldest first.
    pub fn logs(&self) -> &[(LogLevel, String)] {
        &self.logs
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.views.get(&view)
    }

    /// Hand the view's interface what the plugin posted to it, oldest first; `None` when the
    /// view is not open.
    pub fn take_view_events(&mut self, view: u64) -> Option<Vec<String>> {
        let record = self.views.get_mut(&view)?;
        Some(std::mem::take(&mut record.events))
    }

    pub fn changed_feeds(&self) -> &[String] {
        &self.changed_feeds
    }

    /// A bounded log line; past the line limit, lines are dropped.
    pub fn log(&mut self, level: LogLevel, message: String) {
        if self.logs.len() == limits::MAX_LOG_LINES {
            return;
        }
        let bounded: String = message.chars().take(limits::MAX_LOG_CHARS).collect();
        self.logs.push((level, bounded));
    }

    /// A view the mediator already checked; it starts titled with its kind.
    pub fn open_view(&mut self, kind: String) -> u64 {
        self.next_view += 1;
        let view = self.next_view;
        let record = ViewRecord {
            title: kind,
            badge: None,
            events: Vec::new(),
        };
        self.views.insert(view, record);
        view
    }

    pub fn view_closed(&mut self, view: u64) {
        self.views.remove(&view);
    }

    pub fn set_view_metadata(
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

    /// Queue an event for the view's interface, unless it has not read enough of them yet.
    pub fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        validate::id("event topic", &topic)?;
        validate::json("event body", &json)?;
        let envelope =
            format!("{{\"v\":1,\"kind\":\"event\",\"topic\":\"{topic}\",\"body\":{json}}}");
        let record = self.view_mut(view)?;
        let waiting: usize = record.events.iter().map(String::len).sum();
        let full = record.events.len() >= limits::MAX_VIEW_EVENTS
            || waiting + envelope.len() > limits::MAX_VIEW_EVENT_BYTES;
        if full {
            return Err(PluginError::Unavailable(
                "the view has not read its waiting events".into(),
            ));
        }
        record.events.push(envelope);
        Ok(())
    }

    pub fn feed_changed(&mut self, feed: String) {
        if !self.changed_feeds.contains(&feed) {
            self.changed_feeds.push(feed);
        }
    }

    fn view_mut(&mut self, view: u64) -> Result<&mut ViewRecord, PluginError> {
        self.views
            .get_mut(&view)
            .ok_or_else(|| PluginError::NotFound(format!("no open view {view}")))
    }
}
