//! Keeps page WebView2 objects on one Windows thread while Rust UI code exchanges owned commands.

mod clear_data;
mod environment;
mod essentials;
mod overlay;
pub use essentials::PageAction;
pub const RENDER_PROCESS_UNRESPONSIVE: i32 = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE.0;
mod hidden_timer;
mod lifecycle;
mod page;
mod page_text;
mod placement;
pub use lifecycle::CallbackTicket;
mod permissions;
mod popups;
mod recovery;
mod security;
pub use security::{ConnectionScheme, ConnectionState, ConnectionStatus};

pub use syrinx_core::{Ineligible, PageTextPolicy};

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
    time::Duration,
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

pub use clear_data::{BrowsingDataKinds, ClearOutcome, ClearSpan};
pub use overlay::{
    OverlayMonitor, OverlayPointer, OverlayRect, activate_if_foreground, hide_overlay,
    overlay_monitor, overlay_pointer, show_overlay, syrinx_in_foreground,
};
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
    Fullscreen,
    Find,
    /// F3 or Ctrl G: the next match in Syrinx's find bar, opening it if closed (#82).
    FindNext,
    /// Shift F3: the previous match in Syrinx's find bar (#82).
    FindPrevious,
    /// Ctrl H: show or hide Syrinx's history list.
    History,
    Downloads,
    Print,
    ZoomIn,
    ZoomOut,
    ZoomReset,
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
    /// F6, or Tab past a page's last element: move to Syrinx's next pane (P1-36).
    FocusNextPane,
    /// Shift F6 from a focused page: move to Syrinx's previous pane, at its first control.
    FocusPreviousPane,
    /// Shift Tab past a page's first element: the previous pane, at its last control.
    LeavePageBackward,
}

pub enum BrowserCommand {
    ExitFullscreen {
        id: u64,
    },
    PageAction {
        id: u64,
        action: PageAction,
    },
    ResolvePermission {
        id: u64,
        request: u64,
        allowed: bool,
    },
    Create {
        id: u64,
        url: String,
    },
    AdoptPopup {
        request: u64,
        id: u64,
    },
    CancelPopup {
        request: u64,
    },
    Navigate {
        id: u64,
        url: String,
    },
    Close {
        id: u64,
    },
    ShowActive(Option<u64>),
    Resize(PageBounds),
    Back {
        id: u64,
    },
    Forward {
        id: u64,
    },
    Reload {
        id: u64,
    },
    Focus {
        id: u64,
    },
    /// Clears WebView2's own data for the shared profile (P1-27); `span: None` is all time.
    ClearData {
        request: u64,
        kinds: BrowsingDataKinds,
        span: Option<ClearSpan>,
    },
    /// Switches page-text capture on or off (P1-25). Only pages created while it is on are
    /// observed; switching on later starts clean only for new tabs. With `None`, no new call
    /// is made and a snapshot still in flight is dropped unread, but one already queued or
    /// being parsed can still report `Captured`, so the receiver re-checks the setting.
    /// Pages already observed keep tracking excluded-site taint with the last lists.
    SetPageTextPolicy(Option<PageTextPolicy>),
    Shutdown,
}

/// What became of one page's scheduled text capture (P1-25).
pub enum PageTextOutcome {
    /// Judged ineligible when it fell due; nothing was read.
    Ineligible(Ineligible),
    /// The tab navigated before the capture fell due.
    Cancelled,
    /// The one DevTools call was made for this address.
    Requested {
        address: String,
    },
    /// The tab changed, or capture was switched off, while the snapshot was in flight; it
    /// was dropped unread.
    Superseded,
    /// The web view does not show the judged document (a navigation that never committed,
    /// such as a 204), so no call was made.
    NotCommitted,
    /// The snapshot was read and then thrown away.
    Discarded(DiscardReason),
    Captured(CapturedText),
    /// WebView2 refused or failed the call.
    Failed,
}

/// Why a snapshot was thrown away after the call (steps 9 to 11).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardReason {
    /// Step 9: the snapshot's root document is not the judged address.
    AddressMismatch,
    /// Step 10: the document has a password or payment-card field.
    SignInOrPayment,
    /// The response exceeded the parse bound.
    TooLarge,
    /// Another snapshot was parsing and one waiting, so this one was skipped.
    Busy,
    /// The response did not have the documented shape.
    Malformed,
}

/// The kept text of one document, with the measurements the feasibility harness reports.
pub struct CapturedText {
    pub address: String,
    pub text: String,
    /// Documents in the response, including child frames that were not read.
    pub documents: usize,
    pub truncated: bool,
    pub response_bytes: usize,
    pub call_time: Duration,
    pub parse_time: Duration,
}

impl fmt::Debug for BrowserCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserCommand { content: redacted }")
    }
}

pub enum BrowserEvent {
    PageCrashed {
        id: u64,
    },
    PageRestored {
        id: u64,
    },
    RecoveryStarted {
        sequence: u64,
        tabs: Vec<u64>,
    },
    RecoveryFinished {
        sequence: u64,
        restored: Vec<u64>,
        failed: Vec<u64>,
        stopped: bool,
    },
    /// Unwrap with into_current before handling a native callback.
    Callback {
        ticket: CallbackTicket,
        event: Box<BrowserEvent>,
    },
    Fullscreen {
        id: u64,
        active: bool,
    },
    Suspended {
        id: u64,
        suspended: bool,
    },
    Connection {
        id: u64,
        state: ConnectionState,
    },
    /// A find's latest result: when its search completes, and again whenever the active
    /// match or the count changes. `active` is WebView2's `ActiveMatchIndex`, -1 for none.
    FindResult {
        id: u64,
        request: u64,
        count: i32,
        active: i32,
    },
    DownloadDialog {
        id: u64,
        open: bool,
    },
    ZoomChanged {
        id: u64,
        factor: f64,
    },
    PermissionRequested {
        id: u64,
        request: u64,
        uri: String,
        kind: syrinx_store::PermissionKind,
    },
    PermissionFinished {
        id: u64,
        request: u64,
    },
    Ready,
    /// The runtime version, sent once after `Ready`; `None` when WebView2 cannot report it.
    RuntimeVersion(Option<String>),
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
        /// The navigation failed only because it was stopped or replaced (WebView2's
        /// `OperationCanceled`), which is not a failure to count (AN-01).
        cancelled: bool,
    },
    ProcessFailed {
        id: u64,
        kind: i32,
    },
    Shortcut(BrowserShortcut),
    /// The page took keyboard focus (WebView2 `GotFocus`).
    PageFocused {
        id: u64,
    },
    PopupRequested {
        id: u64,
        request: u64,
        url: String,
    },
    PopupBlocked {
        id: u64,
    },
    /// The outcome of one `ClearData` request.
    DataCleared {
        request: u64,
        outcome: ClearOutcome,
    },
    /// A page-text capture's progress; only sent while capture is switched on.
    PageText {
        id: u64,
        outcome: PageTextOutcome,
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
    Popup,
    Permission,
    PageControl,
    Security,
    Settings,
    EventRegistration,
    Navigation,
    Recovery,
    Suspension,
    Visibility,
    Bounds,
    Focus,
    Metadata,
    OverlayOwner,
    /// Windows refused to move, size or show a kept overlay popup.
    OverlayPlacement,
    /// WebView2 refused a data clear; `code` is its HRESULT.
    ClearData {
        code: i32,
    },
    /// Page-text observation or its DevTools call failed; the page is not read.
    PageText,
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
            .name("syrinx-pages".into())
            .spawn(move || {
                let completion = HostCompletion {
                    stopped: worker_stopped,
                    events: EventSink(events, None),
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
struct EventSink(
    UnboundedSender<BrowserEvent>,
    Option<lifecycle::CallbackLife>,
);

impl BrowserEvent {
    pub fn into_current(self) -> Option<Self> {
        match self {
            Self::Callback { ticket, event } => ticket.current().then_some(*event),
            event => Some(event),
        }
    }
}

impl EventSink {
    fn for_page(&self, life: lifecycle::CallbackLife) -> Self {
        Self(self.0.clone(), Some(life))
    }
    fn accepts_callbacks(&self) -> bool {
        self.1
            .as_ref()
            .is_none_or(|life| life.ticket(false).current())
    }
    fn prepare(&self, event: BrowserEvent) -> Option<BrowserEvent> {
        let event = if let Some(life) = &self.1 {
            let lifetime_only = matches!(
                event,
                BrowserEvent::ProcessFailed { .. }
                    | BrowserEvent::Title { .. }
                    | BrowserEvent::Url { .. }
                    | BrowserEvent::Loading { .. }
                    | BrowserEvent::NavigationCompleted { .. }
                    | BrowserEvent::Fullscreen { .. }
                    | BrowserEvent::Suspended { .. }
                    | BrowserEvent::PermissionFinished { .. }
                    | BrowserEvent::PageText { .. }
                    | BrowserEvent::Error { .. }
            );
            let ticket = life.ticket(lifetime_only);
            if !ticket.current() {
                return None;
            }
            BrowserEvent::Callback {
                ticket,
                event: Box::new(event),
            }
        } else {
            event
        };
        Some(event)
    }
    fn emit(&self, event: BrowserEvent) {
        let Some(event) = self.prepare(event) else {
            return;
        };
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
struct NativeSink {
    send: Sender<BrowserEvent>,
    thread_id: u32,
    peek_generation: Arc<AtomicU64>,
}

impl NativeSink {
    fn post(&self, event: BrowserEvent, events: &EventSink) -> Result<(), BrowserError> {
        let Some(event) = events.prepare(event) else {
            return Ok(());
        };
        self.send
            .send(event)
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
        popups: popups::PopupRequests::default(),
        environment: Some(environment),
        profile,
        unavailable: HashMap::new(),
        active: None,
        recovery_sequence: 0,
        recovery: recovery::RecoveryBudget::default(),
        bounds: PageBounds::default(),
        parent,
        page_text: page_text::TextContext::default(),
        events: events.clone(),
        shortcuts: NativeSink {
            send: shortcut_send,
            thread_id,
            peek_generation,
        },
    };
    events.emit(BrowserEvent::Ready);
    events.emit(BrowserEvent::RuntimeVersion(
        state
            .environment
            .as_ref()
            .and_then(environment::runtime_version),
    ));

    let mut timer = lifecycle::WakeTimer::default();
    loop {
        // Creation pumps messages internally; drain commands before blocking again.
        if !drain_commands(&incoming, events, |command| {
            drain_native(&shortcuts, &mut state)?;
            state.apply(command)
        }) {
            return Ok(());
        }
        // Handle native callbacks even when there are no commands to drain.
        drain_native(&shortcuts, &mut state)?;
        // Rebuilding can consume wake messages while leaving native callbacks queued.
        if state.recovery_tick(std::time::Instant::now()) {
            continue;
        }
        for (id, page) in &state.pages {
            if let Err(error) = page.tick() {
                events.emit(BrowserEvent::Error {
                    tab: Some(*id),
                    error,
                });
            }
        }
        timer.arm(
            state
                .pages
                .values()
                .flat_map(|page| [page.suspend_due(), page.text_due()])
                .flatten()
                .chain(state.recovery.due)
                .min(),
        )?;
        let mut message = MSG::default();
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(BrowserError::MessageLoop),
            0 => return Ok(()),
            _ => dispatch(message),
        }
    }
}

fn drain_native(
    incoming: &Receiver<BrowserEvent>,
    state: &mut BrowserState,
) -> Result<(), BrowserError> {
    loop {
        match incoming.try_recv() {
            Ok(event) => state.native_event(event),
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => return Err(BrowserError::QueueClosed),
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
            BrowserCommand::Create { id, .. } | BrowserCommand::AdoptPopup { id, .. } => Some(*id),
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
    popups: popups::PopupRequests,
    environment: Option<ICoreWebView2Environment>,
    profile: PathBuf,
    unavailable: HashMap<u64, recovery::Unavailable>,
    active: Option<u64>,
    recovery_sequence: u64,
    recovery: recovery::RecoveryBudget,
    bounds: PageBounds,
    parent: isize,
    page_text: page_text::TextContext,
    events: EventSink,
    shortcuts: NativeSink,
}

impl BrowserState {
    fn new_page(&self, id: u64) -> Result<Page, BrowserError> {
        self.new_page_with_taint(id, false)
    }

    /// A new page; an adopted popup inherits its opener's page-text taint.
    fn new_page_with_taint(&self, id: u64, starts_tainted: bool) -> Result<Page, BrowserError> {
        if id == 0 {
            return Err(BrowserError::InvalidTab);
        }
        if self.pages.contains_key(&id) || self.unavailable.contains_key(&id) {
            return Err(BrowserError::DuplicateTab);
        }
        self.build_page(id, starts_tainted)
    }

    fn build_page(&self, id: u64, starts_tainted: bool) -> Result<Page, BrowserError> {
        Page::new(
            id,
            self.parent,
            self.environment.as_ref().ok_or(BrowserError::Environment)?,
            self.bounds,
            page::PageContext {
                events: self.events.clone(),
                shortcuts: self.shortcuts.clone(),
                popups: self.popups.clone(),
                text: self.page_text.clone(),
                starts_tainted,
            },
        )
    }

    fn apply(&mut self, command: BrowserCommand) -> Result<(), BrowserError> {
        match command {
            BrowserCommand::Create { id, url } => {
                self.ensure_environment()?;
                let page = self.new_page(id)?;
                page.navigate(&url)?;
                self.pages.insert(id, page);
            }
            BrowserCommand::AdoptPopup { request, id } => {
                let pending = self.popups.take(request).ok_or(BrowserError::Popup)?;
                // Every page uses this environment and the default profile; do not navigate the target.
                let starts_tainted = self
                    .pages
                    .get(&pending.opener)
                    .ok_or(BrowserError::UnknownTab)?
                    .popup_tainted();
                let page = self.new_page_with_taint(id, starts_tainted)?;
                pending.adopt(&page.webview())?;
                self.pages.insert(id, page);
            }
            BrowserCommand::CancelPopup { request } => {
                drop(self.popups.take(request));
            }
            BrowserCommand::Close { id } => {
                self.popups.cancel_opener(id);
                let page = self.pages.remove(&id);
                let unavailable = self.forget_unavailable(id);
                if page.is_none() && unavailable.is_none() {
                    return Err(BrowserError::UnknownTab);
                }
                if self.active == Some(id) {
                    self.active = None;
                }
            }
            BrowserCommand::ShowActive(active) => {
                if active.is_some_and(|id| {
                    !self.pages.contains_key(&id) && !self.unavailable.contains_key(&id)
                }) {
                    return Err(BrowserError::UnknownTab);
                }
                show_active(
                    self.pages.keys().copied(),
                    active,
                    &mut self.active,
                    |id, visible| {
                        self.pages
                            .get(&id)
                            .map_or(Ok(()), |page| page.show(visible, self.bounds))
                    },
                )?;
            }
            BrowserCommand::ClearData {
                request,
                kinds,
                span,
            } => {
                // The outcome always reaches the requester, never as an unrelated page error.
                let outcome = clear_outcome(self.environment.as_ref(), self.parent, kinds, span);
                self.events
                    .emit(BrowserEvent::DataCleared { request, outcome });
            }
            BrowserCommand::SetPageTextPolicy(policy) => {
                self.page_text.set_policy(policy);
            }
            BrowserCommand::Resize(next) => {
                next.validate()?;
                for page in self.pages.values() {
                    page.resize(next)?;
                }
                self.bounds = next;
            }
            BrowserCommand::Reload { id } if self.unavailable.contains_key(&id) => {
                self.retry_unavailable(id)?;
            }
            BrowserCommand::Navigate { id, url } if self.unavailable.contains_key(&id) => {
                page::validate_url(&url)?;
                // The owner's own navigation: Syrinx chose the address, so no taint carries.
                self.unavailable
                    .insert(id, recovery::Unavailable::chosen(url));
                self.retry_unavailable(id)?;
            }
            BrowserCommand::Focus { id } if self.unavailable.contains_key(&id) => {}
            command => {
                let id = command.tab_id().ok_or(BrowserError::InvalidTab)?;
                let page = self.pages.get(&id).ok_or(BrowserError::UnknownTab)?;
                page.apply(command)?;
            }
        }
        Ok(())
    }
}

/// Clears through the shared environment. While P1-16 rebuilds the engine there is none, and
/// the owner is told to clear again once it is back.
fn clear_outcome(
    environment: Option<&ICoreWebView2Environment>,
    parent: isize,
    kinds: BrowsingDataKinds,
    span: Option<ClearSpan>,
) -> ClearOutcome {
    let Some(environment) = environment else {
        return ClearOutcome {
            cleared: Err(BrowserError::Recovery),
            closed: Ok(()),
        };
    };
    clear_data::clear(environment, parent, kinds, span)
}

#[cfg(test)]
mod clear_tests {
    use super::*;

    #[test]
    fn a_clear_during_engine_recovery_reports_recovery_without_touching_a_window() {
        let kinds = BrowsingDataKinds {
            history: true,
            cookies_and_site_data: true,
            cached_files: true,
        };
        for span in [None, Some(ClearSpan { start: 0, end: 60 })] {
            let outcome = clear_outcome(None, 0, kinds, span);
            assert_eq!(outcome.cleared, Err(BrowserError::Recovery));
            assert_eq!(outcome.closed, Ok(()));
        }
    }
}

impl Drop for BrowserState {
    fn drop(&mut self) {
        // Complete pending requests while their openers and the COM apartment still exist.
        self.popups.clear();
    }
}

// Resume the selected page before hiding anything, so a failed resume preserves the old view.
fn show_active(
    ids: impl Iterator<Item = u64>,
    active: Option<u64>,
    selection: &mut Option<u64>,
    mut show: impl FnMut(u64, bool) -> Result<(), BrowserError>,
) -> Result<(), BrowserError> {
    // Recovery must use the requested selection even if its controller cannot resume.
    *selection = active;
    if let Some(id) = active {
        show(id, true)?;
    }
    let mut failure = None;
    for id in ids.filter(|id| Some(*id) != active) {
        if let Err(error) = show(id, false) {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn resume_failure_leaves_previous_page_visible_and_reports_failure() {
        let mut visible = HashMap::from([(7, true), (8, false), (9, false)]);
        let mut selection = Some(7);
        let result = show_active(
            [7, 8, 9].into_iter(),
            Some(8),
            &mut selection,
            |id, show| {
                if id == 8 && show {
                    return Err(BrowserError::Suspension);
                }
                visible.insert(id, show);
                Ok(())
            },
        );
        assert_eq!(result, Err(BrowserError::Suspension));
        assert_eq!(
            selection,
            Some(8),
            "recovery must not retain the old active ID"
        );
        assert_eq!(visible, HashMap::from([(7, true), (8, false), (9, false)]));
        show_active(
            [7, 8, 9].into_iter(),
            Some(8),
            &mut selection,
            |id, show| {
                visible.insert(id, show);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(visible, HashMap::from([(7, false), (8, true), (9, false)]));
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
            Self::ExitFullscreen { id }
            | Self::PageAction { id, .. }
            | Self::ResolvePermission { id, .. }
            | Self::AdoptPopup { id, .. }
            | Self::Create { id, .. }
            | Self::Navigate { id, .. }
            | Self::Close { id }
            | Self::Back { id }
            | Self::Forward { id }
            | Self::Reload { id }
            | Self::Focus { id } => Some(*id),
            Self::ShowActive(id) => *id,
            Self::CancelPopup { .. }
            | Self::Resize(_)
            | Self::ClearData { .. }
            | Self::SetPageTextPolicy(_)
            | Self::Shutdown => None,
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

/// Return keyboard focus from the native page child to its Syrinx chrome window.
pub fn focus_chrome(parent: isize) -> Result<(), BrowserError> {
    validate_window(parent)?;
    unsafe {
        windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(HWND(parent as *mut _)))
            .map_err(|_| BrowserError::Focus)?;
    }
    Ok(())
}

/// Whether Windows keyboard focus is on the Syrinx chrome window itself, not a page child.
/// WebView2 can raise `GotFocus` while the keys never leave the chrome (#57).
/// `GetFocus` reads the calling thread's input state: call this on the chrome's own thread.
pub fn chrome_has_keyboard_focus(parent: isize) -> bool {
    if parent == 0 {
        return false;
    }
    let focused = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
    focused == HWND(parent as *mut _)
}

/// Keep GPUI's separate overlay above Syrinx, without making it topmost over other apps.
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
        assert!(!drain_commands(
            &incoming,
            &EventSink(events, None),
            |command| {
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
            }
        ));
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
        assert!(drain_commands(
            &incoming,
            &EventSink(events, None),
            |command| {
                applied += 1;
                match command {
                    BrowserCommand::Create { .. } => Err(BrowserError::CreatePage),
                    BrowserCommand::Navigate { .. } => Err(BrowserError::UnknownTab),
                    _ => panic!("unexpected synthetic command"),
                }
            }
        ));
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
    fn popup_acknowledgment_follows_native_adoption_and_reports_failure() {
        for result in [Ok(()), Err(BrowserError::Popup)] {
            let (commands, incoming) = mpsc::channel();
            let (events, mut received) = unbounded();
            commands
                .send(BrowserCommand::AdoptPopup { request: 41, id: 8 })
                .expect("queue synthetic popup adoption");
            assert!(drain_commands(
                &incoming,
                &EventSink(events, None),
                |command| {
                    assert!(matches!(
                        command,
                        BrowserCommand::AdoptPopup { request: 41, id: 8 }
                    ));
                    assert!(received.try_recv().is_err(), "no premature acknowledgment");
                    result
                }
            ));
            match result {
                Ok(()) => assert!(matches!(
                    received.try_recv(),
                    Ok(BrowserEvent::Created { id: 8 })
                )),
                Err(_) => assert!(matches!(
                    received.try_recv(),
                    Ok(BrowserEvent::CreationFailed {
                        id: 8,
                        error: BrowserError::Popup,
                    })
                )),
            }
            assert!(received.try_recv().is_err());
        }
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
        assert!(!drain_commands(
            &incoming,
            &EventSink(events, None),
            |command| {
                assert!(matches!(command, BrowserCommand::Close { id: 9 }));
                closed = true;
                Ok(())
            }
        ));
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
                BrowserEvent::PopupRequested {
                    id: 1,
                    request: 41,
                    url: sentinel.into()
                }
            ),
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
