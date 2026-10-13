//! Carries one forwarded request per connection over a same-user, local-only named pipe.

use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use futures::channel::mpsc::UnboundedSender;
use windows::{
    Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NOT_FOUND,
            ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
            HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
            OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
            SECURITY_SQOS_PRESENT, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_MESSAGE, PIPE_WAIT, SetNamedPipeHandleState,
            },
            Threading::{
                CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
                WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::AllowSetForegroundWindow,
    },
    core::HSTRING,
};

use super::{
    InstanceError, LinkRequest,
    protocol::{ACCEPTED, CLOSING, MAX_MESSAGE_BYTES, REFUSED},
    security::{OwnedHandle, OwnerOnly},
};

/// How long either side waits for the other to read or write one message.
const IO_TIMEOUT_MS: u32 = 2_000;

/// How the running Syrinx answered a launch.
pub(super) enum Delivery {
    Accepted {
        foreground_granted: bool,
    },
    Refused,
    /// The pipe belongs to a process of another user; nothing was sent to it.
    Foreign,
    /// Nobody listens, the listener is busy or closing, or it hung up: try again shortly.
    Unavailable,
}

/// Stops the listener; the window raises it as soon as it starts closing.
#[derive(Clone)]
pub struct StopSignal(Arc<OwnedHandle>);

impl StopSignal {
    fn new() -> Result<Self, InstanceError> {
        let event = unsafe { CreateEventW(None, true, false, None) }
            .map_err(|_| InstanceError::ThreadStart)?;
        Ok(Self(Arc::new(OwnedHandle(event))))
    }

    /// Stops accepting forwarded links; later launches wait until this Syrinx has exited.
    pub fn raise(&self) -> Result<(), InstanceError> {
        unsafe { SetEvent(self.handle()) }.map_err(|_| InstanceError::Pipe)
    }

    fn raised(&self) -> bool {
        let waited = unsafe { WaitForSingleObject(self.handle(), 0) };
        waited == WAIT_OBJECT_0
    }

    fn handle(&self) -> HANDLE {
        self.0.0
    }
}

/// The listener thread and the switch that ends it.
pub(super) struct PipeServer {
    stop: StopSignal,
    worker: Option<JoinHandle<OwnedHandle>>,
    /// The pipe after the listener stops, held so no other process can take the name yet.
    retained: Option<OwnedHandle>,
}

impl PipeServer {
    /// Creates the pipe here, so a name another process already holds fails this launch.
    pub(super) fn start(
        name: &str,
        owner: &OwnerOnly,
        requests: UnboundedSender<LinkRequest>,
    ) -> Result<Self, InstanceError> {
        let pipe = create_pipe(name, owner)?;
        let stop = StopSignal::new()?;
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("syrinx-links".into())
            .spawn(move || serve(pipe, &worker_stop, &requests))
            .map_err(|_| InstanceError::ThreadStart)?;
        Ok(Self {
            stop,
            worker: Some(worker),
            retained: None,
        })
    }

    pub(super) fn stop_signal(&self) -> StopSignal {
        self.stop.clone()
    }

    /// Stops the listener and waits for it, keeping the pipe name reserved.
    pub(super) fn stop_and_join(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        // Without the stop event the thread would wait for a client forever; leave it detached.
        if let Err(error) = self.stop.raise() {
            eprintln!("{error}");
            return;
        }
        match worker.join() {
            Ok(pipe) => self.retained = Some(pipe),
            Err(_) => eprintln!("Syrinx's link listener stopped unexpectedly."),
        }
    }

    /// Stops the listener and gives up the pipe name.
    pub(super) fn close(&mut self) {
        self.stop_and_join();
        self.retained = None;
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.close();
    }
}

fn create_pipe(name: &str, owner: &OwnerOnly) -> Result<OwnedHandle, InstanceError> {
    let attributes = owner.attributes();
    let buffer = MAX_MESSAGE_BYTES as u32;
    // First-instance-only refuses a pipe someone else created; remote clients are refused too.
    let pipe = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(name),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            buffer,
            buffer,
            0,
            Some(&attributes),
        )
    };
    if !pipe.is_invalid() {
        return Ok(OwnedHandle(pipe));
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_ACCESS_DENIED || error == ERROR_PIPE_BUSY {
        return Err(InstanceError::PipeTaken);
    }
    Err(InstanceError::Pipe)
}

/// Answers one launch at a time until the stop signal is raised, then hands the pipe back.
fn serve(
    pipe: OwnedHandle,
    stop: &StopSignal,
    requests: &UnboundedSender<LinkRequest>,
) -> OwnedHandle {
    let mut operation = match Operation::new() {
        Ok(operation) => operation,
        Err(error) => {
            eprintln!("{error}");
            return pipe;
        }
    };
    let mut buffer = vec![0u8; MAX_MESSAGE_BYTES];
    loop {
        match operation.connect(pipe.0, stop) {
            Outcome::Done(_) => converse(&mut operation, pipe.0, &mut buffer, stop, requests),
            Outcome::Stopped => return pipe,
            // A client that vanished while connecting only leaves the instance to reset.
            Outcome::TimedOut | Outcome::Failed => {}
        }
        if let Err(error) = unsafe { DisconnectNamedPipe(pipe.0) } {
            eprintln!("Syrinx's link listener stopped: {error}");
            return pipe;
        }
        if stop.raised() {
            return pipe;
        }
    }
}

/// Reads one request, answers it and waits for the client to hang up.
fn converse(
    operation: &mut Operation,
    pipe: HANDLE,
    buffer: &mut [u8],
    stop: &StopSignal,
    requests: &UnboundedSender<LinkRequest>,
) {
    // Every step also ends on the stop event, so closing never waits for a slow client.
    let reply = match operation.read(pipe, buffer, Some(stop)) {
        Outcome::Done(length) => decide(&buffer[..length as usize], stop, requests),
        Outcome::Stopped => CLOSING,
        // An overlong, broken or silent request is refused rather than left unanswered.
        Outcome::Failed | Outcome::TimedOut => REFUSED,
    };
    if !matches!(
        operation.write(pipe, &[reply], Some(stop)),
        Outcome::Done(1)
    ) {
        return;
    }
    // Disconnecting discards an unread reply, so wait for the client to read it and close.
    let _client_hung_up = operation.read(pipe, buffer, Some(stop));
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

/// Sends one request to the running Syrinx and reads its one-byte answer. The pipe's name is
/// predictable, so its server must run as this user before anything is sent or granted.
pub(super) fn send(
    name: &str,
    message: &[u8],
    owner: &OwnerOnly,
) -> Result<Delivery, InstanceError> {
    let Some(pipe) = open_client(name)? else {
        return Ok(Delivery::Unavailable);
    };
    let Some(server) = server_process(pipe.0) else {
        return Ok(Delivery::Unavailable);
    };
    if !owner.owns_process(server) {
        return Ok(Delivery::Foreign);
    }
    let foreground_granted = grant_foreground(server);
    let mut operation = Operation::new()?;
    let written = operation.write(pipe.0, message, None);
    if !matches!(written, Outcome::Done(length) if length as usize == message.len()) {
        return Ok(Delivery::Unavailable);
    }
    let mut reply = [0u8; 1];
    Ok(match (operation.read(pipe.0, &mut reply, None), reply[0]) {
        (Outcome::Done(1), ACCEPTED) => Delivery::Accepted { foreground_granted },
        (Outcome::Done(1), REFUSED) => Delivery::Refused,
        // Closing, a timeout or a broken pipe: retry until this launch can own the profile.
        _ => Delivery::Unavailable,
    })
}

fn open_client(name: &str) -> Result<Option<OwnedHandle>, InstanceError> {
    // Identification level stops whoever holds the pipe from impersonating this launch.
    let opened = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )
    };
    let pipe = match opened {
        Ok(handle) => OwnedHandle(handle),
        Err(error)
            if error.code() == ERROR_FILE_NOT_FOUND.to_hresult()
                || error.code() == ERROR_PIPE_BUSY.to_hresult() =>
        {
            return Ok(None);
        }
        Err(_) => return Err(InstanceError::Pipe),
    };
    unsafe { SetNamedPipeHandleState(pipe.0, Some(&PIPE_READMODE_MESSAGE), None, None) }
        .map_err(|_| InstanceError::Pipe)?;
    Ok(Some(pipe))
}

/// The process that created the pipe, or `None` if Windows cannot say.
fn server_process(pipe: HANDLE) -> Option<u32> {
    let mut process = 0u32;
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut process) }.is_err() {
        return None;
    }
    Some(process)
}

/// Lets the running Syrinx come forward; Windows allows this only from the foreground launch.
fn grant_foreground(process: u32) -> bool {
    unsafe { AllowSetForegroundWindow(process) }.is_ok()
}

/// How one overlapped step ended.
enum Outcome {
    Done(u32),
    Failed,
    TimedOut,
    Stopped,
}

/// One overlapped record and its event, reused for each step of a conversation. The kernel
/// owns the record and buffer until a step completes, so every step finishes before returning.
struct Operation {
    overlapped: OVERLAPPED,
    event: OwnedHandle,
}

impl Operation {
    fn new() -> Result<Self, InstanceError> {
        let event =
            unsafe { CreateEventW(None, true, false, None) }.map_err(|_| InstanceError::Pipe)?;
        Ok(Self {
            overlapped: OVERLAPPED::default(),
            event: OwnedHandle(event),
        })
    }

    fn connect(&mut self, pipe: HANDLE, stop: &StopSignal) -> Outcome {
        self.prepare();
        let started = unsafe { ConnectNamedPipe(pipe, Some(&mut self.overlapped)) };
        if matches!(&started, Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult()) {
            return Outcome::Done(0);
        }
        self.finish(pipe, started, INFINITE, Some(stop))
    }

    fn read(&mut self, pipe: HANDLE, buffer: &mut [u8], stop: Option<&StopSignal>) -> Outcome {
        self.prepare();
        let started = unsafe { ReadFile(pipe, Some(buffer), None, Some(&mut self.overlapped)) };
        self.finish(pipe, started, IO_TIMEOUT_MS, stop)
    }

    fn write(&mut self, pipe: HANDLE, bytes: &[u8], stop: Option<&StopSignal>) -> Outcome {
        self.prepare();
        let started = unsafe { WriteFile(pipe, Some(bytes), None, Some(&mut self.overlapped)) };
        self.finish(pipe, started, IO_TIMEOUT_MS, stop)
    }

    fn prepare(&mut self) {
        if let Err(error) = unsafe { ResetEvent(self.event.0) } {
            eprintln!("Syrinx could not reset a link-channel event: {error}");
        }
        self.overlapped = OVERLAPPED {
            hEvent: self.event.0,
            ..OVERLAPPED::default()
        };
    }

    /// Waits for a started step, cancelling it on timeout or stop.
    fn finish(
        &self,
        pipe: HANDLE,
        started: windows::core::Result<()>,
        timeout: u32,
        stop: Option<&StopSignal>,
    ) -> Outcome {
        match started {
            Ok(()) => return self.result(pipe),
            Err(error) if error.code() != ERROR_IO_PENDING.to_hresult() => {
                return Outcome::Failed;
            }
            Err(_) => {}
        }
        let mut handles = vec![self.event.0];
        handles.extend(stop.map(StopSignal::handle));
        let signalled = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if signalled == WAIT_OBJECT_0 {
            return self.result(pipe);
        }
        let interrupted = if signalled == WAIT_TIMEOUT {
            Outcome::TimedOut
        } else {
            Outcome::Stopped
        };
        self.cancel(pipe, interrupted)
    }

    fn result(&self, pipe: HANDLE) -> Outcome {
        let mut bytes = 0u32;
        match unsafe { GetOverlappedResult(pipe, &self.overlapped, &mut bytes, false) } {
            Ok(()) => Outcome::Done(bytes),
            Err(_) => Outcome::Failed,
        }
    }

    /// Cancels a pending step and waits until the kernel has released the record and buffer.
    fn cancel(&self, pipe: HANDLE, interrupted: Outcome) -> Outcome {
        let cancelled = unsafe { CancelIoEx(pipe, Some(&self.overlapped)) };
        let mut bytes = 0u32;
        let finished = unsafe { GetOverlappedResult(pipe, &self.overlapped, &mut bytes, true) };
        match (cancelled, finished) {
            // The step completed before the cancellation reached it.
            (_, Ok(())) => Outcome::Done(bytes),
            (Ok(()), Err(_)) => interrupted,
            (Err(error), Err(_)) if error.code() == ERROR_NOT_FOUND.to_hresult() => interrupted,
            (Err(_), Err(_)) => Outcome::Failed,
        }
    }
}
