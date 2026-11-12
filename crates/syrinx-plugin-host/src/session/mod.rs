//! One instantiated plugin and the contract's rules around every call into it, shared by the
//! test host and the per-plugin workers.
//!
//! It keeps the contract's ordering: deferred calls run first in, first out,
//! after the export that queued them; a view's `opened` precedes its messages
//! and `closed` comes last; a cancelled request is dropped before delivery.

mod mediator;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

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
use crate::instance::Stop;
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
    host_version: String,
    requests: Arc<RequestIds>,
    replies: BTreeMap<u64, Result<String, PluginError>>,
    refused_views: Vec<(u64, PluginError)>,
    /// The worker's limits on its current job; the test host runs without them.
    job: Option<JobLimits>,
}

/// What bounds one worker job: its export and every call that export deferred.
pub(crate) struct JobLimits {
    /// Raised when the worker is told to stop; deferred calls still queued are then dropped.
    pub(crate) stop: Arc<AtomicBool>,
    /// The job's budget (`limits::JOB_BUDGET`), checked between deferred calls.
    pub(crate) until: Instant,
}

/// What a view message or an outcome dropped by a stop request is answered with.
const STOPPING: &str = "the plugin is stopping";

/// Whether the host may start a call of its own now.
enum Gate {
    Go,
    /// The worker was told to stop; the instance is kept and deactivated next.
    Stopping,
}

impl<S: PluginHost> Session<S> {
    pub(crate) fn new(
        plugin: LoadedPlugin<Mediator<S>>,
        host_version: String,
        requests: Arc<RequestIds>,
    ) -> Self {
        Self {
            plugin,
            host_version,
            requests,
            replies: BTreeMap::new(),
            refused_views: Vec::new(),
            job: None,
        }
    }

    /// A worker job starts under `limits`.
    pub(crate) fn begin_job(&mut self, limits: JobLimits) {
        self.job = Some(limits);
    }

    /// Call `lifecycle.activate` with the host's version and the granted capabilities.
    /// An error says why the instance stopped.
    pub fn activate(&mut self) -> Result<Result<(), PluginError>, Failure> {
        let info = Activation {
            host_version: self.host_version.clone(),
            granted: self
                .mediator()
                .granted()
                .iter()
                .map(|kind| kind.name().to_owned())
                .collect(),
        };
        let activated = self
            .call(Export::Activate, |plugin, store| {
                plugin.syrinx_plugin_lifecycle().call_activate(store, &info)
            })
            .map_err(|stop| stop.failure)?;
        Ok(activated.map_err(validate::bound_error))
    }

    /// Call `lifecycle.deactivate`; an error says why the instance stopped.
    pub fn deactivate(&mut self) -> Result<(), Failure> {
        self.call(Export::Deactivate, |plugin, store| {
            plugin.syrinx_plugin_lifecycle().call_deactivate(store)
        })
        .map_err(|stop| stop.failure)
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
    /// for `complete`. Once the plugin has answered, the answer is returned even if a deferred
    /// call then stops the plugin; that stop shows in `failure()`. If the plugin stops during
    /// the submit itself, the error names the request, whose outcome is then unknown.
    pub fn submit(&mut self, id: &str, values: &[(&str, &str)]) -> CallResult<(u64, SubmitResult)> {
        self.require_command(id, true)?;
        if self.is_discarded() {
            return Err(HostError::Discarded);
        }
        let request = self.requests.allocate();
        let (id, values) = (id.to_owned(), field_values(values));
        self.mediator_mut().arm(request);
        let result = self
            .plugin
            .call(Export::Submit, |plugin, store| {
                plugin
                    .syrinx_plugin_forms()
                    .call_submit(store, &id, request, &values)
            })
            .map_err(|stop| HostError::SubmitStopped {
                request,
                cause: Box::new(stop.error),
            })?;
        let result = self.check_submit(request, result);
        self.deliver_after_answer();
        Ok((request, result))
    }

    /// The service behind an armed request finished; deliver its outcome after any calls
    /// already deferred. If the worker is being stopped, the outcome is not delivered and
    /// the answer is `unavailable`, never success.
    pub fn complete(&mut self, request: u64, outcome: Outcome) -> CallResult<()> {
        // The plugin gave up on it through `host.cancel`; nothing is delivered.
        if self.mediator_mut().take_cancelled(request) {
            return Ok(());
        }
        if !self.mediator_mut().settle(request) {
            return Err(HostError::Refused(format!(
                "request {request} is not armed"
            )));
        }
        self.run_deferred()?;
        let undelivered = HostError::Unavailable(format!(
            "{STOPPING}, so request {request} was not delivered"
        ));
        match self.before_host_call() {
            Ok(Gate::Go) => {}
            Ok(Gate::Stopping) => return Err(undelivered),
            // The budget is spent: the instance is stopped, and `failure()` says so.
            Err(stop) => {
                debug_assert_eq!(self.failure(), Some(&stop.failure));
                return Err(undelivered);
            }
        }
        self.call(Export::Deliver, |plugin, store| {
            plugin
                .syrinx_plugin_events()
                .call_deliver(store, request, &outcome)
        })?;
        Ok(())
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
        self.mediator_mut().settle(request);
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
        self.mediator_mut().drop_view_messages(view);
        self.run_deferred()?;
        self.mediator_mut().view_closed(view);
        self.call(Export::Closed, |plugin, store| {
            plugin.syrinx_plugin_views().call_closed(store, view)
        })?;
        Ok(())
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
        self.deliver_all()?;
        Ok(())
    }

    /// Writes whose outcome is not known yet: a submit that never returned, or one that
    /// answered `pending` and was not completed or cancelled.
    pub fn writes_in_flight(&self) -> Vec<u64> {
        self.mediator().writes_in_flight()
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
    ) -> Result<R, Stop> {
        let result = self.plugin.call(export, call)?;
        self.deliver_all()?;
        Ok(result)
    }

    /// Deliver what an export deferred once its answer is known. A stop here is kept in
    /// `failure()` and reported from there, never in place of that answer.
    fn deliver_after_answer(&mut self) {
        if let Err(stop) = self.deliver_all() {
            debug_assert_eq!(self.failure(), Some(&stop.failure));
        }
    }

    /// Deliver every deferred call in order. A stop request drops the rest, telling each
    /// caller, and keeps the instance; the instance is stopped if it keeps queueing more or
    /// the job's budget is spent.
    fn deliver_all(&mut self) -> Result<(), Stop> {
        for _ in 0..limits::MAX_DEFERRED_PER_EXPORT {
            let Some(deferred) = self.mediator_mut().pop_deferred() else {
                return Ok(());
            };
            if let Gate::Stopping = self.before_host_call()? {
                self.drop_for_stop(deferred);
                return Ok(());
            }
            self.deliver(deferred)?;
        }
        Err(self.stop_delivering(Failure::Flooded))
    }

    /// Drop `first` and everything still deferred: a view message is answered `unavailable`.
    /// A view whose `opened` is dropped stays open, and is closed with every open view when
    /// the worker retires the instance.
    fn drop_for_stop(&mut self, first: Deferred) {
        let mut dropped = vec![first];
        dropped.extend(self.mediator_mut().take_deferred());
        for deferred in dropped {
            match deferred {
                Deferred::Message(request) => {
                    let stopping = PluginError::Unavailable(STOPPING.into());
                    self.replies.insert(request.request_id, Err(stopping));
                }
                Deferred::Opened(_) => {}
            }
        }
    }

    /// Tell the services every view still open is closed, before the store is dropped.
    pub(crate) fn close_open_views(&mut self) {
        for view in self.open_views() {
            self.mediator_mut().view_closed(view);
        }
    }

    /// Before the host starts a call of its own: a stop request means stopping, and a
    /// spent job budget stops the instance (`Failure::OverBudget`).
    fn before_host_call(&mut self) -> Result<Gate, Stop> {
        if self.stop_requested() {
            return Ok(Gate::Stopping);
        }
        if self.budget_spent() {
            return Err(self.stop_delivering(Failure::OverBudget));
        }
        Ok(Gate::Go)
    }

    fn stop_requested(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.stop.load(Ordering::Relaxed))
    }

    fn budget_spent(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| Instant::now() >= job.until)
    }

    /// Drop what is still deferred and discard the instance.
    fn stop_delivering(&mut self, failure: Failure) -> Stop {
        self.mediator_mut().clear_deferred();
        self.plugin.discard(failure.clone());
        let error = HostError::Refused(format!("the plugin was stopped: {failure}"));
        Stop { failure, error }
    }

    fn deliver(&mut self, deferred: Deferred) -> Result<(), Stop> {
        match deferred {
            Deferred::Opened(view) => self.deliver_opened(view),
            Deferred::Message(request) => self.deliver_message(request),
        }
    }

    /// A view the plugin refuses to open is closed again and recorded.
    fn deliver_opened(&mut self, view: u64) -> Result<(), Stop> {
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

    fn deliver_message(&mut self, request: ViewRequest) -> Result<(), Stop> {
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
        self.mediator_mut().settle(request);
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
        let Some(command) = self.mediator().declarations().command(id) else {
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
        let feeds = &self.mediator().declarations().feeds;
        if feeds.iter().any(|declared| declared == feed) {
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
