//! Suspends hidden controllers and rejects callbacks after their native lifetime or suspension changes.

use super::{BrowserError, BrowserEvent, EventSink, hidden_timer::HiddenTimer};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use webview2_com::{
    ContainsFullScreenElementChangedEventHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2_3,
    NavigationCompletedEventHandler, NavigationStartingEventHandler, TrySuspendCompletedHandler,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, ERROR_SUCCESS, HWND, WIN32_ERROR},
        UI::WindowsAndMessaging::{KillTimer, SetTimer},
    },
    core::{BOOL, Interface},
};
use wry::{WebView, WebViewExtWindows};

pub(super) fn exit_fullscreen(view: &WebView) -> Result<(), BrowserError> {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            GUITHREADINFO, GetGUIThreadInfo, IsChild, PostMessageW, WM_KEYDOWN, WM_KEYUP,
        },
    };
    // A late exit command must not send Escape to a page which already left fullscreen.
    let mut fullscreen = BOOL::default();
    unsafe { view.webview().ContainsFullScreenElement(&mut fullscreen) }
        .map_err(|_| BrowserError::Metadata)?;
    if !fullscreen.as_bool() {
        return Ok(());
    }
    // Reestablish the host's native focus before MoveFocus, including after Alt+Tab.
    focus_host_window(view.hwnd())?;
    view.focus().map_err(|_| BrowserError::Focus)?;
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetGUIThreadInfo(
            windows::Win32::System::Threading::GetCurrentThreadId(),
            &mut info,
        )
    }
    .map_err(|_| BrowserError::Focus)?;
    if !unsafe { IsChild(view.hwnd(), info.hwndFocus) }.as_bool() && info.hwndFocus != view.hwnd() {
        return Err(BrowserError::Focus);
    }
    unsafe {
        PostMessageW(Some(info.hwndFocus), WM_KEYDOWN, WPARAM(0x1B), LPARAM(1))
            .map_err(|_| BrowserError::Focus)?;
        PostMessageW(
            Some(info.hwndFocus),
            WM_KEYUP,
            WPARAM(0x1B),
            LPARAM(0xC0000001),
        )
    }
    .map_err(|_| BrowserError::Focus)
}

/// Gives the host window native focus, accepting a NULL result that only means nothing had focus.
fn focus_host_window(hwnd: HWND) -> Result<(), BrowserError> {
    use windows::Win32::{
        Foundation::{GetLastError, SetLastError},
        UI::Input::KeyboardAndMouse::SetFocus,
    };
    // Clear the thread's last error so a NULL return can be told apart from a failure.
    // windows-rs only reads GetLastError when building the Err, so the code survives.
    let (had_previous, error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let previous = SetFocus(Some(hwnd));
        (previous.is_ok(), GetLastError())
    };
    if !focus_succeeded(had_previous, error) {
        return Err(BrowserError::Focus);
    }
    Ok(())
}

// A NULL previous-focus handle also means success when no window previously had focus.
fn focus_succeeded(had_previous: bool, error: WIN32_ERROR) -> bool {
    had_previous || error == ERROR_SUCCESS
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
pub(super) struct CallbackLife(Arc<AtomicU64>);

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
    fn suspend(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
    fn resume(&self) {
        if self.0.load(Ordering::Acquire).is_multiple_of(2) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
    pub(super) fn retire(&self) {
        self.0.store(0, Ordering::Release);
    }
}

struct HiddenState {
    timer: HiddenTimer,
    request: u64,
    pending: bool,
    alive: bool,
}

impl HiddenState {
    fn accepts_completion(&self, request: u64) -> bool {
        self.alive && !self.timer.visible() && self.pending && self.request == request
    }
}

pub(super) struct Lifecycle {
    id: u64,
    state: Rc<RefCell<HiddenState>>,
    pub(super) life: CallbackLife,
    events: EventSink,
}

impl Lifecycle {
    pub(super) fn new(id: u64, life: CallbackLife, events: EventSink) -> Self {
        Self {
            id,
            state: Rc::new(RefCell::new(HiddenState {
                timer: HiddenTimer::new(Instant::now()),
                request: 0,
                pending: false,
                alive: true,
            })),
            life,
            events,
        }
    }

    /// Shows or hides the page. `place` runs after a resume and before the page becomes
    /// visible, so a suspended controller is never asked to take new bounds.
    pub(super) fn show(
        &self,
        view: &WebView,
        visible: bool,
        place: impl FnOnce() -> Result<(), BrowserError>,
    ) -> Result<(), BrowserError> {
        if self.state.borrow().timer.visible() == visible {
            return place();
        }
        if visible {
            self.resume(view)?;
        }
        place()?;
        // IsVisible must be false before TrySuspend is ever issued.
        view.set_visible(visible)
            .map_err(|_| BrowserError::Visibility)?;
        self.state.borrow_mut().timer.shown(visible, Instant::now());
        Ok(())
    }

    /// Records Syrinx's own navigation request before WebView2 reports that it started.
    pub(super) fn navigation_requested(&self) {
        let now = Instant::now();
        self.state.borrow_mut().timer.navigation_requested(now);
    }

    /// Releases a navigation request that WebView2 refused before starting it.
    pub(super) fn navigation_refused(&self) {
        let now = Instant::now();
        self.state.borrow_mut().timer.navigation_refused(now);
    }

    /// Follows every main-frame navigation, so a hidden page settles only once it finishes.
    pub(super) fn register_navigation(&self, view: &WebView) -> Result<(), BrowserError> {
        let starts = self.state.clone();
        let started = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            let id = navigation_id(args, |args, id| unsafe { args.NavigationId(id) })?;
            let now = Instant::now();
            starts.borrow_mut().timer.navigation_started(id, now);
            Ok(())
        }));
        let completions = self.state.clone();
        let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            let id = navigation_id(args, |args, id| unsafe { args.NavigationId(id) })?;
            let now = Instant::now();
            completions.borrow_mut().timer.navigation_completed(id, now);
            Ok(())
        }));
        // The handlers live as long as the controller, so their tokens are never removed.
        let core = view.webview();
        let mut starting_token = 0;
        unsafe { core.add_NavigationStarting(&started, &mut starting_token) }
            .map_err(|_| BrowserError::EventRegistration)?;
        let mut completed_token = 0;
        unsafe { core.add_NavigationCompleted(&completed, &mut completed_token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    pub(super) fn resume(&self, view: &WebView) -> Result<(), BrowserError> {
        let core: ICoreWebView2_3 = view
            .webview()
            .cast()
            .map_err(|_| BrowserError::Suspension)?;
        // Invalidate a pending completion before making a call that can pump callbacks.
        {
            let mut state = self.state.borrow_mut();
            state.request += 1;
            state.pending = false;
            state.timer.restart(Instant::now());
        }
        unsafe { core.Resume() }.map_err(|_| BrowserError::Suspension)?;
        let mut suspended = BOOL::default();
        unsafe { core.IsSuspended(&mut suspended) }.map_err(|_| BrowserError::Suspension)?;
        if suspended.as_bool() {
            return Err(BrowserError::Suspension);
        }
        self.life.resume();
        self.events.emit(BrowserEvent::Suspended {
            id: self.id,
            suspended: false,
        });
        Ok(())
    }

    pub(super) fn due(&self) -> Option<Instant> {
        self.state.borrow().timer.due()
    }

    /// A `TrySuspend` is pending or has succeeded; `IsSuspended` reads false until it completes.
    pub(super) fn suspension_requested(&self) -> bool {
        let state = self.state.borrow();
        state.pending || state.timer.suspending()
    }

    pub(super) fn tick(&self, view: &WebView) -> Result<(), BrowserError> {
        let request = {
            let mut state = self.state.borrow_mut();
            if state.pending || !state.timer.suspension_due(Instant::now()) {
                return Ok(());
            }
            state.request += 1;
            state.pending = true;
            state.timer.suspension_requested();
            state.request
        };
        let core: ICoreWebView2_3 = view
            .webview()
            .cast()
            .map_err(|_| BrowserError::Suspension)?;
        let state = self.state.clone();
        let life = self.life.clone();
        let events = self.events.clone();
        let id = self.id;
        let check = core.clone();
        let completion = TrySuspendCompletedHandler::create(Box::new(move |status, success| {
            let mut state = state.borrow_mut();
            if !state.accepts_completion(request) {
                return Ok(());
            }
            state.pending = false;
            let mut suspended = BOOL::default();
            let readback = unsafe { check.IsSuspended(&mut suspended) };
            if status.is_err() || readback.is_err() {
                events.emit(BrowserEvent::Error {
                    tab: Some(id),
                    error: BrowserError::Suspension,
                });
            }
            if status.is_ok() && success && readback.is_ok() && suspended.as_bool() {
                life.suspend();
                events.emit(BrowserEvent::Suspended {
                    id,
                    suspended: true,
                });
            } else {
                state.timer.restart(Instant::now());
            }
            Ok(())
        }));
        if unsafe { core.TrySuspend(&completion) }.is_err() {
            let mut state = self.state.borrow_mut();
            state.pending = false;
            state.timer.restart(Instant::now());
            return Err(BrowserError::Suspension);
        }
        Ok(())
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        self.state.borrow_mut().alive = false;
        self.life.retire();
    }
}

/// Reads the `NavigationId` both navigation event argument types carry.
fn navigation_id<Args>(
    args: Option<Args>,
    read: impl FnOnce(&Args, &mut u64) -> windows::core::Result<()>,
) -> windows::core::Result<u64> {
    let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    let mut id = 0;
    read(&args, &mut id)?;
    Ok(id)
}

pub(super) fn register_fullscreen(
    view: &WebView,
    id: u64,
    events: EventSink,
) -> Result<(), BrowserError> {
    let handler = ContainsFullScreenElementChangedEventHandler::create(Box::new(move |core, _| {
        let core =
            core.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
        let mut active = BOOL::default();
        unsafe {
            core.ContainsFullScreenElement(&mut active)?;
        }
        events.emit(BrowserEvent::Fullscreen {
            id,
            active: active.as_bool(),
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.webview()
            .add_ContainsFullScreenElementChanged(&handler, &mut token)
    }
    .map_err(|_| BrowserError::EventRegistration)
}

/// A timer exists only while hidden pages have pending grace periods.
#[derive(Default)]
pub(super) struct WakeTimer(usize);
impl WakeTimer {
    pub(super) fn arm(&mut self, due: Option<Instant>) -> Result<(), BrowserError> {
        self.cancel()?;
        if let Some(due) = due {
            let delay = due
                .saturating_duration_since(Instant::now())
                .as_millis()
                .clamp(1, u32::MAX as u128) as u32;
            self.0 = unsafe { SetTimer(None, 0, delay, None) };
            if self.0 == 0 {
                return Err(BrowserError::Wake);
            }
        }
        Ok(())
    }
    fn cancel(&mut self) -> Result<(), BrowserError> {
        if self.0 != 0 {
            unsafe { KillTimer(None, self.0) }.map_err(|_| BrowserError::Wake)?;
            self.0 = 0;
        }
        Ok(())
    }
}
impl Drop for WakeTimer {
    fn drop(&mut self) {
        if let Err(error) = self.cancel() {
            eprintln!("{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_without_previous_window_succeeds_only_without_native_error() {
        use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;

        assert!(focus_succeeded(false, ERROR_SUCCESS));
        assert!(!focus_succeeded(false, ERROR_INVALID_PARAMETER));
        assert!(focus_succeeded(true, ERROR_SUCCESS));
        assert!(focus_succeeded(true, ERROR_INVALID_PARAMETER));
    }

    #[test]
    fn queued_metadata_survives_suspension_but_not_controller_replacement() {
        let life = CallbackLife::new();
        let (sender, mut received) = futures::channel::mpsc::unbounded();
        let events = EventSink(sender, Some(life.clone()));
        events.emit(BrowserEvent::Title {
            id: 7,
            title: "Finished synthetic page".into(),
        });
        events.emit(BrowserEvent::Url {
            id: 7,
            url: "https://finished.example.test/".into(),
        });
        events.emit(BrowserEvent::Loading {
            id: 7,
            loading: false,
        });
        events.emit(BrowserEvent::NavigationCompleted {
            id: 7,
            success: true,
        });
        life.suspend();
        assert!(
            matches!(received.try_recv().unwrap().into_current(), Some(BrowserEvent::Title { title, .. }) if title == "Finished synthetic page")
        );
        assert!(
            matches!(received.try_recv().unwrap().into_current(), Some(BrowserEvent::Url { url, .. }) if url == "https://finished.example.test/")
        );
        life.resume();
        assert!(matches!(
            received.try_recv().unwrap().into_current(),
            Some(BrowserEvent::Loading { loading: false, .. })
        ));
        assert!(matches!(
            received.try_recv().unwrap().into_current(),
            Some(BrowserEvent::NavigationCompleted { success: true, .. })
        ));
        events.emit(BrowserEvent::Title {
            id: 7,
            title: "Retired synthetic page".into(),
        });
        life.retire();
        assert!(received.try_recv().unwrap().into_current().is_none());
    }

    #[test]
    fn suspension_completion_rejects_activation_rehide_duplicates_and_close() {
        let now = Instant::now();
        let mut state = HiddenState {
            timer: HiddenTimer::new(now),
            request: 7,
            pending: true,
            alive: true,
        };
        assert!(state.accepts_completion(7));
        state.timer.shown(true, now);
        assert!(!state.accepts_completion(7));
        state.request += 1;
        state.timer.shown(false, now);
        assert!(!state.accepts_completion(7));
        assert!(state.accepts_completion(8));
        state.pending = false;
        assert!(!state.accepts_completion(8));
        state.pending = true;
        state.alive = false;
        assert!(!state.accepts_completion(8));
    }
    #[test]
    fn queued_callbacks_cannot_cross_suspend_resume_close_or_recreation() {
        let life = CallbackLife::new();
        let queued = life.ticket(false);
        let failure = life.ticket(true);
        assert!(queued.current());
        life.suspend();
        assert!(!queued.current());
        assert!(!life.ticket(false).current());
        assert!(failure.current());
        life.resume();
        assert!(!queued.current());
        assert!(life.ticket(false).current());
        life.retire();
        let replacement = CallbackLife::new();
        assert!(!failure.current());
        assert!(!queued.current());
        assert!(replacement.ticket(false).current());
    }
}
