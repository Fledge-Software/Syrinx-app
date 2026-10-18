//! Keeps the palette's owned popup for the whole session, then shows, resets and hides it (DW-90).

use std::time::Instant;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::Pixels;
use syrinx_platform::browser::{BrowserError, OverlayRect, activate_if_foreground, show_overlay};

use super::{
    owned_popup::{OwnedPopupError, PopupSpec, open_owned_popup, show_and_activate},
    *,
};
use crate::palette::{PaletteEntry, PaletteNative};

/// The design's palette is 880 by 560, 72 below the window's top edge.
const PALETTE_WIDTH: f32 = 880.;
const PALETTE_HEIGHT: f32 = 560.;
const PALETTE_TOP: f32 = 72.;
/// Narrower windows shrink the palette to this, keeping a 24-pixel margin each side.
const PALETTE_MIN_WIDTH: f32 = 320.;
const PALETTE_MARGIN: f32 = 24.;
/// The status line while an entry is open; ending the entry puts back what it replaced.
const PALETTE_OPEN_STATUS: &str = "Palette open";

/// The palette popup, created once and hidden between entries.
#[derive(Clone)]
pub(super) struct KeptPalette {
    window: WindowHandle<Root>,
    view: Entity<PaletteView>,
    hwnd: isize,
}

/// The open entry: the kept popup and the identity of this opening.
pub(super) struct PalettePopup {
    pub(super) window: WindowHandle<Root>,
    pub(super) view: Entity<PaletteView>,
    pub(super) hwnd: isize,
    entry: u64,
    /// The status line this entry replaced, shown again when the entry ends (#14).
    resting_status: String,
}

impl BrowserView {
    /// Builds the hidden popup ahead of the first Ctrl K, so that one is fast too.
    pub(super) fn prepare_palette(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if let Err(error) = self.kept_palette(cx) {
            self.message = error;
            cx.notify();
        }
    }

    /// Ctrl K and the address field: open a new entry in the kept popup, or put the keys
    /// back in the entry that is already open.
    pub(super) fn open_palette(
        &mut self,
        address: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_main_window(window) {
            self.queue_peek_action(peek::PeekAction::Palette(address), window, cx);
            return;
        }
        self.retire_peek(cx);
        self.retire_menu(cx);
        if self.closing || self.session_revision.is_none() {
            return;
        }
        if self.palette.is_some() {
            if self.palette_entry_live(cx) {
                self.refocus_palette(cx);
                return;
            }
            // The entry already ended in the popup while its last event is queued.
            self.retire_palette(cx);
        }
        let kept = match self.kept_palette(cx) {
            Ok(kept) => kept,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let Some(entry) = self.begin_entry(kept, address, window, cx) else {
            return;
        };
        if let Some(probe) = self.acceptance_probe() {
            probe.show_requested(Instant::now());
        }
        let rect = palette_rect(window.bounds(), window.scale_factor());
        self.show_palette(entry, rect, cx);
    }

    /// Resets the kept popup for a new entry and listens to it; returns the entry's identity.
    fn begin_entry(
        &mut self,
        kept: KeptPalette,
        address: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        // Each entry restarts what the browser waits for; replies to an earlier one are obsolete.
        self.pending_query = None;
        self.page_focus_on_activation = false;
        let (events, receiver) = unbounded();
        let start = PaletteEntry {
            initial: if address {
                self.active_address()
            } else {
                String::new()
            },
            events,
            context: self.command_context(),
            opened: Instant::now(),
        };
        let view = kept.view.clone();
        let begun = kept.window.update(cx, |_, popup, cx| {
            view.update(cx, |palette, cx| palette.begin(start, popup, cx));
        });
        if begun.is_err() {
            self.kept_palette = None;
            self.message = "The palette window is no longer available. Open it again.".to_owned();
            return None;
        }
        self.palette_entries = self
            .palette_entries
            .checked_add(1)
            .expect("palette entries exhausted");
        let entry = self.palette_entries;
        self.palette = Some(PalettePopup {
            window: kept.window,
            view: kept.view,
            hwnd: kept.hwnd,
            entry,
            resting_status: std::mem::replace(&mut self.message, PALETTE_OPEN_STATUS.to_owned()),
        });
        self.listen_to_palette(entry, receiver, window, cx);
        Some(entry)
    }

    /// The kept popup, created now if it does not exist yet or was lost.
    fn kept_palette(&mut self, cx: &mut Context<Self>) -> Result<KeptPalette, String> {
        if let Some(kept) = &self.kept_palette {
            let handle: gpui::AnyWindowHandle = kept.window.into();
            if cx.windows().contains(&handle) {
                return Ok(kept.clone());
            }
        }
        let kept = self.create_palette(cx)?;
        self.kept_palette = Some(kept.clone());
        Ok(kept)
    }

    /// Creates the owned, non-topmost popup hidden; showing it is `show_palette`'s job.
    fn create_palette(&mut self, cx: &mut Context<Self>) -> Result<KeptPalette, String> {
        let owner = self.parent_hwnd;
        let registry = self.commands.clone();
        let probe = self.acceptance_probe();
        let usage = self.usage.recorder.clone();
        let created = Rc::new(RefCell::new(None));
        let retained = created.clone();
        let bounds = Bounds::new(
            point(px(0.), px(0.)),
            size(px(PALETTE_WIDTH), px(PALETTE_HEIGHT)),
        );
        let spec = PopupSpec {
            title: "Syrinx command palette",
            noun: "command palette",
            owner,
            options: WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: false,
                show: false,
                ..Default::default()
            },
        };
        let opened = open_owned_popup(spec, cx, move |hwnd, popup, cx| {
            let native = PaletteNative { popup: hwnd, owner };
            let palette = cx.new(|cx| PaletteView::new(registry, native, probe, usage, popup, cx));
            // Closing the popup from Windows ends the entry like Esc; it is only hidden.
            let closing = palette.downgrade();
            popup.on_window_should_close(cx, move |popup, cx| {
                closing
                    .update(cx, |palette, cx| palette.dismiss(popup, cx))
                    .is_err()
            });
            *retained.borrow_mut() = Some(palette.clone());
            palette.into()
        });
        let popup = match opened {
            Ok(popup) => popup,
            Err(OwnedPopupError::Notice(error)) => return Err(error),
            Err(OwnedPopupError::Orphaned) => {
                return Err("Cannot close the failed palette window.".to_owned());
            }
        };
        let view = created.borrow_mut().take().expect("created palette view");
        Ok(KeptPalette {
            window: popup.window,
            view,
            hwnd: popup.hwnd,
        })
    }

    /// A late event from an earlier entry must not act on a newer one.
    fn listen_to_palette(
        &mut self,
        entry: u64,
        mut receiver: UnboundedReceiver<PaletteEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |view, cx| {
            while let Some(event) = receiver.next().await {
                let closed = event.finishes_entry();
                let delivered = view.update_in(cx, |view, window, cx| {
                    if !is_current(view.open_entry(), entry) {
                        return false;
                    }
                    view.palette_event(event, window, cx);
                    true
                });
                if !matches!(delivered, Ok(true)) || closed {
                    break;
                }
            }
        })
        .detach();
    }

    /// Shows the reset popup from a task, outside any GPUI update: Windows delivers its resize
    /// and first frame from these calls, and GPUI drops them while an update holds the app.
    fn show_palette(&mut self, entry: u64, rect: OverlayRect, cx: &mut Context<Self>) {
        let probe = self.acceptance_probe();
        cx.spawn(async move |view, cx| {
            let Ok(Some(hwnd)) = view.update(cx, |view, cx| view.entry_to_show(entry, cx)) else {
                return; // Esc or Enter already ended this entry, or the browser is gone.
            };
            if let Some(probe) = &probe {
                probe.show_started(Instant::now());
                probe.arm_open();
            }
            let shown = show_and_activate(hwnd, rect);
            if view
                .update(cx, |view, cx| view.palette_shown(entry, shown, cx))
                .is_err()
            {
                eprintln!("The palette was shown after its browser was removed.");
            }
        })
        .detach();
    }

    /// Keeps an open palette centred over the main window when the window moves or resizes.
    pub(super) fn follow_palette(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(entry) = self.open_entry() else {
            return;
        };
        if !self.palette_entry_live(cx) {
            return;
        }
        let rect = palette_rect(window.bounds(), window.scale_factor());
        // Moved from a task for the same reason as `show_palette`.
        cx.spawn(async move |view, cx| {
            let Ok(Some(hwnd)) = view.update(cx, |view, cx| view.entry_to_show(entry, cx)) else {
                return;
            };
            if let Err(error) = show_overlay(hwnd, rect) {
                let reported = view.update(cx, |view, cx| {
                    view.message = format!("Cannot move the command palette: {error}");
                    cx.notify();
                });
                if reported.is_err() {
                    eprintln!("The palette moved after its browser was removed.");
                }
            }
        })
        .detach();
    }

    /// The popup to show for `entry`, with its view marked for a fresh first frame.
    fn entry_to_show(&mut self, entry: u64, cx: &mut Context<Self>) -> Option<isize> {
        let palette = self.palette.as_ref()?;
        let current = is_current(Some(palette.entry), entry);
        let view = palette.view.clone();
        let hwnd = palette.hwnd;
        view.update(cx, |palette, cx| {
            cx.notify();
            show_target(current, palette.is_open(), hwnd)
        })
    }

    /// The identity of the open entry, if any.
    fn open_entry(&self) -> Option<u64> {
        self.palette.as_ref().map(|palette| palette.entry)
    }

    fn palette_shown(
        &mut self,
        entry: u64,
        shown: Result<bool, BrowserError>,
        cx: &mut Context<Self>,
    ) {
        if !is_current(self.open_entry(), entry) {
            return;
        }
        match shown {
            Ok(true) => {}
            // Another application is in front: the entry ends rather than float above it.
            Ok(false) => {
                self.message = "Syrinx was not in front, so the palette stayed closed.".to_owned();
                self.dismiss_palette(cx);
            }
            Err(error) => {
                self.message = format!("Cannot show the command palette: {error}");
                self.dismiss_palette(cx);
            }
        }
        cx.notify();
    }

    /// A second Ctrl K keeps the open entry and puts the keys back in its input.
    fn refocus_palette(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let view = palette.view.clone();
        let hwnd = palette.hwnd;
        let refocused = palette.window.update(cx, |_, popup, cx| {
            view.update(cx, |palette, cx| palette.focus_input(popup, cx));
        });
        if refocused.is_err() {
            self.palette = None;
            self.kept_palette = None;
            self.pending_query = None;
            self.home_palette.finish_entry();
            self.message = "The palette window is no longer available. Open it again.".to_owned();
            cx.notify();
            return;
        }
        if let Err(error) = activate_if_foreground(hwnd) {
            self.message = error.to_string();
        }
    }

    /// Ends the open entry as Esc does, so the receiver returns focus to the page.
    fn dismiss_palette(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let view = palette.view.clone();
        let dismissed = palette.window.update(cx, |_, popup, cx| {
            view.update(cx, |palette, cx| palette.dismiss(popup, cx));
        });
        if dismissed.is_err() {
            self.settle_palette_status();
            self.palette = None;
            self.kept_palette = None;
        }
    }

    /// Hides the open entry without an event; the caller decides where focus goes.
    pub(super) fn retire_palette(&mut self, cx: &mut Context<Self>) {
        // Invalidate queued popup events before hiding the popup.
        self.pending_query = None;
        self.home_palette.finish_entry();
        self.settle_palette_status();
        let Some(palette) = self.palette.take() else {
            return;
        };
        let view = palette.view.clone();
        let closed = palette.window.update(cx, |_, popup, cx| {
            view.update(cx, |palette, cx| palette.close(popup, cx));
        });
        if closed.is_err() {
            // The popup is already gone; the next Ctrl K builds a new one.
            self.kept_palette = None;
        }
    }

    /// Puts back the status line the open entry replaced, unless something has said more since.
    pub(super) fn settle_palette_status(&mut self) {
        let Some(palette) = &self.palette else {
            return;
        };
        if let Some(status) = status_after_palette(&self.message, &palette.resting_status) {
            self.message = status;
        }
    }

    /// Removes the kept popup at shutdown.
    pub(super) fn remove_palette(&mut self, cx: &mut Context<Self>) {
        self.retire_palette(cx);
        let Some(kept) = self.kept_palette.take() else {
            return;
        };
        let handle: gpui::AnyWindowHandle = kept.window.into();
        if !cx.windows().contains(&handle) {
            return;
        }
        if handle
            .update(cx, |_, window, _| window.remove_window())
            .is_err()
        {
            self.fail("Cannot close the command palette during shutdown.");
        }
    }

    /// The kept popup's native window, shown or hidden, for P1-33's acceptance run.
    pub(super) fn kept_palette_hwnd(&self) -> Option<isize> {
        self.kept_palette.as_ref().map(|kept| kept.hwnd)
    }

    /// Whether the open entry still takes input; Esc or Enter may have ended it already.
    pub(super) fn palette_entry_live(&self, cx: &App) -> bool {
        self.palette
            .as_ref()
            .is_some_and(|palette| palette.view.read(cx).is_open())
    }
}

/// Whether an event or show task for `entry` still belongs to the open entry.
fn is_current(open: Option<u64>, entry: u64) -> bool {
    open == Some(entry)
}

/// The status line once an entry ends: what it replaced, while "Palette open" still shows.
/// A notice written during the entry (a failed show, a search error) stays.
fn status_after_palette(current: &str, resting: &str) -> Option<String> {
    (current == PALETTE_OPEN_STATUS).then(|| resting.to_owned())
}

/// The popup to show: only for the current entry, and only while it still takes input,
/// since Esc can end it before its show task runs.
fn show_target(current: bool, open: bool, hwnd: isize) -> Option<isize> {
    (current && open).then_some(hwnd)
}

/// Where the palette sits over the main window, in GPUI's logical pixels.
fn palette_bounds(main: Bounds<Pixels>) -> Bounds<Pixels> {
    let main_width = f32::from(main.size.width);
    // Narrower windows drop the preview pane.
    let width = (main_width - 2. * PALETTE_MARGIN)
        .clamp(PALETTE_MIN_WIDTH, PALETTE_WIDTH)
        .min(main_width);
    Bounds::new(
        point(
            main.origin.x + px((main_width - width) / 2.),
            main.origin.y + px(PALETTE_TOP),
        ),
        size(px(width), px(PALETTE_HEIGHT)),
    )
}

/// The palette's place in physical pixels, at the main window's scale.
fn palette_rect(main: Bounds<Pixels>, scale: f32) -> OverlayRect {
    let bounds = palette_bounds(main);
    let physical = |logical: Pixels| (f32::from(logical) * scale).round() as i32;
    OverlayRect {
        left: physical(bounds.origin.x),
        top: physical(bounds.origin.y),
        width: physical(bounds.size.width),
        height: physical(bounds.size.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_window(x: f32, y: f32, width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(900.)))
    }

    #[test]
    fn only_the_open_entry_receives_events_and_shows() {
        assert!(is_current(Some(3), 3));
        assert!(!is_current(Some(4), 3), "a stale entry's event is dropped");
        assert!(!is_current(None, 3), "no entry is open");
        assert_eq!(show_target(true, true, 42), Some(42));
        assert_eq!(
            show_target(true, false, 42),
            None,
            "Esc ended it before the show"
        );
        assert_eq!(
            show_target(false, true, 42),
            None,
            "a newer entry owns the popup"
        );
    }

    #[test]
    fn an_ended_entry_puts_back_the_status_it_replaced() {
        assert_eq!(
            status_after_palette(PALETTE_OPEN_STATUS, "Saved locally").as_deref(),
            Some("Saved locally"),
            "Esc must not leave the status line saying the palette is open"
        );
        assert_eq!(
            status_after_palette(
                "Cannot show the command palette: synthetic",
                "Saved locally"
            ),
            None,
            "a notice written while the entry was open stays"
        );
    }

    #[test]
    fn a_wide_window_centres_the_full_size_palette_below_its_top() {
        let rect = palette_rect(main_window(100., 50., 1280.), 1.);
        assert_eq!(
            rect,
            OverlayRect {
                left: 300,
                top: 122,
                width: 880,
                height: 560
            }
        );
    }

    #[test]
    fn a_narrow_window_keeps_a_margin_and_a_minimum_width() {
        assert_eq!(palette_rect(main_window(0., 0., 600.), 1.).width, 552);
        assert_eq!(palette_rect(main_window(0., 0., 300.), 1.).width, 300);
    }

    #[test]
    fn physical_pixels_follow_the_main_windows_scale_on_any_monitor() {
        // A window on a 150% monitor to the left of the primary one has negative coordinates.
        let rect = palette_rect(main_window(-1600., 40., 1280.), 1.5);
        assert_eq!(
            rect,
            OverlayRect {
                left: -2100,
                top: 168,
                width: 1320,
                height: 840
            }
        );
    }
}
