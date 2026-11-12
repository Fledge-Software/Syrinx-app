//! The body of one plugin's worker thread: activate on start, run queued calls one at a
//! time, and on a trap or a stop settle every write in flight as `uncertain` and exit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::bindings::syrinx::plugin::types::Outcome;
use crate::manifest::PluginManifest;
use crate::session::{JobLimits, Mediator, RequestIds, Session};
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
    /// The one stop signal: the worker stops before its next call, and the session drops
    /// deferred calls still queued.
    stop: Arc<AtomicBool>,
    /// Why the stop was asked for; set before `stop` is raised.
    stop_reason: Mutex<Option<StopReason>>,
}

impl Shared {
    pub(super) fn new() -> Self {
        Self {
            status: Mutex::new(PluginStatus::Inactive),
            stop: Arc::new(AtomicBool::new(false)),
            stop_reason: Mutex::new(None),
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
        self.stop_reason
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(reason);
        self.stop.store(true, Ordering::Release);
    }

    /// Why a stop was asked for, once it was.
    pub(super) fn stop_requested(&self) -> Option<StopReason> {
        if !self.stop.load(Ordering::Acquire) {
            return None;
        }
        self.stop_reason
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
    /// Hands over the plugin's previous worker, possibly still stopping, once this thread
    /// runs; two instances never run at once.
    pub(super) predecessor: Receiver<Option<JoinHandle<()>>>,
}

impl<S: PluginHost> Worker<S> {
    /// Activate, serve calls until stopped or failed, then drop the store.
    pub(super) fn run(mut self) {
        self.wait_for_predecessor();
        if let Some(reason) = self.shared.stop_requested() {
            // Stopped before it ever ran: nothing was instantiated, so nothing is reported.
            return self.shared.set_status(PluginStatus::Deactivated(reason));
        }
        let mut session = match self.instantiate() {
            Ok(session) => session,
            Err(failure) => return self.fail(None, failure),
        };
        if let Err(failure) = self.activate(&mut session) {
            return self.fail(Some(session), failure);
        }
        while let Some(call) = self.next_job() {
            session.begin_job(self.job_limits());
            call(&mut session);
            if let Some(failure) = session.failure().cloned() {
                return self.fail(Some(session), failure);
            }
        }
        self.deactivate(session);
    }

    /// One job's limits: the stop signal and the job budget from now.
    fn job_limits(&self) -> JobLimits {
        JobLimits {
            stop: Arc::clone(&self.shared.stop),
            until: Instant::now() + limits::JOB_BUDGET,
        }
    }

    /// Wait for the handover, then for the previous worker to finish.
    fn wait_for_predecessor(&mut self) {
        let Ok(Some(predecessor)) = self.predecessor.recv() else {
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

    /// Instantiate the plugin, compiling it only the first time.
    fn instantiate(&self) -> Result<Session<S>, Failure> {
        let prepared = self.prepared()?;
        let services = Mediator::new(
            (self.services)(&self.manifest),
            (&self.manifest).into(),
            Vec::new(),
        );
        let plugin = LoadedPlugin::from_prepared(&self.runtime, &prepared, services)
            .map_err(Failure::from_instantiation)?;
        Ok(Session::new(
            plugin,
            self.host_version.clone(),
            Arc::clone(&self.requests),
        ))
    }

    /// Call `lifecycle.activate`, under the job limits like any call.
    fn activate(&self, session: &mut Session<S>) -> Result<(), Failure> {
        session.begin_job(self.job_limits());
        session.activate()?.map_err(Failure::ActivationRefused)
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
            return self.fail(Some(session), failure);
        }
        self.finish(session);
    }

    /// Every way out of the worker ends here with its instance: the services hear that each
    /// view still open is closed, and the writes still in flight are returned for settling.
    /// The store is dropped with the session.
    fn retire(&self, mut session: Session<S>) -> Vec<u64> {
        session.close_open_views();
        session.writes_in_flight()
    }

    /// Retire the instance, if there was one, settle its writes in flight as `uncertain`,
    /// and show the plugin as failed. Nothing is resubmitted.
    fn fail(&self, session: Option<Session<S>>, failure: Failure) {
        let writes_in_flight = match session {
            Some(session) => self.retire(session),
            None => Vec::new(),
        };
        self.settle_uncertain(&writes_in_flight);
        self.shared
            .set_status(PluginStatus::Failed(failure.clone()));
        self.send(PluginEvent::Failed {
            plugin: self.manifest.id().to_owned(),
            failure,
        });
    }

    /// Retire the instance, settle its writes in flight and report the plugin deactivated.
    /// Nothing is resubmitted.
    fn finish(&self, session: Session<S>) {
        let reason = self.shared.stop_requested().unwrap_or(StopReason::Shutdown);
        let writes_in_flight = self.retire(session);
        self.settle_uncertain(&writes_in_flight);
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
