//! Runs native find, download, print and zoom controls outside synchronous accelerator callbacks.

use std::{cell::Cell, rc::Rc};

use webview2_com::{
    FindActiveMatchIndexChangedEventHandler, FindMatchCountChangedEventHandler,
    FindStartCompletedHandler, IsDefaultDownloadDialogOpenChangedEventHandler,
    Microsoft::Web::WebView2::Win32::*, ZoomFactorChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, HSTRING, IUnknown, Interface},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

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
    /// Restores a site's remembered level (#90); clamped to WebView2's 25% to 500%.
    SetZoom(f64),
}

pub(super) struct Essentials {
    id: u64,
    events: EventSink,
    search: Rc<FindState>,
}

/// Which find request is current and which has finished starting, so a late result from an
/// older search is dropped; and whether the match-change events are registered yet.
#[derive(Default)]
struct FindState {
    /// The newest request; `None` once find stops.
    current: Cell<Option<u64>>,
    /// The newest request whose `Start` completed.
    settled: Cell<Option<u64>>,
    /// Whether registering the match-change events has been tried.
    watching: Cell<bool>,
}

impl Essentials {
    pub(super) fn register(
        view: &WebView,
        id: u64,
        events: EventSink,
    ) -> Result<Self, BrowserError> {
        register_zoom(view, id, events.clone()).map_err(|_| BrowserError::EventRegistration)?;
        emit_zoom(&view.controller(), id, &events).map_err(|_| BrowserError::PageControl)?;
        register_downloads(view, id, events.clone())
            .map_err(|_| BrowserError::EventRegistration)?;
        Ok(Self {
            id,
            events,
            search: Rc::default(),
        })
    }

    pub(super) fn apply(&self, view: &WebView, action: PageAction) -> Result<(), BrowserError> {
        self.native_action(view, action)
            .map_err(|_| BrowserError::PageControl)
    }

    fn native_action(&self, view: &WebView, action: PageAction) -> windows::core::Result<()> {
        let core = view.webview();
        unsafe {
            match action {
                PageAction::Find {
                    request,
                    text,
                    case_sensitive,
                } => self.find(&core, request, &text, case_sensitive),
                PageAction::FindNext => core.cast::<ICoreWebView2_28>()?.Find()?.FindNext(),
                PageAction::FindPrevious => core.cast::<ICoreWebView2_28>()?.Find()?.FindPrevious(),
                PageAction::StopFind => {
                    self.search.current.set(None);
                    core.cast::<ICoreWebView2_28>()?.Find()?.Stop()
                }
                PageAction::Downloads => {
                    core.cast::<ICoreWebView2_9>()?.OpenDefaultDownloadDialog()
                }
                PageAction::CloseDownloads => {
                    core.cast::<ICoreWebView2_9>()?.CloseDefaultDownloadDialog()
                }
                PageAction::Print => core
                    .cast::<ICoreWebView2_16>()?
                    .ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER),
                action @ (PageAction::ZoomIn
                | PageAction::ZoomOut
                | PageAction::ZoomReset
                | PageAction::SetZoom(_)) => {
                    let controller = view.controller();
                    let mut factor = 1.;
                    controller.ZoomFactor(&mut factor)?;
                    controller.SetZoomFactor(next_zoom(factor, &action))?;
                    // Reset at 100% may not raise a change event, but the displayed value still needs an acknowledgment.
                    emit_zoom(&controller, self.id, &self.events)
                }
            }
        }
    }

    fn find(
        &self,
        core: &ICoreWebView2,
        request: u64,
        text: &str,
        case_sensitive: bool,
    ) -> windows::core::Result<()> {
        self.search.current.set(Some(request));
        let find = unsafe { core.cast::<ICoreWebView2_28>()?.Find()? };
        // Empty Start never completes on the tested runtime. Clearing explicitly stops the search.
        if text.is_empty() {
            unsafe {
                find.Stop()?;
            }
            self.events.emit(BrowserEvent::FindResult {
                id: self.id,
                request,
                count: 0,
                active: -1,
            });
            return Ok(());
        }
        // Without change events the bar still gets Start's result, so search regardless.
        if self.watch_matches(&find).is_err() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.id),
                error: BrowserError::EventRegistration,
            });
        }
        let environment: ICoreWebView2Environment15 =
            unsafe { core.cast::<ICoreWebView2_2>()?.Environment()? }.cast()?;
        let options = unsafe { environment.CreateFindOptions()? };
        unsafe {
            options.SetFindTerm(&HSTRING::from(text))?;
            options.SetIsCaseSensitive(case_sensitive)?;
            options.SetSuppressDefaultFindDialog(true)?;
            options.SetShouldHighlightAllMatches(true)?;
        }
        let completed_find = find.clone();
        let events = self.events.clone();
        let id = self.id;
        let search = self.search.clone();
        let handler = FindStartCompletedHandler::create(Box::new(move |result| {
            if search.current.get() != Some(request) {
                return Ok(());
            }
            search.settled.set(Some(request));
            report_on_error(
                result.and_then(|()| emit_matches(&completed_find, id, request, &events)),
                id,
                &events,
            )
        }));
        unsafe { find.Start(&options, &handler) }
    }

    /// Registers the active-match and count change events on the first search rather than at
    /// page creation, so a runtime without `ICoreWebView2_28` still opens pages. Tried once
    /// only, so a partial failure can never add a second handler for the same event.
    fn watch_matches(&self, find: &ICoreWebView2Find) -> windows::core::Result<()> {
        if self.search.watching.replace(true) {
            return Ok(());
        }
        // The handlers stay for the page's life, so their tokens are never used to remove them.
        let (mut index_token, mut count_token) = (0, 0);
        unsafe {
            find.add_ActiveMatchIndexChanged(
                &FindActiveMatchIndexChangedEventHandler::create(self.match_change_handler()),
                &mut index_token,
            )?;
            find.add_MatchCountChanged(
                &FindMatchCountChangedEventHandler::create(self.match_change_handler()),
                &mut count_token,
            )
        }
    }

    /// Reports the current match position and count, for the settled search only.
    fn match_change_handler(&self) -> MatchChangeClosure {
        let events = self.events.clone();
        let id = self.id;
        let search = self.search.clone();
        Box::new(move |find, _| {
            let Some(request) = settled_request(search.current.get(), search.settled.get()) else {
                return Ok(());
            };
            let find = find.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            report_on_error(emit_matches(&find, id, request, &events), id, &events)
        })
    }
}

type MatchChangeClosure =
    Box<dyn FnMut(Option<ICoreWebView2Find>, Option<IUnknown>) -> windows::core::Result<()>>;

/// The request a match change belongs to: the current one, once its `Start` has completed.
fn settled_request(current: Option<u64>, settled: Option<u64>) -> Option<u64> {
    current.filter(|request| settled == Some(*request))
}

/// Sends the search's match count and the 1-based position of the active match.
fn emit_matches(
    find: &ICoreWebView2Find,
    id: u64,
    request: u64,
    events: &EventSink,
) -> windows::core::Result<()> {
    let mut count = 0;
    let mut active = -1;
    unsafe {
        find.MatchCount(&mut count)?;
        find.ActiveMatchIndex(&mut active)?;
    }
    events.emit(BrowserEvent::FindResult {
        id,
        request,
        count,
        active,
    });
    Ok(())
}

/// Shows a failed find read as a page-control error, and passes the result on.
fn report_on_error(
    result: windows::core::Result<()>,
    id: u64,
    events: &EventSink,
) -> windows::core::Result<()> {
    if result.is_err() {
        events.emit(BrowserEvent::Error {
            tab: Some(id),
            error: BrowserError::PageControl,
        });
    }
    result
}

fn register_zoom(view: &WebView, id: u64, events: EventSink) -> windows::core::Result<()> {
    let handler = ZoomFactorChangedEventHandler::create(Box::new(move |controller, _| {
        let controller = controller.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let result = emit_zoom(&controller, id, &events);
        if result.is_err() {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error: BrowserError::PageControl,
            });
        }
        result
    }));
    let mut token = 0;
    unsafe {
        view.controller()
            .add_ZoomFactorChanged(&handler, &mut token)
    }
}

fn emit_zoom(
    controller: &ICoreWebView2Controller,
    id: u64,
    events: &EventSink,
) -> windows::core::Result<()> {
    let mut factor = 1.;
    unsafe {
        controller.ZoomFactor(&mut factor)?;
    }
    events.emit(BrowserEvent::ZoomChanged { id, factor });
    Ok(())
}

fn register_downloads(view: &WebView, id: u64, events: EventSink) -> windows::core::Result<()> {
    // Without wry download callbacks, the engine already owns its progress/cancel UI.
    let core = view.webview();
    let mut token = 0;
    let changed =
        IsDefaultDownloadDialogOpenChangedEventHandler::create(Box::new(move |core, _| {
            let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let core: ICoreWebView2_9 = core.cast()?;
            let mut open = BOOL::default();
            unsafe {
                core.IsDefaultDownloadDialogOpen(&mut open)?;
            }
            events.emit(BrowserEvent::DownloadDialog {
                id,
                open: open.as_bool(),
            });
            Ok(())
        }));
    unsafe {
        core.cast::<ICoreWebView2_9>()?
            .add_IsDefaultDownloadDialogOpenChanged(&changed, &mut token)
    }
}

fn next_zoom(factor: f64, action: &PageAction) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_changes_report_only_for_the_current_settled_search() {
        // Before the current search's Start completes, a change belongs to an older one.
        assert_eq!(settled_request(Some(4), Some(3)), None);
        assert_eq!(settled_request(Some(4), None), None);
        assert_eq!(settled_request(Some(4), Some(4)), Some(4));
        // A stopped search reports nothing.
        assert_eq!(settled_request(None, Some(4)), None);
        assert_eq!(settled_request(None, None), None);
    }

    #[test]
    fn zoom_steps_boundaries_and_reset_are_predictable() {
        assert_eq!(next_zoom(1., &PageAction::ZoomIn), 1.1);
        assert_eq!(next_zoom(1.1, &PageAction::ZoomOut), 1.);
        assert_eq!(next_zoom(1.13, &PageAction::ZoomIn), 1.25);
        assert_eq!(next_zoom(1.13, &PageAction::ZoomOut), 1.1);
        assert_eq!(next_zoom(5., &PageAction::ZoomIn), 5.);
        assert_eq!(next_zoom(0.25, &PageAction::ZoomOut), 0.25);
        assert_eq!(next_zoom(3., &PageAction::ZoomReset), 1.);
        assert_eq!(next_zoom(1., &PageAction::SetZoom(1.25)), 1.25);
        assert_eq!(next_zoom(1., &PageAction::SetZoom(9.)), 5.);
        assert_eq!(next_zoom(1.5, &PageAction::SetZoom(f64::NAN)), 1.);
    }
}
