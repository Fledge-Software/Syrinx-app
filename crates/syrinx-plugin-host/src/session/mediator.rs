//! Stands between a plugin's imports and the services behind them. It alone keeps the
//! contract's rules: what the manifest declares, which views are open, the delivery order,
//! the request bookkeeping, and refusing a service the owner has not granted or withdrew.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use crate::PluginHost;
use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, Outcome, PluginError, Timestamp};
use crate::deadline::CallDeadline;
use crate::grants::{self, Grants};
use crate::manifest::CapabilityKind;
use crate::{limits, validate};

use super::Declarations;

/// A call the host defers until the current export returns. An outcome is not deferred:
/// `Session::complete` delivers it itself, after anything already deferred.
#[derive(Debug, Clone)]
pub(crate) enum Deferred {
    Opened(u64),
    Message(ViewRequest),
}

impl Deferred {
    fn request(&self) -> Option<u64> {
        match self {
            Self::Opened(_) => None,
            Self::Message(request) => Some(request.request_id),
        }
    }
}

/// The plugin's imports as the store sees them, wrapping the services `S`.
pub struct Mediator<S> {
    services: S,
    declarations: Declarations,
    /// The owner's grants, asked on every gated call.
    grants: Arc<dyn Grants>,
    /// Kinds this instance has held: one no longer granted answers `revoked`, not `denied`.
    held: BTreeSet<CapabilityKind>,
    /// Held kinds whose revocation the services have already been told of.
    withdrawn: BTreeSet<CapabilityKind>,
    deferred: VecDeque<Deferred>,
    /// Writes in flight: a submit still running, or one that answered `pending`.
    armed: BTreeSet<u64>,
    /// Armed requests the plugin gave up on; never more than were armed.
    cancelled: BTreeSet<u64>,
    /// Each open view's kind and address, as `views.opened` will be told them.
    open_views: BTreeMap<u64, (String, String)>,
}

impl<S: PluginHost> Mediator<S> {
    pub(crate) fn new(services: S, declarations: Declarations, grants: Arc<dyn Grants>) -> Self {
        Self {
            services,
            declarations,
            grants,
            held: BTreeSet::new(),
            withdrawn: BTreeSet::new(),
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

    pub(crate) fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// The kinds granted now, which this instance then holds; for `lifecycle.activate`.
    pub(crate) fn take_granted(&mut self) -> Vec<CapabilityKind> {
        let granted = grants::granted_kinds(self.grants.as_ref(), &self.declarations.plugin_id);
        self.held.extend(granted.iter().copied());
        granted
    }

    /// Tell the services of each held kind the owner withdrew since the last look: they close
    /// what they opened under it and abandon its requests. Returns the outcome of each abandoned
    /// write still in flight, for the plugin to be told; nothing is resubmitted.
    pub(crate) fn apply_revocations(&mut self) -> Vec<(u64, Outcome)> {
        let plugin = self.declarations.plugin_id.clone();
        let newly_withdrawn: Vec<CapabilityKind> = self
            .held
            .iter()
            .copied()
            .filter(|kind| {
                !self.withdrawn.contains(kind) && !self.grants.is_granted(&plugin, *kind)
            })
            .collect();
        let mut abandoned = Vec::new();
        for kind in newly_withdrawn {
            self.withdrawn.insert(kind);
            abandoned.extend(self.services.capability_revoked(kind));
        }
        abandoned.retain(|(request, _)| {
            self.armed.contains(request) || self.cancelled.contains(request)
        });
        abandoned
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

    /// A write to `request` starts: it stays in flight until settled.
    pub(crate) fn arm(&mut self, request: u64) {
        self.armed.insert(request);
    }

    /// The write's outcome is known, or the host gave up on it: it is no longer in flight.
    /// Returns whether it was.
    pub(crate) fn settle(&mut self, request: u64) -> bool {
        self.armed.remove(&request)
    }

    /// Whether the plugin cancelled `request` itself, forgetting that it did.
    pub(crate) fn take_cancelled(&mut self, request: u64) -> bool {
        self.cancelled.remove(&request)
    }

    pub(crate) fn writes_in_flight(&self) -> Vec<u64> {
        self.armed.iter().copied().collect()
    }

    pub(crate) fn pop_deferred(&mut self) -> Option<Deferred> {
        self.deferred.pop_front()
    }

    pub(crate) fn clear_deferred(&mut self) {
        self.deferred.clear();
    }

    pub(crate) fn has_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }

    /// Take every deferred call still queued, oldest first.
    pub(crate) fn take_deferred(&mut self) -> Vec<Deferred> {
        self.deferred.drain(..).collect()
    }

    /// Drop every message still queued for `view`.
    pub(crate) fn drop_view_messages(&mut self, view: u64) {
        self.deferred.retain(
            |deferred| !matches!(deferred, Deferred::Message(request) if request.view == view),
        );
    }

    /// Drop anything still queued for `request`.
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

    /// The plugin gave up on a request: an armed one is never delivered, and
    /// anything still queued for it is dropped.
    fn plugin_cancelled(&mut self, request: u64) {
        if self.armed.remove(&request) {
            self.cancelled.insert(request);
        }
        self.drop_queued(request);
    }

    /// Refuse a service whose capability is not granted now, naming it: `revoked` if this
    /// instance held it, `denied` if it never did.
    fn require(&mut self, kind: CapabilityKind) -> Result<(), PluginError> {
        if self.grants.is_granted(&self.declarations.plugin_id, kind) {
            self.held.insert(kind);
            self.withdrawn.remove(&kind);
            return Ok(());
        }
        if self.held.contains(&kind) {
            return Err(PluginError::Revoked(format!(
                "the {} capability was revoked",
                kind.name()
            )));
        }
        Err(PluginError::Denied(format!(
            "the {} capability is not granted",
            kind.name()
        )))
    }

    /// A view may open only as a declared kind, within the open-view limit.
    fn check_view(&self, kind: &str, address: &str) -> Result<(), PluginError> {
        if !self
            .declarations
            .view_kinds
            .iter()
            .any(|declared| declared == kind)
        {
            return Err(PluginError::InvalidInput(format!(
                "view kind {kind} is not declared"
            )));
        }
        validate::bytes("view address", address, limits::MAX_ADDRESS_BYTES)?;
        if self.open_views.len() >= limits::MAX_OPEN_VIEWS {
            return Err(PluginError::Unavailable(format!(
                "a plugin may hold {} views open",
                limits::MAX_OPEN_VIEWS
            )));
        }
        Ok(())
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

    /// The services read only this plugin's own namespace: they are made for one plugin.
    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.require(CapabilityKind::Storage)?;
        validate::settings_key(&key)?;
        self.services.settings_get(key)
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.require(CapabilityKind::Storage)?;
        validate::settings_key(&key)?;
        validate::settings_value(&value)?;
        self.services.settings_set(key, value)
    }

    /// Open a declared view kind; `views.opened` follows once the current export returns.
    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        self.check_view(&kind, &address)?;
        // Checked before the service opens anything, so the `defer` below cannot fail.
        self.check_room()?;
        let target = (kind.clone(), address.clone());
        let view = self.services.open_view(kind, address)?;
        self.open_views.insert(view, target);
        self.defer(Deferred::Opened(view))?;
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

    /// Only a declared feed reaches the services; any other is noted and ignored.
    fn feed_changed(&mut self, feed: String) {
        if self.declarations.feeds.contains(&feed) {
            return self.services.feed_changed(feed);
        }
        self.services.log(
            LogLevel::Warn,
            format!("feed-changed for undeclared feed {feed:?}"),
        );
    }

    fn widget_changed(&mut self, widget: String) {
        self.services.widget_changed(widget);
    }

    /// Forget the request here first, then let the service abandon any work it started for it.
    fn cancel(&mut self, request: u64) {
        self.plugin_cancelled(request);
        self.services.cancel(request);
    }

    fn now(&mut self) -> Timestamp {
        self.services.now()
    }
}

#[cfg(test)]
mod tests;
