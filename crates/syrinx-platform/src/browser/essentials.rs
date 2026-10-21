//! Runs native find, download, print, zoom and mute controls outside synchronous accelerator callbacks.

use std::{cell::Cell, rc::Rc};

use webview2_com::{
    FindStartCompletedHandler, IsDefaultDownloadDialogOpenChangedEventHandler,
    IsDocumentPlayingAudioChangedEventHandler, IsMutedChangedEventHandler,
    Microsoft::Web::WebView2::Win32::*, ZoomFactorChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, HSTRING, Interface},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

pub enum PageAction {
    Find { request: u64, text: String },
    FindNext,
    FindPrevious,
    StopFind,
    Downloads,
    CloseDownloads,
    Print,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

pub(super) struct Essentials {
    id: u64,
    events: EventSink,
    search: Rc<Cell<Option<u64>>>,
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
        // A runtime without the audio events still opens the page; only its audible state is lost.
        if register_audio(view, id, events.clone()).is_err() {
            events.emit(BrowserEvent::Error {
                tab: Some(id),
                error: BrowserError::EventRegistration,
            });
        }
        Ok(Self {
            id,
            events,
            search: Rc::new(Cell::new(None)),
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
                PageAction::Find { request, text } => self.find(&core, request, &text),
                PageAction::FindNext => core.cast::<ICoreWebView2_28>()?.Find()?.FindNext(),
                PageAction::FindPrevious => core.cast::<ICoreWebView2_28>()?.Find()?.FindPrevious(),
                PageAction::StopFind => {
                    self.search.set(None);
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
                action @ (PageAction::ZoomIn | PageAction::ZoomOut | PageAction::ZoomReset) => {
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

    /// Mutes through WebView2 itself; a runtime without `ICoreWebView2_8` reports a tab error.
    pub(super) fn set_muted(&self, view: &WebView, muted: bool) -> Result<(), BrowserError> {
        let core: ICoreWebView2_8 = view
            .webview()
            .cast()
            .map_err(|_| BrowserError::PageControl)?;
        unsafe { core.SetIsMuted(muted) }.map_err(|_| BrowserError::PageControl)
    }

    fn find(&self, core: &ICoreWebView2, request: u64, text: &str) -> windows::core::Result<()> {
        self.search.set(Some(request));
        let find = unsafe { core.cast::<ICoreWebView2_28>()?.Find()? };
        // Empty Start never completes on the tested runtime. Clearing explicitly stops the search.
        if text.is_empty() {
            unsafe {
                find.Stop()?;
            }
            self.events.emit(BrowserEvent::FindCompleted {
                id: self.id,
                request,
                count: 0,
            });
            return Ok(());
        }
        let environment: ICoreWebView2Environment15 =
            unsafe { core.cast::<ICoreWebView2_2>()?.Environment()? }.cast()?;
        let options = unsafe { environment.CreateFindOptions()? };
        unsafe {
            options.SetFindTerm(&HSTRING::from(text))?;
            options.SetSuppressDefaultFindDialog(true)?;
            options.SetShouldHighlightAllMatches(true)?;
        }
        let completed_find = find.clone();
        let events = self.events.clone();
        let id = self.id;
        let search = self.search.clone();
        let handler = FindStartCompletedHandler::create(Box::new(move |result| {
            if search.get() != Some(request) {
                return Ok(());
            }
            let result = result.and_then(|()| {
                let mut count = 0;
                unsafe {
                    completed_find.MatchCount(&mut count)?;
                }
                events.emit(BrowserEvent::FindCompleted { id, request, count });
                Ok(())
            });
            if result.is_err() {
                events.emit(BrowserEvent::Error {
                    tab: Some(id),
                    error: BrowserError::PageControl,
                });
            }
            result
        }));
        unsafe { find.Start(&options, &handler) }
    }
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

/// Reports mute and audible changes as one event, read back from the page each time.
fn register_audio(view: &WebView, id: u64, events: EventSink) -> windows::core::Result<()> {
    let core: ICoreWebView2_8 = view.webview().cast()?;
    let muted_events = events.clone();
    let muted = IsMutedChangedEventHandler::create(Box::new(move |core, _| {
        report_audio(core, id, &muted_events)
    }));
    let playing = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |core, _| {
        report_audio(core, id, &events)
    }));
    // The handlers live as long as the controller, so their tokens are never removed.
    let mut muted_token = 0;
    let mut playing_token = 0;
    unsafe {
        core.add_IsMutedChanged(&muted, &mut muted_token)?;
        core.add_IsDocumentPlayingAudioChanged(&playing, &mut playing_token)
    }
}

/// Emits the page's audio state, or a tab error when it cannot be read.
fn report_audio(
    core: Option<ICoreWebView2>,
    id: u64,
    events: &EventSink,
) -> windows::core::Result<()> {
    let result = emit_audio(core, id, events);
    if result.is_err() {
        events.emit(BrowserEvent::Error {
            tab: Some(id),
            error: BrowserError::PageControl,
        });
    }
    result
}

fn emit_audio(
    core: Option<ICoreWebView2>,
    id: u64,
    events: &EventSink,
) -> windows::core::Result<()> {
    let core: ICoreWebView2_8 = core
        .ok_or_else(|| windows::core::Error::from(E_POINTER))?
        .cast()?;
    let mut muted = BOOL::default();
    let mut playing = BOOL::default();
    unsafe {
        core.IsMuted(&mut muted)?;
        core.IsDocumentPlayingAudio(&mut playing)?;
    }
    events.emit(BrowserEvent::AudioChanged {
        id,
        muted: muted.as_bool(),
        playing: playing.as_bool(),
    });
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
        _ => 1.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_boundaries_and_reset_are_predictable() {
        assert_eq!(next_zoom(1., &PageAction::ZoomIn), 1.1);
        assert_eq!(next_zoom(1.1, &PageAction::ZoomOut), 1.);
        assert_eq!(next_zoom(1.13, &PageAction::ZoomIn), 1.25);
        assert_eq!(next_zoom(1.13, &PageAction::ZoomOut), 1.1);
        assert_eq!(next_zoom(5., &PageAction::ZoomIn), 5.);
        assert_eq!(next_zoom(0.25, &PageAction::ZoomOut), 0.25);
        assert_eq!(next_zoom(3., &PageAction::ZoomReset), 1.);
    }
}
