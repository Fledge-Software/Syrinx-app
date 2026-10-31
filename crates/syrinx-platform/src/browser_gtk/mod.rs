//! Keeps page WebKitGTK views on Syrinx's GTK thread while Rust UI code exchanges owned commands.

#[path = "../browser/history_state.rs"]
mod history_state;
#[path = "../browser/keys.rs"]
mod keys;
#[path = "../browser/protocol.rs"]
mod protocol;

mod downloads;
mod engine;
mod page;
mod x11;

pub use protocol::*;
pub use syrinx_core::{Ineligible, PageTextPolicy};
pub use x11::{
    activate_if_foreground, chrome_has_keyboard_focus, focus_chrome, hide_overlay,
    holds_keyboard_focus, overlay_monitor, overlay_pointer, set_overlay_owner, show_overlay,
    syrinx_in_foreground,
};

use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::gtk_thread;
use engine::Engine;

/// The `ProcessFailed` kind for a page whose web process stopped answering, as on Windows.
pub const RENDER_PROCESS_UNRESPONSIVE: i32 = 2;
/// The `ProcessFailed` kind for a page whose web process exited or crashed.
const RENDER_PROCESS_EXITED: i32 = 1;

/// How often a shutdown wait looks at whether the GTK side has finished.
const SHUTDOWN_POLL: Duration = Duration::from_millis(10);

thread_local! {
    /// The running host's state; it lives only on the GTK thread.
    static HOST: RefCell<Option<HostState>> = const { RefCell::new(None) };
}

pub struct BrowserHost {
    commands: Sender<BrowserCommand>,
    stopped: Arc<AtomicBool>,
    peek_generation: Arc<AtomicU64>,
}

impl BrowserHost {
    /// The parent must remain alive until `Stopped`; profile must be a dedicated local directory.
    pub fn spawn(
        parent_window: isize,
        profile: PathBuf,
    ) -> Result<(Self, UnboundedReceiver<BrowserEvent>), BrowserError> {
        x11::validate_window(parent_window)?;
        validate_profile(&profile)?;
        let (commands, incoming) = mpsc::channel();
        let (events, outgoing) = unbounded();
        let stopped = Arc::new(AtomicBool::new(false));
        let peek_generation = Arc::new(AtomicU64::new(0));
        let host = HostSetup {
            parent: parent_window,
            profile,
            incoming,
            events: EventSink(events, None),
            stopped: stopped.clone(),
            peek_generation: peek_generation.clone(),
        };
        gtk_thread::run(move || start(host)).map_err(|_| BrowserError::ThreadStart)??;
        Ok((
            Self {
                commands,
                stopped,
                peek_generation,
            },
            outgoing,
        ))
    }

    pub fn send(&self, command: BrowserCommand) -> Result<(), BrowserError> {
        self.commands
            .send(command)
            .map_err(|_| BrowserError::QueueClosed)?;
        gtk_thread::post(pump).map_err(|_| BrowserError::Wake)
    }

    /// Escape belongs to a live peek only; zero leaves it with the focused page.
    pub fn set_peek_generation(&self, generation: u64) {
        self.peek_generation.store(generation, Ordering::Release);
    }

    /// Join only after `Stopped`, as on Windows; the GTK thread itself lives on.
    pub fn join(self) -> Result<(), BrowserError> {
        if !self.stopped.load(Ordering::Acquire) {
            return Err(BrowserError::StillRunning);
        }
        Ok(())
    }

    /// Stops the host when no event loop will run again, waiting at most `bound`.
    pub fn shutdown_within(self, bound: Duration) -> Result<(), BrowserError> {
        if !self.stopped.load(Ordering::Acquire)
            && let Err(error) = self.send(BrowserCommand::Shutdown)
        {
            eprintln!("Syrinx could not ask its page host to stop; waiting for it anyway: {error}");
        }
        let deadline = Instant::now() + bound;
        while !self.stopped.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return Err(BrowserError::StillRunning);
            }
            thread::sleep(SHUTDOWN_POLL);
        }
        Ok(())
    }
}

impl Drop for BrowserHost {
    fn drop(&mut self) {
        if !self.stopped.load(Ordering::Acquire)
            && let Err(error) = self.send(BrowserCommand::Shutdown)
        {
            // A dropped UI can no longer receive this structural shutdown failure.
            eprintln!("{error}");
        }
    }
}

impl EventSink {
    fn emit(&self, event: BrowserEvent) {
        let Some(event) = self.prepare(event) else {
            return;
        };
        if self.0.unbounded_send(event).is_err() {
            // The owner UI is gone; stop the host instead of keeping its pages alive.
            if gtk_thread::post(stop).is_err() {
                eprintln!("Syrinx could not stop its page host after the window closed.");
            }
        }
    }
}

/// What the GTK thread needs to start the host.
struct HostSetup {
    parent: isize,
    profile: PathBuf,
    incoming: Receiver<BrowserCommand>,
    events: EventSink,
    stopped: Arc<AtomicBool>,
    peek_generation: Arc<AtomicU64>,
}

struct HostState {
    incoming: Receiver<BrowserCommand>,
    events: EventSink,
    stopped: Arc<AtomicBool>,
    engine: Engine,
}

impl Drop for HostState {
    fn drop(&mut self) {
        // Pages close before `Stopped`, so the window may go once it arrives.
        self.engine.close_all();
        self.stopped.store(true, Ordering::Release);
        self.events.emit(BrowserEvent::Stopped);
    }
}

/// Builds the engine on the GTK thread; only one host runs per process.
fn start(setup: HostSetup) -> Result<(), BrowserError> {
    let running = HOST.with(|host| host.borrow().is_some());
    if running {
        return Err(BrowserError::ThreadStart);
    }
    let engine = Engine::new(
        setup.parent,
        &setup.profile,
        setup.events.clone(),
        setup.peek_generation,
    )?;
    setup.events.emit(BrowserEvent::Ready);
    setup.events.emit(BrowserEvent::RuntimeVersion(
        Some(engine::runtime_version()),
    ));
    HOST.with(|host| {
        *host.borrow_mut() = Some(HostState {
            incoming: setup.incoming,
            events: setup.events,
            stopped: setup.stopped,
            engine,
        });
    });
    Ok(())
}

/// Applies every queued command; a shutdown or a closed queue stops the host.
fn pump() {
    let keep = HOST.with(|host| {
        let mut host = host.borrow_mut();
        let Some(state) = host.as_mut() else {
            return true;
        };
        let HostState {
            incoming,
            events,
            engine,
            ..
        } = state;
        drain_commands(incoming, events, |command| engine.apply(command))
    });
    if !keep {
        stop();
    }
}

fn stop() {
    // Take the state out first, so page teardown never runs inside the host's borrow.
    let state = HOST.with(|host| host.borrow_mut().take());
    drop(state);
}

/// WebKit settings that would switch its sandbox off or open a remote inspector port.
const ENGINE_OVERRIDES: [&str; 3] = [
    "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
    "WEBKIT_INSPECTOR_SERVER",
    "WEBKIT_INSPECTOR_HTTP_SERVER",
];

/// The profile must be absolute; WebKit creates it if it is missing, but an existing name must
/// be a real folder. As on Windows, an environment override of the engine is refused.
fn validate_profile(profile: &std::path::Path) -> Result<(), BrowserError> {
    if !profile.is_absolute() || profile.as_os_str().is_empty() {
        return Err(BrowserError::InvalidProfile);
    }
    match std::fs::symlink_metadata(profile) {
        Ok(metadata) if !metadata.is_dir() => return Err(BrowserError::InvalidProfile),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(BrowserError::InvalidProfile);
        }
        _ => {}
    }
    if ENGINE_OVERRIDES
        .iter()
        .any(|variable| std::env::var_os(variable).is_some())
    {
        return Err(BrowserError::EnvironmentOverride);
    }
    Ok(())
}
