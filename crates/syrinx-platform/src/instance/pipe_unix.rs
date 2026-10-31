//! Carries one forwarded request per connection over a same-user Unix socket.

use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::PermissionsExt,
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use futures::channel::mpsc::UnboundedSender;

use super::{
    InstanceError, LinkRequest,
    protocol::{ACCEPTED, CLOSING, MAX_MESSAGE_BYTES, REFUSED},
    security::OwnerOnly,
};

/// How long either side waits for the other to read or write one message.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// How often the listener looks at its stop signal between connections.
const STOP_POLL: Duration = Duration::from_millis(100);

/// How the running Syrinx answered a launch.
pub(super) enum Delivery {
    Accepted {
        foreground_granted: bool,
    },
    Refused,
    /// The socket belongs to a process of another user; nothing was sent to it.
    Foreign,
    /// Nobody listens, the listener is closing, or it hung up: try again shortly.
    Unavailable,
}

/// Stops the listener; the window raises it as soon as it starts closing.
#[derive(Clone, Default)]
pub struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    pub fn raise(&self) -> Result<(), InstanceError> {
        self.0.store(true, Ordering::Release);
        Ok(())
    }

    fn raised(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub(super) struct PipeServer {
    path: String,
    stop: StopSignal,
    worker: Option<JoinHandle<()>>,
}

impl PipeServer {
    /// Binds the socket and answers launches on a thread until stopped. The caller holds the
    /// profile lock, so a socket file left by a crashed predecessor is safe to replace.
    pub(super) fn start(
        name: &str,
        owner: &OwnerOnly,
        requests: UnboundedSender<LinkRequest>,
    ) -> Result<Self, InstanceError> {
        let listener = bind(name)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| InstanceError::Pipe)?;
        let stop = StopSignal::default();
        let worker_stop = stop.clone();
        let uid = owner.uid();
        let worker = thread::Builder::new()
            .name("syrinx-links".into())
            .spawn(move || serve(&listener, uid, &worker_stop, &requests))
            .map_err(|_| InstanceError::ThreadStart)?;
        Ok(Self {
            path: name.to_owned(),
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn stop_signal(&self) -> StopSignal {
        self.stop.clone()
    }

    pub(super) fn stop_and_join(&mut self) {
        // Raising never fails here; the listener sees it within one poll interval.
        if self.stop.raise().is_err() {
            return;
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            eprintln!("Syrinx's link listener stopped unexpectedly.");
        }
    }

    pub(super) fn close(&mut self) {
        self.stop_and_join();
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("Syrinx could not remove its link socket: {error}"),
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.close();
    }
}

/// Binds the link socket, refusing one a live process still answers on.
fn bind(name: &str) -> Result<UnixListener, InstanceError> {
    match UnixListener::bind(name) {
        Ok(listener) => return restrict(name).map(|()| listener),
        Err(error) if error.kind() != io::ErrorKind::AddrInUse => {
            return Err(InstanceError::Pipe);
        }
        Err(_) => {}
    }
    if UnixStream::connect(name).is_ok() {
        return Err(InstanceError::PipeTaken);
    }
    std::fs::remove_file(name).map_err(|_| InstanceError::Pipe)?;
    let listener = UnixListener::bind(name).map_err(|_| InstanceError::PipeTaken)?;
    restrict(name)?;
    Ok(listener)
}

fn restrict(name: &str) -> Result<(), InstanceError> {
    std::fs::set_permissions(Path::new(name), std::fs::Permissions::from_mode(0o600))
        .map_err(|_| InstanceError::Security)
}

/// Answers one launch at a time until the stop signal is raised.
fn serve(
    listener: &UnixListener,
    uid: libc::uid_t,
    stop: &StopSignal,
    requests: &UnboundedSender<LinkRequest>,
) {
    while !stop.raised() {
        match listener.accept() {
            Ok((stream, _)) => converse(stream, uid, stop, requests),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(STOP_POLL),
            Err(error) => {
                eprintln!("Syrinx's link listener stopped: {error}");
                return;
            }
        }
    }
}

/// Reads one request from a same-user client and answers it with one byte.
fn converse(
    mut stream: UnixStream,
    uid: libc::uid_t,
    stop: &StopSignal,
    requests: &UnboundedSender<LinkRequest>,
) {
    if peer_uid(&stream) != Some(uid) || prepare(&stream).is_err() {
        return;
    }
    let reply = match read_message(&mut stream, stop) {
        Ok(Some(message)) => decide(&message, stop, requests),
        // Closing never waits for a slow client; it is told Syrinx is closing.
        Ok(None) => CLOSING,
        // An overlong, broken or silent request is refused rather than left unanswered.
        Err(_) => REFUSED,
    };
    if let Err(error) = stream.write_all(&[reply]) {
        eprintln!("Syrinx could not answer a forwarded launch: {error}");
    }
}

/// The reply to one request; only a well-formed http(s) link or an activation is queued.
fn decide(message: &[u8], stop: &StopSignal, requests: &UnboundedSender<LinkRequest>) -> u8 {
    let Ok(request) = LinkRequest::decode(message) else {
        return REFUSED;
    };
    if stop.raised() {
        return CLOSING;
    }
    match requests.unbounded_send(request) {
        Ok(()) => ACCEPTED,
        Err(_) => CLOSING,
    }
}

/// Sends one request to the running Syrinx and reads its one-byte answer, after checking that
/// the socket's listener runs as this user.
pub(super) fn send(
    name: &str,
    message: &[u8],
    owner: &OwnerOnly,
) -> Result<Delivery, InstanceError> {
    let Ok(mut stream) = UnixStream::connect(name) else {
        return Ok(Delivery::Unavailable);
    };
    if peer_uid(&stream) != Some(owner.uid()) {
        return Ok(Delivery::Foreign);
    }
    if prepare(&stream).is_err()
        || stream.write_all(message).is_err()
        || stream.shutdown(Shutdown::Write).is_err()
    {
        return Ok(Delivery::Unavailable);
    }
    let mut reply = [0u8; 1];
    Ok(match (stream.read(&mut reply), reply[0]) {
        // X11 lets the running Syrinx raise its own window, so the grant always holds.
        (Ok(1), ACCEPTED) => Delivery::Accepted {
            foreground_granted: true,
        },
        (Ok(1), REFUSED) => Delivery::Refused,
        _ => Delivery::Unavailable,
    })
}

fn prepare(stream: &UnixStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))
}

/// Reads until the client half-closes, refusing anything longer than one message. Waits in
/// short slices so a raised stop signal ends the read at once (`None`).
fn read_message(stream: &mut UnixStream, stop: &StopSignal) -> io::Result<Option<Vec<u8>>> {
    stream.set_read_timeout(Some(STOP_POLL))?;
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut message = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if stop.raised() {
            return Ok(None);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(Some(message)),
            Ok(read) => message.extend_from_slice(&chunk[..read]),
            Err(error) if is_timeout(&error) && Instant::now() < deadline => continue,
            Err(error) => return Err(error),
        }
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<libc::uid_t> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &mut length,
        )
    };
    (status == 0).then_some(credentials.uid)
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<libc::uid_t> {
    let mut uid = 0;
    let mut gid = 0;
    let status = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (status == 0).then_some(uid)
}
