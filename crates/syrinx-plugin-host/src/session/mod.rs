//! One instantiated plugin and the contract's rules around every call into it, shared by the
//! test host and the per-plugin workers.
//!
//! It keeps the contract's ordering: deferred calls run first in, first out,
//! after the export that queued them; a view's `opened` precedes its messages
//! and `closed` comes last; a cancelled request is dropped before delivery.

mod mediator;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use mediator::Deferred;
pub use mediator::Mediator;

use crate::bindings::Plugin;
use crate::bindings::exports::syrinx::plugin::forms::{
    FieldError, FieldValue, FormSpec, SubmitResult,
};
use crate::bindings::exports::syrinx::plugin::lifecycle::Activation;
use crate::bindings::exports::syrinx::plugin::search_feed::Batch;
use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::LogLevel;
use crate::bindings::syrinx::plugin::types::{Outcome, PluginError};
use crate::deadline::Export;
use crate::manifest::PluginManifest;
use crate::{Failure, HostError, LoadedPlugin, PluginHost, StoreData, WasiState, limits, validate};

/// An export's answer, or the host's reason it never got one.
pub type CallResult<T> = Result<T, HostError>;

/// What a plugin's manifest declares, which the host checks every call against.
#[derive(Debug, Clone, Default)]
pub struct Declarations {
    pub plugin_id: String,
    pub commands: Vec<DeclaredCommand>,
    pub view_kinds: Vec<String>,
    pub feeds: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DeclaredCommand {
    pub id: String,
    /// A command with a form is reached through `forms`, never `commands.run`.
    pub form: bool,
}

/// What a checked manifest declares.
impl From<&PluginManifest> for Declarations {
    fn from(manifest: &PluginManifest) -> Self {
        let commands = manifest
            .commands()
            .iter()
            .map(|command| DeclaredCommand {
                id: command.id.as_str().to_owned(),
                form: command.form,
            })
            .collect();
        Self {
            plugin_id: manifest.id().to_owned(),
            commands,
            view_kinds: manifest
                .tab_types()
                .iter()
                .map(|tab| tab.id.clone())
                .collect(),
            feeds: manifest
                .search_feeds()
                .iter()
                .map(|feed| feed.id.clone())
                .collect(),
        }
    }
}

impl Declarations {
    pub(crate) fn check(&self) -> Result<(), HostError> {
        let refuse = |error: PluginError| HostError::Refused(format!("declarations: {error:?}"));
        validate::id("plugin id", &self.plugin_id).map_err(refuse)?;
        for command in &self.commands {
            validate::command_id(&self.plugin_id, &command.id).map_err(refuse)?;
        }
        for name in self.view_kinds.iter().chain(&self.feeds) {
            validate::id("declared name", name).map_err(refuse)?;
        }
        Ok(())
    }

    fn command(&self, id: &str) -> Option<&DeclaredCommand> {
        self.commands.iter().find(|command| command.id == id)
    }
}

/// Allocates request ids. One allocator outlives every instance of a plugin, so an id is never
/// reused across view or controller lifetimes (the trap in `docs/dev/trapstoknow.md`).
#[derive(Debug)]
pub struct RequestIds(AtomicU64);

impl Default for RequestIds {
    fn default() -> Self {
        Self(AtomicU64::new(1))
    }
}

impl RequestIds {
    pub fn allocate(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

/// One plugin instance, with its services `S` behind the mediator.
pub struct Session<S: 'static> {
    plugin: LoadedPlugin<Mediator<S>>,
    declarations: Declarations,
    host_version: String,
    requests: std::sync::Arc<RequestIds>,
    replies: BTreeMap<u64, Result<String, PluginError>>,
    refused_views: Vec<(u64, PluginError)>,
}

impl<S: PluginHost> Session<S> {
    pub(crate) fn new(
        plugin: LoadedPlugin<Mediator<S>>,
        declarations: Declarations,
        host_version: String,
        requests: std::sync::Arc<RequestIds>,
    ) -> Self {
        Self {
            plugin,
            declarations,
            host_version,
            requests,
            replies: BTreeMap::new(),
            refused_views: Vec::new(),
        }
    }

    /// Call `lifecycle.activate` with the host's version and the granted capabilities.
    pub fn activate(&mut self) -> CallResult<Result<(), PluginError>> {
        let info = Activation {
            host_version: self.host_version.clone(),
            granted: self
                .mediator()
                .granted()
                .iter()
                .map(|kind| kind.name().to_owned())
                .collect(),
        };
        let activated = self.call(Export::Activate, |plugin, store| {
            plugin.syrinx_plugin_lifecycle().call_activate(store, &info)
        })?;
        Ok(activated.map_err(validate::bound_error))
    }

    pub fn deactivate(&mut self) -> CallResult<()> {
        self.call(Export::Deactivate, |plugin, store| {
            plugin.syrinx_plugin_lifecycle().call_deactivate(store)
        })
    }

    /// Run a declared command that has no form.
    pub fn run_command(&mut self, id: &str) -> CallResult<Result<(), PluginError>> {
        self.require_command(id, false)?;
        let id = id.to_owned();
        let ran = self.call(Export::Run, |plugin, store| {
            plugin.syrinx_plugin_commands().call_run(store, &id)
        })?;
        Ok(ran.map_err(validate::bound_error))
    }

    pub fn form_for(&mut self, id: &str) -> CallResult<Result<FormSpec, PluginError>> {
        self.require_command(id, true)?;
        let id = id.to_owned();
        let spec = self.call(Export::FormFor, |plugin, store| {
            plugin.syrinx_plugin_forms().call_form_for(store, &id)
        })?;
        Ok(spec
            .map_err(validate::bound_error)
            .and_then(|spec| validate::form_spec(&spec).map(|()| spec)))
    }

    pub fn validate_form(
        &mut self,
        id: &str,
        values: &[(&str, &str)],
    ) -> CallResult<Result<Vec<FieldError>, PluginError>> {
        self.require_command(id, true)?;
        let (id, values) = (id.to_owned(), field_values(values));
        let errors = self.call(Export::Validate, |plugin, store| {
            plugin
                .syrinx_plugin_forms()
                .call_validate(store, &id, &values)
        })?;
        Ok(errors
            .map_err(validate::bound_error)
            .and_then(|errors| validate::field_errors(&errors).map(|()| errors)))
    }

    /// Submit a form under a fresh request id. The write counts as in flight from the call
    /// until its outcome is known; a `pending` answer must return that id, and keeps it armed
    /// for `complete`. The answer is checked before deferred calls run.
    pub fn submit(&mut self, id: &str, values: &[(&str, &str)]) -> CallResult<(u64, SubmitResult)> {
        self.require_command(id, true)?;
        let request = self.requests.allocate();
        let (id, values) = (id.to_owned(), field_values(values));
        self.mediator_mut().armed.insert(request);
        let result = self.plugin.call(Export::Submit, |plugin, store| {
            plugin
                .syrinx_plugin_forms()
                .call_submit(store, &id, request, &values)
        })?;
        let result = self.check_submit(request, result);
        self.run_deferred()?;
        Ok((request, result))
    }

    /// The service behind an armed request finished; deliver its outcome.
    pub fn complete(&mut self, request: u64, outcome: Outcome) -> CallResult<()> {
        // The plugin gave up on it through `host.cancel`; nothing is delivered.
        if self.mediator_mut().cancelled.remove(&request) {
            return Ok(());
        }
        if !self.mediator_mut().armed.remove(&request) {
            return Err(HostError::Refused(format!(
                "request {request} is not armed"
            )));
        }
        self.defer(Deferred::Deliver(request, outcome))?;
        self.run_deferred()
    }

    /// Queue a message from a view's interface; it is delivered by `run_deferred`.
    pub fn queue_view_message(&mut self, view: u64, method: &str, json: &str) -> CallResult<u64> {
        if !self.mediator().is_open(view) {
            return Err(HostError::Refused(format!("no open view {view}")));
        }
        validate::json("view message", json)
            .map_err(|error| HostError::Refused(format!("{error:?}")))?;
        let request = ViewRequest {
            view,
            request_id: self.requests.allocate(),
            method: method.into(),
            json: json.into(),
        };
        let request_id = request.request_id;
        self.defer(Deferred::Message(request))?;
        Ok(request_id)
    }

    /// The view or the owner gave up on a request; anything still queued for it is dropped
    /// and a late completion is never delivered.
    pub fn cancel_request(&mut self, request: u64) {
        self.mediator_mut().armed.remove(&request);
        self.mediator_mut().drop_queued(request);
    }

    /// The plugin's answer to a view message, once delivered.
    pub fn reply(&self, request: u64) -> Option<&Result<String, PluginError>> {
        self.replies.get(&request)
    }

    /// The owner closed a tab: undelivered messages are dropped, then `closed` comes last.
    pub fn close_view(&mut self, view: u64) -> CallResult<()> {
        if !self.mediator().is_open(view) {
            return Err(HostError::Refused(format!("no open view {view}")));
        }
        self.mediator_mut().deferred.retain(
            |deferred| !matches!(deferred, Deferred::Message(request) if request.view == view),
        );
        self.run_deferred()?;
        self.mediator_mut().view_closed(view);
        self.call(Export::Closed, |plugin, store| {
            plugin.syrinx_plugin_views().call_closed(store, view)
        })
    }

    /// Pull a bounded batch from a declared feed; over-limit keywords are dropped and logged.
    pub fn changes(
        &mut self,
        feed: &str,
        after: Option<&str>,
        max: u32,
    ) -> CallResult<Result<Batch, PluginError>> {
        self.require_feed(feed)?;
        let max = max.min(limits::MAX_BATCH_CHANGES);
        let (feed, after) = (feed.to_owned(), after.map(str::to_owned));
        let batch = self.call(Export::Changes, |plugin, store| {
            plugin
                .syrinx_plugin_search_feed()
                .call_changes(store, &feed, after.as_deref(), max)
        })?;
        let mut batch = match batch {
            Ok(batch) => batch,
            Err(error) => return Ok(Err(validate::bound_error(error))),
        };
        match validate::batch(&mut batch, max) {
            Ok(dropped) => {
                for reason in dropped {
                    self.services_mut()
                        .log(LogLevel::Warn, format!("dropped {reason}"));
                }
                Ok(Ok(batch))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    pub fn resolve(&mut self, feed: &str, id: &str) -> CallResult<Result<String, PluginError>> {
        self.require_feed(feed)?;
        let (feed, id) = (feed.to_owned(), id.to_owned());
        let address = self.call(Export::Resolve, |plugin, store| {
            plugin
                .syrinx_plugin_search_feed()
                .call_resolve(store, &feed, &id)
        })?;
        Ok(address.map_err(validate::bound_error).and_then(|address| {
            validate::bytes("address", &address, limits::MAX_ADDRESS_BYTES).map(|()| address)
        }))
    }

    /// Deliver every deferred call, first in, first out. A plugin that keeps
    /// queueing more is stopped, as a trap would stop it.
    pub fn run_deferred(&mut self) -> CallResult<()> {
        for _ in 0..limits::MAX_DEFERRED_PER_EXPORT {
            let Some(deferred) = self.mediator_mut().deferred.pop_front() else {
                return Ok(());
            };
            self.deliver(deferred)?;
        }
        self.mediator_mut().deferred.clear();
        self.plugin.discard(Failure::Flooded);
        Err(HostError::Refused(
            "the plugin kept queueing deferred calls, so it was stopped".into(),
        ))
    }

    /// Writes whose outcome is not known yet: a submit that never returned, or one that
    /// answered `pending` and was not completed or cancelled.
    pub fn writes_in_flight(&self) -> Vec<u64> {
        self.mediator().armed.iter().copied().collect()
    }

    pub fn wasi(&self) -> &WasiState {
        self.plugin.wasi()
    }

    pub fn open_views(&self) -> Vec<u64> {
        self.mediator().open_views()
    }

    /// Views whose `opened` the plugin refused, with its error; the host closed them.
    pub fn refused_views(&self) -> &[(u64, PluginError)] {
        &self.refused_views
    }

    pub fn export_calls(&self) -> u64 {
        self.plugin.export_calls()
    }

    pub fn is_discarded(&self) -> bool {
        self.plugin.is_discarded()
    }

    /// Why the instance was stopped, once it has been.
    pub fn failure(&self) -> Option<&Failure> {
        self.plugin.failure()
    }

    /// The services behind the plugin's imports.
    pub fn services(&self) -> &S {
        self.mediator().services()
    }

    pub fn services_mut(&mut self) -> &mut S {
        self.mediator_mut().services_mut()
    }

    fn mediator(&self) -> &Mediator<S> {
        self.plugin.host()
    }

    fn mediator_mut(&mut self) -> &mut Mediator<S> {
        self.plugin.host_mut()
    }

    /// One export call, then whatever it deferred.
    fn call<R>(
        &mut self,
        export: Export,
        call: impl FnOnce(&Plugin, &mut wasmtime::Store<StoreData<Mediator<S>>>) -> wasmtime::Result<R>,
    ) -> CallResult<R> {
        let result = self.plugin.call(export, call)?;
        self.run_deferred()?;
        Ok(result)
    }

    fn deliver(&mut self, deferred: Deferred) -> CallResult<()> {
        match deferred {
            Deferred::Opened(view) => self.deliver_opened(view),
            Deferred::Message(request) => self.deliver_message(request),
            Deferred::Deliver(request, outcome) => {
                self.plugin.call(Export::Deliver, |plugin, store| {
                    plugin
                        .syrinx_plugin_events()
                        .call_deliver(store, request, &outcome)
                })
            }
        }
    }

    /// A view the plugin refuses to open is closed again and recorded.
    fn deliver_opened(&mut self, view: u64) -> CallResult<()> {
        let Some((kind, address)) = self.mediator().view_target(view) else {
            return Ok(());
        };
        let opened = self.plugin.call(Export::Opened, |plugin, store| {
            plugin
                .syrinx_plugin_views()
                .call_opened(store, view, &kind, &address)
        })?;
        if let Err(error) = opened {
            self.mediator_mut().view_closed(view);
            self.refused_views.push((view, error));
        }
        Ok(())
    }

    fn deliver_message(&mut self, request: ViewRequest) -> CallResult<()> {
        let request_id = request.request_id;
        // A view refused or closed since the message was queued never reaches the plugin.
        if !self.mediator().is_open(request.view) {
            let closed = PluginError::Unavailable(format!("view {} is not open", request.view));
            self.replies.insert(request_id, Err(closed));
            return Ok(());
        }
        let reply = self.plugin.call(Export::Message, |plugin, store| {
            plugin.syrinx_plugin_views().call_message(store, &request)
        })?;
        let reply = reply
            .map_err(validate::bound_error)
            .and_then(|json| validate::json("view reply", &json).map(|()| json));
        self.replies.insert(request_id, reply);
        Ok(())
    }

    /// A `pending` answer must carry the host's id and keeps the request armed; anything
    /// else settles the write, and an answer with another id is the plugin's error.
    fn check_submit(&mut self, request: u64, result: SubmitResult) -> SubmitResult {
        if matches!(result, SubmitResult::Pending(returned) if returned == request) {
            return result;
        }
        self.mediator_mut().armed.remove(&request);
        match result {
            SubmitResult::Pending(returned) => {
                SubmitResult::Finished(Outcome::Failed(PluginError::InvalidInput(format!(
                    "pending returned request {returned}, not the host's {request}"
                ))))
            }
            SubmitResult::Invalid(errors) => match validate::field_errors(&errors) {
                Ok(()) => SubmitResult::Invalid(errors),
                Err(error) => SubmitResult::Finished(Outcome::Failed(error)),
            },
            SubmitResult::Finished(outcome) => {
                SubmitResult::Finished(validate::bound_outcome(outcome))
            }
        }
    }

    /// Queue a host-side deferred call, unless the queue is full.
    fn defer(&mut self, deferred: Deferred) -> CallResult<()> {
        self.mediator_mut()
            .defer(deferred)
            .map_err(|error| HostError::Refused(format!("{error:?}")))
    }

    /// A declared command, with or without a form as the caller expects.
    fn require_command(&self, id: &str, form: bool) -> CallResult<()> {
        let Some(command) = self.declarations.command(id) else {
            return Err(HostError::Refused(format!("command {id} is not declared")));
        };
        if command.form == form {
            return Ok(());
        }
        let reason = if command.form {
            "has a form, so it is never run directly"
        } else {
            "has no form"
        };
        Err(HostError::Refused(format!("command {id} {reason}")))
    }

    fn require_feed(&self, feed: &str) -> CallResult<()> {
        if self
            .declarations
            .feeds
            .iter()
            .any(|declared| declared == feed)
        {
            return Ok(());
        }
        Err(HostError::Refused(format!("feed {feed} is not declared")))
    }
}

fn field_values(values: &[(&str, &str)]) -> Vec<FieldValue> {
    values
        .iter()
        .map(|(name, value)| FieldValue {
            name: (*name).into(),
            value: (*value).into(),
        })
        .collect()
}
