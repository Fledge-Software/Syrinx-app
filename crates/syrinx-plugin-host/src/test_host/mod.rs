//! The test host: Syrinx's plugin host with synthetic capabilities, published
//! so plugin repositories run their contract tests against the real rules.
//!
//! It keeps the contract's ordering: deferred calls run first in, first out,
//! after the export that queued them; a view's `opened` precedes its messages
//! and `closed` comes last; a cancelled request is dropped before delivery.

mod synthetic;

use std::collections::BTreeMap;

use synthetic::Deferred;
pub use synthetic::{SYNTHETIC_NOW, SyntheticHost, ViewRecord};

use crate::bindings::exports::syrinx::plugin::forms::{
    FieldError, FieldValue, FormSpec, SubmitResult,
};
use crate::bindings::exports::syrinx::plugin::lifecycle::Activation;
use crate::bindings::exports::syrinx::plugin::search_feed::Batch;
use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::LogLevel;
use crate::bindings::syrinx::plugin::types::{Outcome, PluginError};
use crate::manifest::PluginManifest;
use crate::{HostError, LoadedPlugin, Runtime, WasiState, limits, validate};

/// The version the test host reports to `lifecycle.activate`.
pub const HOST_VERSION: &str = "0.1.0-test-host";

/// What a manifest will declare (#134); the test host takes it directly.
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

/// The test host takes what a checked manifest declares, as the real host will.
impl From<&PluginManifest> for Declarations {
    fn from(manifest: &PluginManifest) -> Self {
        let commands = manifest
            .commands()
            .iter()
            .map(|command| DeclaredCommand {
                id: command.id.clone(),
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
    fn check(&self) -> Result<(), HostError> {
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

/// One loaded plugin under the test host.
pub struct TestHost {
    plugin: LoadedPlugin<SyntheticHost>,
    next_request: u64,
    replies: BTreeMap<u64, Result<String, PluginError>>,
    refused_views: Vec<(u64, PluginError)>,
}

/// An export's answer, or the host's reason it never got one.
pub type CallResult<T> = Result<T, HostError>;

impl TestHost {
    /// Check the declarations and the component, then instantiate it.
    pub fn load(bytes: &[u8], declarations: Declarations) -> CallResult<Self> {
        declarations.check()?;
        let runtime = Runtime::new()?;
        let component = runtime.compile(bytes)?;
        let plugin =
            LoadedPlugin::instantiate(&runtime, &component, SyntheticHost::new(declarations))?;
        Ok(Self {
            plugin,
            next_request: 1,
            replies: BTreeMap::new(),
            refused_views: Vec::new(),
        })
    }

    pub fn activate(&mut self) -> CallResult<Result<(), PluginError>> {
        let info = Activation {
            host_version: HOST_VERSION.into(),
            granted: Vec::new(),
        };
        let activated = self
            .call(|plugin, store| plugin.syrinx_plugin_lifecycle().call_activate(store, &info))?;
        Ok(activated.map_err(validate::bound_error))
    }

    pub fn deactivate(&mut self) -> CallResult<()> {
        self.call(|plugin, store| plugin.syrinx_plugin_lifecycle().call_deactivate(store))
    }

    /// Run a declared command that has no form.
    pub fn run_command(&mut self, id: &str) -> CallResult<Result<(), PluginError>> {
        self.require_command(id, false)?;
        let id = id.to_owned();
        let ran =
            self.call(|plugin, store| plugin.syrinx_plugin_commands().call_run(store, &id))?;
        Ok(ran.map_err(validate::bound_error))
    }

    pub fn form_for(&mut self, id: &str) -> CallResult<Result<FormSpec, PluginError>> {
        self.require_command(id, true)?;
        let id = id.to_owned();
        let spec =
            self.call(|plugin, store| plugin.syrinx_plugin_forms().call_form_for(store, &id))?;
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
        let errors = self.call(|plugin, store| {
            plugin
                .syrinx_plugin_forms()
                .call_validate(store, &id, &values)
        })?;
        Ok(errors
            .map_err(validate::bound_error)
            .and_then(|errors| validate::field_errors(&errors).map(|()| errors)))
    }

    /// Submit a form under a fresh request id. A `pending` answer must return that id,
    /// and arms it for `complete`; the answer is checked before deferred calls run.
    pub fn submit(&mut self, id: &str, values: &[(&str, &str)]) -> CallResult<(u64, SubmitResult)> {
        self.require_command(id, true)?;
        let request = self.allocate_request();
        let (id, values) = (id.to_owned(), field_values(values));
        let result = self.plugin.call(|plugin, store| {
            plugin
                .syrinx_plugin_forms()
                .call_submit(store, &id, request, &values)
        })?;
        let result = self.check_submit(request, result);
        self.run_deferred()?;
        Ok((request, result))
    }

    /// The synthetic service behind an armed request finished; deliver its outcome.
    pub fn complete(&mut self, request: u64, outcome: Outcome) -> CallResult<()> {
        // The plugin gave up on it through `host.cancel`; nothing is delivered.
        if self.host_mut().cancelled.remove(&request) {
            return Ok(());
        }
        if !self.host_mut().armed.remove(&request) {
            return Err(HostError::Refused(format!(
                "request {request} is not armed"
            )));
        }
        self.defer(Deferred::Deliver(request, outcome))?;
        self.run_deferred()
    }

    /// Queue a message from a view's interface; it is delivered by `run_deferred`.
    pub fn queue_view_message(&mut self, view: u64, method: &str, json: &str) -> CallResult<u64> {
        if !self.host().views.contains_key(&view) {
            return Err(HostError::Refused(format!("no open view {view}")));
        }
        validate::json("view message", json)
            .map_err(|error| HostError::Refused(format!("{error:?}")))?;
        let request = ViewRequest {
            view,
            request_id: self.allocate_request(),
            method: method.into(),
            json: json.into(),
        };
        let request_id = request.request_id;
        self.defer(Deferred::Message(request))?;
        Ok(request_id)
    }

    /// The view gave up on a request; anything still queued for it is dropped.
    pub fn cancel_request(&mut self, request: u64) {
        self.host_mut().armed.remove(&request);
        self.host_mut().drop_queued(request);
    }

    /// The view's interface reads what the plugin posted to it, oldest first;
    /// `None` when the view is not open.
    pub fn take_view_events(&mut self, view: u64) -> Option<Vec<String>> {
        let record = self.host_mut().views.get_mut(&view)?;
        Some(std::mem::take(&mut record.events))
    }

    /// The plugin's answer to a view message, once delivered.
    pub fn reply(&self, request: u64) -> Option<&Result<String, PluginError>> {
        self.replies.get(&request)
    }

    /// The owner closed a tab: undelivered messages are dropped, then `closed` comes last.
    pub fn close_view(&mut self, view: u64) -> CallResult<()> {
        if !self.host().views.contains_key(&view) {
            return Err(HostError::Refused(format!("no open view {view}")));
        }
        self.host_mut().deferred.retain(
            |deferred| !matches!(deferred, Deferred::Message(request) if request.view == view),
        );
        self.run_deferred()?;
        self.host_mut().views.remove(&view);
        self.call(|plugin, store| plugin.syrinx_plugin_views().call_closed(store, view))
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
        let batch = self.call(|plugin, store| {
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
                    self.host_mut()
                        .note(LogLevel::Warn, format!("dropped {reason}"));
                }
                Ok(Ok(batch))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    pub fn resolve(&mut self, feed: &str, id: &str) -> CallResult<Result<String, PluginError>> {
        self.require_feed(feed)?;
        let (feed, id) = (feed.to_owned(), id.to_owned());
        let address = self.call(|plugin, store| {
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
            let Some(deferred) = self.host_mut().deferred.pop_front() else {
                return Ok(());
            };
            self.deliver(deferred)?;
        }
        self.host_mut().deferred.clear();
        self.plugin.discard();
        Err(HostError::Refused(
            "the plugin kept queueing deferred calls, so it was stopped".into(),
        ))
    }

    pub fn logs(&self) -> &[(LogLevel, String)] {
        &self.host().logs
    }

    pub fn wasi(&self) -> &WasiState {
        self.plugin.wasi()
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.host().views.get(&view)
    }

    pub fn open_views(&self) -> Vec<u64> {
        self.host().views.keys().copied().collect()
    }

    /// Views whose `opened` the plugin refused, with its error; the host closed them.
    pub fn refused_views(&self) -> &[(u64, PluginError)] {
        &self.refused_views
    }

    pub fn changed_feeds(&self) -> &[String] {
        &self.host().changed_feeds
    }

    pub fn export_calls(&self) -> u64 {
        self.plugin.export_calls()
    }

    pub fn is_discarded(&self) -> bool {
        self.plugin.is_discarded()
    }

    fn host(&self) -> &SyntheticHost {
        self.plugin.host()
    }

    fn host_mut(&mut self) -> &mut SyntheticHost {
        self.plugin.host_mut()
    }

    /// One export call, then whatever it deferred.
    fn call<R>(
        &mut self,
        export: impl FnOnce(
            &crate::bindings::Plugin,
            &mut wasmtime::Store<crate::StoreData<SyntheticHost>>,
        ) -> wasmtime::Result<R>,
    ) -> CallResult<R> {
        let result = self.plugin.call(export)?;
        self.run_deferred()?;
        Ok(result)
    }

    fn deliver(&mut self, deferred: Deferred) -> CallResult<()> {
        match deferred {
            Deferred::Opened(view) => self.deliver_opened(view),
            Deferred::Message(request) => self.deliver_message(request),
            Deferred::Deliver(request, outcome) => self.plugin.call(|plugin, store| {
                plugin
                    .syrinx_plugin_events()
                    .call_deliver(store, request, &outcome)
            }),
        }
    }

    /// A view the plugin refuses to open is closed again and recorded.
    fn deliver_opened(&mut self, view: u64) -> CallResult<()> {
        let Some(record) = self.host().views.get(&view).cloned() else {
            return Ok(());
        };
        let opened = self.plugin.call(|plugin, store| {
            plugin
                .syrinx_plugin_views()
                .call_opened(store, view, &record.kind, &record.address)
        })?;
        if let Err(error) = opened {
            self.host_mut().views.remove(&view);
            self.refused_views.push((view, error));
        }
        Ok(())
    }

    fn deliver_message(&mut self, request: ViewRequest) -> CallResult<()> {
        let request_id = request.request_id;
        // A view refused or closed since the message was queued never reaches the plugin.
        if !self.host().views.contains_key(&request.view) {
            let closed = PluginError::Unavailable(format!("view {} is not open", request.view));
            self.replies.insert(request_id, Err(closed));
            return Ok(());
        }
        let reply = self
            .plugin
            .call(|plugin, store| plugin.syrinx_plugin_views().call_message(store, &request))?;
        let reply = reply
            .map_err(validate::bound_error)
            .and_then(|json| validate::json("view reply", &json).map(|()| json));
        self.replies.insert(request_id, reply);
        Ok(())
    }

    /// A `pending` answer must carry the host's id; anything else is the plugin's error.
    fn check_submit(&mut self, request: u64, result: SubmitResult) -> SubmitResult {
        match result {
            SubmitResult::Pending(returned) if returned == request => {
                self.host_mut().armed.insert(request);
                SubmitResult::Pending(returned)
            }
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
        self.host_mut()
            .defer(deferred)
            .map_err(|error| HostError::Refused(format!("{error:?}")))
    }

    /// Request ids are never reused, across views, forms and reloads.
    fn allocate_request(&mut self) -> u64 {
        let request = self.next_request;
        self.next_request += 1;
        request
    }

    /// A declared command, with or without a form as the caller expects.
    fn require_command(&self, id: &str, form: bool) -> CallResult<()> {
        let Some(command) = self.host().declarations.command(id) else {
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
            .host()
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
