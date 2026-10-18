//! Reuses the palette's owned GPUI popup for the hidden sidebar, leaving page bounds alone.

use std::time::Instant;

use gpui::{Pixels, WeakEntity};
use syrinx_platform::browser::{OverlayMonitor, overlay_monitor, overlay_pointer};
use syrinx_ui::sidebar::{PEEK_EDGE_ZONE, PEEK_WIDTH, in_peek_zone};

use super::{
    peek_state::{HoverSample, PeekState, SAMPLE_INTERVAL},
    *,
};

pub(super) struct PeekPopup {
    window: WindowHandle<Root>,
    hwnd: isize,
    pub(super) generation: u64,
}

pub(super) enum PeekAction {
    Dismiss,
    Palette(bool),
    Toggle,
}

struct PeekView {
    browser: WeakEntity<BrowserView>,
    focus: FocusHandle,
    _changes: Subscription,
    _activation: Subscription,
}

impl Render for PeekView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Never drawn inside a browser update: the popup opens from `open_requested_peek`.
        // The rows take their focus handles from the peek's own tree, not the docked one's.
        let content = match self.browser.update(cx, |browser, cx| {
            browser.sidebar_at(PEEK_WIDTH, true, window, cx)
        }) {
            Ok(content) => content,
            Err(_) => div(), // The owner has gone; native ownership also tears down this window.
        };
        crate::controls::watch_pointer(div())
            .key_context("SyrinxPeek")
            .size_full()
            .text_color(theme::text())
            .text_sm()
            .font_family("Segoe UI Variable Text")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &crate::palette::Dismiss, window, cx| {
                    this.dispatch(PeekAction::Dismiss, window, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.dispatch(PeekAction::Toggle, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Address, window, cx| {
                this.dispatch(PeekAction::Palette(true), window, cx);
            }))
            .on_action(cx.listener(|this, _: &Palette, window, cx| {
                this.dispatch(PeekAction::Palette(false), window, cx);
            }))
            .child(content)
    }
}

impl PeekView {
    fn dispatch(&self, action: PeekAction, window: &Window, cx: &mut Context<Self>) {
        if self
            .browser
            .update(cx, |browser, cx| {
                browser.queue_peek_action(action, window, cx)
            })
            .is_err()
        {
            eprintln!("Sidebar input arrived after its browser was removed.");
        }
    }
}

impl BrowserView {
    /// Hover and eligibility transitions start sampling; a pointer away from the edge stops it.
    pub(super) fn watch_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.peek_monitoring || !self.sidebar_sizing.state().can_peek() || self.closing {
            return;
        }
        self.peek_monitoring = true;
        let started = Instant::now();
        cx.spawn_in(window, async move |view, cx| {
            loop {
                let keep_watching = update_in_main(&view, cx, |view, window, cx| {
                    view.sample_peek(started.elapsed(), window, cx)
                });
                match keep_watching {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(_) => break, // Removing the owner terminates its pointer monitor.
                }
                cx.background_executor().timer(SAMPLE_INTERVAL).await;
            }
        })
        .detach();
    }

    fn sample_peek(
        &mut self,
        elapsed: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pointer =
            match overlay_pointer(self.parent_hwnd, self.peek.as_ref().map(|peek| peek.hwnd)) {
                Ok(pointer) => pointer,
                Err(error) => {
                    self.message = error;
                    self.retire_peek(cx);
                    self.peek_monitoring = false;
                    cx.notify();
                    return false;
                }
            };
        let scale = window.scale_factor();
        let y = pointer.y as f32 / scale;
        let edge = in_peek_zone(self.sidebar_sizing.state(), pointer.x as f32 / scale)
            && (page_card::chrome_height()
                ..f32::from(window.viewport_size().height) - FOOTER_HEIGHT)
                .contains(&y);
        // A fullscreen page (P1-15) has no sidebar to peek at.
        let eligible = peek_eligible(
            pointer.active,
            self.closing,
            self.palette.is_some(),
            self.sidebar_sizing.state(),
            self.sidebar_sizing.is_dragging(),
        ) && self.fullscreen.is_none();
        let sample = HoverSample {
            eligible,
            edge,
            popup: pointer.in_popup,
            pressed: pointer.pressed,
        };
        self.peek_state.sample(elapsed, sample);
        let step = peek_step(
            self.peek_state == PeekState::Open,
            self.peek.is_some(),
            self.peek_opening.is_some(),
        );
        match step {
            PeekStep::Request => self.request_peek(window, cx),
            // Nothing is on screen yet; the deferred opening finds no request and stops.
            PeekStep::Cancel => self.peek_opening = None,
            PeekStep::Retire => {
                self.retire_peek(cx);
                self.peek_state.sample(elapsed, sample);
                if pointer.active && !pointer.pressed && self.palette.is_none() {
                    self.focus_page(window, cx);
                }
            }
            PeekStep::Keep => {}
        }
        let shown = self.peek.is_some() || self.peek_opening.is_some();
        let watching = (eligible && (edge || shown))
            || (self.peek_state == PeekState::Suppressed && pointer.pressed);
        self.peek_monitoring = watching;
        watching
    }

    /// Places the popup now and opens it after this update ends (see `open_requested_peek`).
    fn request_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_peek_editor();
        let main_bounds = window.bounds();
        let scale = window.scale_factor();
        let anchor = peek_anchor(main_bounds, scale);
        let monitor = match overlay_monitor(anchor.0, anchor.1) {
            Ok(monitor) => monitor,
            Err(error) => {
                self.message = error;
                self.peek_state.dismiss();
                return;
            }
        };
        self.peek_generation = self
            .peek_generation
            .checked_add(1)
            .expect("sidebar popup generations exhausted");
        self.peek_opening = Some(self.peek_generation);
        let request = PeekRequest {
            generation: self.peek_generation,
            parent: self.parent_hwnd,
            bounds: peek_bounds(
                main_bounds,
                f32::from(window.viewport_size().height),
                scale,
                &monitor,
            ),
            display_id: gpui::DisplayId::new(monitor.id),
        };
        let browser = cx.weak_entity();
        cx.defer(move |cx| open_requested_peek(browser, request, cx));
    }

    /// Whether a popup opened for `generation` is still wanted.
    fn awaits_peek(&self, generation: u64) -> bool {
        accepts_opened_peek(
            self.closing,
            self.peek_state == PeekState::Open,
            self.peek_opening,
            generation,
        )
    }

    /// Adopts the opened popup, or closes it when hover moved on while it was opening.
    fn install_peek(
        &mut self,
        generation: u64,
        opened: Result<PeekPopup, PeekOpenError>,
        cx: &mut Context<Self>,
    ) {
        let wanted = self.awaits_peek(generation);
        if wanted {
            self.peek_opening = None;
        }
        match opened {
            Ok(peek) if wanted => {
                if let Some(browser) = &self.browser {
                    browser.set_peek_generation(generation);
                }
                self.count(Feature::SidebarPeekShown);
                self.peek = Some(peek);
            }
            Ok(stale) => {
                let handle: gpui::AnyWindowHandle = stale.window.into();
                if handle
                    .update(cx, |_, popup, _| popup.remove_window())
                    .is_err()
                {
                    self.fail("Cannot close a superseded sidebar popup.");
                }
            }
            Err(error) => {
                if wanted {
                    self.peek_state.dismiss();
                }
                match error {
                    // A superseded request's failure is no longer the owner's concern.
                    PeekOpenError::Notice(message) if wanted => self.message = message,
                    PeekOpenError::Notice(_) => {}
                    PeekOpenError::Orphaned => self.fail("Cannot close the failed sidebar popup."),
                }
            }
        }
        cx.notify();
    }

    /// Popup listeners defer into the main window, retaining their generation until delivery.
    pub(super) fn queue_peek_action(
        &self,
        action: PeekAction,
        origin: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(peek) = &self.peek else {
            return;
        };
        if Window::window_handle(origin) != peek.window.into() {
            return;
        }
        let generation = peek.generation;
        let main = self.main_window;
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            if !cx.windows().contains(&main) {
                return;
            }
            if main
                .update(cx, |_, window, cx| {
                    if view
                        .update(cx, |view, cx| {
                            view.apply_peek_action(generation, action, window, cx)
                        })
                        .is_err()
                    {
                        eprintln!("Sidebar action arrived after its browser was removed.");
                    }
                })
                .is_err()
            {
                eprintln!("Sidebar action could not reach its owner window.");
            }
        });
    }

    fn apply_peek_action(
        &mut self,
        generation: u64,
        action: PeekAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.peek.as_ref().map(|peek| peek.generation);
        if !current_action(self.closing, current, generation) {
            return;
        }
        // A queued key or click must not reactivate Syrinx after Alt Tab.
        let active = self.owner_in_front();
        self.retire_peek(cx);
        if !active {
            cx.notify();
            return;
        }
        match action {
            PeekAction::Dismiss => self.focus_page(window, cx),
            PeekAction::Palette(address) => self.palette_key(address, window, cx),
            PeekAction::Toggle => self.toggle_sidebar(window, cx),
        }
        cx.notify();
    }

    pub(super) fn retire_peek(&mut self, cx: &mut Context<Self>) {
        self.peek_state.dismiss();
        self.peek_opening = None;
        if let Some(browser) = &self.browser {
            browser.set_peek_generation(0);
        }
        let Some(peek) = self.peek.take() else {
            return;
        };
        self.close_peek_editor();
        let handle: gpui::AnyWindowHandle = peek.window.into();
        if cx.windows().contains(&handle)
            && handle
                .update(cx, |_, popup, _| popup.remove_window())
                .is_err()
        {
            self.fail("Cannot close the sidebar popup.");
        }
    }

    /// A real move, resize or rescale retires the peek, which would otherwise sit at stale
    /// screen coordinates. GPUI also reports bounds on every activation change; those leave
    /// it alone, so clicking into the peek keeps it and Alt Tab back can rearm the edge.
    pub(super) fn owner_bounds_observed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = OwnerGeometry::of(window);
        if !owner_moved(self.peek_owner_geometry, current) {
            return;
        }
        self.peek_owner_geometry = current;
        let was_open = self.peek.is_some();
        // Minimizing moves the window while it is still active; it must not take the page's focus.
        let refocus = was_open && window.is_window_active() && self.owner_in_front();
        self.retire_peek(cx);
        if refocus {
            self.focus_page(window, cx);
        }
    }

    /// Whether the main window is in front and not minimized, natively rather than as GPUI
    /// last heard. Unknown counts as not in front, so nothing is refocused.
    fn owner_in_front(&mut self) -> bool {
        match overlay_pointer(self.parent_hwnd, self.peek.as_ref().map(|peek| peek.hwnd)) {
            Ok(pointer) => pointer.active,
            Err(error) => {
                self.message = error;
                false
            }
        }
    }

    /// Whether `window` is the open peek popup. Unlike `!is_main_window`, it is false while a
    /// peek is still opening or already retired, when no peek action should be queued.
    pub(super) fn is_peek_window(&self, window: &Window) -> bool {
        self.peek
            .as_ref()
            .is_some_and(|peek| Window::window_handle(window) == peek.window.into())
    }

    /// Input subscriptions belong to their window, so a transient form cannot cross windows.
    fn close_peek_editor(&mut self) {
        self.folder_editor = None;
        if self
            .home
            .editor
            .as_ref()
            .is_some_and(|editor| editor.surface == syrinx_core::DestinationSurface::Favorites)
        {
            self.home.editor = None;
        }
    }

    pub(super) fn dismiss_peek(
        &mut self,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_peek_action(generation, PeekAction::Dismiss, window, cx);
    }

    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.retire_peek(cx);
        self.retire_palette(cx);
        let previous = self.sidebar_sizing.state();
        self.sidebar_sizing.toggle_hidden();
        self.sidebar_state_settled(previous, window, cx);
        self.focus_page(window, cx);
    }
}

/// Where the main window sits and at what scale; the peek is placed from these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OwnerGeometry {
    bounds: Bounds<Pixels>,
    viewport: gpui::Size<Pixels>,
    scale: f32,
}

impl OwnerGeometry {
    pub(super) fn of(window: &Window) -> Self {
        Self {
            bounds: window.bounds(),
            viewport: window.viewport_size(),
            scale: window.scale_factor(),
        }
    }
}

/// Only a changed placement invalidates the peek; a repeated report is not a move.
fn owner_moved(previous: OwnerGeometry, current: OwnerGeometry) -> bool {
    previous != current
}

/// Where a requested peek opens, and the generation that must still be awaited when it does.
struct PeekRequest {
    generation: u64,
    parent: isize,
    bounds: Bounds<Pixels>,
    display_id: gpui::DisplayId,
}

/// Why a requested popup could not be opened.
enum PeekOpenError {
    /// Shown to the owner; a later hover can try again.
    Notice(String),
    /// A popup without a usable native handle could not be closed.
    Orphaned,
}

/// Opens a requested peek outside every entity update. GPUI draws a new window before
/// `open_window` returns, and that draw updates the browser to render its sidebar.
fn open_requested_peek(browser: WeakEntity<BrowserView>, request: PeekRequest, cx: &mut App) {
    let Some(browser) = browser.upgrade() else {
        return; // The owner has gone, so nothing awaits this popup.
    };
    if !browser.read(cx).awaits_peek(request.generation) {
        return;
    }
    let generation = request.generation;
    let opened = open_peek_window(&browser, request, cx);
    browser.update(cx, |browser, cx| {
        browser.install_peek(generation, opened, cx);
    });
}

/// Creates the owned, non-activating popup that renders the shared sidebar.
fn open_peek_window(
    browser: &Entity<BrowserView>,
    request: PeekRequest,
    cx: &mut App,
) -> Result<PeekPopup, PeekOpenError> {
    let native = Rc::new(RefCell::new(Err(
        "Sidebar popup was not created.".to_owned()
    )));
    let created_native = native.clone();
    let parent = request.parent;
    let browser = browser.clone();
    let opened = cx.open_window(
        WindowOptions {
            kind: WindowKind::PopUp,
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(request.bounds)),
            display_id: Some(request.display_id),
            focus: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        move |popup, cx| {
            popup.set_window_title("Syrinx sidebar peek");
            let closing_browser = browser.downgrade();
            popup.on_window_should_close(cx, move |window, cx| {
                if closing_browser
                    .update(cx, |browser, cx| {
                        browser.queue_peek_action(PeekAction::Dismiss, window, cx);
                    })
                    .is_err()
                {
                    return true; // With no owner entity, there is no shutdown work left here.
                }
                false
            });
            *created_native.borrow_mut() =
                native_handle(popup)
                    .map_err(str::to_owned)
                    .and_then(|hwnd| {
                        set_overlay_owner(hwnd, parent).map_err(|error| error.to_string())?;
                        show_app_icon(hwnd);
                        Ok(hwnd)
                    });
            let view = cx.new(|cx: &mut Context<PeekView>| PeekView {
                browser: browser.downgrade(),
                focus: cx.focus_handle(),
                _changes: cx.observe(&browser, |_, _, cx| cx.notify()),
                _activation: cx.observe_window_activation(popup, |this, popup, cx| {
                    if popup.is_window_active() {
                        popup.focus(&this.focus, cx);
                    }
                }),
            });
            cx.new(|cx| Root::new(view, popup, cx))
        },
    );
    let Ok(handle) = opened else {
        return Err(PeekOpenError::Notice(
            "Cannot open the sidebar popup.".to_owned(),
        ));
    };
    let native = native.borrow().clone();
    match native {
        Ok(hwnd) => Ok(PeekPopup {
            window: handle,
            hwnd,
            generation: request.generation,
        }),
        Err(error) => match handle.update(cx, |_, popup, _| popup.remove_window()) {
            Ok(()) => Err(PeekOpenError::Notice(error)),
            Err(_) => Err(PeekOpenError::Orphaned),
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PeekStep {
    /// Ask for a popup; it opens after the current update.
    Request,
    /// Withdraw a request whose popup has not opened yet.
    Cancel,
    /// Close the popup on screen.
    Retire,
    /// Leave the popup, or the pending request, as it is.
    Keep,
}

/// What a hover sample does to the popup, given whether one is shown or opening.
fn peek_step(open: bool, shown: bool, opening: bool) -> PeekStep {
    match (open, shown, opening) {
        (true, false, false) => PeekStep::Request,
        (false, false, true) => PeekStep::Cancel,
        (false, true, _) => PeekStep::Retire,
        _ => PeekStep::Keep,
    }
}

/// A deferred popup is adopted only while its request is still the one awaited.
fn accepts_opened_peek(closing: bool, open: bool, awaited: Option<u64>, opened: u64) -> bool {
    !closing && open && awaited == Some(opened)
}

/// The physical edge anchor uses GPUI's window origin, not a client point at another DPI.
fn peek_anchor(main: Bounds<Pixels>, scale: f32) -> (i32, i32) {
    (
        ((f32::from(main.origin.x) + PEEK_EDGE_ZONE) * scale).round() as i32,
        ((f32::from(main.origin.y) + page_card::chrome_height()) * scale).round() as i32,
    )
}

/// Keep the popup on the edge's monitor, using that monitor's logical coordinate system.
fn peek_bounds(
    main: Bounds<Pixels>,
    viewport_height: f32,
    scale: f32,
    monitor: &OverlayMonitor,
) -> Bounds<Pixels> {
    let (anchor_x, anchor_y) = peek_anchor(main, scale);
    let width = (PEEK_WIDTH * monitor.scale).min((monitor.right - monitor.left) as f32);
    let height = ((viewport_height - page_card::chrome_height() - FOOTER_HEIGHT) * scale)
        .max(1.)
        .min((monitor.bottom - monitor.top) as f32);
    // GPUI recenters windows whose center misses the selected monitor. Clamp before creation.
    let left = (anchor_x as f32).clamp(monitor.left as f32, monitor.right as f32 - width);
    let top = (anchor_y as f32).clamp(monitor.top as f32, monitor.bottom as f32 - height);
    Bounds::new(
        point(px(left / monitor.scale), px(top / monitor.scale)),
        size(px(width / monitor.scale), px(height / monitor.scale)),
    )
}

fn peek_eligible(
    active: bool,
    closing: bool,
    palette_open: bool,
    state: syrinx_ui::SidebarWidthState,
    dragging: bool,
) -> bool {
    active && !closing && !palette_open && state.can_peek() && !dragging
}

/// Dismissed and replaced popups cannot act on the current window.
fn current_action(closing: bool, current: Option<u64>, incoming: u64) -> bool {
    !closing && current == Some(incoming)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(left: i32, right: i32, scale: f32) -> OverlayMonitor {
        OverlayMonitor {
            id: 1,
            scale,
            left,
            right,
            top: 0,
            bottom: 1440,
        }
    }

    fn main_bounds(x: f32, y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(1280.), px(800.)))
    }

    #[test]
    fn placement_stays_on_the_left_monitor_when_the_owner_straddles() {
        // The owner's center is on the right monitor, but its exposed edge is on the left.
        let bounds = peek_bounds(main_bounds(-400., 100.), 800., 1., &monitor(-1920, 0, 1.));
        assert_eq!(
            bounds,
            Bounds::new(point(px(-392.), px(136.)), size(px(264.), px(736.)))
        );
    }

    #[test]
    fn placement_converts_the_owner_origin_to_the_edge_monitors_scale() {
        // A 150% owner begins at physical x=1500; its left edge remains on the 100% monitor.
        let bounds = peek_bounds(main_bounds(1000., 100.), 800., 1.5, &monitor(0, 1920, 1.));
        assert_eq!(
            bounds,
            Bounds::new(point(px(1512.), px(204.)), size(px(264.), px(1104.)))
        );
        // Reverse the scale direction: a 100% owner overlaps a 200% monitor on its left.
        let bounds = peek_bounds(main_bounds(-800., 100.), 800., 1., &monitor(-2560, 0, 2.));
        assert_eq!(
            bounds,
            Bounds::new(point(px(-396.), px(68.)), size(px(264.), px(368.)))
        );
    }

    #[test]
    fn placement_near_a_monitor_boundary_keeps_its_center_on_that_monitor() {
        let bounds = peek_bounds(main_bounds(-100., 100.), 800., 1., &monitor(-1920, 0, 1.));
        assert_eq!(bounds.origin, point(px(-264.), px(136.)));
        assert_eq!(bounds.center(), point(px(-132.), px(504.)));
    }

    #[test]
    fn palette_open_prevents_peek() {
        assert!(!peek_eligible(
            true,
            false,
            true,
            syrinx_ui::SidebarWidthState::Hidden,
            false
        ));
    }

    #[test]
    fn dragging_prevents_peek() {
        assert!(!peek_eligible(
            true,
            false,
            false,
            syrinx_ui::SidebarWidthState::Hidden,
            true
        ));
    }

    #[test]
    fn inactive_window_prevents_peek() {
        assert!(!peek_eligible(
            false,
            false,
            false,
            syrinx_ui::SidebarWidthState::Hidden,
            false
        ));
    }

    #[test]
    fn only_a_live_hidden_sidebar_can_peek() {
        use syrinx_ui::SidebarWidthState::*;
        assert!(peek_eligible(true, false, false, Hidden, false));
        assert!(!peek_eligible(true, true, false, Hidden, false));
        for state in [Compact, Wide] {
            assert!(!peek_eligible(true, false, false, state, false));
        }
    }

    #[test]
    fn only_a_real_move_resize_or_rescale_retires_the_peek() {
        let placed = OwnerGeometry {
            bounds: main_bounds(100., 100.),
            viewport: size(px(1280.), px(764.)),
            scale: 1.5,
        };
        // Activation changes (Alt Tab, clicking into the peek) repeat the same placement.
        assert!(!owner_moved(placed, placed));
        let moved = OwnerGeometry {
            bounds: main_bounds(300., 100.),
            ..placed
        };
        let resized = OwnerGeometry {
            viewport: size(px(1000.), px(764.)),
            ..placed
        };
        let rescaled = OwnerGeometry {
            scale: 2.,
            ..placed
        };
        for changed in [moved, resized, rescaled] {
            assert!(owner_moved(placed, changed), "{changed:?}");
        }
    }

    #[test]
    fn an_opening_peek_is_requested_once_and_withdrawn_before_it_appears() {
        // The window opens after the sampling update, so later samples see it as opening.
        assert_eq!(peek_step(true, false, false), PeekStep::Request);
        assert_eq!(peek_step(true, false, true), PeekStep::Keep);
        assert_eq!(peek_step(true, true, false), PeekStep::Keep);
        // Leaving before it opens withdraws the request; nothing on screen needs closing.
        assert_eq!(peek_step(false, false, true), PeekStep::Cancel);
        assert_eq!(peek_step(false, true, false), PeekStep::Retire);
        assert_eq!(peek_step(false, false, false), PeekStep::Keep);
    }

    #[test]
    fn a_deferred_popup_is_adopted_only_for_the_request_still_awaited() {
        assert!(accepts_opened_peek(false, true, Some(4), 4));
        assert!(
            !accepts_opened_peek(false, true, None, 4),
            "withdrawn or retired before opening"
        );
        assert!(
            !accepts_opened_peek(false, true, Some(5), 4),
            "a newer request replaced it"
        );
        assert!(
            !accepts_opened_peek(false, false, Some(4), 4),
            "hover dismissed while it opened"
        );
        assert!(
            !accepts_opened_peek(true, true, Some(4), 4),
            "shutdown owns teardown"
        );
    }

    #[test]
    fn queued_dismissal_cannot_close_a_replacement_popup() {
        let deliver = |current: &mut Option<u64>, closing, queued| {
            if current_action(closing, *current, queued) {
                *current = None;
            }
        };
        let mut current = Some(8);
        deliver(&mut current, false, 7);
        assert_eq!(
            current,
            Some(8),
            "the replacement survives an old dismissal"
        );
        deliver(&mut current, true, 8);
        assert_eq!(
            current,
            Some(8),
            "shutdown retains sole ownership of teardown"
        );
        deliver(&mut current, false, 8);
        assert_eq!(current, None, "the current dismissal takes effect");
        deliver(&mut current, false, 8);
        assert_eq!(current, None, "duplicate delivery is harmless");
    }
}
