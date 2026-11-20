//! The owned commands and events Syrinx exchanges with its page engine, on every platform.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, SystemTime},
};

use syrinx_core::{Ineligible, PageTextPolicy};

/// How long a hidden page stays awake after its sound stops, so a pause between tracks or a
/// short buffer never puts a listening tab to sleep (#130).
pub const AUDIO_GRACE: Duration = Duration::from_secs(60);

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
    /// Ctrl , : open Syrinx's Settings page (#91).
    Settings,
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
    /// Opens a tab Syrinx puts back rather than one the owner just chose: a restored, reopened
    /// or duplicated tab (#54). Its page starts tainted and loading it is not Syrinx's own
    /// navigation, so page text never reads it until the owner navigates the tab (D35).
    Reopen {
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
    /// Steps back or forward `steps` entries in one go, as that many Back or Forward calls
    /// in a row, which commit only the last entry (#126's spike).
    GoToOffset {
        id: u64,
        direction: syrinx_core::HistoryDirection,
        steps: u32,
    },
    Reload {
        id: u64,
    },
    Focus {
        id: u64,
    },
    /// Mutes or unmutes one page through the engine's own call (WebView2's `SetIsMuted`,
    /// WebKitGTK's `set_is_muted`; #95); no script runs.
    /// A page waiting for engine recovery ignores it; the app sends it again once restored.
    SetMuted {
        id: u64,
        muted: bool,
    },
    /// Clears the engine's own data for the shared profile (P1-27); `span: None` is all time.
    /// WebKitGTK clears only from a moment until now, so it refuses an earlier closed span.
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
    /// Pauses, resumes or cancels a download from the flyout (#143). It may outlive its tab.
    ControlDownload {
        download: u64,
        control: syrinx_core::DownloadControl,
    },
    Shutdown,
}

/// What became of one page's scheduled text capture (P1-25).
pub enum PageTextOutcome {
    /// Judged ineligible when it fell due; nothing was read.
    Ineligible(Ineligible),
    /// The tab navigated before the capture fell due.
    Cancelled,
    /// The one read of the page was started for this address (on Windows, its DevTools
    /// snapshot; Linux reads no page).
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
    /// The engine refused or failed the read.
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
    /// When the page finished loading, or its address last changed through the History
    /// API: the visit this text belongs to. `None` when the clock could not say.
    pub visited_at: Option<SystemTime>,
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
    /// match or the count changes. `active` is the one-based match the engine shows, -1 for
    /// none: WebView2's `ActiveMatchIndex`, or on Linux the match Syrinx last stepped to.
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
    /// Page `tab`'s navigation became a download (#143). Download events are not tied to the
    /// page's lifetime: they keep arriving after a popup that started one has closed.
    DownloadStarted {
        download: u64,
        tab: u64,
        /// The saved file's name only; its folder and source address never leave the platform.
        file_name: String,
        total: Option<u64>,
    },
    /// A download's state or byte count changed.
    DownloadChanged {
        download: u64,
        state: syrinx_core::DownloadState,
        received: u64,
        total: Option<u64>,
        /// Whether the engine says an interrupted download can resume; always false on
        /// Linux, where WebKitGTK cannot resume one.
        can_resume: bool,
    },
    ZoomChanged {
        id: u64,
        factor: f64,
    },
    /// A page's mute or audible state changed (WebView2's `IsMuted` and
    /// `IsDocumentPlayingAudio`, WebKitGTK's `is-muted` and `is-playing-audio`).
    AudioChanged {
        id: u64,
        muted: bool,
        playing: bool,
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
    /// The engine's version (the WebView2 runtime's, or WebKitGTK's), sent once after
    /// `Ready`; `None` when the engine cannot report it.
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
    /// The page's back and forward list changed; the app keeps its own copy (#126).
    History {
        id: u64,
        report: syrinx_core::HistoryReport,
    },
    Loading {
        id: u64,
        loading: bool,
    },
    NavigationCompleted {
        id: u64,
        success: bool,
        /// The navigation failed only because it was stopped or replaced (WebView2's
        /// `OperationCanceled`, WebKit's cancelled load), which is not a failure to count
        /// (AN-01).
        cancelled: bool,
    },
    ProcessFailed {
        id: u64,
        kind: i32,
    },
    Shortcut(BrowserShortcut),
    /// The page took keyboard focus (WebView2's `GotFocus`, or GTK's focus-in on the page).
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
    /// WebView2 refused to mute or unmute a page, as a runtime without `ICoreWebView2_8` does.
    Mute,
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
    /// WebView2 refused a data clear; `code` is its HRESULT (Windows only).
    ClearDataHresult {
        code: i32,
    },
    /// WebKitGTK refused a data clear; `domain` and `code` are its GLib error's.
    ClearDataGlib {
        domain: &'static str,
        code: i32,
    },
    /// The engine cannot clear the chosen span: WebKitGTK clears only from a moment until now.
    ClearSpanUnsupported,
    /// The engine has no website-data store to clear.
    ClearDataUnavailable,
    /// No X11 display connection is open, so Syrinx cannot place or focus its windows.
    NoDisplay,
    /// The X11 connection's lock was poisoned by a panic on another thread.
    DisplayLock,
    /// Page-text observation or the read of the page failed; the page is not read.
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

impl BrowserEvent {
    pub fn into_current(self) -> Option<Self> {
        match self {
            Self::Callback { ticket, event } => ticket.current().then_some(*event),
            event => Some(event),
        }
    }
}

impl BrowserCommand {
    pub(super) fn tab_id(&self) -> Option<u64> {
        match self {
            Self::ExitFullscreen { id }
            | Self::PageAction { id, .. }
            | Self::ResolvePermission { id, .. }
            | Self::AdoptPopup { id, .. }
            | Self::Create { id, .. }
            | Self::Reopen { id, .. }
            | Self::Navigate { id, .. }
            | Self::Close { id }
            | Self::GoToOffset { id, .. }
            | Self::Reload { id }
            | Self::Focus { id }
            | Self::SetMuted { id, .. } => Some(*id),
            Self::ShowActive(id) => *id,
            Self::CancelPopup { .. }
            | Self::Resize(_)
            | Self::ClearData { .. }
            | Self::SetPageTextPolicy(_)
            | Self::ControlDownload { .. }
            | Self::Shutdown => None,
        }
    }
}

impl PageBounds {
    pub(super) fn validate(self) -> Result<(), BrowserError> {
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

pub enum PageAction {
    /// Searches the page for `text`, matching case only when `case_sensitive`; an empty
    /// `text` stops the search instead.
    Find {
        request: u64,
        text: String,
        case_sensitive: bool,
    },
    FindNext,
    FindPrevious,
    StopFind,
    Downloads,
    CloseDownloads,
    Print,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Restores a site's remembered level (#90); clamped to 25% to 500%, the range
    /// `next_zoom` steps through on both engines.
    SetZoom(f64),
    /// Points the profile's downloads at a synthetic folder. Only the native fixture sends
    /// this; Syrinx itself never changes where the engine saves files.
    SetDownloadFolder(std::path::PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionScheme {
    Http,
    Https,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Loading,
    Loaded,
    Failed,
    CertificateRejected,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionState {
    pub scheme: ConnectionScheme,
    pub status: ConnectionStatus,
}

/// The owner's choices, mapped onto the engine's data kinds; Syrinx's own history is
/// separate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowsingDataKinds {
    /// The engine's own visited-page record, beside Syrinx's history database (WebKitGTK
    /// keeps none beyond each page's back list).
    pub history: bool,
    /// Cookies plus every kind of site storage, which signs the owner out of sites.
    pub cookies_and_site_data: bool,
    /// The HTTP disk cache.
    pub cached_files: bool,
}

/// A half-open span of Unix seconds; `None` in a command means all time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClearSpan {
    pub start: i64,
    pub end: i64,
}

/// Whether the data was cleared, and separately whether the hidden view closed afterwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClearOutcome {
    pub cleared: Result<(), BrowserError>,
    pub closed: Result<(), BrowserError>,
}

/// Carries validity across the worker/UI queue without retaining a native controller.
#[derive(Clone)]
pub struct CallbackTicket {
    epoch: Arc<AtomicU64>,
    captured: u64,
    lifetime_only: bool,
}

impl CallbackTicket {
    pub(super) fn current(&self) -> bool {
        let now = self.epoch.load(Ordering::Acquire);
        now != 0 && (self.lifetime_only || now == self.captured && !now.is_multiple_of(2))
    }
}

#[derive(Clone)]
pub(super) struct CallbackLife(pub(super) Arc<AtomicU64>);

impl CallbackLife {
    pub(super) fn new() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }
    pub(super) fn ticket(&self, lifetime_only: bool) -> CallbackTicket {
        CallbackTicket {
            epoch: self.0.clone(),
            captured: self.0.load(Ordering::Acquire),
            lifetime_only,
        }
    }
    pub(super) fn retire(&self) {
        self.0.store(0, Ordering::Release);
    }
}

pub struct OverlayPointer {
    pub x: i32,
    pub y: i32,
    pub in_popup: bool,
    pub active: bool,
    pub pressed: bool,
}

/// The monitor under an overlay's physical anchor, with its own DPI and physical extent.
pub struct OverlayMonitor {
    pub id: u64,
    pub scale: f32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// An overlay popup's place on screen, in physical pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayRect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl BrowsingDataKinds {
    pub fn is_empty(self) -> bool {
        !(self.history || self.cookies_and_site_data || self.cached_files)
    }
}

pub(super) fn validate_url(url: &str) -> Result<(), BrowserError> {
    if url.contains('\0')
        || !(url.starts_with("https://") || url.starts_with("http://") || url == "about:blank")
    {
        return Err(BrowserError::InvalidUrl);
    }
    Ok(())
}

/// A download's file name only: its folder never leaves the platform layer.
pub(super) fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The zoom level after a zoom action, stepping through Chrome's levels from 25% to 500%.
pub(super) fn next_zoom(factor: f64, action: &PageAction) -> f64 {
    const LEVELS: [f64; 15] = [
        0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 5.,
    ];
    match action {
        PageAction::ZoomIn => LEVELS
            .into_iter()
            .find(|level| *level > factor + 0.001)
            .unwrap_or(5.),
        PageAction::ZoomOut => LEVELS
            .into_iter()
            .rev()
            .find(|level| *level < factor - 0.001)
            .unwrap_or(0.25),
        PageAction::SetZoom(level) if level.is_finite() => level.clamp(0.25, 5.),
        _ => 1.,
    }
}

pub(super) fn drain_commands(
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
            BrowserCommand::Create { id, .. }
            | BrowserCommand::Reopen { id, .. }
            | BrowserCommand::AdoptPopup { id, .. } => Some(*id),
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

// Resume the selected page before hiding anything, so a failed resume preserves the old view.
pub(super) fn show_active(
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

/// Where page events go, with the page's callback life so stale native callbacks are dropped.
/// `emit` is per platform: each decides how its host stops once the window has gone.
#[derive(Clone)]
pub(super) struct EventSink(
    pub(super) futures::channel::mpsc::UnboundedSender<BrowserEvent>,
    pub(super) Option<CallbackLife>,
);

impl EventSink {
    pub(super) fn for_page(&self, life: CallbackLife) -> Self {
        Self(self.0.clone(), Some(life))
    }
    /// A sink whose events outlive the page, for downloads that run on after it closes.
    pub(super) fn detached(&self) -> Self {
        Self(self.0.clone(), None)
    }
    pub(super) fn accepts_callbacks(&self) -> bool {
        self.1
            .as_ref()
            .is_none_or(|life| life.ticket(false).current())
    }
    pub(super) fn prepare(&self, event: BrowserEvent) -> Option<BrowserEvent> {
        let event = if let Some(life) = &self.1 {
            let lifetime_only = matches!(
                event,
                BrowserEvent::ProcessFailed { .. }
                    | BrowserEvent::Title { .. }
                    | BrowserEvent::Url { .. }
                    | BrowserEvent::History { .. }
                    | BrowserEvent::Loading { .. }
                    | BrowserEvent::NavigationCompleted { .. }
                    | BrowserEvent::Fullscreen { .. }
                    | BrowserEvent::Suspended { .. }
                    | BrowserEvent::AudioChanged { .. }
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
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
