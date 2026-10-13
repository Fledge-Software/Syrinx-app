//! Keeps one running Syrinx per profile and forwards later launches' links to it (P1-34).

mod names;
mod pipe;
mod protocol;
mod security;
#[cfg(test)]
mod tests;

use std::{
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use names::InstanceNames;
use pipe::{Delivery, PipeServer};
use security::{NamedMutex, OwnerOnly};

pub use pipe::StopSignal;
pub use protocol::{CheckedAddress, LinkRequest};

/// How long a later launch keeps trying while the running Syrinx starts up or exits.
pub const FORWARD_PATIENCE: Duration = Duration::from_secs(15);

/// The pause between attempts, short enough that a link opens promptly once Syrinx listens.
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// How long a new running Syrinx waits for its predecessor to give up the pipe name, which
/// happens just after the predecessor releases the mutex.
const PIPE_HANDOVER: Duration = Duration::from_secs(3);

/// What this launch became.
pub enum Claim {
    /// This process owns the profile and listens for later launches.
    Primary(PrimaryInstance),
    /// The running Syrinx accepted the request, so this process should exit.
    Forwarded {
        /// Whether this launch could let the running Syrinx come to the foreground.
        foreground_granted: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceError {
    /// The profile directory has no absolute spelling.
    Directory,
    /// The signed-in Windows user could not be identified.
    Identity,
    /// The owner-only security descriptor could not be built.
    Security,
    /// The profile's instance mutex could not be created or checked.
    Mutex,
    /// Another process already holds this profile's link pipe.
    PipeTaken,
    /// The process holding the link pipe runs as another Windows user.
    ForeignListener,
    /// The link pipe failed in a way a retry would not fix.
    Pipe,
    /// The running Syrinx refused the link.
    Refused,
    /// The running Syrinx neither answered nor exited within the patience allowed.
    NoResponse,
    /// The listener thread could not start.
    ThreadStart,
    /// The link queue was already handed out or has closed.
    Queue,
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "Syrinx cannot resolve its profile directory.",
            Self::Identity => "Syrinx cannot identify the signed-in Windows user.",
            Self::Security => "Syrinx cannot restrict its link channel to this user.",
            Self::Mutex => "Syrinx cannot check whether it is already running.",
            Self::PipeTaken => {
                "Another program holds Syrinx's link channel, so Syrinx cannot start. \
                 Sign out of Windows and back in, then try again."
            }
            Self::ForeignListener => {
                "Syrinx's link channel belongs to another Windows user, so the link was not sent."
            }
            Self::Pipe => "Syrinx's link channel failed.",
            Self::Refused => "The running Syrinx refused the link.",
            Self::NoResponse => {
                "Syrinx is running but did not answer in time, so the link was not opened."
            }
            Self::ThreadStart => "Syrinx cannot start its link listener.",
            Self::Queue => "Syrinx's link queue is unavailable.",
        })
    }
}

impl std::error::Error for InstanceError {}

/// Owns the profile until dropped. Drop it on the thread that called [`claim`]: Windows ties a
/// mutex to its owning thread, and a mutex left by another thread is only taken over later.
pub struct PrimaryInstance {
    server: PipeServer,
    sender: UnboundedSender<LinkRequest>,
    requests: Option<UnboundedReceiver<LinkRequest>>,
    dropped: DroppedLinks,
    lock: NamedMutex,
}

/// The forwarded requests for the window, the switch that stops accepting more, and where
/// the window records a link that arrived too late to open.
pub struct LinkFeed {
    pub requests: UnboundedReceiver<LinkRequest>,
    pub stop: StopSignal,
    pub dropped: DroppedLinks,
}

/// Set when an accepted link reached a window that was already closing.
#[derive(Clone, Default)]
pub struct DroppedLinks(Arc<AtomicBool>);

impl DroppedLinks {
    pub fn record(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn any(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl PrimaryInstance {
    /// Hands the window its request queue once, starting with this launch's own link.
    pub fn take_feed(&mut self, own: Option<LinkRequest>) -> Result<LinkFeed, InstanceError> {
        let requests = self.requests.take().ok_or(InstanceError::Queue)?;
        if let Some(request) = own {
            self.sender
                .unbounded_send(request)
                .map_err(|_| InstanceError::Queue)?;
        }
        Ok(LinkFeed {
            requests,
            stop: self.server.stop_signal(),
            dropped: self.dropped.clone(),
        })
    }

    /// Whether a forwarded link was accepted but never opened because Syrinx was closing.
    pub fn links_dropped(&self) -> bool {
        self.dropped.any()
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        // Keep the pipe name until the mutex is released, so no other process can take it
        // between the two; the successor waits briefly for the name (`PIPE_HANDOVER`).
        self.server.stop_and_join();
        self.lock.release();
        self.server.close();
    }
}

/// Becomes the profile's running Syrinx, or forwards `request` to the one already running.
/// Retries for `patience` while that Syrinx is still starting up or shutting down.
pub fn claim(
    directory: &Path,
    request: &LinkRequest,
    patience: Duration,
) -> Result<Claim, InstanceError> {
    let owner = OwnerOnly::current_user()?;
    let names = InstanceNames::new(directory, owner.sid(), owner.session())?;
    let mut lock = NamedMutex::open(&names.mutex, &owner)?;
    let message = request.encode();
    let deadline = Instant::now() + patience;
    loop {
        if lock.try_acquire()? {
            return become_primary(lock, &names, &owner);
        }
        match pipe::send(&names.pipe, &message, &owner)? {
            Delivery::Accepted { foreground_granted } => {
                return Ok(Claim::Forwarded { foreground_granted });
            }
            Delivery::Refused => return Err(InstanceError::Refused),
            Delivery::Foreign => return Err(InstanceError::ForeignListener),
            Delivery::Unavailable => {}
        }
        if Instant::now() >= deadline {
            return Err(InstanceError::NoResponse);
        }
        thread::sleep(RETRY_INTERVAL);
    }
}

/// Starts listening while holding the profile's mutex; a failure releases the mutex again.
fn become_primary(
    lock: NamedMutex,
    names: &InstanceNames,
    owner: &OwnerOnly,
) -> Result<Claim, InstanceError> {
    let (sender, requests) = unbounded();
    let server = start_listening(&names.pipe, owner, &sender)?;
    Ok(Claim::Primary(PrimaryInstance {
        server,
        sender,
        requests: Some(requests),
        dropped: DroppedLinks::default(),
        lock,
    }))
}

/// Creates the pipe, waiting briefly while a predecessor that just exited still holds it.
fn start_listening(
    pipe_name: &str,
    owner: &OwnerOnly,
    sender: &UnboundedSender<LinkRequest>,
) -> Result<PipeServer, InstanceError> {
    let deadline = Instant::now() + PIPE_HANDOVER;
    loop {
        match PipeServer::start(pipe_name, owner, sender.clone()) {
            Err(InstanceError::PipeTaken) if Instant::now() < deadline => {
                thread::sleep(RETRY_INTERVAL);
            }
            started => return started,
        }
    }
}
