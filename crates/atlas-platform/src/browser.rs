//! Keeps page WebView2 objects on one Windows thread while Rust UI code exchanges owned commands.

mod environment;
mod overlay;
mod page;
mod placement;

use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;
use windows::Win32::{
    Foundation::{ERROR_SUCCESS, GetLastError, HWND, LPARAM, SetLastError, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        DispatchMessageW, GWLP_HWNDPARENT, GetMessageW, HWND_NOTOPMOST, IsWindow, MSG, PM_NOREMOVE,
        PeekMessageW, PostQuitMessage, PostThreadMessageW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SetWindowLongPtrW, SetWindowPos, TranslateMessage, WM_APP,
    },
};

pub use overlay::{OverlayMonitor, OverlayPointer, overlay_monitor, overlay_pointer};
use page::Page;

const COMMAND_MESSAGE: u32 = WM_APP + 41;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserShortcut {
    NewTab,
    CloseTab,
    Address,
    Palette,
    NextTab,
    PreviousTab,
    ReopenTab,
    SavePage,
    Back,
    Forward,
    Reload,
    ToggleSidebar,
    DismissPeek(u64),
}

pub enum BrowserCommand {
    Create { id: u64, url: String },
    Navigate { id: u64, url: String },
    Close { id: u64 },
    ShowActive(Option<u64>),
    Resize(PageBounds),
    Back { id: u64 },
    Forward { id: u64 },
    Reload { id: u64 },
    Focus { id: u64 },
    Shutdown,
}

impl fmt::Debug for BrowserCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserCommand { content: redacted }")
    }
}

pub enum BrowserEvent {
    Ready,
    Created {
        id: u64,
    },
    /// The host must persist the logical close before releasing the native page.
    CloseRequested {
        id: u64,
    },
    CreationFailed {
        id: u64,
        error: BrowserError,
    },
    Title {
        id: u64,
        title: String,
    },
    Url {
        id: u64,
        url: String,
    },
    Loading {
        id: u64,
        loading: bool,
    },
    NavigationCompleted {
        id: u64,
        success: bool,
    },
    ProcessFailed {
        id: u64,
        kind: i32,
    },
    Shortcut(BrowserShortcut),
    PopupBlocked {
        id: u64,
    },
    Error {
        tab: Option<u64>,
        error: BrowserError,
    },
    Stopped,
}

impl fmt::Debug for BrowserEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error { tab, error } => formatter
                .debug_struct("BrowserError")
                .field("tab", tab)
                .field("error", error)
                .finish(),
            Self::CreationFailed { id, error } => formatter
                .debug_struct("CreationFailed")
                .field("id", id)
                .field("error", error)
                .finish(),
            Self::ProcessFailed { id, kind } => formatter
                .debug_struct("ProcessFailed")
                .field("id", id)
                .field("kind", kind)
                .finish(),
            _ => formatter.write_str("BrowserEvent { content: redacted }"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserError {
    InvalidWindow,
    InvalidProfile,
    EnvironmentOverride,
    ThreadStart,
    Apartment,
    Environment,
    QueueClosed,
    Wake,
    MessageLoop,
    InvalidBounds,
    InvalidUrl,
    InvalidTab,
    DuplicateTab,
    UnknownTab,
    CreatePage,
    Settings,
    EventRegistration,
    Navigation,
    Visibility,
    Bounds,
    Focus,
    Metadata,
    OverlayOwner,
    StillRunning,
    WorkerPanicked,
}

impl fmt::Display for BrowserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Browser operation failed: {self:?}")
    }
}

impl std::error::Error for BrowserError {}

pub struct BrowserHost {
    commands: Sender<BrowserCommand>,
    thread_id: u32,
    stopped: Arc<AtomicBool>,
    peek_generation: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl BrowserHost {
    /// The parent must remain alive until `Stopped`; profile must be a dedicated local directory.
    pub fn spawn(
        parent_hwnd: isize,
        profile: PathBuf,
    ) -> Result<(Self, UnboundedReceiver<BrowserEvent>), BrowserError> {
        validate_window(parent_hwnd)?;
        environment::validate_profile(&profile)?;
        let (commands, incoming) = mpsc::channel();
        let (events, outgoing) = unbounded();
        let (started, ready) = mpsc::sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let peek_generation = Arc::new(AtomicU64::new(0));
        let worker_peek = peek_generation.clone();

        // Establish the message queue before exposing its thread ID to callers.
        let worker = thread::Builder::new()
            .name("atlas-pages".into())
            .spawn(move || {
                let completion = HostCompletion {
                    stopped: worker_stopped,
                    events: EventSink(events),
                };
                let mut message = MSG::default();
                // The return only says whether a message existed; either result creates the queue.
                let _queue_had_message =
                    unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
                let thread_id = unsafe { GetCurrentThreadId() };
                if started.send(thread_id).is_err() {
                    return;
                }
                if let Err(error) = run(
                    parent_hwnd,
                    profile,
                    incoming,
                    &completion.events,
                    thread_id,
                    worker_peek,
                ) {
                    completion
                        .events
                        .emit(BrowserEvent::Error { tab: None, error });
                }
            })
            .map_err(|_| BrowserError::ThreadStart)?;
        let thread_id = ready.recv().map_err(|_| BrowserError::ThreadStart)?;
        Ok((
            Self {
                commands,
                thread_id,
                stopped,
                peek_generation,
                worker: Some(worker),
            },
            outgoing,
        ))
    }

    pub fn send(&self, command: BrowserCommand) -> Result<(), BrowserError> {
        self.commands
            .send(command)
            .map_err(|_| BrowserError::QueueClosed)?;
        unsafe { PostThreadMessageW(self.thread_id, COMMAND_MESSAGE, WPARAM(0), LPARAM(0)) }
            .map_err(|_| BrowserError::Wake)
    }

    /// Escape belongs to a live peek only; zero leaves it with the focused page.
    pub fn set_peek_generation(&self, generation: u64) {
        self.peek_generation.store(generation, Ordering::Release);
    }

    /// Join only after `Stopped`, so the GUI does not wait while WebView2 needs its parent.
    pub fn join(mut self) -> Result<(), BrowserError> {
        if !self.stopped.load(Ordering::Acquire) {
            return Err(BrowserError::StillRunning);
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| BrowserError::WorkerPanicked)?;
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

#[derive(Clone)]
struct EventSink(UnboundedSender<BrowserEvent>);

impl EventSink {
    fn emit(&self, event: BrowserEvent) {
        if self.0.unbounded_send(event).is_err() {
            // The owner UI is gone; terminate this thread instead of keeping its pages alive.
            unsafe { PostQuitMessage(0) };
        }
    }
}

struct HostCompletion {
    stopped: Arc<AtomicBool>,
    events: EventSink,
}

impl Drop for HostCompletion {
    fn drop(&mut self) {
        // The nested run scope releases pages and COM before this, including during unwinding.
        self.stopped.store(true, Ordering::Release);
        self.events.emit(BrowserEvent::Stopped);
    }
}

#[derive(Clone)]
struct ShortcutSink {
    send: Sender<BrowserShortcut>,
    thread_id: u32,
    peek_generation: Arc<AtomicU64>,
}

impl ShortcutSink {
    fn post(&self, shortcut: BrowserShortcut) -> Result<(), BrowserError> {
        self.send
            .send(shortcut)
            .map_err(|_| BrowserError::QueueClosed)?;
        unsafe { PostThreadMessageW(self.thread_id, COMMAND_MESSAGE, WPARAM(0), LPARAM(0)) }
            .map_err(|_| BrowserError::Wake)
    }
}

fn run(
    parent: isize,
    profile: PathBuf,
    incoming: Receiver<BrowserCommand>,
    events: &EventSink,
    thread_id: u32,
    peek_generation: Arc<AtomicU64>,
) -> Result<(), BrowserError> {
    let _apartment = environment::Apartment::new()?;
    let environment = environment::create(&profile)?;
    let (shortcut_send, shortcuts) = mpsc::channel();
    let mut state = BrowserState {
        pages: HashMap::new(),
        environment,
        bounds: PageBounds::default(),
        parent,
        events: events.clone(),
        shortcuts: ShortcutSink {
            send: shortcut_send,
            thread_id,
            peek_generation,
        },
    };
    events.emit(BrowserEvent::Ready);

    loop {
        // Creation pumps messages internally; drain commands before blocking again.
        if !drain_commands(&incoming, events, |command| state.apply(command)) {
            return Ok(());
        }
        // Nested controller creation can consume wake messages, but cannot consume this queue.
        loop {
            match shortcuts.try_recv() {
                Ok(shortcut) => events.emit(BrowserEvent::Shortcut(shortcut)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(BrowserError::QueueClosed),
            }
        }
        let mut message = MSG::default();
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(BrowserError::MessageLoop),
            0 => return Ok(()),
            _ => dispatch(message),
        }
    }
}

fn drain_commands(
    incoming: &Receiver<BrowserCommand>,
    events: &EventSink,
    mut apply: impl FnMut(BrowserCommand) -> Result<(), BrowserError>,
) -> bool {
    loop {
        let command = match incoming.try_recv() {
            Ok(BrowserCommand::Shutdown) | Err(TryRecvError::Disconnected) => return false,
            Err(TryRecvError::Empty) => return true,
            Ok(command) => command,
        };
        let tab = command.tab_id();
        let creating = match &command {
            BrowserCommand::Create { id, .. } => Some(*id),
            _ => None,
        };
        let event = match (creating, apply(command)) {
            (Some(id), Ok(())) => Some(BrowserEvent::Created { id }),
            (Some(id), Err(error)) => Some(BrowserEvent::CreationFailed { id, error }),
            (None, Err(error)) => Some(BrowserEvent::Error { tab, error }),
            (None, Ok(())) => None,
        };
        if let Some(event) = event {
            events.emit(event);
        }
    }
}

struct BrowserState {
    pages: HashMap<u64, Page>,
    environment: ICoreWebView2Environment,
    bounds: PageBounds,
    parent: isize,
    events: EventSink,
    shortcuts: ShortcutSink,
}

impl BrowserState {
    fn apply(&mut self, command: BrowserCommand) -> Result<(), BrowserError> {
        match command {
            BrowserCommand::Create { id, url } => {
                if id == 0 {
                    return Err(BrowserError::InvalidTab);
                }
                if self.pages.contains_key(&id) {
                    return Err(BrowserError::DuplicateTab);
                }
                let page = Page::new(
                    id,
                    self.parent,
                    &self.environment,
                    self.bounds,
                    self.events.clone(),
                    self.shortcuts.clone(),
                )?;
                page.navigate(&url)?;
                self.pages.insert(id, page);
            }
            BrowserCommand::Close { id } => {
                self.pages.remove(&id).ok_or(BrowserError::UnknownTab)?;
            }
            BrowserCommand::ShowActive(active) => {
                if active.is_some_and(|id| !self.pages.contains_key(&id)) {
                    return Err(BrowserError::UnknownTab);
                }
                for (id, page) in &self.pages {
                    page.show(Some(*id) == active, self.bounds)?;
                }
            }
            BrowserCommand::Resize(next) => {
                next.validate()?;
                for page in self.pages.values() {
                    page.resize(next)?;
                }
                self.bounds = next;
            }
            command => {
                let id = command.tab_id().ok_or(BrowserError::InvalidTab)?;
                let page = self.pages.get(&id).ok_or(BrowserError::UnknownTab)?;
                page.apply(command)?;
            }
        }
        Ok(())
    }
}

fn dispatch(message: MSG) {
    unsafe {
        // A false result means this message needed no keyboard translation, not a failure.
        let _translated = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

impl BrowserCommand {
    fn tab_id(&self) -> Option<u64> {
        match self {
            Self::Create { id, .. }
            | Self::Navigate { id, .. }
            | Self::Close { id }
            | Self::Back { id }
            | Self::Forward { id }
            | Self::Reload { id }
            | Self::Focus { id } => Some(*id),
            Self::ShowActive(id) => *id,
            Self::Resize(_) | Self::Shutdown => None,
        }
    }
}

impl PageBounds {
    fn validate(self) -> Result<(), BrowserError> {
        let width = i32::try_from(self.width).map_err(|_| BrowserError::InvalidBounds)?;
        let height = i32::try_from(self.height).map_err(|_| BrowserError::InvalidBounds)?;
        self.x
            .checked_add(width)
            .ok_or(BrowserError::InvalidBounds)?;
        self.y
            .checked_add(height)
            .ok_or(BrowserError::InvalidBounds)?;
        Ok(())
    }
}

fn validate_window(window: isize) -> Result<(), BrowserError> {
    if window == 0 || !unsafe { IsWindow(Some(HWND(window as *mut _))) }.as_bool() {
        return Err(BrowserError::InvalidWindow);
    }
    Ok(())
}

/// Keep GPUI's separate overlay above Atlas, without making it topmost over other apps.
pub fn set_overlay_owner(popup: isize, parent: isize) -> Result<(), BrowserError> {
    validate_window(popup)?;
    validate_window(parent)?;
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let previous = SetWindowLongPtrW(HWND(popup as *mut _), GWLP_HWNDPARENT, parent);
        if previous == 0 && GetLastError() != ERROR_SUCCESS {
            return Err(BrowserError::OverlayOwner);
        }
        SetWindowPos(
            HWND(popup as *mut _),
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .map_err(|_| BrowserError::OverlayOwner)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_acknowledgment_follows_apply_and_navigation_stays_in_order() {
        let (commands, incoming) = mpsc::channel();
        let (events, mut received) = unbounded();
        for command in [
            BrowserCommand::Create {
                id: 7,
                url: "https://first.example.test/".into(),
            },
            BrowserCommand::Navigate {
                id: 7,
                url: "https://second.example.test/".into(),
            },
            BrowserCommand::Shutdown,
            BrowserCommand::Reload { id: 7 },
        ] {
            commands.send(command).expect("queue synthetic command");
        }

        let mut applied = Vec::new();
        assert!(!drain_commands(&incoming, &EventSink(events), |command| {
            match command {
                BrowserCommand::Create { id: 7, url } => {
                    assert!(received.try_recv().is_err(), "no premature acknowledgment");
                    assert_eq!(url, "https://first.example.test/");
                    applied.push("create");
                }
                BrowserCommand::Navigate { id: 7, url } => {
                    assert!(matches!(
                        received.try_recv(),
                        Ok(BrowserEvent::Created { id: 7 })
                    ));
                    assert_eq!(url, "https://second.example.test/");
                    applied.push("navigate");
                }
                _ => panic!("shutdown must stop command application"),
            }
            Ok(())
        }));
        assert_eq!(applied, ["create", "navigate"]);
        assert!(matches!(
            incoming.try_recv(),
            Ok(BrowserCommand::Reload { id: 7 })
        ));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn failed_creation_is_not_acknowledged_and_later_commands_still_run() {
        let (commands, incoming) = mpsc::channel();
        let (events, mut received) = unbounded();
        commands
            .send(BrowserCommand::Create {
                id: 8,
                url: "https://failed.example.test/".into(),
            })
            .expect("queue synthetic creation");
        commands
            .send(BrowserCommand::Navigate {
                id: 8,
                url: "https://retry.example.test/".into(),
            })
            .expect("queue synthetic navigation");
        let mut applied = 0;
        assert!(drain_commands(&incoming, &EventSink(events), |command| {
            applied += 1;
            match command {
                BrowserCommand::Create { .. } => Err(BrowserError::CreatePage),
                BrowserCommand::Navigate { .. } => Err(BrowserError::UnknownTab),
                _ => panic!("unexpected synthetic command"),
            }
        }));
        assert_eq!(applied, 2);
        assert!(matches!(
            received.try_recv(),
            Ok(BrowserEvent::CreationFailed {
                id: 8,
                error: BrowserError::CreatePage,
            })
        ));
        assert!(matches!(
            received.try_recv(),
            Ok(BrowserEvent::Error {
                tab: Some(8),
                error: BrowserError::UnknownTab,
            })
        ));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn disconnect_drains_existing_commands_then_ends_the_host_loop() {
        let (commands, incoming) = mpsc::channel();
        let (events, mut received) = unbounded();
        commands
            .send(BrowserCommand::Close { id: 9 })
            .expect("queue synthetic close");
        drop(commands);
        let mut closed = false;
        assert!(!drain_commands(&incoming, &EventSink(events), |command| {
            assert!(matches!(command, BrowserCommand::Close { id: 9 }));
            closed = true;
            Ok(())
        }));
        assert!(closed);
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn physical_bounds_reject_coordinate_overflow_without_clipping() {
        for (x, y, width, height) in [
            (0, 0, 2_147_483_648, 1),
            (0, 0, 1, u32::MAX),
            (i32::MAX, 0, 1, 1),
            (0, i32::MAX, 1, 1),
        ] {
            assert_eq!(
                PageBounds {
                    x,
                    y,
                    width,
                    height
                }
                .validate(),
                Err(BrowserError::InvalidBounds)
            );
        }
        for bounds in [
            PageBounds::default(),
            PageBounds {
                x: i32::MIN,
                y: -1,
                width: 2_147_483_647,
                height: 1,
            },
            PageBounds {
                x: i32::MAX,
                y: i32::MAX,
                width: 0,
                height: 0,
            },
        ] {
            assert_eq!(bounds.validate(), Ok(()));
        }
    }

    #[test]
    fn browser_diagnostics_do_not_include_page_content() {
        let sentinel = "https://private-debug-sentinel.example.test/";
        for debug in [
            format!(
                "{:?}",
                BrowserCommand::Create {
                    id: 1,
                    url: sentinel.into()
                }
            ),
            format!(
                "{:?}",
                BrowserCommand::Navigate {
                    id: 1,
                    url: sentinel.into()
                }
            ),
            format!(
                "{:?}",
                BrowserEvent::Title {
                    id: 1,
                    title: sentinel.into()
                }
            ),
            format!(
                "{:?}",
                BrowserEvent::Url {
                    id: 1,
                    url: sentinel.into()
                }
            ),
        ] {
            assert!(!debug.contains(sentinel));
        }
    }
}
