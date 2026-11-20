//! Drives the ordinary page-host protocol against fresh profiles and bounded native events.

use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use super::trace::Trace;

use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use syrinx_core::{HistoryDirection, HistoryObserved, NavigationList};
use syrinx_platform::{
    acceptance::FixtureWindow,
    browser::{BrowserCommand, BrowserError, BrowserEvent, BrowserHost, PageBounds},
};

pub const LIMIT: Duration = Duration::from_secs(15);

pub struct Fixture {
    pub parent: FixtureWindow,
    host: Option<BrowserHost>,
    events: UnboundedReceiver<BrowserEvent>,
    pub list: NavigationList,
    trace: Trace,
}

impl Fixture {
    pub fn new(profile: PathBuf) -> Result<Self, String> {
        let parent = FixtureWindow::new()?;
        let (host, events) =
            BrowserHost::spawn(parent.handle(), profile).map_err(|error| error.to_string())?;
        let mut fixture = Self {
            parent,
            host: Some(host),
            events,
            list: NavigationList::default(),
            trace: Trace::default(),
        };
        fixture.wait("host ready", |event, _| {
            matches!(event, BrowserEvent::Ready)
        })?;
        fixture.send(BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 900,
            height: 700,
        }))?;
        Ok(fixture)
    }

    pub fn send(&mut self, command: BrowserCommand) -> Result<(), String> {
        self.trace.command(&command);
        self.host
            .as_ref()
            .ok_or("Host already stopped")?
            .send(command)
            .map_err(|error| error.to_string())
    }

    pub fn create(&mut self, id: u64, url: String) -> Result<(), String> {
        self.send(BrowserCommand::Create { id, url })?;
        self.wait(
            "page created",
            |event, _| matches!(event, BrowserEvent::Created { id: actual } if *actual == id),
        )?;
        self.send(BrowserCommand::ShowActive(Some(id)))
    }

    /// Apply FIFO commands before inspecting native children; this invalid tab is a barrier.
    pub fn barrier(&mut self) -> Result<(), String> {
        self.barrier_checked(|_| Ok(()))
    }

    /// Retain suite invariants while draining a FIFO command acknowledgement.
    pub fn barrier_checked(
        &mut self,
        mut inspect: impl FnMut(&BrowserEvent) -> Result<(), String>,
    ) -> Result<(), String> {
        self.send(BrowserCommand::Reload { id: u64::MAX })?;
        self.wait_checked("command barrier", |event, _| {
            inspect(event)?;
            Ok(matches!(
                event,
                BrowserEvent::Error {
                    tab: Some(u64::MAX),
                    error: BrowserError::UnknownTab
                }
            ))
        })
    }

    pub fn title(&mut self, expected: &str) -> Result<(), String> {
        self.wait(
            "authored document title",
            |event, _| matches!(event, BrowserEvent::Title { title, .. } if title == expected),
        )
    }

    pub fn at(&mut self, step: &str, url: &str) -> Result<(), String> {
        println!("History step: {step}");
        if self.list.current_address() != Some(url) {
            self.wait_expected(step, Some(url), |_, list| {
                list.current_address() == Some(url)
            })?;
        }
        self.trace.reached(step, url, &self.list);
        Ok(())
    }

    pub fn expect_rows(
        &self,
        step: &str,
        direction: HistoryDirection,
        expected: &[String],
    ) -> Result<(), String> {
        self.trace
            .expect_rows(step, direction, expected, &self.list)
    }

    pub fn start_jump(
        &mut self,
        step: &str,
        direction: HistoryDirection,
        steps: usize,
    ) -> Result<(), String> {
        let actual = self.list.start_jump(direction, steps);
        self.trace
            .expect_jump(step, direction, steps, actual, &self.list)
    }

    pub fn wait(
        &mut self,
        reason: &str,
        done: impl FnMut(&BrowserEvent, &NavigationList) -> bool,
    ) -> Result<(), String> {
        let mut done = done;
        self.wait_checked(reason, |event, list| Ok(done(event, list)))
    }

    fn wait_expected(
        &mut self,
        reason: &str,
        expected: Option<&str>,
        mut done: impl FnMut(&BrowserEvent, &NavigationList) -> bool,
    ) -> Result<(), String> {
        self.observe(reason, expected, |event, list| Ok(done(event, list)))
    }

    pub fn wait_checked(
        &mut self,
        reason: &str,
        done: impl FnMut(&BrowserEvent, &NavigationList) -> Result<bool, String>,
    ) -> Result<(), String> {
        self.observe(reason, None, done)
    }

    fn observe(
        &mut self,
        reason: &str,
        expected: Option<&str>,
        done: impl FnMut(&BrowserEvent, &NavigationList) -> Result<bool, String>,
    ) -> Result<(), String> {
        if self.observe_for(reason, LIMIT, done)? {
            return Ok(());
        }
        Err(self.trace.timeout(reason, expected, &self.list))
    }

    /// Observe a bounded quiet interval while retaining every ordinary host error check.
    pub fn watch_for(
        &mut self,
        reason: &str,
        duration: Duration,
        mut inspect: impl FnMut(&BrowserEvent) -> Result<(), String>,
    ) -> Result<(), String> {
        self.observe_for(reason, duration, |event, _| {
            inspect(event)?;
            Ok(false)
        })?;
        Ok(())
    }

    fn observe_for(
        &mut self,
        reason: &str,
        duration: Duration,
        mut done: impl FnMut(&BrowserEvent, &NavigationList) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            let event = match self.events.try_recv() {
                Ok(event) => event.into_current(),
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(TryRecvError::Closed) => {
                    return Err(format!("Host disconnected waiting for {reason}"));
                }
            };
            let Some(event) = event else {
                continue;
            };
            self.trace.record(&event);
            if let BrowserEvent::History { report, .. } = &event
                && self.list.observe(report) == HistoryObserved::Reset
            {
                println!("Shared navigation list reset after {:?}", report.change);
            }
            if let BrowserEvent::Title { title, .. } = &event {
                self.list.retitle(title);
            }
            if done(&event, &self.list)? {
                return Ok(true);
            }
            match event {
                BrowserEvent::Error { error, .. } | BrowserEvent::CreationFailed { error, .. } => {
                    return Err(error.to_string());
                }
                BrowserEvent::ProcessFailed { .. } => return Err("WebKit process failed".into()),
                BrowserEvent::PermissionRequested { id, request, .. } => {
                    self.send(BrowserCommand::ResolvePermission {
                        id,
                        request,
                        allowed: false,
                    })?;
                    return Err("Unexpected permission request was denied".into());
                }
                _ => {}
            }
        }
        Ok(false)
    }

    /// Wait for the actual document load before issuing focus-sensitive commands.
    pub fn loaded(&mut self, id: u64) -> Result<(), String> {
        self.wait_checked("successful document load", |event, _| {
            let BrowserEvent::NavigationCompleted {
                id: actual,
                success,
                cancelled,
            } = event
            else {
                return Ok(false);
            };
            if *actual != id {
                return Ok(false);
            }
            if !success && !cancelled {
                return Err("Synthetic navigation failed".into());
            }
            Ok(*success && !cancelled)
        })
    }

    pub fn focus_page(&mut self, id: u64) -> Result<(), String> {
        // These callers have one page; popup adoption collects its focus report separately.
        self.parent.activate()?;
        self.send(BrowserCommand::Focus { id })?;
        self.barrier()?;
        self.parent.wait_page_focus(self.only_child()?)
    }

    pub fn only_child(&self) -> Result<isize, String> {
        match self.parent.children()?.as_slice() {
            [child] => Ok(*child),
            _ => Err("Expected exactly one native fixture child".into()),
        }
    }

    pub fn stop(&mut self, bound: Duration) -> Result<(), String> {
        self.stop_checked(bound, |_| Ok(()))
    }

    /// Retain suite invariants through the final Stopped event while draining shutdown callbacks.
    pub fn stop_checked(
        &mut self,
        bound: Duration,
        mut inspect: impl FnMut(&BrowserEvent) -> Result<(), String>,
    ) -> Result<(), String> {
        let host = self.host.take().ok_or("Host already stopped")?;
        host.shutdown_within(bound)
            .map_err(|error| error.to_string())?;
        self.wait_checked("Stopped", |event, _| {
            inspect(event)?;
            Ok(matches!(event, BrowserEvent::Stopped))
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(host) = self.host.take()
            && let Err(error) = host.shutdown_within(Duration::from_secs(5))
        {
            eprintln!("Synthetic host cleanup failed: {error}");
        }
    }
}

pub fn reserve_profile() -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let profile = std::env::temp_dir().join(format!(
        "syrinx-linux-fixture-{}-{stamp}",
        std::process::id()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

/// A separate deadline catches a stalled GTK call, not merely a quiet event receiver.
pub fn run(job: impl FnOnce() -> Result<(), String>) -> std::process::ExitCode {
    let (cancel, receive) = std::sync::mpsc::channel();
    let watchdog = thread::spawn(
        move || match receive.recv_timeout(Duration::from_secs(180)) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("Native fixture watchdog: {error}");
                std::process::exit(1);
            }
        },
    );
    let result = job();
    let cancelled = cancel.send(());
    let joined = watchdog.join();
    if cancelled.is_err() || joined.is_err() {
        eprintln!("Fixture watchdog cleanup failed");
        return std::process::ExitCode::FAILURE;
    }
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Linux fixture failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
