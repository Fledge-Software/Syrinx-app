//! The Windows fixture itself; it drives WebView2 through Syrinx's page host.

#[path = "lifecycle.rs"]
mod lifecycle;

#[path = "popups.rs"]
mod popups;

#[path = "permissions.rs"]
mod permissions;

#[path = "essentials.rs"]
mod essentials;

#[path = "downloads.rs"]
mod downloads;

#[path = "audio.rs"]
mod audio;
#[path = "security.rs"]
mod security;
#[path = "suspension.rs"]
mod suspension;

use std::{
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use syrinx_platform::browser::{
    BrowserCommand, BrowserError, BrowserEvent, BrowserHost, PageBounds,
};
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

type Exercise = fn(
    &BrowserHost,
    &mut UnboundedReceiver<BrowserEvent>,
    HWND,
    &str,
    Instant,
) -> Result<(), String>;

const DEADLINE: Duration = Duration::from_secs(45);
/// `next_event`'s error once a wait's deadline passes; the audio suite tells it from a failure.
const EVENT_DEADLINE: &str =
    "Synthetic native event deadline expired; close delivery is not proven.";
/// Longer than the platform's five-second grace before an idle hidden page is suspended.
const STALL_PAST_SUSPENSION_GRACE: Duration = Duration::from_secs(6);
static TRACE: AtomicBool = AtomicBool::new(false);
/// Every synthetic directory this run created, in creation order.
static RESERVED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Synthetic close fixture</title>\
    <link rel=icon href=data:,><p>Synthetic close fixture</p>\
    <script>addEventListener('load',()=>setTimeout(()=>window.close(),100));</script>";

pub(super) fn main() -> ExitCode {
    // An independent deadline also catches a stalled native call or message pump. It sits above
    // the sum of every suite's budget and shutdown (seven suites at 45 s, the audio suite at
    // 150 s, eight 10 s shutdowns: about 545 s), so it fires only when the pump itself is stuck.
    let (cancel, cancellation) = mpsc::channel();
    let watchdog = match thread::Builder::new()
        .name("syrinx-close-harness-deadline".into())
        .spawn(
            move || match cancellation.recv_timeout(Duration::from_secs(600)) {
                Ok(()) => {}
                Err(error) => {
                    eprintln!("Synthetic page-close harness deadline failed: {error}.");
                    report_kept();
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
        report_kept();
        return ExitCode::FAILURE;
    }
    match result {
        Ok(()) => {
            println!(
                "Selected synthetic native suites passed; the reported WebView2 crash is not diagnosed."
            );
            remove_reserved();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            report_kept();
            ExitCode::FAILURE
        }
    }
}

fn option(name: &str) -> bool {
    std::env::args().any(|argument| argument == name)
}

fn run() -> Result<(), String> {
    TRACE.store(option("--trace"), Ordering::Release);
    let profile = reserve_profile()?;
    let stall = option("--stall-first-close").then_some(STALL_PAST_SUSPENSION_GRACE);
    let server = Server::start(stall)?;
    if option("--suspension-only") {
        return combine(
            run_browser(profile, &server.url, suspension::exercise, DEADLINE),
            server.stop(),
        );
    }
    if option("--audio-only") {
        return combine(
            run_browser(profile, &server.url, audio::exercise, audio::DEADLINE),
            server.stop(),
        );
    }
    if option("--downloads-only") {
        return combine(
            run_browser(profile, &server.url, downloads::exercise, DEADLINE),
            server.stop(),
        );
    }
    let result = run_browser(profile, &server.url, exercise, DEADLINE);
    if result.is_ok() {
        println!("Synthetic deferred page close and repeated lifecycle passed.");
    }
    // Run every suite even if one fails; fresh profiles keep their evidence independent.
    let popup_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, popups::exercise, DEADLINE));
    if popup_result.is_ok() {
        println!("Synthetic popup opener, message reply and native page focus passed.");
    }
    let permission_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, permissions::exercise, DEADLINE));
    if permission_result.is_ok() {
        println!(
            "Synthetic permission deny, allow, repeated prompt and default notification UI passed."
        );
    }
    let essentials_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, essentials::exercise, DEADLINE));
    if essentials_result.is_ok() {
        println!("Synthetic native find, download list, zoom and print-preview command passed.");
    }
    let downloads_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, downloads::exercise, DEADLINE));
    if downloads_result.is_ok() {
        println!("Synthetic same-tab, popup and self-closing popup downloads passed.");
    }
    let suspension_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, suspension::exercise, DEADLINE));
    let security_result = combine(security::run(&server.url), suspension_result);
    if security_result.is_ok() {
        println!(
            "Synthetic certificate cancellation, repeat rejection and safe navigation recovery passed."
        );
    }
    let audio_result = reserve_profile()
        .and_then(|profile| run_browser(profile, &server.url, audio::exercise, audio::DEADLINE));
    if audio_result.is_ok() {
        println!("Synthetic tone page stayed awake while playing and slept after the audio grace.");
    }
    combine(
        combine(
            combine(
                combine(
                    combine(combine(result, popup_result), permission_result),
                    combine(essentials_result, downloads_result),
                ),
                security_result,
            ),
            audio_result,
        ),
        server.stop(),
    )
}

fn reserve_profile() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Synthetic fixture clock is unavailable.".to_owned())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!(
        "syrinx-page-close-harness-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&profile).map_err(|error| io_error("reserve synthetic profile", error))?;
    // Runtime files remain in this exclusively created synthetic directory; no owner profile is read.
    match RESERVED.lock() {
        Ok(mut reserved) => reserved.push(profile.clone()),
        // Still record the directory, so the failure report lists it as kept.
        Err(poisoned) => {
            poisoned.into_inner().push(profile);
            return Err("Synthetic profile register is poisoned.".into());
        }
    }
    Ok(profile)
}

/// Takes the directories this run created, so cleanup can never reach another run's.
fn take_reserved() -> Vec<PathBuf> {
    match RESERVED.lock() {
        Ok(mut reserved) => std::mem::take(&mut *reserved),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

/// Deletes this run's synthetic directories after every suite passed; a failure only warns.
fn remove_reserved() {
    let temp = std::env::temp_dir();
    for directory in take_reserved() {
        // The registry holds only what this run's create_dir made; this is a cheap invariant check.
        if directory.parent() != Some(temp.as_path()) {
            eprintln!(
                "Warning: kept synthetic directory outside TEMP: {}.",
                directory.display()
            );
            continue;
        }
        if let Err(error) = remove_with_retry(&directory) {
            eprintln!(
                "Warning: could not delete synthetic directory {}: {}",
                directory.display(),
                io_error("delete synthetic directory", error)
            );
        }
    }
}

/// Retries briefly, since WebView2's child processes can hold files for a moment after shutdown.
fn remove_with_retry(directory: &std::path::Path) -> Result<(), io::Error> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::remove_dir_all(directory) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Names the directories kept after a failure, so their evidence is easy to find.
fn report_kept() {
    for directory in take_reserved() {
        eprintln!(
            "Kept synthetic directory for diagnosis: {}",
            directory.display()
        );
    }
}

/// Runs one suite in a fresh parent window; `budget` bounds it, since the audio suite needs
/// longer than `DEADLINE`.
fn run_browser(
    profile: PathBuf,
    url: &str,
    exercise: Exercise,
    budget: Duration,
) -> Result<(), String> {
    let parent = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Syrinx synthetic page-close harness"),
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
    let deadline = Instant::now() + budget;
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
    create_close_fixture(host, events, parent, &[], 1, url, deadline)?;
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

/// Creates a page that closes itself after loading, and waits for its load and close request.
/// The page is shown once created, so hidden-page suspension and the throttled timers of a
/// hidden page cannot delay its close request past the deadline.
fn create_close_fixture(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    existing: &[HWND],
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
            format!(
                "Synthetic tab {expected}, fixture creation/navigation wait: {error} \
                 (created={created}, source={source_changed}, navigated={navigated}, close={requested})"
            )
        })? {
            BrowserEvent::Created { id } if id == expected => {
                // A new page starts hidden; showing it keeps suspension out of this wait.
                let child = lifecycle::new_child(parent, existing)?;
                lifecycle::require_visible(child, false)?;
                send(host, BrowserCommand::ShowActive(Some(expected)))?;
                created = true;
            }
            BrowserEvent::Url { id, url: actual } if id == expected && actual == url => {
                source_changed = true;
            }
            BrowserEvent::NavigationCompleted { id, success: true, .. } if id == expected => {
                navigated = true
            }
            BrowserEvent::CloseRequested { id } if id == expected => requested = true,
            _ => {}
        }
    }
    Ok(())
}

fn send(host: &BrowserHost, command: BrowserCommand) -> Result<(), String> {
    trace(|| format!("send {}", describe_command(&command)));
    host.send(command).map_err(|error| error.to_string())
}

/// Prints one line under `--trace`, stamped in milliseconds since the first traced line.
fn trace(message: impl FnOnce() -> String) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    if !TRACE.load(Ordering::Acquire) {
        return;
    }
    let millis = START.get_or_init(Instant::now).elapsed().as_millis();
    eprintln!("[trace {millis:>7}] {}", message());
}

fn describe_command(command: &BrowserCommand) -> String {
    match command {
        BrowserCommand::Create { id, url } => format!("Create {id} {url}"),
        BrowserCommand::Navigate { id, url } => format!("Navigate {id} {url}"),
        BrowserCommand::Close { id } => format!("Close {id}"),
        BrowserCommand::Reload { id } => format!("Reload {id}"),
        BrowserCommand::ShowActive(active) => format!("ShowActive {active:?}"),
        BrowserCommand::Resize(bounds) => format!("Resize {bounds:?}"),
        other => format!("{other:?}"),
    }
}

fn describe_event(event: &BrowserEvent) -> String {
    match event {
        BrowserEvent::Callback { event, .. } => format!("callback {}", describe_event(event)),
        BrowserEvent::Ready => "Ready".into(),
        BrowserEvent::Created { id } => format!("Created {id}"),
        BrowserEvent::CloseRequested { id } => format!("CloseRequested {id}"),
        BrowserEvent::Url { id, url } => format!("Url {id} {url}"),
        BrowserEvent::Loading { id, loading } => format!("Loading {id} {loading}"),
        BrowserEvent::NavigationCompleted { id, success, .. } => {
            format!("NavigationCompleted {id} {success}")
        }
        BrowserEvent::Suspended { id, suspended } => format!("Suspended {id} {suspended}"),
        BrowserEvent::AudioChanged { id, muted, playing } => {
            format!("AudioChanged {id} muted {muted} playing {playing}")
        }
        other => format!("{other:?}"),
    }
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
            BrowserEvent::NavigationCompleted {
                id, success: true, ..
            } if id == expected => {
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
        BrowserEvent::NavigationCompleted {
            id, success: false, ..
        } => Err(format!("Synthetic tab {id} navigation failed.")),
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
            return Err(EVENT_DEADLINE.into());
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
            Ok(event) => {
                let description = TRACE
                    .load(Ordering::Acquire)
                    .then(|| describe_event(&event));
                let current = event.into_current();
                if let Some(description) = description {
                    let state = if current.is_some() { "event" } else { "stale" };
                    trace(|| format!("{state} {description}"));
                }
                if let Some(event) = current {
                    return Ok(event);
                }
            }
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
    fn start(stall: Option<Duration>) -> Result<Self, String> {
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
            .name("syrinx-close-loopback".into())
            .spawn(move || serve(listener, observed, stall))
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

fn serve(
    listener: TcpListener,
    stopping: Arc<AtomicBool>,
    mut stall: Option<Duration>,
) -> Result<(), String> {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                trace(|| format!("server accepted {peer}"));
                let outcome = respond(stream, &mut stall);
                trace(|| format!("server finished {peer}: {outcome:?}"));
                outcome?
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(error) => return Err(io_error("accept synthetic request", error)),
        }
    }
    Ok(())
}

fn respond(mut stream: TcpStream, stall: &mut Option<Duration>) -> Result<(), String> {
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
    if let Some(name) = downloads::file(&request[..length]) {
        return downloads::send_file(&mut stream, name);
    }
    let suite_page = popups::response(&request[..length])
        .or_else(|| permissions::response(&request[..length]))
        .or_else(|| essentials::response(&request[..length]))
        .or_else(|| downloads::response(&request[..length]))
        .or_else(|| security::response(&request[..length]))
        .or_else(|| suspension::response(&request[..length]))
        .or_else(|| audio::response(&request[..length]));
    let page = suite_page.unwrap_or(PAGE);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    // Hold the slow suspension page, and the first close page only when asked to.
    let held = if suspension::is_slow(&request[..length]) {
        Some(STALL_PAST_SUSPENSION_GRACE)
    } else if suite_page.is_none() {
        stall.take()
    } else {
        None
    };
    match held {
        Some(delay) => send_stalled(&mut stream, &response, delay),
        None => stream
            .write_all(response.as_bytes())
            .map_err(|error| io_error("send synthetic response", error)),
    }
}

/// Sends the headers and the page's title, then holds the rest so the load stays in flight.
fn send_stalled(stream: &mut TcpStream, response: &str, delay: Duration) -> Result<(), String> {
    let split = response
        .find("<p>")
        .ok_or("Synthetic close page lost its stall point.")?;
    let (head, tail) = response.as_bytes().split_at(split);
    stream
        .write_all(head)
        .map_err(|error| io_error("send stalled synthetic head", error))?;
    trace(|| format!("server holding the close page for {delay:?}"));
    thread::sleep(delay);
    stream
        .write_all(tail)
        .map_err(|error| io_error("send stalled synthetic tail", error))
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
