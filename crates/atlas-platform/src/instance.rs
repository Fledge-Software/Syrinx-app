//! Keeps one running Atlas per profile and forwards later launches' links to it (P1-34).

mod names;
mod pipe;
mod protocol;
mod security;
#[cfg(test)]
mod tests;

use std::{
    fmt,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use names::InstanceNames;
use pipe::{Delivery, PipeServer};
use security::{NamedMutex, OwnerOnly};

pub use pipe::StopSignal;
pub use protocol::{CheckedAddress, LinkRequest};

/// How long a later launch keeps trying while the running Atlas starts up or exits.
pub const FORWARD_PATIENCE: Duration = Duration::from_secs(15);

/// The pause between attempts, short enough that a link opens promptly once Atlas listens.
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// What this launch became.
pub enum Claim {
    /// This process owns the profile and listens for later launches.
    Primary(PrimaryInstance),
    /// The running Atlas accepted the request, so this process should exit.
    Forwarded {
        /// Whether this launch could let the running Atlas come to the foreground.
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
    /// The link pipe failed in a way a retry would not fix.
    Pipe,
    /// The running Atlas refused the link.
    Refused,
    /// The running Atlas neither answered nor exited within the patience allowed.
    NoResponse,
    /// The listener thread could not start.
    ThreadStart,
    /// The link queue was already handed out or has closed.
    Queue,
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "Atlas cannot resolve its profile directory.",
            Self::Identity => "Atlas cannot identify the signed-in Windows user.",
            Self::Security => "Atlas cannot restrict its link channel to this user.",
            Self::Mutex => "Atlas cannot check whether it is already running.",
            Self::PipeTaken => "Another program already holds Atlas's link channel.",
            Self::Pipe => "Atlas's link channel failed.",
            Self::Refused => "The running Atlas refused the link.",
            Self::NoResponse => "The running Atlas did not answer in time.",
            Self::ThreadStart => "Atlas cannot start its link listener.",
            Self::Queue => "Atlas's link queue is unavailable.",
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
    lock: NamedMutex,
}

/// The forwarded requests for the window, and the switch that stops accepting more.
pub struct LinkFeed {
    pub requests: UnboundedReceiver<LinkRequest>,
    pub stop: StopSignal,
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
        })
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        // Close the pipe before the mutex, so a waiting launch takes over only a quiet profile.
        self.server.stop_and_join();
        self.lock.release();
    }
}

/// Becomes the profile's running Atlas, or forwards `request` to the one already running.
/// Retries for `patience` while that Atlas is still starting up or shutting down.
pub fn claim(
    directory: &Path,
    request: &LinkRequest,
    patience: Duration,
) -> Result<Claim, InstanceError> {
    let owner = OwnerOnly::current_user()?;
    let names = InstanceNames::new(directory, owner.sid())?;
    let mut lock = NamedMutex::open(&names.mutex, &owner)?;
    let message = request.encode();
    let deadline = Instant::now() + patience;
    loop {
        if lock.try_acquire()? {
            return become_primary(lock, &names, &owner);
        }
        match pipe::send(&names.pipe, &message)? {
            Delivery::Accepted { foreground_granted } => {
                return Ok(Claim::Forwarded { foreground_granted });
            }
            Delivery::Refused => return Err(InstanceError::Refused),
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
    let server = PipeServer::start(&names.pipe, owner, sender.clone())?;
    Ok(Claim::Primary(PrimaryInstance {
        server,
        sender,
        requests: Some(requests),
        lock,
    }))
}
