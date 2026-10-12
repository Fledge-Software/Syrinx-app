//! Verifies a synthetic page close stays deferred until the host releases its native view.

#[path = "page_close/lifecycle.rs"]
mod lifecycle;

use std::{
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use atlas_platform::browser::{
    BrowserCommand, BrowserError, BrowserEvent, BrowserHost, PageBounds,
};
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GW_CHILD, GetWindow, IsWindow, MSG,
            PM_REMOVE, PeekMessageW, TranslateMessage, WINDOW_EX_STYLE, WM_QUIT,
            WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};

const DEADLINE: Duration = Duration::from_secs(45);
const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Synthetic close fixture</title>\
    <link rel=icon href=data:,><p>Synthetic close fixture</p>\
    <script>addEventListener('load',()=>setTimeout(()=>window.close(),100));</script>";

fn main() -> ExitCode {
    // An independent deadline also catches a stalled native call or message pump.
    let (cancel, cancellation) = mpsc::channel();
    let watchdog = match thread::Builder::new()
        .name("atlas-close-harness-deadline".into())
        .spawn(
            move || match cancellation.recv_timeout(Duration::from_secs(60)) {
                Ok(()) => {}
                Err(error) => {
                    eprintln!("Synthetic page-close harness deadline failed: {error}.");
                    std::process::exit(1);
                }
            },
        ) {
        Ok(worker) => worker,
        Err(error) => {
            eprintln!("{}", io_error("start harness deadline", error));
            return ExitCode::FAILURE;
        }
    };
    let result = run();
    let cancellation_failed = cancel.send(()).is_err();
    let watchdog_failed = watchdog.join().is_err();
    if cancellation_failed || watchdog_failed {
        eprintln!("Synthetic harness deadline cleanup failed.");
        return ExitCode::FAILURE;
    }
    match result {
        Ok(()) => {
            println!(
                "Synthetic native close, visibility, bounds, retry, 12 lifecycle cycles and multi-page shutdown passed; the reported WebView2 crash is not diagnosed."
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let profile = reserve_profile()?;
    let server = Server::start()?;
    let result = run_browser(profile, &server.url);
    combine(result, server.stop())
}

fn reserve_profile() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Synthetic fixture clock is unavailable.".to_owned())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!(
        "atlas-page-close-harness-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&profile).map_err(|error| io_error("reserve synthetic profile", error))?;
    // Runtime files remain in this exclusively created synthetic directory; no owner profile is read.
    Ok(profile)
}

fn run_browser(profile: PathBuf, url: &str) -> Result<(), String> {
    let parent = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Atlas synthetic page-close harness"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            640,
            480,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|error| format!("Create synthetic parent failed ({}).", error.code().0))?;
    let (host, mut events) = match BrowserHost::spawn(parent.0 as isize, profile) {
        Ok(started) => started,
        Err(error) => return combine(Err(error.to_string()), destroy_parent(parent)),
    };
    let deadline = Instant::now() + DEADLINE;
    let result = exercise(&host, &mut events, parent, url, deadline);
    let shutdown = shutdown(host, &mut events, Instant::now() + Duration::from_secs(10));
    // A failed shutdown must not explicitly destroy the parent beneath a running browser worker.
    if let Err(error) = shutdown {
        return combine(result, Err(error));
    }
    let result = combine(result, lifecycle::require_children(parent, &[]));
    combine(result, destroy_parent(parent))
}

fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
    )?;
    create_close_fixture(host, events, 1, url, deadline)?;
    let child = unsafe { GetWindow(parent, GW_CHILD) }
        .map_err(|error| format!("Deferred native child is missing ({}).", error.code().0))?;
    require_window(child, true)?;

    // The callback must leave a usable controller alive; this is a separate queued navigation.
    send(
        host,
        BrowserCommand::Navigate {
            id: 1,
            url: "about:blank".into(),
        },
    )?;
    wait_navigation(events, 1, deadline)?;
    require_window(child, true)?;
    send(host, BrowserCommand::Close { id: 1 })?;
    send(host, BrowserCommand::Reload { id: 1 })?;
    wait_removed(events, deadline)?;
    require_window(child, false)?;

    lifecycle::exercise(host, events, parent, url, deadline)
}

fn create_close_fixture(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    expected: u64,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    send(
        host,
        BrowserCommand::Create {
            id: expected,
            url: url.into(),
        },
    )?;

    let mut created = false;
    let mut source_changed = false;
    let mut navigated = false;
    let mut requested = false;
    while !(created && source_changed && navigated && requested) {
        match checked_event(events, deadline).map_err(|error| {
            format!("Synthetic tab {expected}, fixture creation/navigation wait: {error}")
        })? {
            BrowserEvent::Created { id } if id == expected => created = true,
            BrowserEvent::Url { id, url: actual } if id == expected && actual == url => {
                source_changed = true;
            }
            BrowserEvent::NavigationCompleted { id, success: true } if id == expected => {
                navigated = true
            }
            BrowserEvent::CloseRequested { id } if id == expected => requested = true,
            _ => {}
        }
    }
    Ok(())
}

fn send(host: &BrowserHost, command: BrowserCommand) -> Result<(), String> {
    host.send(command).map_err(|error| error.to_string())
}

fn wait_navigation(
    events: &mut UnboundedReceiver<BrowserEvent>,
    expected: u64,
    deadline: Instant,
) -> Result<(), String> {
    let mut source_changed = false;
    let mut completed = false;
    while !(source_changed && completed) {
        match checked_event(events, deadline).map_err(|error| {
            format!("Synthetic tab {expected}, about:blank navigation wait: {error}")
        })? {
            BrowserEvent::Url { id, url } if id == expected && url == "about:blank" => {
                source_changed = true;
            }
            BrowserEvent::NavigationCompleted { id, success: true } if id == expected => {
                completed = true;
            }
            _ => {}
        }
    }
    Ok(())
}

fn wait_removed(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        let event = next_event(events, deadline)?;
        if matches!(
            event,
            BrowserEvent::Error {
                tab: Some(1),
                error: BrowserError::UnknownTab
            }
        ) {
            return Ok(());
        }
        check_event(&event)?;
    }
}

fn shutdown(
    host: BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    send(&host, BrowserCommand::Shutdown)?;
    let mut observed = Ok(());
    loop {
        let event = next_event(events, deadline)?;
        if matches!(event, BrowserEvent::Stopped) {
            break;
        }
        observed = combine(observed, check_event(&event));
    }
    combine(observed, host.join().map_err(|error| error.to_string()))
}

fn checked_event(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<BrowserEvent, String> {
    let event = next_event(events, deadline)?;
    check_event(&event)?;
    Ok(event)
}

fn check_event(event: &BrowserEvent) -> Result<(), String> {
    match event {
        BrowserEvent::Error { tab, error } => {
            Err(format!("Synthetic command error for tab {tab:?}: {error}"))
        }
        BrowserEvent::CreationFailed { id, error } => {
            Err(format!("Synthetic tab {id} creation failed: {error}"))
        }
        BrowserEvent::ProcessFailed { id, kind } => Err(format!(
            "Synthetic tab {id} runtime process failed ({kind})."
        )),
        BrowserEvent::NavigationCompleted { id, success: false } => {
            Err(format!("Synthetic tab {id} navigation failed."))
        }
        BrowserEvent::Stopped => Err("Synthetic browser stopped before shutdown.".into()),
        BrowserEvent::PopupBlocked { id } => Err(format!(
            "Synthetic tab {id} unexpectedly requested a popup."
        )),
        _ => Ok(()),
    }
}

fn next_event(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<BrowserEvent, String> {
    loop {
        if Instant::now() >= deadline {
            return Err(
                "Synthetic native event deadline expired; close delivery is not proven.".into(),
            );
        }
        let mut message = MSG::default();
        for _ in 0..256 {
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                return Err("Synthetic parent received an unexpected quit.".into());
            }
            unsafe {
                let _translated = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        match events.try_recv() {
            Ok(event) => return Ok(event),
            Err(TryRecvError::Closed) => {
                return Err("Synthetic browser event channel closed.".into());
            }
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn require_window(window: HWND, expected: bool) -> Result<(), String> {
    if unsafe { IsWindow(Some(window)) }.as_bool() == expected {
        Ok(())
    } else {
        Err("Synthetic native child lifetime disagrees with deferred close.".into())
    }
}

fn destroy_parent(parent: HWND) -> Result<(), String> {
    unsafe { DestroyWindow(parent) }
        .map_err(|error| format!("Destroy synthetic parent failed ({}).", error.code().0))
}

struct Server {
    url: String,
    stopping: Arc<AtomicBool>,
    worker: JoinHandle<Result<(), String>>,
}

impl Server {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| io_error("bind synthetic loopback server", error))?;
        let port = listener
            .local_addr()
            .map_err(|error| io_error("read loopback port", error))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| io_error("bound loopback accepts", error))?;
        let stopping = Arc::new(AtomicBool::new(false));
        let observed = stopping.clone();
        let worker = thread::Builder::new()
            .name("atlas-close-loopback".into())
            .spawn(move || serve(listener, observed))
            .map_err(|error| io_error("start synthetic loopback server", error))?;
        Ok(Self {
            url: format!("http://127.0.0.1:{port}/close"),
            stopping,
            worker,
        })
    }

    fn stop(self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        self.worker
            .join()
            .map_err(|_| "Synthetic loopback worker panicked.".to_owned())?
    }
}

fn serve(listener: TcpListener, stopping: Arc<AtomicBool>) -> Result<(), String> {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => respond(stream)?,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(error) => return Err(io_error("accept synthetic request", error)),
        }
    }
    Ok(())
}

fn respond(mut stream: TcpStream) -> Result<(), String> {
    configure_stream(&stream)?;
    let mut request = [0u8; 4_096];
    let mut length = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !request[..length]
        .windows(4)
        .any(|bytes| bytes == b"\r\n\r\n")
    {
        if Instant::now() >= deadline {
            return Err("Synthetic request headers exceeded their deadline.".into());
        }
        if length == request.len() {
            return Err("Synthetic request headers exceeded their bound.".into());
        }
        match stream.read(&mut request[length..]) {
            Ok(0) => return Ok(()),
            Ok(read) => length += read,
            Err(error)
                if length == 0
                    && matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io_error("read synthetic request", error)),
        }
    }
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{PAGE}",
        PAGE.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| io_error("send synthetic response", error))
}

fn configure_stream(stream: &TcpStream) -> Result<(), String> {
    // Windows accepts inherit nonblocking mode; the timeouts below require blocking I/O.
    stream
        .set_nonblocking(false)
        .map_err(|error| io_error("configure synthetic request blocking reads", error))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| io_error("bound synthetic request read", error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| io_error("bound synthetic response write", error))
}

fn io_error(operation: &str, error: io::Error) -> String {
    format!(
        "{operation} failed ({:?}, code {:?}).",
        error.kind(),
        error.raw_os_error()
    )
}

fn combine(primary: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (primary, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(primary), Err(cleanup)) => Err(format!("{primary} Cleanup also failed: {cleanup}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Ipv4Addr, Read, TcpListener, TcpStream, configure_stream, io};

    #[test]
    fn accepted_stream_waits_for_request_data_after_configuration() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind synthetic stream-mode listener");
        listener
            .set_nonblocking(true)
            .expect("configure synthetic listener polling");
        let address = listener
            .local_addr()
            .expect("read synthetic listener address");
        // Keep the peer open and silent until both reads finish, so neither can observe EOF.
        let client = TcpStream::connect(address).expect("connect synthetic silent client");
        let (mut stream, _) = listener.accept().expect("accept synthetic silent client");
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("bound the unconfigured stream probe");
        let mut byte = [0u8; 1];

        // An inherited nonblocking read returns immediately even though the client remains open.
        let unconfigured = stream
            .read(&mut byte)
            .expect_err("no request bytes were sent");
        assert_eq!(unconfigured.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(unconfigured.raw_os_error(), Some(10035));

        // Exercise the same setup used by respond, without relying on an elapsed-time threshold.
        configure_stream(&stream).expect("configure synthetic accepted stream");
        let configured = stream
            .read(&mut byte)
            .expect_err("silent client must time out");
        assert_eq!(configured.kind(), io::ErrorKind::TimedOut);
        assert_eq!(configured.raw_os_error(), Some(10060));
        drop(client);
    }
}
