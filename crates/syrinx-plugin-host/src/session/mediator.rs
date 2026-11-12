//! Stands between a plugin's imports and the services behind them: it keeps the contract's
//! delivery order and request bookkeeping, and refuses a service the owner has not granted.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::PluginHost;
use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, Outcome, PluginError, Timestamp};
use crate::deadline::CallDeadline;
use crate::limits;
use crate::manifest::CapabilityKind;

/// A call the host defers until the current export returns.
#[derive(Debug, Clone)]
pub(crate) enum Deferred {
    Opened(u64),
    Message(ViewRequest),
    Deliver(u64, Outcome),
}

impl Deferred {
    fn request(&self) -> Option<u64> {
        match self {
            Self::Opened(_) => None,
            Self::Message(request) => Some(request.request_id),
            Self::Deliver(request, _) => Some(*request),
        }
    }
}

/// The plugin's imports as the store sees them, wrapping the services `S`.
pub struct Mediator<S> {
    services: S,
    /// Capabilities the owner granted; every other gated service answers `denied`.
    granted: Vec<CapabilityKind>,
    pub(crate) deferred: VecDeque<Deferred>,
    /// Writes in flight: a submit still running, or one that answered `pending`.
    pub(crate) armed: BTreeSet<u64>,
    /// Armed requests the plugin gave up on; never more than were armed.
    pub(crate) cancelled: BTreeSet<u64>,
    /// Each open view's kind and address, as `views.opened` will be told them.
    open_views: BTreeMap<u64, (String, String)>,
}

impl<S: PluginHost> Mediator<S> {
    pub(crate) fn new(services: S, granted: Vec<CapabilityKind>) -> Self {
        Self {
            services,
            granted,
            deferred: VecDeque::new(),
            armed: BTreeSet::new(),
            cancelled: BTreeSet::new(),
            open_views: BTreeMap::new(),
        }
    }

    pub(crate) fn services(&self) -> &S {
        &self.services
    }

    pub(crate) fn services_mut(&mut self) -> &mut S {
        &mut self.services
    }

    pub(crate) fn granted(&self) -> &[CapabilityKind] {
        &self.granted
    }

    pub(crate) fn is_open(&self, view: u64) -> bool {
        self.open_views.contains_key(&view)
    }

    pub(crate) fn open_views(&self) -> Vec<u64> {
        self.open_views.keys().copied().collect()
    }

    /// The kind and address an open view was opened with.
    pub(crate) fn view_target(&self, view: u64) -> Option<(String, String)> {
        self.open_views.get(&view).cloned()
    }

    /// The plugin gave up on a request: an armed one is never delivered, and
    /// anything still queued for it is dropped.
    fn plugin_cancelled(&mut self, request: u64) {
        if self.armed.remove(&request) {
            self.cancelled.insert(request);
        }
        self.drop_queued(request);
    }

    pub(crate) fn drop_queued(&mut self, request: u64) {
        self.deferred
            .retain(|deferred| deferred.request() != Some(request));
    }

    /// Queue a deferred call, unless the queue is full.
    pub(crate) fn defer(&mut self, deferred: Deferred) -> Result<(), PluginError> {
        self.check_room()?;
        self.deferred.push_back(deferred);
        Ok(())
    }

    fn check_room(&self) -> Result<(), PluginError> {
        if self.deferred.len() < limits::MAX_DEFERRED_QUEUED {
            return Ok(());
        }
        Err(PluginError::Unavailable(
            "too many calls are waiting for the plugin".into(),
        ))
    }

    /// Refuse a service whose capability the owner has not granted, naming it.
    fn require(&self, kind: CapabilityKind) -> Result<(), PluginError> {
        if self.granted.contains(&kind) {
            return Ok(());
        }
        Err(PluginError::Denied(format!(
            "the {} capability is not granted",
            kind.name()
        )))
    }
}

impl<S: PluginHost> PluginHost for Mediator<S> {
    fn begin_export(&mut self, deadline: CallDeadline) {
        self.services.begin_export(deadline);
    }

    fn view_closed(&mut self, view: u64) {
        if self.open_views.remove(&view).is_some() {
            self.services.view_closed(view);
        }
    }
}

impl<S: PluginHost> types::Host for Mediator<S> {}

impl<S: PluginHost> host::Host for Mediator<S> {
    fn log(&mut self, level: LogLevel, message: String) {
        self.services.log(level, message);
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.require(CapabilityKind::Storage)?;
        self.services.settings_get(key)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.require(CapabilityKind::Storage)?;
        self.services.settings_set(key, value)
    }

    /// Open a view; `views.opened` follows once the current export returns.
    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        self.check_room()?;
        let target = (kind.clone(), address.clone());
        let view = self.services.open_view(kind, address)?;
        self.open_views.insert(view, target);
        self.deferred.push_back(Deferred::Opened(view));
        Ok(view)
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        self.services.set_view_metadata(view, title, badge)
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        self.services.post_view_event(view, topic, json)
    }

    fn feed_changed(&mut self, feed: String) {
        self.services.feed_changed(feed);
    }

    fn widget_changed(&mut self, widget: String) {
        self.services.widget_changed(widget);
    }

    /// Forget the request here, then let the service abandon any work it started for it.
    fn cancel(&mut self, request: u64) {
        self.plugin_cancelled(request);
        self.services.cancel(request);
    }

    fn now(&mut self) -> Timestamp {
        self.services.now()
    }
}
