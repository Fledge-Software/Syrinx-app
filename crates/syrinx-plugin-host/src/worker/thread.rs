//! The body of one plugin's worker thread: activate on start, run queued calls one at a
//! time, and on a trap or a stop settle every write in flight as `uncertain` and exit.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use crate::bindings::syrinx::plugin::types::Outcome;
use crate::manifest::PluginManifest;
use crate::session::{Mediator, RequestIds, Session};
use crate::{Failure, LoadedPlugin, PluginHost, PreparedPlugin, Runtime};

use super::{PluginEvent, PluginStatus, ServiceFactory, StopReason};

/// The outcome the host reports for a write whose plugin stopped before it was known.
pub(super) const STOPPED_WRITE: &str = "the plugin stopped before the outcome was known";

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
}

impl Shared {
    pub(super) fn new() -> Self {
        Self {
            status: Mutex::new(PluginStatus::Inactive),
            stop: Mutex::new(None),
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
    }

    fn stop_requested(&self) -> Option<StopReason> {
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
}

impl<S: PluginHost> Worker<S> {
    /// Activate, serve calls until stopped or failed, then drop the store.
    pub(super) fn run(self) {
        let mut session = match self.activate() {
            Ok(session) => session,
            Err(failure) => return self.fail(&[], failure),
        };
        while let Some(job) = self.next_job() {
            job(&mut session);
            if let Some(failure) = session.failure().cloned() {
                return self.fail(&session.writes_in_flight(), failure);
            }
        }
        let reason = self.shared.stop_requested().unwrap_or(StopReason::Shutdown);
        self.deactivate(session, reason);
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
        let services = Mediator::new((self.services)(&self.manifest), Vec::new());
        let plugin = LoadedPlugin::from_prepared(&self.runtime, &prepared, services)
            .map_err(Failure::Load)?;
        let mut session = Session::new(
            plugin,
            (&self.manifest).into(),
            self.host_version.clone(),
            Arc::clone(&self.requests),
        );
        match session.activate() {
            Ok(Ok(())) => Ok(session),
            Ok(Err(refused)) => Err(Failure::ActivationRefused(refused)),
            Err(error) => Err(session.failure().cloned().unwrap_or(Failure::Load(error))),
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

    /// Settle the writes in flight as `uncertain`, show the plugin as failed, and exit; the
    /// store is dropped with the session. Nothing is resubmitted.
    fn fail(&self, writes_in_flight: &[u64], failure: Failure) {
        self.settle_uncertain(writes_in_flight);
        self.shared
            .set_status(PluginStatus::Failed(failure.clone()));
        self.send(PluginEvent::Failed {
            plugin: self.manifest.id().to_owned(),
            failure,
        });
    }

    /// Best effort `lifecycle.deactivate` under its own deadline, then exit.
    fn deactivate(&self, mut session: Session<S>, reason: StopReason) {
        if session.deactivate().is_err() {
            let failure = session.failure().cloned().unwrap_or(Failure::Flooded);
            return self.fail(&session.writes_in_flight(), failure);
        }
        self.settle_uncertain(&session.writes_in_flight());
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
