//! The body of one plugin's worker thread: activate on start, run queued calls one at a
//! time, and on a trap or a stop settle every write in flight as `uncertain` and exit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::bindings::syrinx::plugin::types::Outcome;
use crate::manifest::PluginManifest;
use crate::session::{Mediator, RequestIds, Session};
use crate::{Failure, LoadedPlugin, PluginHost, PreparedPlugin, Runtime, limits};

use super::{PluginEvent, PluginStatus, ServiceFactory, StopReason};

/// The outcome the host reports for a write whose plugin stopped before it was known.
const STOPPED_WRITE: &str = "the plugin stopped before the outcome was known";

/// One queued call, run on the worker against the plugin's session.
pub(super) type Call<S> = Box<dyn FnOnce(&mut Session<S>) + Send>;

/// One call for the worker, or the signal to stop.
pub(super) enum Job<S: 'static> {
    Call(Call<S>),
    Stop,
}

/// A plugin's prepared component, shared by its successive workers.
pub(super) type Prepared<S> = Arc<Mutex<Option<PreparedPlugin<Mediator<S>>>>>;

/// What one worker shares with the supervisor that started it.
pub(super) struct Shared {
    status: Mutex<PluginStatus>,
    /// Set by the supervisor; the worker stops before its next call.
    stop: Mutex<Option<StopReason>>,
    /// Raised with `stop`, and read by the session between deferred calls.
    stopping: Arc<AtomicBool>,
}

impl Shared {
    pub(super) fn new() -> Self {
        Self {
            status: Mutex::new(PluginStatus::Inactive),
            stop: Mutex::new(None),
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn status(&self) -> PluginStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn set_status(&self, status: PluginStatus) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }

    pub(super) fn request_stop(&self, reason: StopReason) {
        self.stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(reason);
        self.stopping.store(true, Ordering::Relaxed);
    }

    pub(super) fn stop_requested(&self) -> Option<StopReason> {
        self.stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Everything one worker thread owns.
pub(super) struct Worker<S: 'static> {
    pub(super) manifest: PluginManifest,
    pub(super) component: Arc<[u8]>,
    pub(super) prepared: Prepared<S>,
    pub(super) runtime: Runtime,
    pub(super) services: ServiceFactory<S>,
    pub(super) requests: Arc<RequestIds>,
    pub(super) host_version: String,
    pub(super) shared: Arc<Shared>,
    pub(super) events: Sender<PluginEvent>,
    pub(super) jobs: Receiver<Job<S>>,
    /// The plugin's previous worker, still stopping; two instances never run at once.
    pub(super) predecessor: Option<JoinHandle<()>>,
}

impl<S: PluginHost> Worker<S> {
    /// Activate, serve calls until stopped or failed, then drop the store.
    pub(super) fn run(mut self) {
        self.wait_for_predecessor();
        let mut session = match self.activate() {
            Ok(session) => session,
            Err(failure) => return self.stopped(&[], failure),
        };
        while let Some(call) = self.next_job() {
            session.begin_job(Instant::now() + limits::JOB_BUDGET);
            call(&mut session);
            if let Some(failure) = session.failure().cloned() {
                return self.stopped(&session.writes_in_flight(), failure);
            }
        }
        self.deactivate(session);
    }

    fn wait_for_predecessor(&mut self) {
        let Some(predecessor) = self.predecessor.take() else {
            return;
        };
        if predecessor.join().is_err() {
            eprintln!("a plugin worker thread panicked");
        }
    }

    /// The next call to run, or `None` once a stop was asked for or the supervisor is gone.
    fn next_job(&self) -> Option<Call<S>> {
        if self.shared.stop_requested().is_some() {
            return None;
        }
        match self.jobs.recv() {
            Ok(Job::Call(call)) if self.shared.stop_requested().is_none() => Some(call),
            Ok(_) | Err(_) => None,
        }
    }

    /// Instantiate (compiling only the first time) and call `lifecycle.activate`.
    fn activate(&self) -> Result<Session<S>, Failure> {
        let prepared = self.prepared()?;
        let services = Mediator::new(
            (self.services)(&self.manifest),
            (&self.manifest).into(),
            Vec::new(),
        );
        let plugin = LoadedPlugin::from_prepared(&self.runtime, &prepared, services)
            .map_err(Failure::from_instantiation)?;
        let mut session = Session::new(
            plugin,
            self.host_version.clone(),
            Arc::clone(&self.requests),
        );
        session.watch_stop(Arc::clone(&self.shared.stopping));
        session.begin_job(Instant::now() + limits::JOB_BUDGET);
        match session.activate()? {
            Ok(()) => Ok(session),
            Err(refused) => Err(Failure::ActivationRefused(refused)),
        }
    }

    /// The prepared component, compiling and linking it on the plugin's first activation.
    fn prepared(&self) -> Result<PreparedPlugin<Mediator<S>>, Failure> {
        let mut cached = self.prepared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(prepared) = cached.as_ref() {
            return Ok(prepared.clone());
        }
        let component = self
            .runtime
            .compile_manifest(&self.manifest, &self.component)
            .map_err(Failure::Load)?;
        let prepared = LoadedPlugin::<Mediator<S>>::prepare(&self.runtime, &component)
            .map_err(Failure::Load)?;
        Ok(cached.insert(prepared).clone())
    }

    /// Best effort `lifecycle.deactivate` under its own deadline, then exit.
    fn deactivate(&self, mut session: Session<S>) {
        if let Err(failure) = session.deactivate() {
            return self.stopped(&session.writes_in_flight(), failure);
        }
        self.finish(&session.writes_in_flight());
    }

    /// The instance stopped. One stopped between deferred calls because the host was
    /// deactivating it ends as deactivated; any other failure shows the plugin as failed.
    fn stopped(&self, writes_in_flight: &[u64], failure: Failure) {
        if failure == Failure::Interrupted && self.shared.stop_requested().is_some() {
            return self.finish(writes_in_flight);
        }
        self.settle_uncertain(writes_in_flight);
        self.shared
            .set_status(PluginStatus::Failed(failure.clone()));
        self.send(PluginEvent::Failed {
            plugin: self.manifest.id().to_owned(),
            failure,
        });
    }

    /// Settle the writes in flight and report the plugin deactivated; the store is dropped
    /// with the session. Nothing is resubmitted.
    fn finish(&self, writes_in_flight: &[u64]) {
        let reason = self.shared.stop_requested().unwrap_or(StopReason::Shutdown);
        self.settle_uncertain(writes_in_flight);
        self.shared
            .set_status(PluginStatus::Deactivated(reason.clone()));
        self.send(PluginEvent::Deactivated {
            plugin: self.manifest.id().to_owned(),
            reason,
        });
    }

    fn settle_uncertain(&self, writes_in_flight: &[u64]) {
        for &request in writes_in_flight {
            self.send(PluginEvent::Outcome {
                plugin: self.manifest.id().to_owned(),
                request,
                outcome: Outcome::Uncertain(STOPPED_WRITE.into()),
            });
        }
    }

    /// Tell the application; once it stopped listening there is no one left to tell.
    fn send(&self, event: PluginEvent) {
        if self.events.send(event).is_err() {
            self.shared.request_stop(StopReason::Shutdown);
        }
    }
}
