//! Reports each change to a page's back and forward list from host-side events only (#126).

use std::{cell::Cell, rc::Rc};

use syrinx_core::{HISTORY_ENTRY_LIMIT, HistoryChange, HistoryDirection, HistoryReport};
use webview2_com::{
    CoTaskMemPWSTR, HistoryChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_NAVIGATION_KIND, COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD,
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD, ICoreWebView2,
        ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
        ICoreWebView2NavigationStartingEventArgs3, ICoreWebView2SourceChangedEventArgs,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{
    BrowserError, BrowserEvent, EventSink,
    history_state::{Pending, Started},
};

/// Registers four host-side events; none adds anything to the page.
pub(super) fn register(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let core = view.webview();
    let pending = Rc::new(Cell::new(Pending::default()));
    let watch = Watch {
        id,
        events,
        pending,
    };
    register_starting(&core, watch.clone())?;
    register_source(&core, watch.clone())?;
    register_failure(&core, watch.clone())?;
    register_changed(&core, watch)
}

/// Sends `steps` Back or Forward calls in a row; the engine commits only the last (spike).
pub(super) fn step(
    core: &ICoreWebView2,
    direction: HistoryDirection,
    steps: u32,
) -> Result<(), BrowserError> {
    let too_many = usize::try_from(steps).map_or(true, |steps| steps > HISTORY_ENTRY_LIMIT);
    if steps == 0 || too_many {
        return Err(BrowserError::Navigation);
    }
    let backward = direction == HistoryDirection::Back;
    let mut allowed = BOOL::default();
    unsafe {
        if backward {
            core.CanGoBack(&mut allowed)
        } else {
            core.CanGoForward(&mut allowed)
        }
    }
    .map_err(|_| BrowserError::Navigation)?;
    if !allowed.as_bool() {
        return Ok(());
    }
    for _ in 0..steps {
        unsafe {
            if backward {
                core.GoBack()
            } else {
                core.GoForward()
            }
        }
        .map_err(|_| BrowserError::Navigation)?;
    }
    Ok(())
}

/// What every handler shares: the page, its event sink and the facts gathered so far.
#[derive(Clone)]
struct Watch {
    id: u64,
    events: EventSink,
    pending: Rc<Cell<Pending>>,
}

impl Watch {
    /// Reports a handler that could not read its event, so a lost kind is never silent.
    fn reported(&self, result: windows::core::Result<()>) -> windows::core::Result<()> {
        if result.is_err() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.id),
                error: BrowserError::Metadata,
            });
        }
        result
    }

    /// Replaces the gathered facts with `change` applied to them.
    fn update(&self, change: impl FnOnce(Pending) -> Pending) {
        self.pending.set(change(self.pending.get()));
    }

    /// Records the kind and origin of a navigation that is starting.
    fn starting(
        &self,
        args: &ICoreWebView2NavigationStartingEventArgs,
    ) -> windows::core::Result<()> {
        let mut kind = COREWEBVIEW2_NAVIGATION_KIND::default();
        let (mut navigation, mut user) = (0, BOOL::default());
        unsafe {
            args.cast::<ICoreWebView2NavigationStartingEventArgs3>()?
                .NavigationKind(&mut kind)?;
            args.NavigationId(&mut navigation)?;
            args.IsUserInitiated(&mut user)?;
        }
        let started = match kind {
            COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD => Started::BackOrForward,
            COREWEBVIEW2_NAVIGATION_KIND_RELOAD => Started::Reload,
            _ => Started::NewDocument {
                user_initiated: user.as_bool(),
            },
        };
        self.update(|pending| pending.start(navigation, started));
        Ok(())
    }

    /// Records whether the new address came with a new document.
    fn source(&self, args: &ICoreWebView2SourceChangedEventArgs) -> windows::core::Result<()> {
        let mut new_document = BOOL::default();
        unsafe { args.IsNewDocument(&mut new_document) }?;
        self.update(|pending| Pending {
            new_document: Some(new_document.as_bool()),
            ..pending
        });
        Ok(())
    }

    /// Forgets a navigation that finished; a failed latest step marks its jump abandoned.
    fn completed(
        &self,
        args: &ICoreWebView2NavigationCompletedEventArgs,
    ) -> windows::core::Result<()> {
        let (mut navigation, mut success) = (0, BOOL::default());
        unsafe {
            args.NavigationId(&mut navigation)?;
            args.IsSuccess(&mut success)?;
        }
        self.update(|pending| pending.complete(navigation, success.as_bool()));
        Ok(())
    }

    /// Reports the changes the gathered facts describe, with the engine's current state.
    fn changed(&self, core: &ICoreWebView2) {
        let (reports, next) = self.pending.get().reports();
        self.update(|_| next);
        for change in reports {
            self.report(core, change);
        }
    }

    /// Emits `change` with the address shown and where the engine can go.
    fn report(&self, core: &ICoreWebView2, change: HistoryChange) {
        let event = match read_report(core, change) {
            Ok(report) => BrowserEvent::History {
                id: self.id,
                report,
            },
            Err(error) => BrowserEvent::Error {
                tab: Some(self.id),
                error,
            },
        };
        self.events.emit(event);
    }
}

/// The error a handler returns when WebView2 passed it no sender or arguments.
fn missing() -> windows::core::Error {
    windows::core::Error::from(E_POINTER)
}

/// Each `register_*` adds one host-side handler to the page's web view.
fn register_starting(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.starting(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_NavigationStarting(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_source(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = SourceChangedEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.source(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_SourceChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_failure(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.completed(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_NavigationCompleted(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_changed(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = HistoryChangedEventHandler::create(Box::new(move |core, _| {
        let Some(core) = core else {
            return watch.reported(Err(missing()));
        };
        watch.changed(&core);
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_HistoryChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

/// Reads the address the engine shows now and where it can go from there.
fn read_report(core: &ICoreWebView2, change: HistoryChange) -> Result<HistoryReport, BrowserError> {
    let (mut back, mut forward) = (BOOL::default(), BOOL::default());
    unsafe {
        core.CanGoBack(&mut back)
            .and_then(|()| core.CanGoForward(&mut forward))
    }
    .map_err(|_| BrowserError::Metadata)?;
    let mut raw = PWSTR::null();
    let result = unsafe { core.Source(&mut raw) };
    let address = CoTaskMemPWSTR::from(raw).to_string();
    result.map_err(|_| BrowserError::Metadata)?;
    Ok(HistoryReport {
        change,
        address,
        can_go_back: back.as_bool(),
        can_go_forward: forward.as_bool(),
    })
}
