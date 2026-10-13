//! Runs P1-25's synthetic page-text feasibility cases against local loopback sites only;
//! see `docs/p1-25-page-text.md` for the results and `--help` for the options.

#[path = "page_text_feasibility/cases.rs"]
mod cases;
#[path = "page_text_feasibility/pages.rs"]
mod pages;
#[path = "page_text_feasibility/server.rs"]
mod server;

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::ExitCode,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use atlas_platform::browser::{
    BrowserCommand, BrowserEvent, BrowserHost, DiscardReason, Ineligible, PageBounds,
    PageTextOutcome, PageTextPolicy,
};
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            SW_SHOW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_QUIT, WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};

use server::{Role, Server, Sites};

/// One case's verdict, printed in the report table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Measured or observed, with no pass condition this harness can decide.
    Recorded,
}

pub struct CaseResult {
    pub case: &'static str,
    pub verdict: Verdict,
    pub detail: String,
}

impl CaseResult {
    pub fn new(case: &'static str, passed: bool, detail: impl Into<String>) -> Self {
        let verdict = if passed { Verdict::Pass } else { Verdict::Fail };
        Self {
            case,
            verdict,
            detail: detail.into(),
        }
    }

    pub fn recorded(case: &'static str, detail: impl Into<String>) -> Self {
        Self {
            case,
            verdict: Verdict::Recorded,
            detail: detail.into(),
        }
    }
}

/// A copy of one page-text outcome the harness can keep and compare.
#[derive(Clone, Debug)]
pub enum Seen {
    Ineligible(Ineligible),
    Cancelled,
    Requested(String),
    Superseded,
    NotCommitted,
    Discarded(DiscardReason),
    Captured(Capture),
    Failed,
}

#[derive(Clone, Debug)]
pub struct Capture {
    pub address: String,
    pub text: String,
    pub documents: usize,
    pub response_bytes: usize,
    pub call_time: Duration,
    pub parse_time: Duration,
}

impl Seen {
    fn from_outcome(outcome: PageTextOutcome) -> Self {
        match outcome {
            PageTextOutcome::Ineligible(reason) => Self::Ineligible(reason),
            PageTextOutcome::Cancelled => Self::Cancelled,
            PageTextOutcome::Requested { address } => Self::Requested(address),
            PageTextOutcome::Superseded => Self::Superseded,
            PageTextOutcome::NotCommitted => Self::NotCommitted,
            PageTextOutcome::Discarded(reason) => Self::Discarded(reason),
            PageTextOutcome::Captured(captured) => Self::Captured(Capture {
                address: captured.address,
                text: captured.text,
                documents: captured.documents,
                response_bytes: captured.response_bytes,
                call_time: captured.call_time,
                parse_time: captured.parse_time,
            }),
            PageTextOutcome::Failed => Self::Failed,
        }
    }

    /// Whether this outcome ends a capture attempt.
    pub fn is_final(&self) -> bool {
        !matches!(self, Self::Cancelled | Self::Requested(_))
    }
}

/// Everything the browser reported, per tab, in arrival order.
#[derive(Default)]
pub struct Log {
    pub text: HashMap<u64, Vec<(Instant, Seen)>>,
    pub titles: HashMap<u64, String>,
    pub urls: HashMap<u64, String>,
    pub completed: HashMap<u64, u32>,
    pub suspended: HashMap<u64, bool>,
    pub errors: Vec<String>,
    pub page_text_events: usize,
}

impl Log {
    pub fn outcomes(&self, id: u64) -> Vec<Seen> {
        self.text
            .get(&id)
            .map(|seen| seen.iter().map(|(_, seen)| seen.clone()).collect())
            .unwrap_or_default()
    }

    pub fn requests(&self, id: u64) -> Vec<String> {
        self.outcomes(id)
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Requested(address) => Some(address),
                _ => None,
            })
            .collect()
    }

    pub fn finals(&self, id: u64) -> usize {
        self.outcomes(id)
            .iter()
            .filter(|seen| seen.is_final())
            .count()
    }
}

/// One browser worker with a fresh synthetic profile and a visible parent window.
pub struct Browser {
    host: BrowserHost,
    events: UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    pub profile: PathBuf,
    pub log: Log,
    next_id: u64,
}

impl Browser {
    pub fn start(policy: Option<PageTextPolicy>) -> Result<Self, String> {
        let profile = reserve_profile()?;
        let parent = create_parent()?;
        let (host, events) = match BrowserHost::spawn(parent.0 as isize, profile.clone()) {
            Ok(started) => started,
            Err(error) => {
                let message = error.to_string();
                return Err(match destroy_parent(parent) {
                    Ok(()) => message,
                    Err(cleanup) => format!("{message} Cleanup also failed: {cleanup}"),
                });
            }
        };
        let mut browser = Self {
            host,
            events,
            parent,
            profile,
            log: Log::default(),
            next_id: 1,
        };
        browser.wait("worker ready", Duration::from_secs(30), |_, event| {
            matches!(event, Some(BrowserEvent::Ready))
        })?;
        browser.send(BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 900,
            height: 700,
        }))?;
        browser.send(BrowserCommand::SetPageTextPolicy(policy))?;
        Ok(browser)
    }

    pub fn send(&self, command: BrowserCommand) -> Result<(), String> {
        self.host.send(command).map_err(|error| error.to_string())
    }

    /// Creates a tab at `url`, optionally showing it.
    pub fn open(&mut self, url: &str, show: bool) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(BrowserCommand::Create {
            id,
            url: url.to_owned(),
        })?;
        if show {
            self.send(BrowserCommand::ShowActive(Some(id)))?;
        }
        Ok(id)
    }

    pub fn navigate(&self, id: u64, url: &str) -> Result<(), String> {
        self.send(BrowserCommand::Navigate {
            id,
            url: url.to_owned(),
        })
    }

    pub fn close(&mut self, id: u64) -> Result<(), String> {
        self.send(BrowserCommand::Close { id })?;
        self.pump_for(Duration::from_millis(200))
    }

    /// Waits until `id` has `count` final page-text outcomes.
    pub fn wait_finals(&mut self, id: u64, count: usize, timeout: Duration) -> Result<(), String> {
        self.wait(&format!("tab {id} capture outcome"), timeout, |log, _| {
            log.finals(id) >= count
        })
    }

    pub fn wait_title(
        &mut self,
        id: u64,
        prefix: &str,
        timeout: Duration,
    ) -> Result<String, String> {
        self.wait(&format!("tab {id} title {prefix}"), timeout, |log, _| {
            log.titles
                .get(&id)
                .is_some_and(|title| title.starts_with(prefix))
        })?;
        self.log
            .titles
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Tab {id} has no title."))
    }

    /// Records every event for `duration`.
    pub fn pump_for(&mut self, duration: Duration) -> Result<(), String> {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.next(until)?;
        }
        Ok(())
    }

    /// Records events until `done` accepts the log (and the latest event), or fails at timeout.
    pub fn wait(
        &mut self,
        what: &str,
        timeout: Duration,
        mut done: impl FnMut(&Log, Option<&BrowserEvent>) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        if done(&self.log, None) {
            return Ok(());
        }
        while Instant::now() < deadline {
            let event = self.next(deadline)?;
            if done(&self.log, event.as_ref()) {
                return Ok(());
            }
        }
        Err(format!("Timed out waiting for {what}."))
    }

    /// Pumps this thread's messages and records at most one browser event.
    fn next(&mut self, deadline: Instant) -> Result<Option<BrowserEvent>, String> {
        pump_messages()?;
        match self.events.try_recv() {
            Ok(event) => {
                let Some(event) = event.into_current() else {
                    return Ok(None);
                };
                Ok(self.record(event))
            }
            Err(TryRecvError::Closed) => Err("Browser event channel closed.".into()),
            Err(TryRecvError::Empty) => {
                if Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(2));
                }
                Ok(None)
            }
        }
    }

    /// Stores the event; page-text outcomes are consumed, others are returned too.
    fn record(&mut self, event: BrowserEvent) -> Option<BrowserEvent> {
        match event {
            BrowserEvent::PageText { id, outcome } => {
                self.log.page_text_events += 1;
                self.log
                    .text
                    .entry(id)
                    .or_default()
                    .push((Instant::now(), Seen::from_outcome(outcome)));
                None
            }
            BrowserEvent::Title { id, ref title } => {
                self.log.titles.insert(id, title.clone());
                Some(event)
            }
            BrowserEvent::Url { id, ref url } => {
                self.log.urls.insert(id, url.clone());
                Some(event)
            }
            BrowserEvent::NavigationCompleted { id, .. } => {
                *self.log.completed.entry(id).or_default() += 1;
                Some(event)
            }
            BrowserEvent::Suspended { id, suspended } => {
                self.log.suspended.insert(id, suspended);
                Some(event)
            }
            BrowserEvent::Error { tab, error } => {
                self.log.errors.push(format!("tab {tab:?}: {error}"));
                None
            }
            BrowserEvent::CreationFailed { id, error } => {
                self.log.errors.push(format!("tab {id} creation: {error}"));
                None
            }
            BrowserEvent::ProcessFailed { id, kind } => {
                self.log
                    .errors
                    .push(format!("tab {id} process failed ({kind})"));
                None
            }
            event => Some(event),
        }
    }

    pub fn stop(mut self) -> Result<(), String> {
        let shutdown = self.send(BrowserCommand::Shutdown).and_then(|()| {
            self.wait("worker stop", Duration::from_secs(15), |_, event| {
                matches!(event, Some(BrowserEvent::Stopped))
            })
        });
        // A worker that did not stop still owns the parent; leave it rather than destroy it.
        shutdown?;
        let joined = self.host.join().map_err(|error| error.to_string());
        combine(joined, destroy_parent(self.parent))
    }
}

fn main() -> ExitCode {
    let options = match Options::parse() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    // An independent deadline catches a stalled native call or message pump.
    let (cancel, cancellation) = mpsc::channel::<()>();
    let watchdog = thread::Builder::new()
        .name("atlas-page-text-deadline".into())
        .spawn(move || {
            if cancellation
                .recv_timeout(Duration::from_secs(40 * 60))
                .is_err()
            {
                eprintln!("Synthetic page-text harness deadline expired.");
                std::process::exit(1);
            }
        });
    let Ok(watchdog) = watchdog else {
        eprintln!("Synthetic page-text harness could not start its deadline.");
        return ExitCode::FAILURE;
    };
    let result = run(&options);
    if cancel.send(()).is_err() || watchdog.join().is_err() {
        eprintln!("Synthetic harness deadline cleanup failed.");
        return ExitCode::FAILURE;
    }
    match result {
        Ok(results) => report(&results),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Command-line options: `--race-runs N` and `--only F1,F2,...`.
pub struct Options {
    pub race_runs: u32,
    only: Option<Vec<String>>,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self {
            race_runs: 500,
            only: None,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--race-runs" => {
                    options.race_runs = arguments
                        .next()
                        .and_then(|value| value.parse().ok())
                        .ok_or("--race-runs needs a number")?;
                }
                "--only" => {
                    let cases = arguments.next().ok_or("--only needs a case list")?;
                    options.only = Some(cases.split(',').map(str::to_owned).collect());
                }
                _ => {
                    return Err(format!(
                        "Unknown option {argument}; use --race-runs or --only."
                    ));
                }
            }
        }
        Ok(options)
    }

    pub fn wants(&self, case: &str) -> bool {
        self.only
            .as_ref()
            .is_none_or(|cases| cases.iter().any(|wanted| wanted == case))
    }
}

fn run(options: &Options) -> Result<Vec<CaseResult>, String> {
    let server = Server::start()?;
    show_sites(&server.sites);
    let results = cases::run_all(options, &server);
    combine(Ok(()), server.stop())?;
    results
}

fn show_sites(sites: &Sites) {
    for role in Role::ALL {
        println!("site {:<6} {}", role.name(), sites.base(role));
    }
}

fn report(results: &[CaseResult]) -> ExitCode {
    println!("\n| Case | Verdict | Detail |\n|---|---|---|");
    for result in results {
        println!(
            "| {} | {:?} | {} |",
            result.case, result.verdict, result.detail
        );
    }
    if results.iter().any(|result| result.verdict == Verdict::Fail) {
        eprintln!("At least one synthetic page-text case failed.");
        return ExitCode::FAILURE;
    }
    println!("No synthetic page-text case failed.");
    ExitCode::SUCCESS
}

/// The policy every run uses: the synthetic bank and payment hosts in the banking list,
/// and the owner and probe hosts in the owner's list.
pub fn synthetic_policy(settle: Duration) -> Result<PageTextPolicy, String> {
    PageTextPolicy::new(&[Role::Owner.host(), Role::Probe.host()])
        .and_then(|policy| policy.with_banking(&[Role::Bank.host(), Role::Pay.host()]))
        .map(|policy| policy.with_settle_delay(settle))
        .map_err(|error| error.to_string())
}

fn reserve_profile() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Synthetic fixture clock is unavailable.".to_owned())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!(
        "atlas-page-text-harness-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&profile)
        .map_err(|error| server::io_error("reserve synthetic profile", error))?;
    Ok(profile)
}

fn create_parent() -> Result<HWND, String> {
    let parent = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Atlas synthetic page-text harness"),
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
    .map_err(|error| format!("Create synthetic parent failed ({}).", error.code().0))?;
    // A shown parent keeps synthetic pages rendering as they would in Atlas.
    unsafe {
        let _was_visible = ShowWindow(parent, SW_SHOW);
    }
    Ok(parent)
}

fn destroy_parent(parent: HWND) -> Result<(), String> {
    unsafe { DestroyWindow(parent) }
        .map_err(|error| format!("Destroy synthetic parent failed ({}).", error.code().0))
}

fn pump_messages() -> Result<(), String> {
    let mut message = MSG::default();
    for _ in 0..256 {
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            return Ok(());
        }
        if message.message == WM_QUIT {
            return Err("Synthetic parent received an unexpected quit.".into());
        }
        unsafe {
            let _translated = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

pub fn combine(primary: Result<(), String>, cleanup: Result<(), String>) -> Result<(), String> {
    match (primary, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(primary), Err(cleanup)) => Err(format!("{primary} Cleanup also failed: {cleanup}")),
    }
}
