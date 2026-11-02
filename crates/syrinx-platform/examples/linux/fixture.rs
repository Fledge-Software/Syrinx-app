//! Drives the ordinary page-host protocol against fresh profiles and bounded native events.

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use syrinx_core::{HistoryObserved, NavigationList};
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

    pub fn send(&self, command: BrowserCommand) -> Result<(), String> {
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
        self.send(BrowserCommand::Reload { id: u64::MAX })?;
        self.wait("command barrier", |event, _| {
            matches!(
                event,
                BrowserEvent::Error {
                    tab: Some(u64::MAX),
                    error: BrowserError::UnknownTab
                }
            )
        })
    }

    pub fn title(&mut self, expected: &str) -> Result<(), String> {
        self.wait(
            "authored document title",
            |event, _| matches!(event, BrowserEvent::Title { title, .. } if title == expected),
        )
    }

    pub fn at(&mut self, url: &str) -> Result<(), String> {
        if self.list.current_address() == Some(url) {
            return Ok(());
        }
        self.wait("history address", |_, list| {
            list.current_address() == Some(url)
        })
    }

    pub fn wait(
        &mut self,
        reason: &str,
        mut done: impl FnMut(&BrowserEvent, &NavigationList) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + LIMIT;
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
            if let BrowserEvent::History { report, .. } = &event
                && self.list.observe(report) == HistoryObserved::Reset
            {
                println!("Shared navigation list reset after {:?}", report.change);
            }
            if let BrowserEvent::Title { title, .. } = &event {
                self.list.retitle(title);
            }
            if done(&event, &self.list) {
                return Ok(());
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
        Err(format!("Timed out waiting for {reason}"))
    }

    pub fn stop(&mut self, bound: Duration) -> Result<(), String> {
        let host = self.host.take().ok_or("Host already stopped")?;
        host.shutdown_within(bound)
            .map_err(|error| error.to_string())?;
        self.wait("Stopped", |event, _| matches!(event, BrowserEvent::Stopped))
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
    std::fs::create_dir(&profile).map_err(|error| error.to_string())?;
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
