//! Keeps page WebView2 objects on one Windows thread while Rust UI code exchanges owned commands.

mod clear_data;
mod downloads;
mod engine_exit;
mod environment;
mod essentials;
mod overlay;
pub const RENDER_PROCESS_UNRESPONSIVE: i32 = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE.0;
mod hidden_timer;
mod history;
mod history_state;
mod keys;
mod lifecycle;
mod page;
mod page_text;
mod permissions;
mod placement;
mod popups;
mod protocol;
mod recovery;
mod security;

pub use syrinx_core::{Ineligible, PageTextPolicy};

/// WebView2 pauses and resumes downloads, so the flyout offers both.
pub const PAUSES_DOWNLOADS: bool = true;
/// Pages' text is read with one DevTools snapshot once the owner switches capture on.
pub const READS_PAGE_TEXT: bool = true;

use std::{
    collections::HashMap,
    os::windows::io::AsRawHandle,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;
use windows::Win32::{
    Foundation::{ERROR_SUCCESS, GetLastError, HANDLE, HWND, LPARAM, SetLastError, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        DispatchMessageW, GWLP_HWNDPARENT, GetMessageW, HWND_NOTOPMOST, IsWindow, MSG, PM_NOREMOVE,
        PeekMessageW, PostQuitMessage, PostThreadMessageW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SetWindowLongPtrW, SetWindowPos, TranslateMessage, WM_APP,
    },
};

pub use overlay::{
    activate_if_foreground, hide_overlay, overlay_monitor, overlay_pointer, show_overlay,
    syrinx_in_foreground, window_origin,
};
use page::Page;
pub use protocol::*;

const COMMAND_MESSAGE: u32 = WM_APP + 41;

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

    /// Stops the host when no event loop will run again, as when Windows ends the session (#96).
    /// Call it on the parent window's thread: while waiting it services only messages other
    /// threads send here, which the worker needs to close its pages. On a timeout the worker
    /// is left to finish alone and `StillRunning` is returned.
    pub fn shutdown_within(mut self, bound: Duration) -> Result<(), BrowserError> {
        // A refused request still leaves a worker that may be ending; wait for it either way.
        if !self.stopped.load(Ordering::Acquire)
            && let Err(error) = self.send(BrowserCommand::Shutdown)
        {
            eprintln!("Syrinx could not ask its page host to stop; waiting for it anyway: {error}");
        }
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        if !engine_exit::wait_for_thread(HANDLE(worker.as_raw_handle()), bound)? {
            return Err(BrowserError::StillRunning);
        }
        worker.join().map_err(|_| BrowserError::WorkerPanicked)
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
        downloads: downloads::DownloadOperations::default(),
        environment: None,
        engine: None,
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
    state.install_environment(environment);
    events.emit(BrowserEvent::Ready);
    events.emit(BrowserEvent::RuntimeVersion(
        state
            .environment
            .as_ref()
            .and_then(environment::runtime_version),
    ));

    let served = serve(&incoming, &shortcuts, &mut state, events);
    // Every way out of the loop, a failure included, lets the engine save before COM goes.
    if let Some(line) = state.release_engine().describe() {
        eprintln!("{line}");
    }
    served
}

/// Runs commands and native callbacks until shutdown, a quit or a message-loop failure.
fn serve(
    incoming: &Receiver<BrowserCommand>,
    shortcuts: &Receiver<BrowserEvent>,
    state: &mut BrowserState,
    events: &EventSink,
) -> Result<(), BrowserError> {
    let mut timer = lifecycle::WakeTimer::default();
    loop {
        // Creation pumps messages internally; drain commands before blocking again.
        if !drain_commands(incoming, events, |command| {
            drain_native(shortcuts, state)?;
            state.apply(command)
        }) {
            return Ok(());
        }
        // Handle native callbacks even when there are no commands to drain.
        drain_native(shortcuts, state)?;
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

struct BrowserState {
    pages: HashMap<u64, Page>,
    popups: popups::PopupRequests,
    downloads: downloads::DownloadOperations,
    environment: Option<ICoreWebView2Environment>,
    /// Watches `environment`'s browser process so shutdown can let it save (#96).
    engine: Option<engine_exit::EngineWatch>,
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

    /// A new page; an adopted popup inherits its opener's page-text taint, and a reopened tab
    /// starts tainted.
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
        let page = Page::new(
            id,
            self.parent,
            self.environment.as_ref().ok_or(BrowserError::Environment)?,
            self.bounds,
            page::PageContext {
                events: self.events.clone(),
                shortcuts: self.shortcuts.clone(),
                popups: self.popups.clone(),
                downloads: self.downloads.clone(),
                text: self.page_text.clone(),
                starts_tainted,
            },
        )?;
        if let Some(engine) = &self.engine {
            engine.serving();
        }
        Ok(page)
    }

    fn apply(&mut self, command: BrowserCommand) -> Result<(), BrowserError> {
        match command {
            BrowserCommand::Create { id, url } => {
                self.ensure_environment()?;
                let page = self.new_page(id)?;
                page.navigate(&url)?;
                self.pages.insert(id, page);
            }
            BrowserCommand::Reopen { id, url } => {
                self.ensure_environment()?;
                // As crash recovery does: tainted from the start, loaded without clearing it.
                let page = self.new_page_with_taint(id, true)?;
                page.restore(&url)?;
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
            BrowserCommand::ControlDownload { download, control } => {
                self.downloads.control(download, control, &self.events);
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
                validate_url(&url)?;
                // The owner's own navigation: Syrinx chose the address, so no taint carries.
                self.unavailable
                    .insert(id, recovery::Unavailable::chosen(url));
                self.retry_unavailable(id)?;
            }
            BrowserCommand::Focus { id } if self.unavailable.contains_key(&id) => {}
            BrowserCommand::SetMuted { id, .. } if self.unavailable.contains_key(&id) => {}
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

impl Drop for BrowserState {
    fn drop(&mut self) {
        // Complete pending requests while their openers and the COM apartment still exist.
        self.popups.clear();
        self.downloads.clear();
    }
}

fn dispatch(message: MSG) {
    unsafe {
        // A false result means this message needed no keyboard translation, not a failure.
        let _translated = TranslateMessage(&message);
        DispatchMessageW(&message);
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
    holds_keyboard_focus(parent)
}

/// Whether Windows keyboard focus is on `window` itself, such as the palette's popup.
/// `GetFocus` reads the calling thread's input state: call this on the window's own thread.
pub fn holds_keyboard_focus(window: isize) -> bool {
    if window == 0 {
        return false;
    }
    let focused = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
    focused == HWND(window as *mut _)
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
