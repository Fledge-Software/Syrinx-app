//! Drives P1-33's palette acceptance run in the real window and records what it measured.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use atlas_platform::acceptance::{self as probe_win32, Key};
use gpui::{AsyncWindowContext, WeakEntity};

use super::*;
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
const CLOSED: &str = "The browser window closed during the acceptance run.";

/// The run's state inside the browser view.
pub(super) struct Acceptance {
    plan: AcceptancePlan,
    probe: PresentProbe,
    /// The latest title each synthetic page reported; its script writes its focus there.
    titles: HashMap<u64, String>,
    synthetic: Option<Result<SyntheticCatalogue, SessionError>>,
}

impl Acceptance {
    pub(super) fn new(plan: AcceptancePlan) -> Self {
        Self {
            plan,
            probe: PresentProbe::default(),
            titles: HashMap::new(),
            synthetic: None,
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
        if let Some(acceptance) = self.acceptance.as_mut() {
            acceptance.titles.insert(id, title.to_owned());
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

    /// Arms the probe at the start of the new popup's first frame, the one that presents it.
    pub(super) fn arm_open_probe(&mut self, cx: &mut App) {
        let (Some(acceptance), Some(palette)) = (&self.acceptance, &self.palette) else {
            return;
        };
        let probe = acceptance.probe.clone();
        let view = palette.view.clone();
        let armed = palette.window.update(cx, |_, popup, _| {
            popup.on_next_frame(move |_, cx| {
                probe.arm_open();
                view.update(cx, |_, cx| cx.notify());
            });
        });
        if armed.is_err() {
            self.message = "The palette closed before its first frame could be timed.".to_owned();
        }
    }

    /// Starts the scripted run once the window exists; it ends by closing Atlas.
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

    /// What the synthetic page's own script last reported about `document.hasFocus()`.
    fn page_focus(&self, id: TabId) -> Option<bool> {
        let title = self.acceptance.as_ref()?.titles.get(&id.get())?;
        if title.ends_with("focus:yes") {
            return Some(true);
        }
        title.ends_with("focus:no").then_some(false)
    }
}

/// Writes the report, then closes Atlas with the run's verdict as its exit status.
fn finish(view: &WeakEntity<BrowserView>, cx: &mut AsyncWindowContext, report: &Report) {
    let plan = view.update_in(cx, |view, _, _| {
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
    let closed = view.update_in(cx, |view, _, cx| {
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
        self.view
            .update_in(&mut self.cx, update)
            .map_err(|_| CLOSED.to_owned())
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
                view.page_focus(focus).filter(|focused| *focused)
            })
            .await?
            .is_some();
        let foreground = probe_win32::foreground_window() == self.main;
        self.report.check(
            "page_focus_baseline",
            focused,
            format!("document.hasFocus() reported by the page: {focused}; Atlas in foreground: {foreground}"),
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
            .wait_for(
                Duration::from_secs(5),
                "the palette's first frame",
                |_, _, _| probe.open().filter(|open| open.presented.is_some()),
            )
            .await?;
        let since = |at: Option<Instant>| at.map_or(Duration::ZERO, |at| at - started);
        Ok(OpenSample {
            presented: since(open.presented),
            requested: since(open.requested),
            created: since(open.created),
            rendered: since(open.rendered),
        })
    }

    async fn measure_openings(&mut self) -> Result<(), String> {
        let focus = self.focus_tab()?;
        let mut returned = 0;
        let mut slowest = Duration::ZERO;
        for index in 0..OPEN_SAMPLES {
            let sample = self.open_timed().await?;
            self.report.opens.push(sample);
            if index == 0 {
                self.check_stacking()?;
                self.check_palette_took_focus(focus).await?;
            }
            // A missing focus return is measured; a palette that stays open stops the run.
            if let Some(elapsed) = self.escape_to_page(focus).await? {
                returned += 1;
                slowest = slowest.max(elapsed);
            }
            self.sleep(SETTLE).await;
        }
        let opened = self.report.opens.len();
        self.report.check(
            "escape_returns_page_focus",
            returned == OPEN_SAMPLES,
            format!(
                "{returned}/{opened} openings returned focus to the page; slowest {slowest:?}; \
                 Atlas in foreground now: {}",
                probe_win32::foreground_window() == self.main
            ),
        );
        self.check_no_stale_refocus("no_stale_refocus_after_escape", focus)
            .await
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

    /// Presses Esc in the palette, which must close; returns how long focus took to reach the
    /// page, or `None` when the page never reported focus.
    async fn escape_to_page(&mut self, focus: TabId) -> Result<Option<Duration>, String> {
        let pressed = self.with(|view, _, cx| {
            let popup = view.palette_hwnd(cx)?;
            let pressed = Instant::now();
            probe_win32::post_key(popup, Key::Escape).map_err(|error| error.to_string())?;
            Ok::<_, String>(pressed)
        })??;
        self.wait_for(
            Duration::from_secs(3),
            "Esc to close the palette",
            |view, _, _| view.palette.is_none().then_some(()),
        )
        .await?;
        let returned = self
            .try_wait(Duration::from_secs(3), |view, _, _| {
                (view.page_focus(focus) == Some(true)).then_some(())
            })
            .await?;
        Ok(returned.map(|()| pressed.elapsed()))
    }

    /// After the palette closed, nothing may reopen it or take focus from the page.
    async fn check_no_stale_refocus(
        &mut self,
        name: &'static str,
        page: TabId,
    ) -> Result<(), String> {
        self.sleep(STALE_WINDOW).await;
        let (palette_open, windows, focused, active) = self.with(|view, _, cx| {
            (
                view.palette.is_some(),
                cx.windows().len(),
                view.page_focus(page),
                view.model.active_id,
            )
        })?;
        let passed = !palette_open && windows == 1 && focused == Some(true) && active == Some(page);
        self.report.check(
            name,
            passed,
            format!(
                "after {STALE_WINDOW:?}: palette open {palette_open}; windows {windows}; page focus {focused:?}; expected tab active {}",
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
                (view.palette.is_none() && view.page_focus(focus) == Some(true)).then_some(())
            })
            .await?
            .is_some();
        self.report.check(
            "rapid_open_then_escape",
            posted.is_ok() && returned,
            format!("Esc posted: {posted:?}; focus returned: {returned}"),
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
        let _focus_return = self.escape_to_page(focus).await?;
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
            probe_win32::post_text(popup, ENTER_PAGE).map_err(|error| error.to_string())
        })??;
        // Enter only once the full text's own reply selects the target, as the bursts do.
        let target = Target::Tab(enter);
        let selected = self
            .try_wait(Duration::from_secs(3), |view, _, cx| {
                let palette = view.palette.as_ref()?.view.read(cx);
                let settled = palette.input_text(cx) == ENTER_PAGE && palette.answered();
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
        let _focus_return = self.escape_to_page(focus).await?;
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

#[derive(Clone, Copy)]
enum Typed {
    Character(char),
    Backspace,
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
