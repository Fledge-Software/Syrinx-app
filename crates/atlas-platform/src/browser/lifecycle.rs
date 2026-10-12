//! Suspends hidden controllers and rejects callbacks after their native lifetime or suspension changes.

use super::{BrowserError, BrowserEvent, EventSink};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use webview2_com::{
    ContainsFullScreenElementChangedEventHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2_3,
    TrySuspendCompletedHandler,
};
use windows::{
    Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer},
    core::{BOOL, Interface},
};
use wry::{WebView, WebViewExtWindows};

pub(super) const GRACE: Duration = Duration::from_secs(5);

pub(super) fn exit_fullscreen(view: &WebView) -> Result<(), BrowserError> {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            GUITHREADINFO, GetGUIThreadInfo, IsChild, PostMessageW, WM_KEYDOWN, WM_KEYUP,
        },
    };
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
    visible: bool,
    due: Option<Instant>,
    request: u64,
    pending: bool,
    alive: bool,
}

impl HiddenState {
    fn accepts_completion(&self, request: u64) -> bool {
        self.alive && !self.visible && self.pending && self.request == request
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
                visible: false,
                due: Some(Instant::now() + GRACE),
                request: 0,
                pending: false,
                alive: true,
            })),
            life,
            events,
        }
    }

    pub(super) fn show(&self, view: &WebView, visible: bool) -> Result<(), BrowserError> {
        if self.state.borrow().visible == visible {
            return Ok(());
        }
        if visible {
            self.resume(view)?;
        }
        // IsVisible must be false before TrySuspend is ever issued.
        view.set_visible(visible)
            .map_err(|_| BrowserError::Visibility)?;
        let mut state = self.state.borrow_mut();
        state.visible = visible;
        state.due = (!visible).then(|| Instant::now() + GRACE);
        Ok(())
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
            state.due = (!state.visible).then(|| Instant::now() + GRACE);
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
        self.state.borrow().due
    }

    pub(super) fn tick(&self, view: &WebView) -> Result<(), BrowserError> {
        let request = {
            let mut state = self.state.borrow_mut();
            if state.visible || state.pending || !state.due.is_some_and(|due| due <= Instant::now())
            {
                return Ok(());
            }
            state.request += 1;
            state.pending = true;
            state.due = None;
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
                state.due = Some(Instant::now() + GRACE);
            }
            Ok(())
        }));
        if unsafe { core.TrySuspend(&completion) }.is_err() {
            let mut state = self.state.borrow_mut();
            state.pending = false;
            state.due = Some(Instant::now() + GRACE);
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
    fn suspension_completion_rejects_activation_rehide_duplicates_and_close() {
        let mut state = HiddenState {
            visible: false,
            due: None,
            request: 7,
            pending: true,
            alive: true,
        };
        assert!(state.accepts_completion(7));
        state.visible = true;
        assert!(!state.accepts_completion(7));
        state.request += 1;
        state.visible = false;
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
