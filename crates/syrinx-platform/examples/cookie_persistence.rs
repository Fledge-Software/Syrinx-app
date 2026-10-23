//! Verifies a persistent cookie survives Syrinx's shutdown, even when the engine is then killed (#96).
//!
//! Options: `--trace` prints timestamped harness commands, page events and loopback requests;
//! `--abrupt` also reports, without asserting, whether the cookie survives an engine killed
//! before any shutdown, which is what a Windows session end did before #96's quit hook.

use std::{
    fs,
    hash::{BuildHasher, RandomState},
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
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
use syrinx_platform::browser::{BrowserCommand, BrowserEvent, BrowserHost, PageBounds};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, E_ACCESSDENIED, ERROR_NO_MORE_FILES, HANDLE, HWND, WAIT_OBJECT_0,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
                WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            TranslateMessage, WINDOW_EX_STYLE, WM_QUIT, WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};

const DEADLINE: Duration = Duration::from_secs(45);
/// A host that never served a page has no engine work to wait for.
const IDLE_SHUTDOWN_BOUND: Duration = Duration::from_secs(2);
const ENGINE_NAME: &str = "msedgewebview2.exe";
static TRACE: AtomicBool = AtomicBool::new(false);
/// Every synthetic directory this run created, in creation order.
static RESERVED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Synthetic cookie fixture</title>\
    <link rel=icon href=data:,><p>Synthetic cookie fixture</p>";

/// What happens between the first host's `Stopped` and the second host's start.
#[derive(Clone, Copy, Debug)]
enum AfterStop {
    /// Nothing: the engine may finish its own shutdown.
    Wait,
    /// Every engine process this harness started is killed, as Windows does when a session ends.
    Terminate,
    /// The parent's thread stops the host with `shutdown_within`, without pumping, as the
    /// app's quit hook does when Windows ends the session; then the engine is killed.
    SessionEnd,
    /// The engine is killed while the page is open, before any shutdown (diagnostic only).
    Abrupt,
}

/// How long the session-end stop may block the parent's thread, as Syrinx's quit hook allows.
const SESSION_END_BOUND: Duration = Duration::from_millis(4_500);
/// "Well inside" that bound: the stop must leave Windows time to spare.
const SESSION_END_EXPECTED: Duration = Duration::from_millis(3_500);

fn main() -> ExitCode {
    // An independent deadline also catches a stalled native call or message pump.
    let (cancel, cancellation) = mpsc::channel();
    let watchdog = match thread::Builder::new()
        .name("syrinx-cookie-harness-deadline".into())
        .spawn(
            move || match cancellation.recv_timeout(Duration::from_secs(240)) {
                Ok(()) => {}
                Err(error) => {
                    eprintln!("Synthetic cookie harness deadline failed: {error}.");
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
            println!("Synthetic cookie persistence suites passed.");
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

fn run() -> Result<(), String> {
    TRACE.store(
        std::env::args().any(|argument| argument == "--trace"),
        Ordering::Release,
    );
    let server = Server::start()?;
    let idle = reserve_profile().and_then(idle_shutdown);
    if idle.is_ok() {
        println!("A host that served no page stopped without waiting for the engine.");
    }
    let orderly =
        reserve_profile().and_then(|profile| persistence(profile, &server, AfterStop::Wait));
    if orderly.is_ok() {
        println!("The cookie survived an orderly shutdown and relaunch.");
    }
    let terminated =
        reserve_profile().and_then(|profile| persistence(profile, &server, AfterStop::Terminate));
    if terminated.is_ok() {
        println!("The cookie survived engine processes killed right after Stopped.");
    }
    let session_end =
        reserve_profile().and_then(|profile| persistence(profile, &server, AfterStop::SessionEnd));
    if session_end.is_ok() {
        println!("The cookie survived a session-end stop from the parent's thread.");
    }
    if std::env::args().any(|argument| argument == "--abrupt") {
        let abrupt =
            reserve_profile().and_then(|profile| persistence(profile, &server, AfterStop::Abrupt));
        println!("Diagnostic, not asserted: an engine killed before shutdown: {abrupt:?}");
    }
    combine(
        combine(combine(combine(idle, orderly), terminated), session_end),
        server.stop(),
    )
}

/// Starts and stops a host that never created a page, and bounds how long that took.
fn idle_shutdown(profile: PathBuf) -> Result<(), String> {
    let mut harness = Harness::open(profile)?;
    let ready = harness.wait_ready();
    let elapsed = stopped_after(ready, harness.stop())?;
    trace(|| format!("idle shutdown took {elapsed:?}"));
    if elapsed > IDLE_SHUTDOWN_BOUND {
        return Err(format!(
            "A host that served no page took {elapsed:?} to stop; it should not wait for the engine."
        ));
    }
    Ok(())
}

/// Sets a persistent cookie, stops the host, then checks a fresh host on the profile sends it.
fn persistence(profile: PathBuf, server: &Server, after: AfterStop) -> Result<(), String> {
    let token = server.new_token()?;
    let mut first = Harness::open(profile.clone())?;
    let visited = first
        .wait_ready()
        .and_then(|()| first.visit(&format!("{}/set", server.origin)));
    if matches!(after, AfterStop::Abrupt) {
        visited?;
        let killed = terminate_engine()?;
        trace(|| format!("{after:?}: killed {killed} engine processes before shutdown"));
        // The killed engine makes the host report failures; only the cookie is in question.
        let stopped = first.stop();
        trace(|| format!("{after:?}: first host stop after the kill: {stopped:?}"));
    } else if matches!(after, AfterStop::SessionEnd) {
        let elapsed = stopped_after(visited, first.end_session())?;
        trace(|| format!("{after:?}: shutdown_within returned in {elapsed:?}"));
        if elapsed > SESSION_END_EXPECTED {
            return Err(format!(
                "A session-end stop took {elapsed:?}, not well inside {SESSION_END_BOUND:?}."
            ));
        }
    } else {
        let elapsed = stopped_after(visited, first.stop())?;
        trace(|| format!("{after:?}: first host stopped in {elapsed:?}"));
    }
    if matches!(after, AfterStop::Terminate | AfterStop::SessionEnd) {
        // `Stopped` must mean the engine is gone, so nothing is left for Windows to cut short.
        let killed = terminate_engine()?;
        trace(|| format!("{after:?}: killed {killed} engine processes after Stopped"));
        if killed > 0 {
            return Err(format!(
                "{killed} engine processes were still running after Stopped."
            ));
        }
    }

    let mut second = Harness::open(profile)?;
    let checked = second
        .wait_ready()
        .and_then(|()| second.visit(&format!("{}/check", server.origin)));
    combine(checked, second.stop().map(|_| ()))?;
    let sent = server.cookie_sent_to_check(&token)?;
    if !sent {
        return Err(format!(
            "{after:?}: the relaunched profile did not send the synthetic persistent cookie."
        ));
    }
    Ok(())
}

/// One synthetic parent window and the page host attached to it.
struct Harness {
    parent: HWND,
    host: Option<BrowserHost>,
    events: UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
}

impl Harness {
    fn open(profile: PathBuf) -> Result<Self, String> {
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Syrinx synthetic cookie harness"),
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
        match BrowserHost::spawn(parent.0 as isize, profile) {
            Ok((host, events)) => Ok(Self {
                parent,
                host: Some(host),
                events,
                deadline: Instant::now() + DEADLINE,
            }),
            Err(error) => {
                let message = error.to_string();
                Err(match destroy_parent(parent) {
                    Ok(()) => message,
                    Err(cleanup) => format!("{message} Cleanup also failed: {cleanup}"),
                })
            }
        }
    }

    fn host(&self) -> Result<&BrowserHost, String> {
        self.host
            .as_ref()
            .ok_or_else(|| "Synthetic host already stopped.".to_owned())
    }

    fn send(&self, command: BrowserCommand) -> Result<(), String> {
        trace(|| format!("send {command:?}"));
        self.host()?
            .send(command)
            .map_err(|error| error.to_string())
    }

    fn wait_ready(&mut self) -> Result<(), String> {
        while !matches!(self.checked_event()?, BrowserEvent::Ready) {}
        self.send(BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }))
    }

    /// Opens tab 1 at `url`, shown so hidden-page suspension cannot freeze it, and waits for its load.
    fn visit(&mut self, url: &str) -> Result<(), String> {
        self.send(BrowserCommand::Create {
            id: 1,
            url: url.into(),
        })?;
        loop {
            match self.checked_event()? {
                BrowserEvent::Created { id: 1 } => {
                    self.send(BrowserCommand::ShowActive(Some(1)))?
                }
                BrowserEvent::NavigationCompleted {
                    id: 1,
                    success: true,
                    ..
                } => return Ok(()),
                _ => {}
            }
        }
    }

    /// Shuts the host down, waits for `Stopped`, joins it and destroys the parent.
    fn stop(&mut self) -> Result<Duration, String> {
        let started = Instant::now();
        let host = self
            .host
            .take()
            .ok_or_else(|| "Synthetic host already stopped.".to_owned())?;
        trace(|| "send Shutdown".to_owned());
        host.send(BrowserCommand::Shutdown)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut observed = Ok(());
        loop {
            let event = next_event(&mut self.events, deadline)?;
            if matches!(event, BrowserEvent::Stopped) {
                break;
            }
            observed = combine(observed, check_event(&event));
        }
        let elapsed = started.elapsed();
        observed = combine(observed, host.join().map_err(|error| error.to_string()));
        combine(observed, destroy_parent(self.parent))?;
        Ok(elapsed)
    }

    /// Stops the host the way the quit hook does: one blocking call on this thread, no pump.
    fn end_session(&mut self) -> Result<Duration, String> {
        let host = self
            .host
            .take()
            .ok_or_else(|| "Synthetic host already stopped.".to_owned())?;
        let started = Instant::now();
        let stopped = host
            .shutdown_within(SESSION_END_BOUND)
            .map_err(|error| format!("Session-end stop failed: {error}"));
        let elapsed = started.elapsed();
        combine(stopped, destroy_parent(self.parent))?;
        Ok(elapsed)
    }

    fn checked_event(&mut self) -> Result<BrowserEvent, String> {
        let event = next_event(&mut self.events, self.deadline)?;
        check_event(&event)?;
        Ok(event)
    }
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
        _ => Ok(()),
    }
}

/// Pumps this thread's messages, since the page host needs its parent window serviced.
fn next_event(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<BrowserEvent, String> {
    loop {
        if Instant::now() >= deadline {
            return Err("Synthetic native event deadline expired.".into());
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
                if let Some(event) = event.into_current() {
                    trace(|| format!("event {event:?}"));
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

fn destroy_parent(parent: HWND) -> Result<(), String> {
    unsafe { DestroyWindow(parent) }
        .map_err(|error| format!("Destroy synthetic parent failed ({}).", error.code().0))
}

/// Kills the WebView2 browser processes this harness started; their helpers end with them.
/// Returns how many were still running.
fn terminate_engine() -> Result<usize, String> {
    let harness = std::process::id();
    let browsers: Vec<u32> = snapshot_processes()?
        .into_iter()
        .filter(|(_, parent, name)| *parent == harness && name.eq_ignore_ascii_case(ENGINE_NAME))
        .map(|(pid, _, _)| pid)
        .collect();
    for pid in &browsers {
        terminate(*pid)?;
    }
    Ok(browsers.len())
}

/// Every process as (id, parent id, executable name).
fn snapshot_processes() -> Result<Vec<(u32, u32, String)>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|error| format!("Process snapshot failed ({}).", error.code().0))?;
    let listed = walk_snapshot(snapshot);
    let closed = unsafe { CloseHandle(snapshot) }
        .map_err(|error| format!("Close process snapshot failed ({}).", error.code().0));
    let processes = listed?;
    closed?;
    Ok(processes)
}

fn walk_snapshot(snapshot: HANDLE) -> Result<Vec<(u32, u32, String)>, String> {
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
            .map_err(|_| "Process entry size overflow.".to_owned())?,
        ..Default::default()
    };
    let mut processes = Vec::new();
    let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
    loop {
        match next {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => {
                return Ok(processes);
            }
            Err(error) => return Err(format!("Process walk failed ({}).", error.code().0)),
        }
        let length = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        processes.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
        next = unsafe { Process32NextW(snapshot, &mut entry) };
    }
}

/// Terminates one process this harness started; one that already exited counts as done.
fn terminate(pid: u32) -> Result<(), String> {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) })
    else {
        trace(|| format!("engine process {pid} already exited"));
        return Ok(());
    };
    // Access is denied to a process that is already exiting; the wait below still applies.
    let terminated = match unsafe { TerminateProcess(process, 1) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == E_ACCESSDENIED => {
            trace(|| format!("engine process {pid} was already exiting"));
            Ok(())
        }
        Err(error) => Err(format!(
            "Terminate engine process failed ({}).",
            error.code().0
        )),
    };
    let waited = if unsafe { WaitForSingleObject(process, 10_000) } == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err("A terminated engine process did not exit.".to_owned())
    };
    let closed = unsafe { CloseHandle(process) }
        .map_err(|error| format!("Close engine process failed ({}).", error.code().0));
    combine(combine(terminated, waited), closed)
}

/// Loopback server: `/set` sets the probe cookie, `/check` records the Cookie header it got.
struct Server {
    origin: String,
    state: Arc<Mutex<ServerState>>,
    stopping: Arc<AtomicBool>,
    worker: JoinHandle<Result<(), String>>,
}

#[derive(Default)]
struct ServerState {
    token: String,
    /// The Cookie header of each `/check` request since the token was issued.
    checks: Vec<Option<String>>,
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
        let state = Arc::new(Mutex::new(ServerState::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let (served, observed) = (state.clone(), stopping.clone());
        let worker = thread::Builder::new()
            .name("syrinx-cookie-loopback".into())
            .spawn(move || serve(listener, &served, &observed))
            .map_err(|error| io_error("start synthetic loopback server", error))?;
        Ok(Self {
            origin: format!("http://127.0.0.1:{port}"),
            state,
            stopping,
            worker,
        })
    }

    /// Issues a fresh random cookie value that cannot belong to anyone.
    fn new_token(&self) -> Result<String, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Synthetic fixture clock is unavailable.".to_owned())?
            .as_nanos();
        let token = format!(
            "{:016x}{:016x}",
            RandomState::new().hash_one(nonce),
            RandomState::new().hash_one(std::process::id())
        );
        let mut state = self.lock()?;
        state.token.clone_from(&token);
        state.checks.clear();
        Ok(token)
    }

    fn cookie_sent_to_check(&self, token: &str) -> Result<bool, String> {
        let state = self.lock()?;
        if state.checks.is_empty() {
            return Err("The relaunched host never requested the check page.".into());
        }
        let expected = format!("probe={token}");
        Ok(state
            .checks
            .iter()
            .flatten()
            .any(|cookies| cookies.split(';').any(|cookie| cookie.trim() == expected)))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ServerState>, String> {
        self.state
            .lock()
            .map_err(|_| "Synthetic server state is poisoned.".to_owned())
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
    state: &Mutex<ServerState>,
    stopping: &AtomicBool,
) -> Result<(), String> {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let outcome = respond(stream, state);
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

fn respond(mut stream: TcpStream, state: &Mutex<ServerState>) -> Result<(), String> {
    let Some(head) = read_head(&mut stream)? else {
        return Ok(());
    };
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_owned();
    let cookies = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("cookie")
            .then(|| value.trim().to_owned())
    });
    trace(|| format!("server {path} cookie={}", cookies.is_some()));
    let mut state = state
        .lock()
        .map_err(|_| "Synthetic server state is poisoned.".to_owned())?;
    let extra = match path.as_str() {
        "/set" => format!(
            "Set-Cookie: probe={}; Max-Age=3600; Path=/\r\n",
            state.token
        ),
        "/check" => {
            state.checks.push(cookies);
            String::new()
        }
        _ => return write_response(&mut stream, "404 Not Found", "", ""),
    };
    drop(state);
    write_response(&mut stream, "200 OK", &extra, PAGE)
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    extra: &str,
    body: &str,
) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n{extra}Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| io_error("send synthetic response", error))
}

/// Reads one request head; `None` when the peer closed or stayed silent before sending one.
fn read_head(stream: &mut TcpStream) -> Result<Option<String>, String> {
    // Windows accepts inherit nonblocking mode; the timeouts below require blocking I/O.
    stream
        .set_nonblocking(false)
        .map_err(|error| io_error("configure synthetic request blocking reads", error))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| io_error("bound synthetic request read", error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| io_error("bound synthetic response write", error))?;
    let mut request = [0u8; 8_192];
    let mut length = 0;
    while !request[..length]
        .windows(4)
        .any(|bytes| bytes == b"\r\n\r\n")
    {
        if length == request.len() {
            return Err("Synthetic request headers exceeded their bound.".into());
        }
        match stream.read(&mut request[length..]) {
            Ok(0) => return Ok(None),
            Ok(read) => length += read,
            // A killed engine resets its connections; that request simply never arrived.
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => return Ok(None),
            Err(error)
                if length == 0
                    && matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(io_error("read synthetic request", error)),
        }
    }
    Ok(Some(
        String::from_utf8_lossy(&request[..length]).into_owned(),
    ))
}

fn reserve_profile() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Synthetic fixture clock is unavailable.".to_owned())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!(
        "syrinx-cookie-harness-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&profile).map_err(|error| io_error("reserve synthetic profile", error))?;
    match RESERVED.lock() {
        Ok(mut reserved) => reserved.push(profile.clone()),
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
fn remove_with_retry(directory: &Path) -> Result<(), io::Error> {
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

/// Prints one line under `--trace`, stamped in milliseconds since the first traced line.
fn trace(message: impl FnOnce() -> String) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    if !TRACE.load(Ordering::Acquire) {
        return;
    }
    let millis = START.get_or_init(Instant::now).elapsed().as_millis();
    eprintln!("[trace {millis:>7}] {}", message());
}

fn io_error(operation: &str, error: io::Error) -> String {
    format!(
        "{operation} failed ({:?}, code {:?}).",
        error.kind(),
        error.raw_os_error()
    )
}

/// The stop time, unless the work before the stop or the stop itself failed.
fn stopped_after(
    work: Result<(), String>,
    stopped: Result<Duration, String>,
) -> Result<Duration, String> {
    match (work, stopped) {
        (Ok(()), Ok(elapsed)) => Ok(elapsed),
        (work, stopped) => Err(combine(work, stopped.map(|_| ()))
            .err()
            .unwrap_or_else(|| "Synthetic stop failed.".to_owned())),
    }
}

fn combine(primary: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (primary, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(primary), Err(cleanup)) => Err(format!("{primary} Cleanup also failed: {cleanup}")),
    }
}
