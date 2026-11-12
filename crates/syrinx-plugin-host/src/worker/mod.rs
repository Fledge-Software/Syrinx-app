//! Runs each active plugin on its own worker thread, never the UI thread, behind a bounded
//! queue, so a slow, stuck or crashing plugin never stops the owner browsing (P2-03).

mod thread;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::bindings::syrinx::plugin::types::Outcome;
use crate::manifest::{CapabilityKind, PluginManifest};
use crate::registry::RegisteredPlugin;
use crate::session::{CallResult, RequestIds, Session};
use crate::{Failure, HostError, PluginHost, Runtime, limits};

use thread::{Job, Prepared, Shared, Worker};

/// Makes the services behind a plugin's imports, once per instance.
pub type ServiceFactory<S> = Arc<dyn Fn(&PluginManifest) -> S + Send + Sync>;

/// Where a plugin is in its life, as the owner is shown it.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginStatus {
    /// Registered, but none of its code has run since Syrinx started or it last stopped.
    Inactive,
    /// Its worker is running (or activating) it.
    Active,
    /// It stopped on its own; it starts again only on its next explicit use.
    Failed(Failure),
    /// The host deactivated it.
    Deactivated(StopReason),
}

/// Why the host deactivated a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    Shutdown,
    Uninstalled,
    /// The owner revoked a capability the plugin's manifest marks required.
    CapabilityRevoked(CapabilityKind),
}

/// What a worker tells the application without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginEvent {
    /// A write the host settled itself: `uncertain` once its plugin stopped. Never resubmitted.
    Outcome {
        plugin: String,
        request: u64,
        outcome: Outcome,
    },
    Failed {
        plugin: String,
        failure: Failure,
    },
    Deactivated {
        plugin: String,
        reason: StopReason,
    },
}

/// The answer to one queued call, collected without blocking the caller.
pub struct Pending<R>(Receiver<CallResult<R>>);

impl<R> Pending<R> {
    /// The answer if it has arrived; never blocks, so the UI thread polls this.
    pub fn try_answer(&self) -> Option<CallResult<R>> {
        let received = self.0.try_recv();
        let worker_gone = matches!(received, Err(TryRecvError::Disconnected));
        settle(received, worker_gone)
    }

    /// Wait at most `timeout` for the answer; for tests and shutdown, never the UI thread.
    pub fn wait(&self, timeout: Duration) -> Option<CallResult<R>> {
        let received = self.0.recv_timeout(timeout);
        let worker_gone = matches!(received, Err(RecvTimeoutError::Disconnected));
        settle(received, worker_gone)
    }
}

/// An answer that arrived, or `unavailable` once the worker can no longer send one.
fn settle<R, E>(received: Result<CallResult<R>, E>, worker_gone: bool) -> Option<CallResult<R>> {
    match received {
        Ok(answer) => Some(answer),
        Err(_) if worker_gone => Some(Err(stopped_before_answering())),
        Err(_) => None,
    }
}

fn stopped_before_answering() -> HostError {
    HostError::Unavailable("the plugin stopped before answering".into())
}

/// Every installed plugin and the worker of each active one.
pub struct Plugins<S: PluginHost> {
    runtime: Runtime,
    requests: Arc<RequestIds>,
    services: ServiceFactory<S>,
    host_version: String,
    events: Sender<PluginEvent>,
    slots: BTreeMap<String, Slot<S>>,
    /// Worker threads no slot owns any more, still stopping, with their plugin's id; a
    /// plugin added again waits for its own, and shutdown joins the rest.
    stopping: Vec<(String, JoinHandle<()>)>,
}

struct Slot<S: 'static> {
    manifest: PluginManifest,
    component: Arc<[u8]>,
    /// The component with its imports resolved, kept for the next activation after a trap.
    prepared: Prepared<S>,
    /// The current or last worker's status and stop flag.
    shared: Arc<Shared>,
    /// The current or last worker, kept until its thread has finished.
    worker: Option<WorkerHandle<S>>,
}

/// A worker built for a slot, with the ends the slot keeps once its thread runs.
struct NewWorker<S: 'static> {
    worker: Worker<S>,
    jobs: SyncSender<Job<S>>,
    shared: Arc<Shared>,
    /// Hands the new worker its predecessor's handle.
    handover: SyncSender<Option<JoinHandle<()>>>,
}

struct WorkerHandle<S: 'static> {
    jobs: SyncSender<Job<S>>,
    thread: JoinHandle<()>,
}

impl<S: PluginHost> Plugins<S> {
    /// No plugin runs until it is first used; the receiver gets every `PluginEvent`.
    pub fn new(
        runtime: Runtime,
        host_version: impl Into<String>,
        services: impl Fn(&PluginManifest) -> S + Send + Sync + 'static,
    ) -> (Self, Receiver<PluginEvent>) {
        let (events, received) = mpsc::channel();
        let plugins = Self {
            runtime,
            requests: Arc::new(RequestIds::default()),
            services: Arc::new(services),
            host_version: host_version.into(),
            events,
            slots: BTreeMap::new(),
            stopping: Vec::new(),
        };
        (plugins, received)
    }

    /// Add a registered plugin and its component's bytes. Nothing is compiled or run.
    pub fn add(&mut self, plugin: &RegisteredPlugin, component: Arc<[u8]>) -> CallResult<()> {
        let id = plugin.manifest().id().to_owned();
        if self.slots.contains_key(&id) {
            return Err(HostError::Refused(format!("{id} is already added")));
        }
        let slot = Slot {
            manifest: plugin.manifest().clone(),
            component,
            prepared: Arc::new(Mutex::new(None)),
            shared: Arc::new(Shared::new()),
            worker: None,
        };
        self.slots.insert(id, slot);
        Ok(())
    }

    pub fn status(&self, plugin: &str) -> Option<PluginStatus> {
        self.slots.get(plugin).map(|slot| slot.shared.status())
    }

    /// Queue a call for `plugin`, activating it first if it is not running. Returns at once:
    /// a full queue is `unavailable` without waiting, and the answer arrives on `Pending`.
    pub fn call<R: Send + 'static>(
        &mut self,
        plugin: &str,
        call: impl FnOnce(&mut Session<S>) -> CallResult<R> + Send + 'static,
    ) -> CallResult<Pending<R>> {
        self.ensure_running(plugin)?;
        self.send(plugin, call)
    }

    /// The service behind an armed request finished. A plugin that is not running gets
    /// nothing: the instance that armed the request is gone, and a new one never sees it.
    pub fn complete(
        &mut self,
        plugin: &str,
        request: u64,
        outcome: Outcome,
    ) -> CallResult<Pending<()>> {
        if !self.is_running(plugin) {
            return Err(HostError::Unavailable(format!(
                "{plugin} is not running, so request {request} is dropped"
            )));
        }
        self.send(plugin, move |session| session.complete(request, outcome))
    }

    /// Stop `plugin`: `lifecycle.deactivate` runs under its own deadline after the current
    /// call, queued calls are answered `unavailable`, and the worker exits. A later call
    /// starts a new instance only once this worker has finished.
    pub fn deactivate(&mut self, plugin: &str, reason: StopReason) -> CallResult<()> {
        self.slot_mut(plugin)?.stop(reason);
        Ok(())
    }

    /// Deactivate and forget `plugin`.
    pub fn uninstall(&mut self, plugin: &str) -> CallResult<()> {
        let slot = self.slot_mut(plugin)?;
        slot.stop(StopReason::Uninstalled);
        let worker = slot.worker.take();
        self.slots.remove(plugin);
        self.stopping.retain(|(_, thread)| !thread.is_finished());
        self.stopping
            .extend(worker.map(|worker| (plugin.to_owned(), worker.thread)));
        Ok(())
    }

    /// The owner revoked `kind`. A plugin whose manifest requires it is deactivated; an
    /// optional one keeps running and its services answer `denied`.
    pub fn revoke(&mut self, plugin: &str, kind: CapabilityKind) -> CallResult<()> {
        let required = self
            .slot_mut(plugin)?
            .manifest
            .capabilities()
            .iter()
            .any(|request| request.kind == kind && request.required);
        if !required {
            return Ok(());
        }
        self.deactivate(plugin, StopReason::CapabilityRevoked(kind))
    }

    /// Deactivate every plugin and wait for each worker to exit. The workers stop side by
    /// side, each within one job budget (`limits::JOB_BUDGET`), one deferred call's deadline
    /// (the budget is checked between deferred calls) and `lifecycle.deactivate`'s deadline,
    /// as long as its host services honour their deadlines. Dropping `Plugins` does the same.
    pub fn shutdown(mut self) {
        self.stop_all_and_join();
    }

    fn is_running(&self, plugin: &str) -> bool {
        self.slots.get(plugin).is_some_and(Slot::is_running)
    }

    /// Start a worker for `plugin` unless one is running; this is the plugin's activation.
    /// A previous worker still stopping, the slot's own or one an uninstall left behind, is
    /// handed over, and the new one waits for it.
    fn ensure_running(&mut self, plugin: &str) -> CallResult<()> {
        if self.is_running(plugin) {
            return Ok(());
        }
        let NewWorker {
            worker,
            jobs,
            shared,
            handover,
        } = self.new_worker(plugin)?;
        let thread = std::thread::Builder::new()
            .name(format!("syrinx-plugin-{plugin}"))
            .spawn(move || worker.run())
            .map_err(|error| HostError::Unavailable(format!("no worker thread: {error}")))?;
        // Only once the thread runs does it take over the slot: the previous worker's handle,
        // which it joins before activating, and the plugin's status.
        let slot = self.slot_mut(plugin)?;
        let previous = slot.worker.take().map(|old| old.thread);
        slot.shared = shared;
        slot.worker = Some(WorkerHandle { jobs, thread });
        let predecessor = previous.or_else(|| self.take_stopping(plugin));
        if let Err(unsent) = handover.send(predecessor) {
            self.stopping
                .extend(unsent.0.map(|thread| (plugin.to_owned(), thread)));
        }
        Ok(())
    }

    /// The thread of an uninstalled instance of `plugin` that may still be stopping.
    fn take_stopping(&mut self, plugin: &str) -> Option<JoinHandle<()>> {
        let index = self.stopping.iter().position(|(id, _)| id == plugin)?;
        Some(self.stopping.swap_remove(index).1)
    }

    /// Build a worker for `plugin`; nothing in the slot changes yet.
    fn new_worker(&mut self, plugin: &str) -> CallResult<NewWorker<S>> {
        let slot = self.slot_mut(plugin)?;
        let (jobs, queued) = mpsc::sync_channel(limits::PLUGIN_QUEUE_CAPACITY);
        let (handover, handed_over) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared::new());
        shared.set_status(PluginStatus::Active);
        let worker = Worker {
            manifest: slot.manifest.clone(),
            component: Arc::clone(&slot.component),
            prepared: Arc::clone(&slot.prepared),
            runtime: self.runtime.clone(),
            services: Arc::clone(&self.services),
            requests: Arc::clone(&self.requests),
            host_version: self.host_version.clone(),
            shared: Arc::clone(&shared),
            events: self.events.clone(),
            jobs: queued,
            handover: handed_over,
        };
        Ok(NewWorker {
            worker,
            jobs,
            shared,
            handover,
        })
    }

    /// Hand one call to the running worker without waiting.
    fn send<R: Send + 'static>(
        &mut self,
        plugin: &str,
        call: impl FnOnce(&mut Session<S>) -> CallResult<R> + Send + 'static,
    ) -> CallResult<Pending<R>> {
        let slot = self.slot_mut(plugin)?;
        let Some(worker) = slot.worker.as_ref() else {
            return Err(stopped_before_answering());
        };
        let (reply, answer) = mpsc::channel();
        let job = Job::Call(Box::new(move |session: &mut Session<S>| {
            // A caller that dropped its `Pending` no longer wants the answer; the call ran.
            if let Err(unwanted) = reply.send(call(session)) {
                drop(unwanted.0);
            }
        }));
        match worker.jobs.try_send(job) {
            Ok(()) => Ok(Pending(answer)),
            Err(TrySendError::Full(_)) => Err(HostError::Unavailable(format!(
                "{plugin} already has {} calls waiting",
                limits::PLUGIN_QUEUE_CAPACITY
            ))),
            Err(TrySendError::Disconnected(_)) => Err(stopped_before_answering()),
        }
    }

    fn slot_mut(&mut self, plugin: &str) -> CallResult<&mut Slot<S>> {
        self.slots
            .get_mut(plugin)
            .ok_or_else(|| HostError::Refused(format!("{plugin} is not installed")))
    }
}

impl<S: 'static> Slot<S> {
    /// A worker that is serving calls, not one that failed or was told to stop.
    fn is_running(&self) -> bool {
        let Some(worker) = self.worker.as_ref() else {
            return false;
        };
        !worker.thread.is_finished()
            && self.shared.stop_requested().is_none()
            && self.shared.status() == PluginStatus::Active
    }

    /// Tell the worker to stop after its current call; its handle stays until it finishes.
    fn stop(&mut self, reason: StopReason) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        self.shared.request_stop(reason);
        // A full queue cannot take the wake-up and an exited worker needs none: the stop
        // flag is checked before every call either way.
        match worker.jobs.try_send(Job::Stop) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }
}

impl<S: PluginHost> Plugins<S> {
    /// Tell every worker to stop, then wait for each; a second call finds nothing to do.
    fn stop_all_and_join(&mut self) {
        for (id, slot) in &mut self.slots {
            slot.stop(StopReason::Shutdown);
            self.stopping
                .extend(slot.worker.take().map(|worker| (id.clone(), worker.thread)));
        }
        for (_, thread) in self.stopping.drain(..) {
            if thread.join().is_err() {
                eprintln!("a plugin worker thread panicked");
            }
        }
    }
}

impl<S: PluginHost> Drop for Plugins<S> {
    fn drop(&mut self) {
        self.stop_all_and_join();
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shutdown => write!(formatter, "Syrinx is closing"),
            Self::Uninstalled => write!(formatter, "the plugin was uninstalled"),
            Self::CapabilityRevoked(kind) => {
                write!(
                    formatter,
                    "the required {} capability was revoked",
                    kind.name()
                )
            }
        }
    }
}
