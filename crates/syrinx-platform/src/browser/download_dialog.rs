//! Keeps WebView2's own downloads flyout under Syrinx's ↓ button and lets ↓ close it again (#89).

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use webview2_com::{
    IsDefaultDownloadDialogOpenChangedEventHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::Foundation::{E_POINTER, POINT, RECT},
    core::{BOOL, Interface},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

/// How far, in logical pixels, the ↓ button's right edge sits left of the page card's right
/// edge. The app's title bar layout owns this number; its test keeps the two in step.
pub const DOWNLOADS_BUTTON_FROM_CARD_RIGHT: f32 = 138.;

/// A press this soon after the flyout closed is the press that closed it, by taking focus.
/// 400 ms covers a deliberate click's press-to-release; a later press reopens the flyout.
/// Whether a click outside the flyout closes it at all is unproven in a real window (#89).
const DISMISS_GRACE: Duration = Duration::from_millis(400);

/// What a press of ↓ does to the flyout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DownloadStep {
    Open,
    Close,
    /// The press itself already closed the flyout; reopening it would make ↓ never close it.
    Stay,
}

/// Decides a ↓ press from the flyout's last reported state and when it last closed.
pub(super) fn toggle_step(open: bool, closed_at: Option<Instant>, now: Instant) -> DownloadStep {
    if open {
        return DownloadStep::Close;
    }
    let just_closed = closed_at
        .and_then(|closed| now.checked_duration_since(closed))
        .is_some_and(|since| since < DISMISS_GRACE);
    if just_closed {
        DownloadStep::Stay
    } else {
        DownloadStep::Open
    }
}

/// The flyout's margin from the card's top-right corner. It assumes raw pixels, as the
/// controller's bounds are; WebView2's docs do not say, and a real window has not shown it (#89).
pub(super) fn dialog_margin(scale: f64) -> POINT {
    let scale = if scale.is_finite() && scale > 0. {
        scale
    } else {
        1.
    };
    POINT {
        x: (f64::from(DOWNLOADS_BUTTON_FROM_CARD_RIGHT) * scale).round() as i32,
        y: 0,
    }
}

/// One page's flyout: its reported open state and the margin last applied for it.
pub(super) struct DownloadDialog {
    id: u64,
    events: EventSink,
    open: Rc<Cell<bool>>,
    closed_at: Rc<Cell<Option<Instant>>>,
    margin: Cell<Option<POINT>>,
}

impl DownloadDialog {
    /// Starts following the flyout's open state and anchors it before the page is first laid
    /// out. A failed anchor is reported and leaves the flyout in WebView2's corner; it never
    /// stops the tab opening.
    pub(super) fn register(
        view: &WebView,
        id: u64,
        events: EventSink,
    ) -> windows::core::Result<Self> {
        let dialog = Self {
            id,
            events,
            open: Rc::new(Cell::new(false)),
            closed_at: Rc::new(Cell::new(None)),
            margin: Cell::new(None),
        };
        let core: ICoreWebView2_9 = view.webview().cast()?;
        dialog.follow_open_state(&core)?;
        let anchored = dialog
            .align(&core)
            .and_then(|()| dialog.place(view, &core, false));
        dialog.report_placement(anchored);
        Ok(dialog)
    }

    /// Opens the flyout under ↓, or closes it when it is open.
    pub(super) fn toggle(&self, view: &WebView) -> windows::core::Result<()> {
        let core: ICoreWebView2_9 = view.webview().cast()?;
        match toggle_step(self.open.get(), self.closed_at.get(), Instant::now()) {
            DownloadStep::Close => unsafe { core.CloseDefaultDownloadDialog() },
            DownloadStep::Stay => Ok(()),
            DownloadStep::Open => {
                // The display scale may have changed since the page was created. A failed
                // placement is reported, and the flyout still opens where WebView2 last put it.
                self.report_placement(self.place(view, &core, true));
                unsafe { core.OpenDefaultDownloadDialog() }
            }
        }
    }

    /// Puts the flyout in the page card's top-right corner, the one below ↓.
    fn align(&self, core: &ICoreWebView2_9) -> windows::core::Result<()> {
        unsafe {
            core.SetDefaultDownloadDialogCornerAlignment(
                COREWEBVIEW2_DEFAULT_DOWNLOAD_DIALOG_CORNER_ALIGNMENT_TOP_RIGHT,
            )
        }
    }

    /// Tells the app a placement failed, without failing the caller.
    fn report_placement(&self, result: windows::core::Result<()>) {
        if result.is_err() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.id),
                error: BrowserError::PageControl,
            });
        }
    }

    /// Sets the margin for the current display scale. WebView2 applies a new margin only at
    /// its next layout, so `relayout` repeats the current bounds to cause one; whether a
    /// repeat of the same bounds counts as a layout is unproven in a real window (#89).
    fn place(
        &self,
        view: &WebView,
        core: &ICoreWebView2_9,
        relayout: bool,
    ) -> windows::core::Result<()> {
        let controller = view.controller();
        let mut scale = 1.;
        unsafe {
            controller
                .cast::<ICoreWebView2Controller3>()?
                .RasterizationScale(&mut scale)?;
        }
        let margin = dialog_margin(scale);
        if self.margin.get() == Some(margin) {
            return Ok(());
        }
        unsafe {
            core.SetDefaultDownloadDialogMargin(margin)?;
        }
        self.margin.set(Some(margin));
        if !relayout {
            return Ok(());
        }
        let mut bounds = RECT::default();
        unsafe {
            controller.Bounds(&mut bounds)?;
            controller.SetBounds(bounds)
        }
    }

    /// Records each open and close, and tells the app; the handler only reads and posts.
    fn follow_open_state(&self, core: &ICoreWebView2_9) -> windows::core::Result<()> {
        let (id, events) = (self.id, self.events.clone());
        let open_state = self.open.clone();
        let closed_at = self.closed_at.clone();
        let changed =
            IsDefaultDownloadDialogOpenChangedEventHandler::create(Box::new(move |core, _| {
                let core = core.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                let core: ICoreWebView2_9 = core.cast()?;
                let mut open = BOOL::default();
                unsafe {
                    core.IsDefaultDownloadDialogOpen(&mut open)?;
                }
                let open = open.as_bool();
                open_state.set(open);
                if !open {
                    closed_at.set(Some(Instant::now()));
                }
                events.emit(BrowserEvent::DownloadDialog { id, open });
                Ok(())
            }));
        let mut token = 0;
        unsafe { core.add_IsDefaultDownloadDialogOpenChanged(&changed, &mut token) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_opens_a_closed_flyout_and_closes_an_open_one() {
        let now = Instant::now();
        assert_eq!(toggle_step(false, None, now), DownloadStep::Open);
        assert_eq!(toggle_step(true, None, now), DownloadStep::Close);
        assert_eq!(toggle_step(true, Some(now), now), DownloadStep::Close);
    }

    #[test]
    fn a_press_that_just_dismissed_the_flyout_does_not_reopen_it() {
        let closed = Instant::now();
        assert_eq!(
            toggle_step(false, Some(closed), closed + Duration::from_millis(50)),
            DownloadStep::Stay
        );
        assert_eq!(
            toggle_step(false, Some(closed), closed + DISMISS_GRACE),
            DownloadStep::Open
        );
        assert_eq!(
            toggle_step(false, Some(closed), closed + Duration::from_secs(5)),
            DownloadStep::Open
        );
    }

    #[test]
    fn the_margin_scales_to_raw_pixels_and_ignores_a_broken_scale() {
        assert_eq!(dialog_margin(1.).x, 138);
        assert_eq!(dialog_margin(1.5).x, 207);
        assert_eq!(dialog_margin(1.25).x, 173);
        for broken in [0., -1., f64::NAN, f64::INFINITY] {
            assert_eq!(dialog_margin(broken).x, 138, "{broken}");
        }
        assert_eq!(dialog_margin(2.).y, 0);
    }
}
