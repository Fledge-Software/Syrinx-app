//! Shows the clear-browsing-data dialog in the page card and reports each part's outcome (P1-27).
//! "Cookies and site data" also forgets the saved per-site zoom levels, as Chrome does (#90).

use gpui::{
    Context, Div, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, div, px,
};
use syrinx_platform::browser::{
    BrowserCommand, BrowserError, BrowsingDataKinds, ClearOutcome, ClearSpan,
};

use super::{BrowserView, button};
use crate::{
    session::{self, ClearedHistory, SessionCommand, SessionError},
    theme,
};

/// How far back a clear reaches.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Period {
    #[default]
    LastHour,
    LastDay,
    LastWeek,
    LastFourWeeks,
    AllTime,
}

impl Period {
    const ALL: [Self; 5] = [
        Self::LastHour,
        Self::LastDay,
        Self::LastWeek,
        Self::LastFourWeeks,
        Self::AllTime,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::LastHour => "Last hour",
            Self::LastDay => "Last 24 hours",
            Self::LastWeek => "Last 7 days",
            Self::LastFourWeeks => "Last 4 weeks",
            Self::AllTime => "All time",
        }
    }

    fn phrase(self) -> &'static str {
        match self {
            Self::LastHour => "from the last hour",
            Self::LastDay => "from the last 24 hours",
            Self::LastWeek => "from the last 7 days",
            Self::LastFourWeeks => "from the last 4 weeks",
            Self::AllTime => "from all time",
        }
    }

    /// The span in Unix seconds, or `None` for all time; the end includes the current second.
    fn span(self, now: i64) -> Option<ClearSpan> {
        let seconds = match self {
            Self::LastHour => 3_600,
            Self::LastDay => 86_400,
            Self::LastWeek => 7 * 86_400,
            Self::LastFourWeeks => 28 * 86_400,
            Self::AllTime => return None,
        };
        Some(ClearSpan {
            start: now.saturating_sub(seconds),
            end: now.saturating_add(1),
        })
    }
}

/// One choice the owner can tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    History,
    Cookies,
    Cache,
}

/// The outstanding parts of one submitted clear and what each reported.
#[derive(Clone, Debug, Default)]
struct Pending {
    request: u64,
    kinds: BrowsingDataKinds,
    period: Period,
    history: Option<Option<Result<ClearedHistory, String>>>,
    /// Saved zoom levels forgotten with cookies and site data: how many sites, or why not.
    site_zoom: Option<Option<Result<usize, String>>>,
    engine: Option<Option<Result<(), String>>>,
    /// A cleanup failure after the engine part, reported but not counted as a failed clear.
    cleanup: Option<String>,
}

impl Pending {
    fn finished(&self) -> bool {
        !matches!(self.history, Some(None))
            && !matches!(self.site_zoom, Some(None))
            && !matches!(self.engine, Some(None))
    }
}

/// The requests one submitted clear sends: to the session for Syrinx's own history and zoom
/// levels, and to the page engine for WebView2's data.
struct ClearRequests {
    history: Option<SessionCommand>,
    site_zoom: Option<SessionCommand>,
    engine: BrowserCommand,
}

/// The dialog's choices and the clear in flight, kept while closed so its outcome still lands.
#[derive(Default)]
pub(super) struct ClearDataDialog {
    open: bool,
    kinds: BrowsingDataKinds,
    period: Period,
    sequence: u64,
    pending: Option<Pending>,
    outcome: Option<String>,
}

impl ClearDataDialog {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn close(&mut self) {
        self.open = false;
    }

    fn show(&mut self) {
        // Opening starts from history only; cookies sign the owner out, so they are opt-in.
        if self.pending.is_none() {
            self.kinds = BrowsingDataKinds {
                history: true,
                ..BrowsingDataKinds::default()
            };
            self.outcome = None;
        }
        self.open = true;
    }

    fn toggle(&mut self, kind: Kind) {
        let flag = match kind {
            Kind::History => &mut self.kinds.history,
            Kind::Cookies => &mut self.kinds.cookies_and_site_data,
            Kind::Cache => &mut self.kinds.cached_files,
        };
        *flag = !*flag;
    }

    fn chosen(&self, kind: Kind) -> bool {
        match kind {
            Kind::History => self.kinds.history,
            Kind::Cookies => self.kinds.cookies_and_site_data,
            Kind::Cache => self.kinds.cached_files,
        }
    }

    /// Starts one clear at `now`, returning the session and page-engine requests to send.
    fn submit(&mut self, now: i64) -> Option<ClearRequests> {
        if self.pending.is_some() || self.kinds.is_empty() {
            return None;
        }
        self.sequence = self.sequence.wrapping_add(1);
        let request_id = self.sequence;
        let span = self.period.span(now);
        let (start, end) = span.map_or((0, i64::MAX), |span| (span.start, span.end));
        // Syrinx's own history and zoom levels are cleared by the session; WebView2's data by
        // the page engine.
        let history = self.kinds.history.then_some(SessionCommand::ClearHistory {
            request_id,
            start,
            end,
        });
        let site_zoom =
            self.kinds
                .cookies_and_site_data
                .then_some(SessionCommand::ClearSiteZooms {
                    request_id,
                    start,
                    end,
                });
        self.pending = Some(Pending {
            request: request_id,
            kinds: self.kinds,
            period: self.period,
            history: history.as_ref().map(|_| None),
            site_zoom: site_zoom.as_ref().map(|_| None),
            engine: Some(None),
            cleanup: None,
        });
        self.outcome = None;
        let engine = BrowserCommand::ClearData {
            request: request_id,
            kinds: self.kinds,
            span,
        };
        Some(ClearRequests {
            history,
            site_zoom,
            engine,
        })
    }

    /// Records the session's part; returns the finished summary once every part has reported.
    fn history_result(
        &mut self,
        request: u64,
        result: Result<ClearedHistory, SessionError>,
    ) -> Option<String> {
        let pending = self.pending.as_mut().filter(|p| p.request == request)?;
        pending.history = Some(Some(result.map_err(|error| error.to_string())));
        self.finish()
    }

    /// Records the zoom-level part; returns the summary once every part has reported.
    fn site_zoom_result(
        &mut self,
        request: u64,
        result: Result<usize, SessionError>,
    ) -> Option<String> {
        let pending = self.pending.as_mut().filter(|p| p.request == request)?;
        pending.site_zoom = Some(Some(result.map_err(|error| error.to_string())));
        self.finish()
    }

    fn engine_result(&mut self, request: u64, outcome: ClearOutcome) -> Option<String> {
        let pending = self.pending.as_mut().filter(|p| p.request == request)?;
        pending.engine = Some(Some(outcome.cleared.map_err(engine_error)));
        pending.cleanup = outcome.closed.err().map(engine_error);
        self.finish()
    }

    /// The session stopped before answering, so its parts can no longer arrive.
    fn session_lost(&mut self) -> Option<String> {
        let pending = self.pending.as_mut()?;
        let mut waiting = false;
        if matches!(pending.history, Some(None)) {
            pending.history = session_part_lost();
            waiting = true;
        }
        if matches!(pending.site_zoom, Some(None)) {
            pending.site_zoom = session_part_lost();
            waiting = true;
        }
        if !waiting {
            return None;
        }
        self.finish()
    }

    fn finish(&mut self) -> Option<String> {
        if !self.pending.as_ref()?.finished() {
            return None;
        }
        let pending = self.pending.take()?;
        let summary = summary(&pending);
        self.outcome = Some(summary.clone());
        Some(summary)
    }

    fn status(&self) -> Option<&str> {
        if self.pending.is_some() {
            return Some("Clearing…");
        }
        self.outcome.as_deref()
    }
}

/// A session part that can no longer answer, reported as storage being unavailable.
fn session_part_lost<T>() -> Option<Option<Result<T, String>>> {
    Some(Some(Err(SessionError::StorageUnavailable.to_string())))
}

/// Names what was cleared and, separately, each part that failed and why.
fn summary(pending: &Pending) -> String {
    let kinds = pending.kinds;
    let mut cleared = Vec::new();
    let mut failures = Vec::new();
    if let Some(Some(result)) = &pending.history {
        match result {
            Ok(history) => cleared.push(format!(
                "Syrinx's browsing history ({} visits; {} sites no longer listed)",
                history.visits, history.addresses
            )),
            Err(error) => failures.push(format!(
                "Syrinx's browsing history was not cleared: {error}."
            )),
        }
    }
    if let Some(Some(result)) = &pending.site_zoom {
        match result {
            Ok(sites) => cleared.push(format!("saved zoom levels ({sites} sites)")),
            Err(error) => failures.push(format!("Saved zoom levels were not cleared: {error}.")),
        }
    }
    let engine = engine_kinds(kinds);
    if let Some(Some(result)) = &pending.engine {
        match result {
            Ok(()) => cleared.push(engine.clone()),
            Err(error) => failures.push(format!(
                "The page engine's {engine} were not cleared: {error}."
            )),
        }
    }
    let mut message = String::new();
    if !cleared.is_empty() {
        message = format!(
            "Cleared {} {}.",
            cleared.join(" and "),
            pending.period.phrase()
        );
    }
    if let Some(error) = &pending.cleanup {
        failures.push(format!(
            "The page engine's temporary view did not close: {error}; restart Syrinx if pages misbehave."
        ));
    }
    for failure in failures {
        if !message.is_empty() {
            message.push(' ');
        }
        message.push_str(&failure);
    }
    message
}

/// Names a page-engine failure in words, keeping WebView2's code for diagnosis.
fn engine_error(error: BrowserError) -> String {
    match error {
        BrowserError::ClearData { code } => {
            format!("WebView2 refused (error 0x{code:08X})")
        }
        BrowserError::QueueClosed | BrowserError::Wake => {
            "the page engine is not running".to_owned()
        }
        _ => "the page engine failed".to_owned(),
    }
}

fn engine_kinds(kinds: BrowsingDataKinds) -> String {
    let mut names = Vec::new();
    if kinds.history {
        names.push("visited-page records");
    }
    if kinds.cookies_and_site_data {
        names.push("cookies and site data");
    }
    if kinds.cached_files {
        names.push("cached files");
    }
    names.join(", ")
}

impl BrowserView {
    /// Shows the dialog in the page card in place of the history list.
    pub(super) fn open_clear_data(&mut self) {
        if self.closing || self.session_revision.is_none() {
            return;
        }
        self.history.close();
        self.clear_data.show();
        self.sync_pages();
    }

    fn close_clear_data(&mut self) {
        self.clear_data.close();
        self.sync_pages();
    }

    fn submit_clear_data(&mut self) {
        let now = match session::utc_seconds() {
            Ok(now) => now,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        let Some(requests) = self.clear_data.submit(now) else {
            return;
        };
        let ClearRequests {
            history,
            site_zoom,
            engine,
        } = requests;
        let BrowserCommand::ClearData { request, .. } = &engine else {
            return;
        };
        let request = *request;
        // A request that could not be queued reports at once, so the dialog never waits forever.
        if let Some(command) = history
            && !self.try_command(command)
        {
            self.history_cleared(request, Err(SessionError::StorageUnavailable));
        }
        if let Some(command) = site_zoom
            && !self.try_command(command)
        {
            self.site_zooms_cleared_for_dialog(request, Err(SessionError::StorageUnavailable));
        }
        if !self.page_command(engine) {
            let refused = ClearOutcome {
                cleared: Err(BrowserError::QueueClosed),
                closed: Ok(()),
            };
            self.browser_data_cleared(request, refused);
        }
    }

    pub(super) fn history_cleared(
        &mut self,
        request: u64,
        result: Result<ClearedHistory, SessionError>,
    ) {
        if let Some(summary) = self.clear_data.history_result(request, result) {
            self.message = summary;
        }
    }

    /// The zoom-level part of a clear finished; `site_zooms_cleared` has applied it.
    pub(super) fn site_zooms_cleared_for_dialog(
        &mut self,
        request: u64,
        result: Result<usize, SessionError>,
    ) {
        if let Some(summary) = self.clear_data.site_zoom_result(request, result) {
            self.message = summary;
        }
    }

    pub(super) fn browser_data_cleared(&mut self, request: u64, outcome: ClearOutcome) {
        if let Some(summary) = self.clear_data.engine_result(request, outcome) {
            self.message = summary;
        }
    }

    /// Ends a clear whose session part can no longer answer; the failure itself stays shown.
    pub(super) fn clear_data_session_lost(&mut self) {
        if let Some(summary) = self.clear_data.session_lost() {
            self.message = summary;
        }
    }
}

/// The page card's contents while the dialog is open.
pub(super) fn render(dialog: &ClearDataDialog, cx: &mut Context<BrowserView>) -> Div {
    let header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().text_xl().child("Clear browsing data"))
        .child(
            button("clear-data-close", "Close").on_click(cx.listener(|this, _, _, cx| {
                this.close_clear_data();
                cx.notify();
            })),
        );
    let mut kinds = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(heading("What to clear"));
    for (kind, label, detail) in [
        (
            Kind::History,
            "Browsing history",
            "Syrinx's history, remembered page text and search results, and the page \
             engine's record of visits",
        ),
        (
            Kind::Cookies,
            "Cookies and site data",
            "Signs you out of most sites, including Google",
        ),
        (
            Kind::Cache,
            "Cached files",
            "Pages may load more slowly the next time",
        ),
    ] {
        kinds = kinds.child(choice_row(dialog, kind, label, detail, cx));
    }
    let mut periods = div().flex().flex_wrap().gap_2();
    for period in Period::ALL {
        periods = periods.child(period_chip(dialog, period, cx));
    }
    let ready = dialog.pending.is_none() && !dialog.kinds.is_empty();
    let clear = button("clear-data-submit", "Clear data")
        .when_ready(ready)
        .on_click(cx.listener(|this, _, _, cx| {
            this.submit_clear_data();
            cx.notify();
        }));
    let mut body = div()
        .size_full()
        .p_4()
        .flex()
        .flex_col()
        .gap_4()
        .bg(theme::ground())
        .child(header)
        .child(kinds)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(heading("Time range"))
                .child(periods),
        )
        .child(div().flex().child(clear));
    if let Some(status) = dialog.status() {
        body = body.child(div().text_color(theme::icon()).child(status.to_owned()));
    }
    body
}

fn heading(text: &'static str) -> Div {
    div().text_xs().text_color(theme::icon_dim()).child(text)
}

fn choice_row(
    dialog: &ClearDataDialog,
    kind: Kind,
    label: &'static str,
    detail: &'static str,
    cx: &mut Context<BrowserView>,
) -> impl gpui::IntoElement {
    let mark = if dialog.chosen(kind) { "☑" } else { "☐" };
    crate::controls::control(div().id(label), gpui::Role::CheckBox, label)
        .aria_description(detail)
        .aria_toggled(if dialog.chosen(kind) {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .h(px(44.))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .gap_3()
        .cursor_pointer()
        .hover(|style| style.bg(theme::preview_row_selected()))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.clear_data.toggle(kind);
            cx.notify();
        }))
        .child(div().text_lg().child(mark))
        .child(
            div()
                .flex()
                .flex_col()
                .child(label)
                .child(div().text_xs().text_color(theme::icon_dim()).child(detail)),
        )
}

fn period_chip(
    dialog: &ClearDataDialog,
    period: Period,
    cx: &mut Context<BrowserView>,
) -> impl gpui::IntoElement {
    let chosen = dialog.period == period;
    crate::controls::control(
        div().id(period.label()),
        gpui::Role::RadioButton,
        period.label(),
    )
    .aria_toggled(if chosen {
        gpui::Toggled::True
    } else {
        gpui::Toggled::False
    })
    .px_3()
    .py_1()
    .rounded_md()
    .cursor_pointer()
    .bg(if chosen {
        theme::accent_fill()
    } else {
        theme::line()
    })
    .text_color(if chosen {
        theme::on_accent_fill()
    } else {
        theme::text()
    })
    .on_click(cx.listener(move |this, _, _, cx| {
        this.clear_data.period = period;
        cx.notify();
    }))
    .child(period.label())
}

/// Dims an action that cannot run yet; clicks still reach the handler, which refuses them.
trait ReadyStyle {
    fn when_ready(self, ready: bool) -> Self;
}

impl ReadyStyle for gpui::Stateful<Div> {
    fn when_ready(self, ready: bool) -> Self {
        if ready { self } else { self.opacity(0.5) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleared(result: Result<(), BrowserError>) -> ClearOutcome {
        ClearOutcome {
            cleared: result,
            closed: Ok(()),
        }
    }

    fn submitted(dialog: &mut ClearDataDialog) -> (Option<SessionCommand>, BrowserCommand) {
        let requests = dialog.submit(10_000).expect("a clear starts");
        (requests.history, requests.engine)
    }

    /// The zoom-level request a submitted clear sends, if any, and its page-engine request id.
    fn zoom_request(dialog: &mut ClearDataDialog) -> (Option<(u64, i64, i64)>, u64) {
        let requests = dialog.submit(10_000).expect("a clear starts");
        let zoom = requests.site_zoom.map(|command| {
            let SessionCommand::ClearSiteZooms {
                request_id,
                start,
                end,
            } = command
            else {
                panic!("zoom levels are cleared by the session");
            };
            (request_id, start, end)
        });
        let BrowserCommand::ClearData { request, .. } = requests.engine else {
            panic!("the page engine clears its own data");
        };
        (zoom, request)
    }

    #[test]
    fn cookies_and_site_data_also_clear_zoom_levels_over_the_chosen_range() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        dialog.toggle(Kind::History);
        dialog.toggle(Kind::Cookies);
        dialog.period = Period::LastWeek;
        let (zoom, request) = zoom_request(&mut dialog);
        assert_eq!(zoom, Some((request, 10_000 - 7 * 86_400, 10_001)));
        // Both the engine and the zoom part must report before the clear ends.
        assert_eq!(dialog.engine_result(request, cleared(Ok(()))), None);
        let summary = dialog
            .site_zoom_result(request, Ok(2))
            .expect("both parts reported");
        assert_eq!(
            summary,
            "Cleared saved zoom levels (2 sites) and cookies and site data from the last 7 days."
        );
    }

    #[test]
    fn an_all_time_cookie_clear_reaches_every_zoom_level_and_reports_a_failure() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        dialog.toggle(Kind::History);
        dialog.toggle(Kind::Cookies);
        dialog.period = Period::AllTime;
        let (zoom, request) = zoom_request(&mut dialog);
        assert_eq!(zoom, Some((request, 0, i64::MAX)));
        dialog.engine_result(request, cleared(Ok(())));
        let summary = dialog
            .site_zoom_result(request, Err(SessionError::StorageUnavailable))
            .expect("both parts reported");
        assert!(summary.starts_with("Cleared cookies and site data from all time."));
        assert!(summary.contains("Saved zoom levels were not cleared"));
    }

    #[test]
    fn a_history_only_clear_leaves_zoom_levels_alone() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        let (zoom, _) = zoom_request(&mut dialog);
        assert_eq!(zoom, None);
    }

    #[test]
    fn a_history_clear_sends_matching_spans_and_waits_for_both_parts() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        let (history, engine) = submitted(&mut dialog);
        let Some(SessionCommand::ClearHistory {
            request_id,
            start,
            end,
        }) = history
        else {
            panic!("history is cleared by the session");
        };
        assert_eq!((start, end), (10_000 - 3_600, 10_001));
        let BrowserCommand::ClearData {
            request,
            kinds,
            span,
        } = engine
        else {
            panic!("the page engine clears its own copy");
        };
        assert_eq!(request, request_id);
        assert!(kinds.history && !kinds.cookies_and_site_data);
        assert_eq!(span, Some(ClearSpan { start, end }));

        // A second submit waits; a stale reply is ignored; both parts must report.
        assert!(dialog.submit(10_000).is_none());
        let history = ClearedHistory {
            visits: 5,
            addresses: 3,
        };
        assert_eq!(dialog.history_result(request + 1, Ok(history)), None);
        assert_eq!(dialog.history_result(request, Ok(history)), None);
        let summary = dialog
            .engine_result(request, cleared(Ok(())))
            .expect("both parts reported");
        assert!(
            summary.starts_with(
                "Cleared Syrinx's browsing history (5 visits; 3 sites no longer listed)"
            )
        );
        assert!(summary.ends_with("from the last hour."));
        assert!(dialog.submit(10_000).is_some());
    }

    #[test]
    fn a_partial_failure_names_what_was_cleared_and_what_was_not() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        dialog.toggle(Kind::Cookies);
        dialog.period = Period::AllTime;
        let (history, engine) = submitted(&mut dialog);
        let Some(SessionCommand::ClearHistory { start, end, .. }) = history else {
            panic!("history is cleared by the session");
        };
        assert_eq!((start, end), (0, i64::MAX));
        let BrowserCommand::ClearData { request, span, .. } = engine else {
            panic!("the page engine clears its own copy");
        };
        assert_eq!(span, None);
        dialog.history_result(request, Ok(ClearedHistory::default()));
        dialog.site_zoom_result(request, Ok(0));
        let refused = BrowserError::ClearData {
            code: 0x8007_0005_u32 as i32,
        };
        let summary = dialog
            .engine_result(request, cleared(Err(refused)))
            .expect("both parts reported");
        assert!(summary.starts_with(
            "Cleared Syrinx's browsing history (0 visits; 0 sites no longer listed) and saved zoom levels (0 sites) from all time."
        ));
        assert!(summary.contains("visited-page records, cookies and site data were not cleared"));
        assert!(summary.contains("error 0x80070005"));
    }

    #[test]
    fn a_cleanup_failure_is_reported_apart_from_a_successful_clear() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        dialog.toggle(Kind::History);
        dialog.toggle(Kind::Cache);
        let (_, engine) = submitted(&mut dialog);
        let BrowserCommand::ClearData { request, .. } = engine else {
            panic!("the page engine clears its cache");
        };
        let outcome = ClearOutcome {
            cleared: Ok(()),
            closed: Err(BrowserError::ClearData { code: 1 }),
        };
        let summary = dialog.engine_result(request, outcome).expect("reported");
        assert!(summary.starts_with("Cleared cached files from the last hour."));
        assert!(summary.contains("temporary view did not close"));
    }

    #[test]
    fn a_stopped_session_ends_the_wait_for_history() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        let (_, engine) = submitted(&mut dialog);
        let BrowserCommand::ClearData { request, .. } = engine else {
            panic!("the page engine clears its own copy");
        };
        assert_eq!(dialog.engine_result(request, cleared(Ok(()))), None);
        let summary = dialog.session_lost().expect("the clear ends");
        assert!(summary.contains("Syrinx's browsing history was not cleared"));
        assert!(dialog.submit(10_000).is_some());
        assert_eq!(dialog.session_lost(), None);
    }

    #[test]
    fn cache_only_clears_need_no_session_reply_and_nothing_chosen_starts_nothing() {
        let mut dialog = ClearDataDialog::default();
        dialog.show();
        dialog.toggle(Kind::History);
        assert!(dialog.submit(10_000).is_none());
        dialog.toggle(Kind::Cache);
        let (history, engine) = submitted(&mut dialog);
        assert!(history.is_none());
        let BrowserCommand::ClearData { request, .. } = engine else {
            panic!("the page engine clears its cache");
        };
        let summary = dialog
            .engine_result(request, cleared(Ok(())))
            .expect("one part reported");
        assert_eq!(summary, "Cleared cached files from the last hour.");
        assert_eq!(dialog.status(), Some(summary.as_str()));
    }
}
