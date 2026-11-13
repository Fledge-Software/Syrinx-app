//! Runs the plugin services' store reads and writes on one thread of their own. A service
//! waits only its call's budget, so a busy database answers `unavailable` instead of holding
//! a plugin past the deadline that an epoch cannot interrupt inside a host function.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use syrinx_store::{Store, StoreError};

/// Store work queued at once, across every plugin; more is refused rather than waited for.
const QUEUE: usize = 16;

type Work = Box<dyn FnOnce(&Store) + Send>;

/// Why store work produced no answer for the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordsError {
    /// The queue was full or the answer did not come within the budget. Work already
    /// queued still runs, so a write may land later.
    Busy,
    /// The records thread has stopped.
    Stopped,
    Store(StoreError),
}

/// The thread, owned by the runtime and joined at shutdown.
pub(super) struct PluginRecords {
    sender: Option<SyncSender<Work>>,
    thread: Option<JoinHandle<()>>,
}

/// What each plugin's services hold to reach the records thread.
#[derive(Clone)]
pub struct RecordsHandle(SyncSender<Work>);

impl PluginRecords {
    pub(super) fn start(store: Arc<Store>) -> std::io::Result<Self> {
        let (sender, work) = mpsc::sync_channel::<Work>(QUEUE);
        let thread = std::thread::Builder::new()
            .name("syrinx-plugin-records".into())
            .spawn(move || {
                for job in work {
                    job(&store);
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
        })
    }

    pub(super) fn handle(&self) -> Option<RecordsHandle> {
        self.sender.clone().map(RecordsHandle)
    }

    /// Stop taking work and wait for queued work to finish. Every handle must be gone first,
    /// which is so once the plugins and their services have shut down.
    pub(super) fn stop(&mut self) {
        self.sender.take();
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            eprintln!("the plugin records thread panicked");
        }
    }
}

impl Drop for PluginRecords {
    fn drop(&mut self) {
        self.stop();
    }
}

impl RecordsHandle {
    /// Run `work` against the store and wait at most `budget` for its answer.
    pub fn run<T: Send + 'static>(
        &self,
        budget: Duration,
        work: impl FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, RecordsError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let job: Work = Box::new(move |store| {
            // The service may have stopped waiting; then nobody needs the answer.
            if let Err(unwanted) = reply.send(work(store)) {
                drop(unwanted.0);
            }
        });
        match self.0.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(RecordsError::Busy),
            Err(TrySendError::Disconnected(_)) => return Err(RecordsError::Stopped),
        }
        match answer.recv_timeout(budget) {
            Ok(result) => result.map_err(RecordsError::Store),
            Err(RecvTimeoutError::Timeout) => Err(RecordsError::Busy),
            Err(RecvTimeoutError::Disconnected) => Err(RecordsError::Stopped),
        }
    }
}
