//! The Windows fixture itself; it drives WebView2 through Syrinx's page host.

use std::{
    cell::RefCell,
    fs,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    num::NonZeroIsize,
    path::PathBuf,
    process::ExitCode,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use syrinx_core::{HISTORY_ROW_LIMIT, HistoryDirection, HistoryObserved, NavigationList};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent, BrowserHost, PageBounds};
use webview2_com::{
    CoTaskMemPWSTR, ContentLoadingEventHandler, HistoryChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_NAVIGATION_KIND, COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD,
        COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT, COREWEBVIEW2_NAVIGATION_KIND_RELOAD,
        ICoreWebView2, ICoreWebView2NavigationStartingEventArgs3,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{BOOL, Interface, PWSTR, w},
};
use wry::{
    Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows,
    dpi::{PhysicalPosition, PhysicalSize},
};

/// How long a step must stay silent before the next one starts.
const QUIET: Duration = Duration::from_millis(900);
/// The longest one step may run.
const STEP_LIMIT: Duration = Duration::from_secs(10);

/// One thing the harness does to the tab.
#[derive(Clone)]
enum Action {
    Navigate(String),
    Reload,
    Back(usize),
    Forward(usize),
}

struct Step {
    name: &'static str,
    action: Action,
}

/// Every event received, in order, with the server's request count at that moment.
#[derive(Default)]
struct Recorder {
    lines: Vec<String>,
    last: Option<Instant>,
}

pub(super) fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Spike failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let server = Server::start()?;
    let profile = reserve_profile()?;
    let parent = create_parent()?;
    let result = run_steps(&server, &profile, parent);
    let cleanup = destroy_parent(parent);
    let checked = result.and(cleanup).and_then(|()| host_check(&server));
    server.stop();
    checked
}

fn run_steps(server: &Server, profile: &std::path::Path, parent: HWND) -> Result<(), String> {
    let mut context = WebContext::new(Some(profile.to_path_buf()));
    let holder = Parent(NonZeroIsize::new(parent.0 as isize).ok_or("No parent window.")?);
    let view = WebViewBuilder::new_with_web_context(&mut context)
        .with_additional_browser_args("--enable-features=msSmartScreenProtection")
        .with_devtools(false)
        .with_bounds(Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(900, 700).into(),
        })
        .build_as_child(&holder)
        .map_err(|error| format!("Build web view failed: {error}"))?;
    let recorder = Rc::new(RefCell::new(Recorder::default()));
    register(&view, server, &recorder)?;
    for step in steps(server) {
        run_step(&view, server, &recorder, &step)?;
    }
    Ok(())
}

/// The cases issue #126 lists, in one tab, each followed by the events it caused.
fn steps(server: &Server) -> Vec<Step> {
    let url = |path: &str| server.url(path);
    let step = |name, action| Step { name, action };
    vec![
        step("typed address p1", Action::Navigate(url("/p/1"))),
        step(
            "link to p2 (page script clicks an anchor)",
            Action::Navigate(url("/p/1?do=link:2")),
        ),
        step("typed p3", Action::Navigate(url("/p/3"))),
        step("reload", Action::Reload),
        step(
            "pushState to p3-pushed",
            Action::Navigate(url("/p/4?do=push:4b")),
        ),
        step(
            "replaceState on p5",
            Action::Navigate(url("/p/5?do=replace:5r")),
        ),
        step(
            "location.replace p6 -> p7",
            Action::Navigate(url("/p/6?do=locreplace:7")),
        ),
        step("fragment link on p8", Action::Navigate(url("/p/8?do=frag"))),
        step("302 redirect r9 -> p9", Action::Navigate(url("/r/9"))),
        step(
            "script redirect p10 -> p11",
            Action::Navigate(url("/p/10?do=assign:11")),
        ),
        step(
            "script history.back() from p12",
            Action::Navigate(url("/p/12?do=back")),
        ),
        step(
            "beforeunload page p13 (no user gesture)",
            Action::Navigate(url("/p/13?do=unload")),
        ),
        step("plain p14", Action::Navigate(url("/p/14"))),
        step("GoBack once", Action::Back(1)),
        step("GoBack x3 back-to-back", Action::Back(3)),
        step("GoForward x2 back-to-back", Action::Forward(2)),
        step("cached c1", Action::Navigate(url("/c/1"))),
        step("cached c2", Action::Navigate(url("/c/2"))),
        step("cached c3", Action::Navigate(url("/c/3"))),
        step("cached c4", Action::Navigate(url("/c/4"))),
        step(
            "GoBack x3 back-to-back over cacheable pages",
            Action::Back(3),
        ),
        step("GoForward x1", Action::Forward(1)),
        step("typed p20", Action::Navigate(url("/p/20"))),
        step(
            "pushState p21 -> p21b (no user gesture)",
            Action::Navigate(url("/p/21?do=push:21b")),
        ),
        step("typed p22", Action::Navigate(url("/p/22"))),
        step("GoBack once after a pushState", Action::Back(1)),
        step("GoBack once more", Action::Back(1)),
        step("typed p30", Action::Navigate(url("/p/30"))),
        step(
            "replaceState p31 -> p31r",
            Action::Navigate(url("/p/31?do=replace:31r")),
        ),
        step("typed p32", Action::Navigate(url("/p/32"))),
        step("GoBack once after a replaceState", Action::Back(1)),
        step("GoBack once more ", Action::Back(1)),
        step("typed p60", Action::Navigate(url("/p/60"))),
        step(
            "fragment link on p61",
            Action::Navigate(url("/p/61?do=frag")),
        ),
        step("typed p62", Action::Navigate(url("/p/62"))),
        step("GoBack to the fragment entry", Action::Back(1)),
        step("GoBack within the same document", Action::Back(1)),
        step("GoForward within the same document", Action::Forward(1)),
    ]
}

fn run_step(
    view: &WebView,
    server: &Server,
    recorder: &Rc<RefCell<Recorder>>,
    step: &Step,
) -> Result<(), String> {
    let hits_before = server.hits()?;
    recorder.borrow_mut().lines.clear();
    apply(view, &step.action)?;
    wait_until_quiet(recorder)?;
    let (back, forward) = can_go(&view.webview())?;
    println!("\n## {}", step.name);
    for line in &recorder.borrow().lines {
        println!("  {line}");
    }
    let requested = server.requests_since(hits_before)?;
    println!("  server requests: {requested:?}");
    println!(
        "  after: source={} back={back} forward={forward}",
        server.short(&source(&view.webview())?)
    );
    Ok(())
}

fn apply(view: &WebView, action: &Action) -> Result<(), String> {
    let core = view.webview();
    let result = match action {
        Action::Navigate(url) => view.load_url(url).map_err(|error| error.to_string()),
        Action::Reload => unsafe { core.Reload() }.map_err(|error| error.to_string()),
        Action::Back(times) => repeat(*times, || unsafe { core.GoBack() }),
        Action::Forward(times) => repeat(*times, || unsafe { core.GoForward() }),
    };
    result.map_err(|error| format!("Step action failed: {error}"))
}

/// Calls the step function `times` times in a row, without waiting between calls.
fn repeat(times: usize, mut call: impl FnMut() -> windows::core::Result<()>) -> Result<(), String> {
    for _ in 0..times {
        call().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn wait_until_quiet(recorder: &Rc<RefCell<Recorder>>) -> Result<(), String> {
    let started = Instant::now();
    loop {
        pump_messages()?;
        thread::sleep(Duration::from_millis(5));
        let last = recorder.borrow().last.unwrap_or(started);
        if started.elapsed() > Duration::from_millis(400) && last.elapsed() > QUIET {
            return Ok(());
        }
        if started.elapsed() > STEP_LIMIT {
            return Ok(());
        }
    }
}

/// Registers every host-side event the list would be built from; none touches the page.
fn register(
    view: &WebView,
    server: &Server,
    recorder: &Rc<RefCell<Recorder>>,
) -> Result<(), String> {
    let core = view.webview();
    let mut token = 0;
    let (rec, short) = (recorder.clone(), server.shortener());
    let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut raw = PWSTR::null();
        unsafe { args.Uri(&mut raw) }?;
        let uri = CoTaskMemPWSTR::from(raw).to_string();
        let (mut id, mut redirected, mut user) = (0, BOOL::default(), BOOL::default());
        unsafe {
            args.NavigationId(&mut id)?;
            args.IsRedirected(&mut redirected)?;
            args.IsUserInitiated(&mut user)?;
        }
        let mut kind = COREWEBVIEW2_NAVIGATION_KIND::default();
        unsafe {
            args.cast::<ICoreWebView2NavigationStartingEventArgs3>()?
                .NavigationKind(&mut kind)
        }?;
        rec.borrow_mut().push(format!(
            "NavigationStarting id={id} kind={} redirected={} user={} uri={}",
            kind_name(kind),
            redirected.as_bool(),
            user.as_bool(),
            short(&uri)
        ));
        Ok(())
    }));
    unsafe { core.add_NavigationStarting(&starting, &mut token) }.map_err(|e| e.to_string())?;

    let (rec, short) = (recorder.clone(), server.shortener());
    let changed = SourceChangedEventHandler::create(Box::new(move |core, args| {
        let (core, args) = (
            core.ok_or_else(|| windows::core::Error::from(E_POINTER))?,
            args.ok_or_else(|| windows::core::Error::from(E_POINTER))?,
        );
        let mut new_document = BOOL::default();
        unsafe { args.IsNewDocument(&mut new_document) }?;
        let url = source(&core).map_err(|_| windows::core::Error::from(E_POINTER))?;
        rec.borrow_mut().push(format!(
            "SourceChanged new_document={} source={}",
            new_document.as_bool(),
            short(&url)
        ));
        Ok(())
    }));
    unsafe { core.add_SourceChanged(&changed, &mut token) }.map_err(|e| e.to_string())?;

    let (rec, short) = (recorder.clone(), server.shortener());
    let history = HistoryChangedEventHandler::create(Box::new(move |core, _| {
        let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let (back, forward) = can_go(&core).map_err(|_| windows::core::Error::from(E_POINTER))?;
        let url = source(&core).map_err(|_| windows::core::Error::from(E_POINTER))?;
        rec.borrow_mut().push(format!(
            "HistoryChanged back={back} forward={forward} source={}",
            short(&url)
        ));
        Ok(())
    }));
    unsafe { core.add_HistoryChanged(&history, &mut token) }.map_err(|e| e.to_string())?;

    let rec = recorder.clone();
    let loading = ContentLoadingEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let (mut id, mut error_page) = (0, BOOL::default());
        unsafe {
            args.NavigationId(&mut id)?;
            args.IsErrorPage(&mut error_page)?;
        }
        rec.borrow_mut().push(format!(
            "ContentLoading id={id} error_page={}",
            error_page.as_bool()
        ));
        Ok(())
    }));
    unsafe { core.add_ContentLoading(&loading, &mut token) }.map_err(|e| e.to_string())?;

    let rec = recorder.clone();
    let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let (mut id, mut success) = (0, BOOL::default());
        unsafe {
            args.NavigationId(&mut id)?;
            args.IsSuccess(&mut success)?;
        }
        rec.borrow_mut().push(format!(
            "NavigationCompleted id={id} success={}",
            success.as_bool()
        ));
        Ok(())
    }));
    unsafe { core.add_NavigationCompleted(&completed, &mut token) }.map_err(|e| e.to_string())
}

impl Recorder {
    fn push(&mut self, line: String) {
        self.lines.push(line);
        self.last = Some(Instant::now());
    }
}

fn kind_name(kind: COREWEBVIEW2_NAVIGATION_KIND) -> &'static str {
    match kind {
        COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT => "new_document",
        COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD => "back_or_forward",
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD => "reload",
        _ => "unknown",
    }
}

fn can_go(core: &ICoreWebView2) -> Result<(bool, bool), String> {
    let (mut back, mut forward) = (BOOL::default(), BOOL::default());
    unsafe {
        core.CanGoBack(&mut back).map_err(|e| e.to_string())?;
        core.CanGoForward(&mut forward).map_err(|e| e.to_string())?;
    }
    Ok((back.as_bool(), forward.as_bool()))
}

fn source(core: &ICoreWebView2) -> Result<String, String> {
    let mut raw = PWSTR::null();
    let result = unsafe { core.Source(&mut raw) };
    let owned = CoTaskMemPWSTR::from(raw);
    result.map_err(|e| e.to_string())?;
    Ok(owned.to_string())
}

/// The synthetic pages. Each page's own script performs the case named in its `do` query.
fn page(path: &str) -> (&'static str, String, &'static str) {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    if let Some(number) = route.strip_prefix("/r/") {
        return ("302 Found", format!("/p/{number}"), "");
    }
    let cache = if route.starts_with("/c/") {
        "max-age=600"
    } else {
        "no-store"
    };
    let title = route.trim_start_matches('/').replace('/', "");
    let script = query
        .strip_prefix("do=")
        .map(case_script)
        .unwrap_or_default();
    let body = format!(
        "<!doctype html><title>Synthetic {title}</title><p>Synthetic page {title}.</p>\
         <p id=section>Section.</p><script>{script}</script>"
    );
    ("200 OK", body, cache)
}

fn case_script(case: &str) -> String {
    let (verb, target) = case.split_once(':').unwrap_or((case, ""));
    let later = |code: String| format!("setTimeout(() => {{ {code} }}, 150);");
    match verb {
        "link" => later(format!(
            "const a = document.createElement('a'); a.href = '/p/{target}'; document.body.append(a); a.click();"
        )),
        "push" => later(format!("history.pushState({{}}, '', '/p/{target}');")),
        "replace" => later(format!("history.replaceState({{}}, '', '/p/{target}');")),
        "locreplace" => later(format!("location.replace('/p/{target}');")),
        "assign" => format!("location.href = '/p/{target}';"),
        "frag" => later(
            "const a = document.createElement('a'); a.href = '#section'; document.body.append(a); a.click();".into(),
        ),
        "back" => later("history.back();".into()),
        "unload" => "addEventListener('beforeunload', e => { e.preventDefault(); e.returnValue = ''; });".into(),
        _ => String::new(),
    }
}

/// A loopback-only HTTP server that answers one request per connection and counts requests.
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stopping: Arc<AtomicBool>,
}

impl Server {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let (log, stop) = (requests.clone(), stopping.clone());
        thread::spawn(move || accept_loop(&listener, &log, &stop));
        Ok(Self {
            base: format!("http://127.0.0.1:{port}"),
            requests,
            stopping,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn short(&self, url: &str) -> String {
        url.strip_prefix(&self.base).unwrap_or(url).to_owned()
    }

    fn shortener(&self) -> impl Fn(&str) -> String + 'static {
        let base = self.base.clone();
        move |url| url.strip_prefix(&base).unwrap_or(url).to_owned()
    }

    fn hits(&self) -> Result<usize, String> {
        Ok(self.log()?.len())
    }

    fn requests_since(&self, before: usize) -> Result<Vec<String>, String> {
        let log = self.log()?;
        Ok(log[before.min(log.len())..].to_vec())
    }

    fn log(&self) -> Result<std::sync::MutexGuard<'_, Vec<String>>, String> {
        self.requests
            .lock()
            .map_err(|_| "Synthetic request log poisoned.".to_owned())
    }

    fn stop(self) {
        self.stopping.store(true, Ordering::Release);
    }
}

fn accept_loop(listener: &TcpListener, log: &Arc<Mutex<Vec<String>>>, stopping: &AtomicBool) {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let log = log.clone();
                thread::spawn(move || {
                    if let Err(error) = answer(stream, &log) {
                        eprintln!("A synthetic request failed: {error}");
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                eprintln!("A synthetic accept failed: {error}");
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn answer(stream: TcpStream, log: &Mutex<Vec<String>>) -> Result<(), String> {
    // An accepted socket inherits the listener's nonblocking mode (trapstoknow).
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&stream);
    let mut first = String::new();
    reader.read_line(&mut first).map_err(|e| e.to_string())?;
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
    if path == "/favicon.ico" {
        return respond(&stream, "404 Not Found", "", "no-store", "");
    }
    log.lock()
        .map_err(|_| "Synthetic request log poisoned.".to_owned())?
        .push(path.clone());
    let (status, body, cache) = page(&path);
    if status.starts_with("302") {
        return respond(&stream, status, "", "no-store", &body);
    }
    respond(&stream, status, &body, cache, "")
}

fn respond(
    mut stream: &TcpStream,
    status: &str,
    body: &str,
    cache: &str,
    location: &str,
) -> Result<(), String> {
    let location = if location.is_empty() {
        String::new()
    } else {
        format!("Location: {location}\r\n")
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: {cache}\r\n\
         {location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(reply.as_bytes())
        .map_err(|e| e.to_string())
}

struct Parent(NonZeroIsize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(self.0)))
        })
    }
}

fn reserve_profile() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock unavailable.".to_owned())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!("syrinx-history-spike-{nonce}"));
    fs::create_dir(&profile).map_err(|e| e.to_string())?;
    Ok(profile)
}

fn create_parent() -> Result<HWND, String> {
    let parent = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Syrinx synthetic history spike"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            900,
            700,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    // Shown without activation, so the owner's typing never lands here.
    unsafe {
        let _was_visible = ShowWindow(parent, SW_SHOWNOACTIVATE);
    }
    Ok(parent)
}

fn destroy_parent(parent: HWND) -> Result<(), String> {
    unsafe { DestroyWindow(parent) }.map_err(|e| e.to_string())
}

fn pump_messages() -> Result<(), String> {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _translated = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

/// Drives Syrinx's real page host and its `NavigationList`: four pages, a three-row jump
/// back, a two-row jump forward and a same-page change, checking the list each time.
fn host_check(server: &Server) -> Result<(), String> {
    let parent = create_parent()?;
    let profile = reserve_profile()?;
    let (host, events) = BrowserHost::spawn(parent.0 as isize, profile)
        .map_err(|error| format!("Start page host failed: {error}"))?;
    let mut tab = HostTab {
        host,
        events,
        list: NavigationList::default(),
    };
    let checked = tab.run(server);
    let stopped = tab.stop();
    let destroyed = destroy_parent(parent);
    checked.and(stopped).and(destroyed)
}

struct HostTab {
    host: BrowserHost,
    events: UnboundedReceiver<BrowserEvent>,
    list: NavigationList,
}

impl HostTab {
    const ID: u64 = 1;

    fn run(&mut self, server: &Server) -> Result<(), String> {
        self.wait(|_, event| matches!(event, Some(BrowserEvent::Ready)))?;
        self.send(BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 900,
            height: 700,
        }))?;
        self.send(BrowserCommand::Create {
            id: Self::ID,
            url: server.url("/p/40"),
        })?;
        self.send(BrowserCommand::ShowActive(Some(Self::ID)))?;
        self.wait_at(&server.url("/p/40"))?;
        for page in 41..=44 {
            let url = server.url(&format!("/p/{page}"));
            self.send(BrowserCommand::Navigate {
                id: Self::ID,
                url: url.clone(),
            })?;
            self.wait_at(&url)?;
        }
        self.jump(HistoryDirection::Back, 3, &server.url("/p/41"))?;
        self.expect_rows(HistoryDirection::Forward, &[42, 43, 44], server)?;
        self.jump(HistoryDirection::Forward, 2, &server.url("/p/43"))?;
        self.expect_rows(HistoryDirection::Back, &[42, 41, 40], server)?;
        self.send(BrowserCommand::Navigate {
            id: Self::ID,
            url: server.url("/p/45?do=replace:45r"),
        })?;
        self.wait_at(&server.url("/p/45r"))?;
        self.expect_rows(HistoryDirection::Back, &[43, 42, 41, 40], server)?;
        println!(
            "
## BrowserHost and NavigationList agree: jumps of 3 and 2 rows, same-page replace"
        );
        Ok(())
    }

    fn jump(
        &mut self,
        direction: HistoryDirection,
        steps: usize,
        expected: &str,
    ) -> Result<(), String> {
        if !self.list.start_jump(direction, steps) {
            return Err("The list refused a jump it should allow.".into());
        }
        self.send(BrowserCommand::GoToOffset {
            id: Self::ID,
            direction,
            steps: u32::try_from(steps).map_err(|e| e.to_string())?,
        })?;
        self.wait_at(expected)
    }

    fn expect_rows(
        &self,
        direction: HistoryDirection,
        pages: &[u32],
        server: &Server,
    ) -> Result<(), String> {
        let actual: Vec<String> = self
            .list
            .rows(direction, HISTORY_ROW_LIMIT)
            .iter()
            .map(|row| server.short(row.address))
            .collect();
        let expected: Vec<String> = pages.iter().map(|page| format!("/p/{page}")).collect();
        if actual == expected {
            return Ok(());
        }
        Err(format!(
            "{direction:?} rows were {actual:?}, expected {expected:?}."
        ))
    }

    /// Waits until the list's current row is `url` after a report that agreed with the engine.
    fn wait_at(&mut self, url: &str) -> Result<(), String> {
        self.wait(|list, _| list.current_address() == Some(url))
    }

    fn send(&self, command: BrowserCommand) -> Result<(), String> {
        self.host.send(command).map_err(|error| error.to_string())
    }

    /// Feeds history reports to the list until `done` accepts it, or fails after ten seconds.
    fn wait(
        &mut self,
        mut done: impl FnMut(&NavigationList, Option<&BrowserEvent>) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + STEP_LIMIT;
        while Instant::now() < deadline {
            pump_messages()?;
            let event = match self.events.try_recv() {
                Ok(event) => event.into_current(),
                Err(TryRecvError::Closed) => return Err("Page host stopped early.".into()),
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
            };
            match &event {
                Some(BrowserEvent::History { report, .. }) => {
                    if self.list.observe(report) == HistoryObserved::Reset {
                        println!("  list reset after a {:?}", report.change);
                    }
                }
                Some(BrowserEvent::Title { title, .. }) => self.list.retitle(title),
                Some(BrowserEvent::Error { error, .. }) => return Err(error.to_string()),
                _ => {}
            }
            if done(&self.list, event.as_ref()) {
                return Ok(());
            }
        }
        Err("Timed out waiting for the page host.".into())
    }

    fn stop(mut self) -> Result<(), String> {
        self.send(BrowserCommand::Shutdown)?;
        self.wait(|_, event| matches!(event, Some(BrowserEvent::Stopped)))?;
        self.host.join().map_err(|error| error.to_string())
    }
}
