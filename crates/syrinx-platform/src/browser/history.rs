//! Reports each change to a page's back and forward list from host-side events only (#126).

use std::{cell::Cell, rc::Rc};

use syrinx_core::{HistoryChange, HistoryReport};
use webview2_com::{
    CoTaskMemPWSTR, HistoryChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_NAVIGATION_KIND, COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD,
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD, ICoreWebView2,
        ICoreWebView2NavigationStartingEventArgs3,
    },
    NavigationStartingEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

/// The most Back or Forward steps one jump may send: Chromium keeps 50 entries per tab.
const STEP_LIMIT: u32 = 50;

/// What the latest navigation said about itself when it started.
#[derive(Clone, Copy)]
enum Started {
    NewDocument { user_initiated: bool },
    BackOrForward,
    Reload,
}

/// Facts gathered between two `HistoryChanged` events.
#[derive(Clone, Copy, Default)]
struct Pending {
    started: Option<Started>,
    /// Set by `SourceChanged`: whether the new address came with a new document.
    new_document: Option<bool>,
}

impl Pending {
    /// The change the spike's event sequences describe; a reload raises no `SourceChanged`.
    fn change(self) -> HistoryChange {
        match (self.new_document, self.started) {
            (Some(false), _) => HistoryChange::SamePage,
            (Some(true), Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            (Some(true), Some(Started::NewDocument { user_initiated })) => {
                HistoryChange::NewDocument { user_initiated }
            }
            // A new document with no start seen is Syrinx's own or the engine's: user-made.
            (Some(true), Some(Started::Reload) | None) => HistoryChange::NewDocument {
                user_initiated: true,
            },
            (None, _) => HistoryChange::Unchanged,
        }
    }
}

/// Registers the three host-side events; none adds anything to the page.
pub(super) fn register(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let core = view.webview();
    let pending = Rc::new(Cell::new(Pending::default()));
    register_starting(&core, pending.clone())?;
    register_source(&core, pending.clone())?;
    register_changed(&core, id, events, pending)
}

/// Sends `steps` Back or Forward calls in a row; the engine commits only the last (spike).
pub(super) fn step(core: &ICoreWebView2, backward: bool, steps: u32) -> Result<(), BrowserError> {
    if steps == 0 || steps > STEP_LIMIT {
        return Err(BrowserError::Navigation);
    }
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

fn register_starting(core: &ICoreWebView2, pending: Rc<Cell<Pending>>) -> Result<(), BrowserError> {
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut kind = COREWEBVIEW2_NAVIGATION_KIND::default();
        unsafe {
            args.cast::<ICoreWebView2NavigationStartingEventArgs3>()?
                .NavigationKind(&mut kind)
        }?;
        let mut user = BOOL::default();
        unsafe { args.IsUserInitiated(&mut user) }?;
        let started = match kind {
            COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD => Started::BackOrForward,
            COREWEBVIEW2_NAVIGATION_KIND_RELOAD => Started::Reload,
            _ => Started::NewDocument {
                user_initiated: user.as_bool(),
            },
        };
        pending.set(Pending {
            started: Some(started),
            ..pending.get()
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_NavigationStarting(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_source(core: &ICoreWebView2, pending: Rc<Cell<Pending>>) -> Result<(), BrowserError> {
    let handler = SourceChangedEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let mut new_document = BOOL::default();
        unsafe { args.IsNewDocument(&mut new_document) }?;
        pending.set(Pending {
            new_document: Some(new_document.as_bool()),
            ..pending.get()
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_SourceChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_changed(
    core: &ICoreWebView2,
    id: u64,
    events: EventSink,
    pending: Rc<Cell<Pending>>,
) -> Result<(), BrowserError> {
    let handler = HistoryChangedEventHandler::create(Box::new(move |core, _| {
        let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let change = pending.take().change();
        match read_report(&core, change) {
            Ok(report) => events.emit(BrowserEvent::History { id, report }),
            Err(error) => events.emit(BrowserEvent::Error {
                tab: Some(id),
                error,
            }),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(started: Option<Started>, new_document: Option<bool>) -> HistoryChange {
        Pending {
            started,
            new_document,
        }
        .change()
    }

    #[test]
    fn a_same_document_address_change_is_a_same_page_change() {
        let started = Some(Started::NewDocument {
            user_initiated: true,
        });
        assert_eq!(pending(started, Some(false)), HistoryChange::SamePage);
        assert_eq!(pending(None, Some(false)), HistoryChange::SamePage);
    }

    #[test]
    fn a_new_document_carries_its_navigation_kind() {
        let script = Some(Started::NewDocument {
            user_initiated: false,
        });
        assert_eq!(
            pending(script, Some(true)),
            HistoryChange::NewDocument {
                user_initiated: false
            }
        );
        assert_eq!(
            pending(Some(Started::BackOrForward), Some(true)),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_reload_or_a_change_without_a_new_address_is_unchanged() {
        assert_eq!(
            pending(Some(Started::Reload), None),
            HistoryChange::Unchanged
        );
        assert_eq!(pending(None, None), HistoryChange::Unchanged);
    }
}
