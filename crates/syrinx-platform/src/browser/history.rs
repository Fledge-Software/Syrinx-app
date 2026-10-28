//! Reports each change to a page's back and forward list from host-side events only (#126).

use std::{cell::Cell, rc::Rc};

use syrinx_core::{HISTORY_ENTRY_LIMIT, HistoryChange, HistoryReport};
use webview2_com::{
    CoTaskMemPWSTR, HistoryChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_NAVIGATION_KIND, COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD,
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD, ICoreWebView2,
        ICoreWebView2NavigationStartingEventArgs3,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

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
    /// The latest navigation's id and kind, until it commits or fails.
    started: Option<(u64, Started)>,
    /// Set by `SourceChanged`: whether the new address came with a new document.
    new_document: Option<bool>,
}

impl Pending {
    /// The change the spike's event sequences describe. A reload, and any visit to the
    /// address already shown, raises no `SourceChanged` (trapstoknow, F14).
    fn change(self) -> HistoryChange {
        let started = self.started.map(|(_, started)| started);
        match (self.new_document, started) {
            (Some(false), _) => HistoryChange::SamePage,
            (Some(true), Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            (Some(true), Some(Started::NewDocument { user_initiated })) => {
                HistoryChange::NewDocument { user_initiated }
            }
            // A new document with no start seen is Syrinx's own or the engine's: user-made.
            (Some(true), Some(Started::Reload) | None) => HistoryChange::NewDocument {
                user_initiated: true,
            },
            // A step back or forward to an entry with the shown address still needs its
            // landing checked against the requested jump.
            (None, Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            // Chromium turns a new visit to the shown address into a replacement, so it adds
            // no row; a form posted back to the same address is the known exception.
            (None, Some(Started::NewDocument { .. } | Started::Reload) | None) => {
                HistoryChange::Unchanged
            }
        }
    }

    /// Forgets navigation `id` if it never committed (cancelled, failed or a download), so a
    /// later `HistoryChanged` is not read as that navigation's.
    fn failed(self, id: u64) -> Self {
        let started = self.started.filter(|(started, _)| *started != id);
        Self { started, ..self }
    }
}

/// Registers four host-side events; none adds anything to the page.
pub(super) fn register(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let core = view.webview();
    let pending = Rc::new(Cell::new(Pending::default()));
    register_starting(&core, pending.clone())?;
    register_source(&core, pending.clone())?;
    register_failure(&core, pending.clone())?;
    register_changed(&core, id, events, pending)
}

/// Sends `steps` Back or Forward calls in a row; the engine commits only the last (spike).
pub(super) fn step(core: &ICoreWebView2, backward: bool, steps: u32) -> Result<(), BrowserError> {
    let too_many = usize::try_from(steps).map_or(true, |steps| steps > HISTORY_ENTRY_LIMIT);
    if steps == 0 || too_many {
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
        let (mut navigation, mut user) = (0, BOOL::default());
        unsafe {
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
        pending.set(Pending {
            started: Some((navigation, started)),
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

fn register_failure(core: &ICoreWebView2, pending: Rc<Cell<Pending>>) -> Result<(), BrowserError> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let (mut navigation, mut success) = (0, BOOL::default());
        unsafe {
            args.NavigationId(&mut navigation)?;
            args.IsSuccess(&mut success)?;
        }
        if !success.as_bool() {
            pending.set(pending.get().failed(navigation));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_NavigationCompleted(&handler, &mut token) }
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
            started: started.map(|started| (7, started)),
            new_document,
        }
        .change()
    }

    #[test]
    fn a_step_to_an_entry_at_the_shown_address_is_still_a_step() {
        // No SourceChanged arrives, but the jump's landing must still be checked.
        assert_eq!(
            pending(Some(Started::BackOrForward), None),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_new_visit_to_the_shown_address_replaces_rather_than_adds() {
        let typed = Some(Started::NewDocument {
            user_initiated: true,
        });
        assert_eq!(pending(typed, None), HistoryChange::Unchanged);
    }

    #[test]
    fn a_navigation_that_never_committed_is_forgotten() {
        let started = Pending {
            started: Some((7, Started::BackOrForward)),
            new_document: None,
        };
        assert_eq!(started.failed(7).change(), HistoryChange::Unchanged);
        // A late failure of an older navigation leaves the newer one in place.
        assert_eq!(started.failed(6).change(), HistoryChange::BackOrForward);
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
