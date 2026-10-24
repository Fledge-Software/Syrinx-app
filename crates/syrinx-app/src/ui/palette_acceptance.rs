//! Drives P1-33's palette acceptance run in the real window and records what it measured.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use gpui::{AnyWindowHandle, AsyncWindowContext, Pixels, PlatformInput, Point, WeakEntity};
use syrinx_platform::acceptance::{self as probe_win32, Key};
use syrinx_ui::SidebarWidthState;

use super::{peek_state::PeekState, sidebar_tree::TreeId, *};
use crate::{
    acceptance::{
        AcceptancePlan, BURST_CADENCE, BurstSample, KeySample, OpenSample, PresentProbe, Report,
        SizeResult,
    },
    session::{SessionError, SyntheticCatalogue},
};

/// How often the driver looks at the window between steps.
const POLL: Duration = Duration::from_millis(2);
/// Pause after each dismissal, so one opening's teardown never overlaps the next.
const SETTLE: Duration = Duration::from_millis(300);
/// How long a stale refocus or reopened palette has to appear after the palette closes.
const STALE_WINDOW: Duration = Duration::from_secs(1);
/// A keystroke whose rows take longer than this counts as a timeout.
const KEY_LIMIT: Duration = Duration::from_secs(3);
/// Pause after a keystroke's rows are presented, so each key is measured on its own.
/// This hides queueing between keys; the bursts below measure back-to-back typing.
const TYPING_GAP: Duration = Duration::from_millis(20);
/// How long the run sits idle with the palette hidden, measuring wake-ups and CPU; Windows
/// counts process CPU time in about 15.6 ms ticks, so a short window cannot resolve 0.5%.
const IDLE_WINDOW: Duration = Duration::from_secs(30);
/// A pause before the idle window, so work queued by the openings has finished.
const IDLE_SETTLE: Duration = Duration::from_secs(5);
/// D37: an idle Syrinx uses under half a percent of one core.
const IDLE_CPU_BUDGET_PERCENT: f64 = 0.5;
/// How long the run waits for Windows to put Syrinx in front before it starts.
const FOREGROUND_WAIT: Duration = Duration::from_secs(20);
const OPEN_SAMPLES: usize = 20;
const SIZES: [usize; 3] = [1_000, 10_000, 100_000];
const PASSES: usize = 2;
/// Typed then erased one key at a time; the last matches nothing.
const WORDS: [&str; 6] = [
    "amber",
    "harbor guide",
    "row-0420",
    "café",
    "naviga",
    "zqxv",
];
const FOCUS_PAGE: &str = "focus-page";
const ENTER_PAGE: &str = "enter-target";
/// Plain text searches the web unless it matches exactly (#127). `/` lists open tabs and Enter
/// opens its top row, so this is how the owner switches to a tab by part of its title.
const ENTER_QUERY: &str = "/enter-target";
const CLOSED: &str = "The browser window closed during the acceptance run.";

/// The run's state inside the browser view.
pub(super) struct Acceptance {
    plan: AcceptancePlan,
    probe: PresentProbe,
    /// The latest title each synthetic page reported; its script writes its focus there.
    titles: HashMap<u64, String>,
    /// How often each page went from not focused to focused, to tell late theft from no focus.
    focus_gains: HashMap<u64, usize>,
    /// Focus steps the browser view took, so a focus trace can show their order.
    marks: Vec<(Instant, &'static str)>,
    synthetic: Option<Result<SyntheticCatalogue, SessionError>>,
    /// Set when a page was about to leave the loopback server; the run then aborts.
    refused: Option<String>,
}

impl Acceptance {
    pub(super) fn new(plan: AcceptancePlan) -> Self {
        Self {
            plan,
            probe: PresentProbe::default(),
            titles: HashMap::new(),
            focus_gains: HashMap::new(),
            marks: Vec::new(),
            synthetic: None,
            refused: None,
        }
    }
}

impl BrowserView {
    pub(super) fn acceptance_probe(&self) -> Option<PresentProbe> {
        self.acceptance
            .as_ref()
            .map(|acceptance| acceptance.probe.clone())
    }

    pub(super) fn acceptance_title(&mut self, id: u64, title: &str) {
        let Some(acceptance) = self.acceptance.as_mut() else {
            return;
        };
        let focused = title.ends_with("focus:yes");
        let previous = acceptance.titles.insert(id, title.to_owned());
        if focused && previous.is_none_or(|previous| !previous.ends_with("focus:yes")) {
            *acceptance.focus_gains.entry(id).or_default() += 1;
        }
    }

    /// The palette's local request `request` was just queued for the worker.
    pub(super) fn acceptance_query_sent(&self, request: u64) {
        if let Some(acceptance) = &self.acceptance {
            acceptance.probe.sent(request, Instant::now());
        }
    }

    /// The worker timed the query this view sent as `request_id`.
    pub(super) fn acceptance_query_timed(&self, request_id: u64, elapsed: Duration) {
        let (Some(acceptance), Some((pending, local))) = (&self.acceptance, self.pending_query)
        else {
            return;
        };
        if pending == request_id {
            acceptance.probe.worker(local, elapsed);
        }
    }

    pub(super) fn acceptance_synthetic(
        &mut self,
        result: Result<SyntheticCatalogue, SessionError>,
    ) {
        if let Some(acceptance) = self.acceptance.as_mut() {
            acceptance.synthetic = Some(result);
        }
    }

    /// In an acceptance run, refuses any destination other than the loopback pages, marks the
    /// run failed and makes it abort. Ordinary runs always pass.
    pub(super) fn acceptance_permits(&mut self, url: &str) -> bool {
        let Some(acceptance) = self.acceptance.as_mut() else {
            return true;
        };
        let Some(reason) = acceptance.plan.refusal(url) else {
            return true;
        };
        eprintln!("{reason}");
        self.message = reason.clone();
        acceptance.refused.get_or_insert(reason);
        self.outcome.store(2, Ordering::Release);
        false
    }

    /// A page can leave the loopback server without a command from Syrinx (a redirect, its own
    /// script or a link). Its address arrives after the request was sent; the run then aborts.
    pub(super) fn acceptance_observed(&mut self, url: &str) {
        if !self.acceptance_permits(url) {
            eprintln!("A page left its loopback server by itself; the acceptance run stops.");
        }
    }

    /// Starts the scripted run once the window exists; it ends by closing Syrinx.
    pub(super) fn start_acceptance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(acceptance) = &self.acceptance else {
            return;
        };
        let plan = acceptance.plan.clone();
        let probe = acceptance.probe.clone();
        cx.spawn_in(window, async move |view, cx| {
            let run = Run {
                view: view.clone(),
                cx: cx.clone(),
                plan,
                probe,
                report: Report::default(),
                main: 0,
                focus_tab: None,
                enter_tab: None,
                started_in_front: false,
            };
            let report = run.run().await;
            finish(&view, cx, &report);
        })
        .detach();
    }

    fn palette_hwnd(&self, cx: &mut App) -> Result<isize, String> {
        let palette = self.palette.as_ref().ok_or("The palette is not open.")?;
        palette
            .window
            .update(cx, |_, popup, _| native_handle(popup))
            .map_err(|_| "The palette window is gone.".to_owned())?
            .map_err(str::to_owned)
    }

    /// Records one focus step for the run's focus traces; ordinary runs record nothing.
    pub(super) fn acceptance_mark(&mut self, step: &'static str) {
        if let Some(acceptance) = self.acceptance.as_mut() {
            acceptance.marks.push((Instant::now(), step));
        }
    }

    fn focus_gains(&self, id: TabId) -> usize {
        self.acceptance
            .as_ref()
            .and_then(|acceptance| acceptance.focus_gains.get(&id.get()).copied())
            .unwrap_or(0)
    }

    /// Where Windows keyboard focus is now, for the focus-return diagnosis.
    fn keyboard_focus(&self) -> String {
        let popup = self.kept_palette_hwnd();
        match probe_win32::keyboard_focus(self.parent_hwnd, popup) {
            Ok(focus) => focus.label().to_owned(),
            Err(error) => format!("unreadable ({error})"),
        }
    }

    /// Opens the peek as a hover on the edge would, without moving the owner's pointer.
    fn open_peek_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.peek_state = PeekState::Open;
        self.request_peek(window, cx);
    }

    /// The open peek's window and the middle of `tab`'s row in it, once both are drawn.
    fn peek_row(&self, tab: TabId) -> Option<(AnyWindowHandle, Point<Pixels>)> {
        let popup = self.peek_window()?;
        let row = self.sidebar_trees.peek_row_center(TreeId::Tab(tab))?;
        Some((popup, row))
    }

    /// How far a requested peek got, for a run that never saw its row.
    fn peek_progress(&self, tab: TabId) -> String {
        let row = self.sidebar_trees.peek_row_center(TreeId::Tab(tab));
        format!(
            "peek {:?}, opening {:?}, shown {}, sampling {}, target row at {row:?}, status: {}",
            self.peek_state,
            self.peek_opening,
            self.peek_window().is_some(),
            self.peek_monitoring,
            self.message
        )
    }

    /// What the synthetic page's own script last reported about `document.hasFocus()`.
    fn page_focus(&self, id: TabId) -> Option<bool> {
        let title = self.acceptance.as_ref()?.titles.get(&id.get())?;
        if title.ends_with("focus:yes") {
            return Some(true);
        }
        title.ends_with("focus:no").then_some(false)
    }
}

/// Where focus was around one in-process `PageFocused`, for the find focus checks.
struct PageFocusOutcome {
    chrome_had_keys: bool,
    field_before: bool,
    field_after: bool,
}

impl PageFocusOutcome {
    fn detail(&self) -> String {
        format!(
            "Windows focus on the chrome: {}; find field focused before: {}, after: {}",
            self.chrome_had_keys, self.field_before, self.field_after
        )
    }
}

/// Delivers WebView2's `GotFocus` for `tab` through the page-host event path, as its event
/// loop does, and notes where Windows and GPUI focus were.
fn deliver_page_focus(
    view: &mut BrowserView,
    tab: TabId,
    field: &FocusHandle,
    window: &mut Window,
    cx: &mut Context<BrowserView>,
) -> PageFocusOutcome {
    let chrome_had_keys = syrinx_platform::browser::chrome_has_keyboard_focus(view.parent_hwnd);
    let field_before = field.is_focused(window);
    view.browser_event(BrowserEvent::PageFocused { id: tab.get() }, window, cx);
    PageFocusOutcome {
        chrome_had_keys,
        field_before,
        field_after: field.is_focused(window),
    }
}

/// Writes the report, then closes Syrinx with the run's verdict as its exit status.
fn finish(view: &WeakEntity<BrowserView>, cx: &mut AsyncWindowContext, report: &Report) {
    let plan = update_in_main(view, cx, |view, _, _| {
        view.acceptance.as_ref().map(|run| run.plan.clone())
    });
    let Ok(Some(plan)) = plan else {
        eprintln!("{CLOSED}");
        return;
    };
    let generated = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs(),
        Err(_) => {
            eprintln!("The system clock is unavailable; the report is stamped 0.");
            0
        }
    };
    let written = std::fs::write(&plan.report, report.to_json(generated));
    if let Err(error) = &written {
        eprintln!("Cannot write the acceptance report: {error}");
    }
    let passed = report.passed() && written.is_ok();
    let closed = update_in_main(view, cx, |view, _, cx| {
        // An earlier failure (2) is never replaced by a pass.
        if view.outcome.load(Ordering::Acquire) == 0 {
            view.outcome
                .store(if passed { 1 } else { 2 }, Ordering::Release);
        }
        view.request_close(cx);
    });
    if closed.is_err() {
        eprintln!("{CLOSED}");
    }
}

/// One scripted run; each step waits on what the window shows, never on fixed delays alone.
struct Run {
    view: WeakEntity<BrowserView>,
    cx: AsyncWindowContext,
    plan: AcceptancePlan,
    probe: PresentProbe,
    report: Report,
    main: isize,
    focus_tab: Option<TabId>,
    enter_tab: Option<TabId>,
    /// Whether Syrinx was in front when the run began; a deferral then proves nothing.
    started_in_front: bool,
}

impl Run {
    async fn run(mut self) -> Report {
        if let Err(error) = self.scenario().await {
            self.report.aborted = Some(error);
        }
        self.report
    }

    async fn scenario(&mut self) -> Result<(), String> {
        self.wait_until_ready().await?;
        self.open_pages().await?;
        // First, because a peek click needs no foreground and the palette steps abort without it.
        self.check_peek_click().await?;
        // Also before the palette steps: page focus events are delivered in-process.
        self.check_find_keeps_focus().await?;
        self.measure_openings().await?;
        self.check_rapid_dismissal().await?;
        self.check_second_open().await?;
        self.check_enter_activation().await?;
        for size in SIZES {
            self.measure_ranking(size).await?;
        }
        Ok(())
    }

    fn with<R>(
        &mut self,
        update: impl FnOnce(&mut BrowserView, &mut Window, &mut Context<BrowserView>) -> R,
    ) -> Result<R, String> {
        let (result, refused) = update_in_main(&self.view, &mut self.cx, |view, window, cx| {
            let refused = view
                .acceptance
                .as_ref()
                .and_then(|acceptance| acceptance.refused.clone());
            (update(view, window, cx), refused)
        })
        .map_err(|_| CLOSED.to_owned())?;
        // A refused destination ends the run at its next step.
        match refused {
            Some(reason) => Err(reason),
            None => Ok(result),
        }
    }

    async fn sleep(&self, duration: Duration) {
        self.cx.background_executor().timer(duration).await;
    }

    /// Polls the view until `check` returns a value; `None` after `limit` is a measured outcome.
    async fn try_wait<T>(
        &mut self,
        limit: Duration,
        mut check: impl FnMut(&mut BrowserView, &mut Window, &mut Context<BrowserView>) -> Option<T>,
    ) -> Result<Option<T>, String> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(value) = self.with(|view, window, cx| check(view, window, cx))? {
                return Ok(Some(value));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            self.sleep(POLL).await;
        }
    }

    /// Like `try_wait`, but a timeout stops the run.
    async fn wait_for<T>(
        &mut self,
        limit: Duration,
        what: &str,
        check: impl FnMut(&mut BrowserView, &mut Window, &mut Context<BrowserView>) -> Option<T>,
    ) -> Result<T, String> {
        self.try_wait(limit, check)
            .await?
            .ok_or_else(|| format!("Timed out after {limit:?} waiting for {what}."))
    }

    fn focus_tab(&self) -> Result<TabId, String> {
        self.focus_tab
            .ok_or_else(|| "The focus page is missing.".to_owned())
    }

    async fn wait_until_ready(&mut self) -> Result<(), String> {
        self.wait_for(
            Duration::from_secs(60),
            "the session and page host",
            |view, _, _| (view.session_revision.is_some() && view.browser_ready).then_some(()),
        )
        .await?;
        self.main = self.with(|view, _, _| view.parent_hwnd)?;
        // The palette opens only while Syrinx is in front (owner decision, 2026-10-12).
        let in_front = self
            .try_wait(FOREGROUND_WAIT, |_, _, _| {
                syrinx_platform::browser::syrinx_in_foreground().then_some(())
            })
            .await?
            .is_some();
        self.started_in_front = in_front;
        if !in_front {
            eprintln!("Syrinx is not in front; the palette will stay closed until it is.");
        }
        // An unread interval is reported as such; verdicts then assume 60 Hz.
        match probe_win32::refresh_interval(self.main) {
            Ok(interval) => self.report.refresh = Some(interval),
            Err(error) => self.report.refresh_problem = Some(error.to_string()),
        }
        Ok(())
    }

    /// Opens the Enter target in the background and the focus page in front of it.
    async fn open_pages(&mut self) -> Result<(), String> {
        self.enter_tab = Some(self.open_page(ENTER_PAGE).await?);
        let focus = self.open_page(FOCUS_PAGE).await?;
        self.focus_tab = Some(focus);
        self.with(|view, window, cx| view.focus_page(window, cx))?;
        let focused = self
            .try_wait(Duration::from_secs(10), |view, _, _| {
                focused_or_deferred(view, focus)
            })
            .await?;
        let foreground = probe_win32::foreground_window() == self.main;
        let keyboard = self.with(|view, _, _| view.keyboard_focus())?;
        self.report.check(
            "page_focus_baseline",
            focused.is_some(),
            format!(
                "page focus: {}; \
                 Syrinx in foreground: {foreground}; keyboard focus: {keyboard}",
                focused.unwrap_or("none")
            ),
        );
        Ok(())
    }

    async fn open_page(&mut self, label: &'static str) -> Result<TabId, String> {
        let address = self.plan.page(label);
        let before: HashSet<TabId> = self.with(|view, _, _| {
            view.command(SessionCommand::NewTab { address });
            view.model.tabs.iter().map(|tab| tab.id).collect()
        })?;
        self.wait_for(Duration::from_secs(20), label, |view, _, _| {
            let id = view.model.active_id.filter(|id| !before.contains(id))?;
            (view.shown == Some(id) && view.page_focus(id).is_some()).then_some(id)
        })
        .await
    }

    /// Opens with the Ctrl K action and returns the time to the popup's first presented frame.
    async fn open_timed(&mut self) -> Result<OpenSample, String> {
        let probe = self.probe.clone();
        let started = self.with(|view, window, cx| {
            let started = Instant::now();
            probe.begin_open();
            view.shortcut(BrowserShortcut::Palette, window, cx);
            started
        })?;
        let probe = self.probe.clone();
        let open = self
            .try_wait(Duration::from_secs(5), |_, _, _| {
                probe.open().filter(|open| open.presented.is_some())
            })
            .await?
            .ok_or_else(|| {
                if syrinx_platform::browser::syrinx_in_foreground() {
                    "Timed out after 5s waiting for the palette's first frame.".to_owned()
                } else {
                    "Syrinx is not in front, so the palette stays closed (owner decision \
                     2026-10-12). Run the harness in the foreground and leave it alone."
                        .to_owned()
                }
            })?;
        let since = |at: Option<Instant>| at.map_or(Duration::ZERO, |at| at - started);
        Ok(OpenSample {
            presented: since(open.presented),
            requested: since(open.requested),
            started: since(open.started),
            rendered: since(open.rendered),
        })
    }

    async fn measure_openings(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let mut returned = 0;
        let mut deferred = 0;
        let mut slowest = Duration::ZERO;
        let mut places = FocusTally::default();
        let mut popups = HashSet::new();
        for index in 0..OPEN_SAMPLES {
            let sample = self.open_timed().await?;
            self.report.opens.push(sample);
            popups.insert(self.with(|view, _, cx| view.palette_hwnd(cx))??);
            if index == 0 {
                self.check_stacking()?;
                self.check_palette_took_focus(focus).await?;
            }
            // A missing focus return is measured; a palette that stays open stops the run.
            let outcome = self.escape_to_page(focus).await?;
            places.add(&outcome);
            if let Some(elapsed) = outcome.returned {
                returned += 1;
                slowest = slowest.max(elapsed);
            }
            deferred += usize::from(outcome.deferred);
            self.sleep(SETTLE).await;
        }
        let opened = self.report.opens.len();
        self.report.check(
            "escape_returns_page_focus",
            escape_focus_proven(returned, deferred, self.started_in_front),
            format!(
                "{returned}/{opened} openings returned focus to the page and {deferred} deferred \
                 it until Syrinx is activated (a deferral is unproven: completing it is an owner \
                 hand check); started in front {}; slowest {slowest:?}; \
                 Syrinx in foreground now: {}; {places}",
                self.started_in_front,
                probe_win32::foreground_window() == self.main
            ),
        );
        self.report.check(
            "openings_reuse_one_popup",
            popups.len() == 1,
            format!("{} native popups across {opened} openings", popups.len()),
        );
        self.check_no_stale_refocus("no_stale_refocus_after_escape", focus)
            .await?;
        self.check_hidden_popup_idle().await
    }

    /// The kept popup must not render while hidden, so it adds nothing to idle CPU (D37).
    async fn check_hidden_popup_idle(&mut self) -> Result<(), String> {
        self.sleep(IDLE_SETTLE).await;
        let renders_before = self.probe.renders();
        let wakes_before = self.probe.input_notifications();
        let cpu_before = probe_win32::process_cpu_time().map_err(|error| error.to_string())?;
        let started = Instant::now();
        self.sleep(IDLE_WINDOW).await;
        let cpu = probe_win32::process_cpu_time().map_err(|error| error.to_string())? - cpu_before;
        let elapsed = started.elapsed();
        let renders = self.probe.renders() - renders_before;
        let wakes = self.probe.input_notifications() - wakes_before;
        let open = self.with(|view, _, _| view.palette.is_some())?;
        self.report.check(
            "hidden_popup_renders_nothing",
            renders == 0 && wakes == 0 && !open,
            format!(
                "while hidden for {elapsed:?}: palette renders {renders}; input notifications \
                 (caret blinks) {wakes}; open {open}"
            ),
        );
        let share = cpu.as_secs_f64() / elapsed.as_secs_f64() * 100.;
        self.report.check(
            "idle_cpu_with_hidden_popup",
            share < IDLE_CPU_BUDGET_PERCENT,
            format!(
                "Syrinx's own process used {cpu:?} of CPU in {elapsed:?} ({share:.3}% of one \
                 core; D37 budget {IDLE_CPU_BUDGET_PERCENT}%); WebView2's processes are not \
                 counted"
            ),
        );
        Ok(())
    }

    fn check_stacking(&mut self) -> Result<(), String> {
        let main = self.main;
        let popup = self.with(|view, _, cx| view.palette_hwnd(cx))??;
        let stacking =
            probe_win32::overlay_stacking(popup, main).map_err(|error| error.to_string())?;
        self.report.check(
            "popup_owned_by_main_window",
            stacking.owner == main,
            format!("owner {:#x}, main window {main:#x}", stacking.owner),
        );
        self.report.check(
            "popup_not_topmost",
            !stacking.topmost,
            format!("WS_EX_TOPMOST set: {}", stacking.topmost),
        );
        self.report.check(
            "popup_above_page",
            stacking.visible && stacking.above_main,
            format!(
                "visible: {}; above the main window: {}",
                stacking.visible, stacking.above_main
            ),
        );
        Ok(())
    }

    async fn check_palette_took_focus(&mut self, focus: TabId) -> Result<(), String> {
        let blurred = self
            .try_wait(Duration::from_secs(2), |view, _, _| {
                view.page_focus(focus).filter(|focused| !focused)
            })
            .await?
            .is_some();
        let popup = self.with(|view, _, cx| view.palette_hwnd(cx))??;
        let foreground = probe_win32::foreground_window() == popup;
        self.report.check(
            "palette_takes_keyboard_focus",
            blurred,
            format!("page reported blur: {blurred}; popup in foreground: {foreground}"),
        );
        Ok(())
    }

    /// Presses Esc in the palette, which must close; records how long focus took to reach the
    /// page (`None` when it never did) and where Windows keyboard focus was.
    async fn escape_to_page(&mut self, focus: TabId) -> Result<EscapeOutcome, String> {
        let (pressed, gains_before) = self.with(|view, _, cx| {
            let popup = view.palette_hwnd(cx)?;
            if let Some(acceptance) = view.acceptance.as_mut() {
                acceptance.marks.clear();
            }
            let pressed = Instant::now();
            probe_win32::post_key(popup, Key::Escape).map_err(|error| error.to_string())?;
            Ok::<_, String>((pressed, view.focus_gains(focus)))
        })??;
        let at_close = self
            .wait_for(
                Duration::from_secs(3),
                "Esc to close the palette",
                |view, _, _| view.palette.is_none().then(|| view.keyboard_focus()),
            )
            .await?;
        let mut trace = FocusTrace::new(pressed, &at_close);
        let mut deferred = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        let returned = loop {
            let (focused, waiting, keyboard) = self.with(|view, _, _| {
                let focused = view.page_focus(focus) == Some(true);
                (focused, focus_deferred(view), view.keyboard_focus())
            })?;
            trace.observe(&keyboard);
            if focused {
                break Some(pressed.elapsed());
            }
            if waiting {
                deferred = true;
                break None;
            }
            if Instant::now() >= deadline {
                break None;
            }
            self.sleep(POLL).await;
        };
        let (at_end, gains_after, marks) = self.with(|view, _, _| {
            let marks = view
                .acceptance
                .as_mut()
                .map(|acceptance| std::mem::take(&mut acceptance.marks))
                .unwrap_or_default();
            (view.keyboard_focus(), view.focus_gains(focus), marks)
        })?;
        trace.add_marks(&marks);
        Ok(EscapeOutcome {
            returned,
            deferred,
            at_close,
            at_end,
            gained: gains_after > gains_before,
            trace: trace.to_string(),
        })
    }

    /// After the palette closed, nothing may reopen it or take focus from the page.
    async fn check_no_stale_refocus(
        &mut self,
        name: &'static str,
        page: TabId,
    ) -> Result<(), String> {
        self.sleep(STALE_WINDOW).await;
        let main = self.main;
        let (palette_open, popup_visible, windows, focused, active, keyboard) =
            self.with(|view, _, cx| {
                let popup_visible = view.kept_palette_hwnd().is_some_and(|popup| {
                    probe_win32::overlay_stacking(popup, main)
                        .is_ok_and(|stacking| stacking.visible)
                });
                (
                    view.palette.is_some(),
                    popup_visible,
                    cx.windows().len(),
                    focused_or_deferred(view, page),
                    view.model.active_id,
                    view.keyboard_focus(),
                )
            })?;
        // The main window and the kept, hidden palette popup (DW-90).
        let passed = !palette_open
            && !popup_visible
            && windows == 2
            && focused.is_some()
            && active == Some(page);
        self.report.check(
            name,
            passed,
            format!(
                "after {STALE_WINDOW:?}: palette open {palette_open}; popup visible \
                 {popup_visible}; windows {windows}; page focus {}; expected tab active {}; \
                 keyboard focus: {keyboard}",
                focused.unwrap_or("none"),
                active == Some(page)
            ),
        );
        Ok(())
    }

    /// Esc posted before the popup's first frame must still close it and return focus.
    async fn check_rapid_dismissal(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let posted = self.with(|view, window, cx| {
            view.shortcut(BrowserShortcut::Palette, window, cx);
            let popup = view.palette_hwnd(cx)?;
            probe_win32::post_key(popup, Key::Escape).map_err(|error| error.to_string())
        })?;
        let returned = self
            .try_wait(Duration::from_secs(3), |view, _, _| {
                (view.palette.is_none() && focused_or_deferred(view, focus).is_some()).then_some(())
            })
            .await?
            .is_some();
        let keyboard = self.with(|view, _, _| view.keyboard_focus())?;
        self.report.check(
            "rapid_open_then_escape",
            posted.is_ok() && returned,
            format!(
                "Esc posted: {posted:?}; focus returned: {returned}; keyboard focus: {keyboard}"
            ),
        );
        self.check_no_stale_refocus("no_stale_refocus_after_rapid_escape", focus)
            .await?;
        self.sleep(SETTLE).await;
        Ok(())
    }

    /// A second Ctrl K while the palette is open must reuse it, not stack another popup.
    async fn check_second_open(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        self.open_timed().await?;
        let first = self.with(|view, _, cx| view.palette_hwnd(cx))??;
        self.with(|view, window, cx| view.shortcut(BrowserShortcut::Palette, window, cx))?;
        self.sleep(SETTLE).await;
        let (second, windows) =
            self.with(|view, _, cx| (view.palette_hwnd(cx), cx.windows().len()))?;
        self.report.check(
            "second_ctrl_k_reuses_popup",
            second.as_ref() == Ok(&first) && windows == 2,
            format!(
                "same popup: {}; windows {windows}",
                second.as_ref() == Ok(&first)
            ),
        );
        // Focus return is already measured above; here only the close matters.
        let _closed = self.escape_to_page(focus).await?;
        self.sleep(SETTLE).await;
        Ok(())
    }

    /// Typing the background page's title and pressing Enter must switch to that tab.
    async fn check_enter_activation(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let enter = self
            .enter_tab
            .ok_or_else(|| "The Enter target page is missing.".to_owned())?;
        self.open_timed().await?;
        self.with(|view, _, cx| {
            let popup = view.palette_hwnd(cx)?;
            probe_win32::post_text(popup, ENTER_QUERY).map_err(|error| error.to_string())
        })??;
        // Enter only once the full text's own reply selects the target, as the bursts do.
        let target = Target::Tab(enter);
        let selected = self
            .try_wait(Duration::from_secs(3), |view, _, cx| {
                let palette = view.palette.as_ref()?.view.read(cx);
                let settled = palette.input_text(cx) == ENTER_QUERY && palette.answered();
                (settled && palette.selected_target() == Some(target)).then_some(())
            })
            .await?
            .is_some();
        self.with(|view, _, cx| {
            let popup = view.palette_hwnd(cx)?;
            probe_win32::post_key(popup, Key::Enter).map_err(|error| error.to_string())
        })??;
        let switched = self
            .try_wait(Duration::from_secs(5), |view, _, _| {
                let done = view.palette.is_none() && view.model.active_id == Some(enter);
                done.then_some(())
            })
            .await?
            .is_some();
        let focused = self
            .try_wait(Duration::from_secs(3), |view, _, _| {
                (view.page_focus(enter) == Some(true)).then_some(())
            })
            .await?
            .is_some();
        let (active, address, message) = self.with(|view, _, _| {
            let active = view.model.active_id.map(|id| match id {
                id if id == enter => "the target tab",
                id if id == focus => "the focus page",
                _ => "another tab",
            });
            let address = view
                .model
                .tabs
                .iter()
                .find(|tab| Some(tab.id) == view.model.active_id)
                .map(|tab| tab.metadata.address.clone());
            (active, address, view.message.clone())
        })?;
        self.report.check(
            "enter_activates_selected_tab",
            selected && switched && focused,
            format!(
                "target row selected: {selected}; target tab active: {switched}; \
                 target page reported focus: {focused}; active now: {active:?}; \
                 active address: {address:?}; status: {message}"
            ),
        );
        self.check_no_stale_refocus("no_stale_refocus_after_enter", enter)
            .await?;
        self.return_to(focus).await
    }

    /// A click on a sidebar peek row must switch to that tab, and the session must still answer
    /// the next tab command (#10, and the worker stop in #9). The click is dispatched to the
    /// popup in-process; the owner's own pointer is never moved.
    async fn check_peek_click(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let target = self
            .enter_tab
            .ok_or_else(|| "The Enter target page is missing.".to_owned())?;
        let before = self.hide_sidebar().await?;
        self.with(|view, window, cx| view.open_peek_now(window, cx))?;
        let shown = self
            .try_wait(Duration::from_secs(5), |view, _, _| view.peek_row(target))
            .await?;
        let Some((popup, row)) = shown else {
            let state = self.with(|view, _, _| view.peek_progress(target))?;
            return Err(format!(
                "Timed out after 5s waiting for the peek's target row: {state}."
            ));
        };
        self.click(popup, row)?;
        let switched = self
            .try_wait(Duration::from_secs(5), |view, _, _| {
                let done = view.peek_window().is_none()
                    && view.model.active_id == Some(target)
                    && view.shown == Some(target);
                done.then_some(())
            })
            .await?
            .is_some();
        let (peek_open, message) =
            self.with(|view, _, _| (view.peek_window().is_some(), view.message.clone()))?;
        self.report.check(
            "peek_click_activates_row",
            switched,
            format!(
                "target tab active and shown: {switched}; peek still open: {peek_open}; \
                 status: {message}"
            ),
        );
        self.check_session_answers([focus, target]).await?;
        self.restore_sidebar(before)?;
        self.return_to(focus).await
    }

    /// A page `GotFocus` while Windows focus stays on the chrome must leave the find field
    /// focused (#57); one while the page really holds the keys must still clear it (P1-36).
    /// Both events go in-process through `browser_event`, so neither needs the foreground.
    async fn check_find_keeps_focus(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let (field, kept) = self.page_focus_while_chrome_holds_keys(focus).await?;
        self.report.check(
            "find_keeps_focus_on_stray_page_focus",
            kept.chrome_had_keys && kept.field_before && kept.field_after,
            kept.detail(),
        );
        let left_chrome = self.move_keys_into_page(focus).await?;
        let cleared = self.with(|view, window, cx| {
            window.focus(&field, cx);
            deliver_page_focus(view, focus, &field, window, cx)
        })?;
        self.report.check(
            "find_ring_clears_on_real_page_focus",
            left_chrome && !cleared.chrome_had_keys && cleared.field_before && !cleared.field_after,
            format!(
                "Windows focus left the chrome: {left_chrome}; {}",
                cleared.detail()
            ),
        );
        self.with(|view, window, cx| {
            view.dismiss_find(window);
            cx.notify();
        })?;
        self.return_to(focus).await
    }

    /// Opens the find bar on the shown `tab` and, once the chrome holds both Windows and GPUI
    /// focus, delivers a page focus event in the same update, so nothing real slips between.
    async fn page_focus_while_chrome_holds_keys(
        &mut self,
        tab: TabId,
    ) -> Result<(FocusHandle, PageFocusOutcome), String> {
        let shown = self.with(|view, _, _| view.shown)?;
        if shown != Some(tab) {
            return Err(format!(
                "The find check needs the focus page shown, not {shown:?}."
            ));
        }
        self.with(|view, window, cx| view.open_find(window, cx))?;
        self.wait_for(
            Duration::from_secs(5),
            "the find field with the keys on the chrome",
            |view, window, cx| {
                let field = view.find_field_handle(cx)?;
                let chrome = syrinx_platform::browser::chrome_has_keyboard_focus(view.parent_hwnd);
                if !(chrome && field.is_focused(window)) {
                    // A late page focus from an earlier step took the keys; Ctrl F again.
                    view.open_find(window, cx);
                    return None;
                }
                let outcome = deliver_page_focus(view, tab, &field, window, cx);
                Some((field, outcome))
            },
        )
        .await
    }

    /// Moves Windows focus into `tab`'s page as a click would, without GPUI's involvement;
    /// false when it never left the chrome.
    async fn move_keys_into_page(&mut self, tab: TabId) -> Result<bool, String> {
        self.with(|view, _, _| view.page_command(BrowserCommand::Focus { id: tab.get() }))?;
        let left = self
            .try_wait(Duration::from_secs(5), |view, _, _| {
                let chrome = syrinx_platform::browser::chrome_has_keyboard_focus(view.parent_hwnd);
                (!chrome).then_some(())
            })
            .await?;
        Ok(left.is_some())
    }

    /// Hides the sidebar so it can peek, and waits for the hover sampler that hiding starts
    /// to stop again; a sampler still running would close the peek the run opens.
    async fn hide_sidebar(&mut self) -> Result<SidebarWidthState, String> {
        let before = self.with(|view, window, cx| {
            let before = view.sidebar_sizing.state();
            if before != SidebarWidthState::Hidden {
                view.toggle_sidebar(window, cx);
            }
            before
        })?;
        self.wait_for(
            Duration::from_secs(3),
            "the hover sampler to stop",
            |view, _, _| (!view.peek_monitoring).then_some(()),
        )
        .await?;
        Ok(before)
    }

    /// Shows the sidebar again if the run hid it.
    fn restore_sidebar(&mut self, before: SidebarWidthState) -> Result<(), String> {
        if before == SidebarWidthState::Hidden {
            return Ok(());
        }
        self.with(|view, window, cx| view.toggle_sidebar(window, cx))
    }

    /// Moves to `at` in `popup`, then presses and releases the left button there.
    fn click(&mut self, popup: AnyWindowHandle, at: Point<Pixels>) -> Result<(), String> {
        for event in left_click(at) {
            self.cx
                .update_window(popup, |_, window, cx| {
                    window.dispatch_event(event, cx);
                })
                .map_err(|error| {
                    format!("The peek closed before its row was clicked: {error:#}")
                })?;
        }
        Ok(())
    }

    /// The tab command after a peek click must still reach a live session worker. It switches
    /// to whichever of `tabs` is not active, so a click that changed nothing proves nothing.
    async fn check_session_answers(&mut self, tabs: [TabId; 2]) -> Result<(), String> {
        let tab = self.with(|view, _, _| {
            let tab = if view.model.active_id == Some(tabs[0]) {
                tabs[1]
            } else {
                tabs[0]
            };
            view.command(SessionCommand::FocusTab(tab));
            tab
        })?;
        let answered = self
            .try_wait(Duration::from_secs(5), |view, _, _| {
                (view.model.active_id == Some(tab) && view.shown == Some(tab)).then_some(())
            })
            .await?
            .is_some();
        let message = self.with(|view, _, _| view.message.clone())?;
        self.report.check(
            "peek_click_keeps_session_alive",
            answered,
            format!("next tab command applied: {answered}; status: {message}"),
        );
        if !answered {
            return Err("The session stopped answering after the peek click.".to_owned());
        }
        Ok(())
    }

    async fn return_to(&mut self, tab: TabId) -> Result<(), String> {
        self.with(|view, _, _| view.command(SessionCommand::FocusTab(tab)))?;
        self.wait_for(Duration::from_secs(5), "the focus page", |view, _, _| {
            (view.shown == Some(tab)).then_some(())
        })
        .await?;
        self.with(|view, window, cx| view.focus_page(window, cx))?;
        // Ranking needs no page focus, and focus return is already checked above.
        let _focused = self
            .try_wait(Duration::from_secs(5), |view, _, _| {
                (view.page_focus(tab) == Some(true)).then_some(())
            })
            .await?;
        Ok(())
    }

    /// Grows the catalogue to `rows`, then types and erases each word one key at a time,
    /// then again in bursts.
    async fn measure_ranking(&mut self, rows: usize) -> Result<(), String> {
        let focus = self.focus_tab()?;
        self.with(|view, _, _| {
            if let Some(acceptance) = view.acceptance.as_mut() {
                acceptance.synthetic = None;
            }
            view.command(SessionCommand::GrowSyntheticCatalogue { total: rows });
        })?;
        let grown = self
            .wait_for(
                Duration::from_secs(180),
                "the synthetic catalogue",
                |view, _, _| view.acceptance.as_mut()?.synthetic.take(),
            )
            .await?
            .map_err(|error| error.to_string())?;
        self.open_timed().await?;
        let mut samples = Vec::new();
        for _ in 0..PASSES {
            for word in WORDS {
                for character in word.chars() {
                    samples.push(self.keystroke(Typed::Character(character)).await?);
                }
                for _ in word.chars() {
                    samples.push(self.keystroke(Typed::Backspace).await?);
                }
            }
        }
        let bursts = self.measure_bursts().await?;
        self.report.sizes.push(SizeResult {
            synthetic_rows: grown.synthetic_rows,
            catalogue_rows: grown.catalogue_rows,
            seed: grown.elapsed,
            samples,
            bursts,
        });
        // Focus return is already measured above; here only the close matters.
        let _closed = self.escape_to_page(focus).await?;
        self.sleep(SETTLE).await;
        Ok(())
    }

    /// Types each word and then erases it at `BURST_CADENCE`, never waiting between keys.
    async fn measure_bursts(&mut self) -> Result<Vec<BurstSample>, String> {
        let mut bursts = Vec::new();
        for word in WORDS {
            let typing: Vec<Typed> = word.chars().map(Typed::Character).collect();
            bursts.push(self.burst(&typing, word).await?);
            let erasing = vec![Typed::Backspace; typing.len()];
            bursts.push(self.burst(&erasing, "").await?);
        }
        Ok(bursts)
    }

    /// Times the last key of a burst to the frame showing the final text's rows.
    async fn burst(&mut self, keys: &[Typed], expected: &str) -> Result<BurstSample, String> {
        let (queries_before, replies_before) = self.probe.counts();
        self.probe.begin_burst();
        let mut last_posted = Instant::now();
        for (index, typed) in keys.iter().enumerate() {
            if index > 0 {
                self.sleep(BURST_CADENCE).await;
            }
            last_posted = self.with(|view, _, cx| {
                let posted = Instant::now();
                post_typed(view, cx, *typed)?;
                Ok::<_, String>(posted)
            })??;
        }
        let probe = self.probe.clone();
        let settled = self
            .try_wait(KEY_LIMIT, |view, _, cx| {
                let palette = view.palette.as_ref()?.view.read(cx);
                let key = probe.key()?;
                let current = key.request == Some(palette.current_request())
                    && palette.input_text(cx) == expected;
                key.presented.filter(|_| current)
            })
            .await?;
        let (queries, replies) = self.probe.counts();
        self.probe.take_key();
        self.sleep(SETTLE).await;
        Ok(BurstSample {
            final_rows: settled.map(|presented| presented - last_posted),
            queries: queries - queries_before,
            replies: replies - replies_before,
        })
    }

    /// Posts one key to the palette and waits for the frame that shows its rows.
    async fn keystroke(&mut self, typed: Typed) -> Result<KeySample, String> {
        let probe = self.probe.clone();
        let posted = self.with(|view, _, cx| {
            let posted = Instant::now();
            probe.begin_key();
            post_typed(view, cx, typed)?;
            Ok::<_, String>(posted)
        })??;
        let probe = self.probe.clone();
        // A timeout is a measured failure (no rows), not a reason to stop the run.
        self.try_wait(KEY_LIMIT, |_, _, _| {
            probe.key().filter(|key| key.presented.is_some())
        })
        .await?;
        let key = self
            .probe
            .take_key()
            .ok_or_else(|| "The keystroke timing disappeared.".to_owned())?;
        self.sleep(TYPING_GAP).await;
        let since = |at: Option<Instant>| at.map(|at| at - posted);
        Ok(KeySample {
            echo: since(key.echoed),
            rows: since(key.presented),
            round_trip: key
                .sent
                .zip(key.replied)
                .map(|(sent, replied)| replied - sent),
            worker: key.worker,
            superseded: key.superseded,
        })
    }
}

/// Every Esc must return focus; a run that began with Syrinx in front may defer none, since
/// the harness cannot bring Syrinx back to complete a deferral without taking the foreground.
fn escape_focus_proven(returned: usize, deferred: usize, started_in_front: bool) -> bool {
    if started_in_front {
        return returned == OPEN_SAMPLES;
    }
    returned + deferred == OPEN_SAMPLES
}

/// Whether `focus_page` is waiting for the owner to bring Syrinx back to the front, which
/// is the expected result while another application is in front (owner, 2026-10-12).
fn focus_deferred(view: &BrowserView) -> bool {
    view.page_focus_on_activation && !syrinx_platform::browser::syrinx_in_foreground()
}

/// Whether the page has the keys (`"focused"`) or is waiting for activation (`"deferred"`).
fn focused_or_deferred(view: &BrowserView, page: TabId) -> Option<&'static str> {
    if view.page_focus(page) == Some(true) {
        return Some("focused");
    }
    focus_deferred(view).then_some("deferred until Syrinx is activated")
}

/// What one Esc did to focus.
struct EscapeOutcome {
    /// How long the page took to report focus; `None` when it never did.
    returned: Option<Duration>,
    /// Another application was in front, so the page waits for Syrinx's activation.
    deferred: bool,
    /// Windows keyboard focus when the palette had closed, and after the wait.
    at_close: String,
    at_end: String,
    /// Whether the page reported gaining focus at any point after Esc.
    gained: bool,
    /// Keyboard focus changes and the view's focus steps, in milliseconds after Esc.
    trace: String,
}

/// Where keyboard focus went after one Esc, and when the browser view acted.
struct FocusTrace {
    pressed: Instant,
    last: String,
    steps: Vec<(Duration, String)>,
}

impl FocusTrace {
    fn new(pressed: Instant, at_close: &str) -> Self {
        Self {
            pressed,
            last: at_close.to_owned(),
            steps: vec![(pressed.elapsed(), format!("closed, keys: {at_close}"))],
        }
    }

    fn observe(&mut self, keyboard: &str) {
        if keyboard != self.last {
            self.last = keyboard.to_owned();
            self.steps
                .push((self.pressed.elapsed(), format!("keys: {keyboard}")));
        }
    }

    fn add_marks(&mut self, marks: &[(Instant, &'static str)]) {
        for (at, step) in marks {
            let since = at.saturating_duration_since(self.pressed);
            self.steps.push((since, (*step).to_owned()));
        }
        self.steps.sort_by_key(|(at, _)| *at);
    }
}

impl std::fmt::Display for FocusTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|(at, step)| format!("{:.1} ms {step}", at.as_secs_f64() * 1000.))
            .collect();
        write!(formatter, "[{}]", steps.join("; "))
    }
}

/// Counts where keyboard focus was after each Esc, for the check's detail.
#[derive(Default)]
struct FocusTally {
    at_close: std::collections::BTreeMap<String, usize>,
    at_end: std::collections::BTreeMap<String, usize>,
    gained_then_lost: usize,
    /// The first opening's trace, and the first failed one's.
    first_trace: Option<String>,
    failed_trace: Option<String>,
}

impl FocusTally {
    fn add(&mut self, outcome: &EscapeOutcome) {
        *self.at_close.entry(outcome.at_close.clone()).or_default() += 1;
        *self.at_end.entry(outcome.at_end.clone()).or_default() += 1;
        if outcome.gained && outcome.returned.is_none() {
            self.gained_then_lost += 1;
        }
        self.first_trace
            .get_or_insert_with(|| outcome.trace.clone());
        if outcome.returned.is_none() {
            self.failed_trace
                .get_or_insert_with(|| outcome.trace.clone());
        }
    }
}

impl std::fmt::Display for FocusTally {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "keyboard focus at close {:?}, after the wait {:?}; \
             page gained then lost focus {} times; first trace {}; first failed trace {}",
            self.at_close,
            self.at_end,
            self.gained_then_lost,
            self.first_trace.as_deref().unwrap_or("none"),
            self.failed_trace.as_deref().unwrap_or("none")
        )
    }
}

#[derive(Clone, Copy)]
enum Typed {
    Character(char),
    Backspace,
}

/// The pointer events of one left click at `at`, as Windows delivers them to a GPUI window.
fn left_click(at: Point<Pixels>) -> [PlatformInput; 3] {
    [
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: None,
            modifiers: gpui::Modifiers::default(),
        }),
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::default(),
            click_count: 1,
        }),
    ]
}

/// Posts one typed key to the open palette's own window.
fn post_typed(view: &BrowserView, cx: &mut App, typed: Typed) -> Result<(), String> {
    let popup = view.palette_hwnd(cx)?;
    let sent = match typed {
        Typed::Character(character) => {
            probe_win32::post_text(popup, character.encode_utf8(&mut [0; 4]))
        }
        Typed::Backspace => probe_win32::post_key(popup, Key::Backspace),
    };
    sent.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deferral_counts_only_when_the_run_began_behind() {
        assert!(escape_focus_proven(OPEN_SAMPLES, 0, true));
        assert!(
            !escape_focus_proven(OPEN_SAMPLES - 1, 1, true),
            "a run that began in front cannot complete a deferral itself"
        );
        assert!(escape_focus_proven(OPEN_SAMPLES - 1, 1, false));
        assert!(
            !escape_focus_proven(OPEN_SAMPLES - 2, 1, false),
            "one Esc lost focus"
        );
    }
}
