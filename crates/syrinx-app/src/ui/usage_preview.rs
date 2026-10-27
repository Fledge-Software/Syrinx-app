//! Settings' usage report preview and its only two exports, Save report… and Copy report
//! (AN-02); the table is read from the very text that leaves.

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

use futures::channel::oneshot;
use gpui::{
    App, ClipboardItem, Context, Div, FontWeight, InteractiveElement, ParentElement, Role,
    Stateful, StatefulInteractiveElement, Styled, Toggled, Window, div, px,
};
use syrinx_core::usage::{
    ReportGroup, ReportPeriod, ReportRow, ReportTable, ReportValue, UsageFileError, read_report,
};
use syrinx_platform::file_dialog::choose_new_file;
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;

use super::BrowserView;
use super::customize::switch_control;
use super::updates::pill;
use crate::controls::control;
use crate::session::{SessionCommand, SessionError};
use crate::theme::rgba;

/// The approved suggested prompt (`docs/usage-report-privacy.md`).
pub(super) const SUGGESTED_PROMPT: &str = "This is a usage report from Syrinx, a desktop browser \
     I use. It contains only aggregate counts and timing buckets for one period, described in its \
     \"about\" field. Suggest up to five changes that might make Syrinx faster or easier for me, \
     each tied to the numbers that support it. Say what each number cannot tell you, and do not \
     assume why I did something from how often I did it.";

/// One period's report: its exact text and the table read back from that text.
#[derive(Debug)]
pub(super) struct ReportPreview {
    period: ReportPeriod,
    /// The exact bytes Save writes and Copy places on the clipboard.
    text: Arc<str>,
    table: ReportTable,
    start: String,
    end: String,
}

impl ReportPreview {
    /// Reads the table from the text itself; a text it cannot read is never shown or exported.
    pub(super) fn from_text(period: ReportPeriod, text: String) -> Result<Self, UsageFileError> {
        let table = read_report(&text)?;
        let date = |key| {
            table
                .envelope_value(key)
                .map(str::to_owned)
                .ok_or(UsageFileError::Malformed)
        };
        let (start, end) = (date("period_start")?, date("period_end")?);
        Ok(Self {
            period,
            text: text.into(),
            table,
            start,
            end,
        })
    }

    /// What the preview shows, Save writes and Copy copies: one unchanged text.
    pub(super) fn exported(&self) -> Arc<str> {
        self.text.clone()
    }

    pub(super) fn suggested_name(&self) -> String {
        format!("syrinx-usage-report-{}-to-{}.json", self.start, self.end)
    }
}

/// What the report view shows.
#[derive(Debug, Default)]
enum ReportView {
    #[default]
    Closed,
    Loading {
        request_id: u64,
        period: ReportPeriod,
    },
    Shown(ReportPreview),
    Failed {
        period: ReportPeriod,
        message: String,
    },
}

/// The report view's state, kept apart from drawing so its rules can be tested.
#[derive(Debug, Default)]
pub(super) struct ReportState {
    view: ReportView,
    /// "Show exact file" is on.
    exact: bool,
    last_request: u64,
    /// A save dialog or write is running; nothing else is exported meanwhile.
    saving: bool,
    /// The last export's outcome, shown under the buttons.
    notice: Option<String>,
}

impl ReportState {
    pub(super) fn is_open(&self) -> bool {
        !matches!(self.view, ReportView::Closed)
    }

    /// Asks for a period's report; an answer to any earlier request is then ignored.
    pub(super) fn request(&mut self, period: ReportPeriod) -> u64 {
        self.last_request += 1;
        self.view = ReportView::Loading {
            request_id: self.last_request,
            period,
        };
        self.notice = None;
        self.last_request
    }

    /// The worker's answer; only the latest request's answer is shown.
    pub(super) fn received(
        &mut self,
        request_id: u64,
        answered: ReportPeriod,
        result: Result<String, SessionError>,
    ) {
        let ReportView::Loading {
            request_id: waiting,
            period,
        } = self.view
        else {
            return;
        };
        if waiting != request_id || answered != period {
            return;
        }
        self.view = match result {
            Ok(text) => match ReportPreview::from_text(period, text) {
                Ok(preview) => ReportView::Shown(preview),
                Err(error) => ReportView::Failed {
                    period,
                    message: format!("Syrinx couldn't show this report: {error}."),
                },
            },
            Err(error) => ReportView::Failed {
                period,
                message: format!("{error}."),
            },
        };
    }

    pub(super) fn close(&mut self) {
        self.view = ReportView::Closed;
        self.notice = None;
    }

    /// Once counting is off its counts are deleted, so nothing shown may still be exported.
    pub(super) fn counting_changed(&mut self, counting: bool) {
        if !counting {
            self.close();
        }
    }

    /// The report Save and Copy export; `None` with nothing to export or while a save runs.
    pub(super) fn exportable(&self) -> Option<&ReportPreview> {
        match &self.view {
            ReportView::Shown(preview) if !self.saving => Some(preview),
            _ => None,
        }
    }

    fn period(&self) -> Option<ReportPeriod> {
        match &self.view {
            ReportView::Closed => None,
            ReportView::Loading { period, .. } | ReportView::Failed { period, .. } => Some(*period),
            ReportView::Shown(preview) => Some(preview.period),
        }
    }
}

/// How a save ended, reported without the chosen path.
enum SaveOutcome {
    Cancelled,
    Saved,
    Failed(String),
}

impl BrowserView {
    /// "View report": asks the session worker for the period's exact text.
    pub(super) fn open_usage_report(&mut self, period: ReportPeriod) {
        if !self.usage.counting() {
            return;
        }
        let request_id = self.usage.report.request(period);
        self.command(SessionCommand::UsageReport { request_id, period });
    }

    pub(super) fn usage_report_received(
        &mut self,
        request_id: u64,
        period: ReportPeriod,
        result: Result<String, SessionError>,
    ) {
        self.usage.report.received(request_id, period, result);
    }

    /// "Save report…": Windows' save dialog, then the exact shown bytes, on their own thread.
    fn save_usage_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.usage.report.exportable() else {
            return;
        };
        let (bytes, name) = (preview.exported(), preview.suggested_name());
        let (sender, receiver) = oneshot::channel();
        let owner = self.parent_hwnd;
        let spawned = thread::Builder::new()
            .name("syrinx-usage-report-save".to_owned())
            .spawn(move || {
                if sender.send(choose_and_save(&name, &bytes, owner)).is_err() {
                    eprintln!("The usage report save finished after its window closed.");
                }
            });
        if spawned.is_err() {
            self.usage.report.notice = Some("Syrinx couldn't open the save dialog.".to_owned());
            return;
        }
        self.usage.report.saving = true;
        self.usage.report.notice = None;
        cx.spawn_in(window, async move |view, cx| {
            let outcome = receiver.await.unwrap_or_else(|_| {
                SaveOutcome::Failed("The save stopped unexpectedly.".to_owned())
            });
            if view
                .update(cx, |view, cx| view.usage_report_saved(outcome, cx))
                .is_err()
            {
                eprintln!("The usage report save finished after the browser window closed.");
            }
        })
        .detach();
    }

    fn usage_report_saved(&mut self, outcome: SaveOutcome, cx: &mut Context<Self>) {
        let report = &mut self.usage.report;
        report.saving = false;
        report.notice = match outcome {
            SaveOutcome::Cancelled => None,
            SaveOutcome::Saved => Some("Report saved.".to_owned()),
            SaveOutcome::Failed(reason) => Some(reason),
        };
        cx.notify();
    }

    /// "Copy report": the exact shown text, checked on the clipboard afterwards.
    fn copy_usage_report(&mut self, cx: &mut App) {
        let Some(preview) = self.usage.report.exportable() else {
            return;
        };
        let copied = put_on_clipboard(cx, &preview.exported());
        self.usage.report.notice = Some(if copied {
            "Report copied. Paste it into the model chat you choose.".to_owned()
        } else {
            "Syrinx couldn't put the report on the clipboard.".to_owned()
        });
    }

    fn copy_suggested_prompt(&mut self, cx: &mut App) {
        let copied = put_on_clipboard(cx, SUGGESTED_PROMPT);
        self.usage.report.notice = Some(if copied {
            "Prompt copied.".to_owned()
        } else {
            "Syrinx couldn't put the prompt on the clipboard.".to_owned()
        });
    }

    /// Replaces the Usage report section while the owner reads a report.
    pub(super) fn usage_report_view(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let report = &self.usage.report;
        let mut view = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.report_header(palette, cx))
            .child(self.period_choice(palette, cx));
        view = match &report.view {
            ReportView::Closed | ReportView::Loading { .. } => {
                view.child(note(palette, "Reading…"))
            }
            ReportView::Failed { message, .. } => view.child(note(palette, message)),
            ReportView::Shown(preview) => {
                view.child(self.exact_switch(palette, cx))
                    .child(if report.exact {
                        exact_text(palette, &preview.text)
                    } else {
                        report_table(palette, &preview.table)
                    })
            }
        };
        view = view.child(self.export_buttons(palette, cx));
        if let Some(notice) = &report.notice {
            view = view.child(note(palette, notice));
        }
        view.child(self.prompt_block(palette, cx))
    }

    fn report_header(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let back = control(
            div().id("usage-report-back"),
            Role::Button,
            "Back to Usage report",
        )
        .size(px(28.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgba(palette.icon_dim))
        .cursor_pointer()
        .child("‹")
        .on_click(cx.listener(|this, _, _, cx| {
            this.usage.report.close();
            cx.notify();
        }));
        div().flex().items_center().gap(px(8.)).child(back).child(
            div()
                .flex_1()
                .text_size(px(text::SUBTITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Usage report"),
        )
    }

    /// Current or previous period; the previous one only once a period has closed.
    fn period_choice(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let chosen = self.usage.report.period();
        let mut row = div().flex().items_center().gap(px(8.));
        for (period, id, name) in [
            (
                ReportPeriod::Current,
                "usage-report-current",
                "Current period",
            ),
            (
                ReportPeriod::Previous,
                "usage-report-previous",
                "Previous period",
            ),
        ] {
            let available = period == ReportPeriod::Current || self.usage.has_previous();
            let button = period_button(palette, id, name, chosen == Some(period), available);
            if !available {
                row = row.child(button);
                continue;
            }
            row = row.child(button.on_click(cx.listener(move |this, _, _, cx| {
                this.open_usage_report(period);
                cx.notify();
            })));
        }
        row
    }

    fn exact_switch(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let exact = self.usage.report.exact;
        let switch = switch_control("usage-report-exact", "Show exact file", exact, palette)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.usage.report.exact = !exact;
                cx.notify();
            }));
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child("Show exact file"),
            )
            .child(switch)
    }

    /// Save report… and Copy report, usable only while a report is shown and no save runs.
    fn export_buttons(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let ready = self.usage.report.exportable().is_some();
        let save = action_button(palette, "usage-report-save", "Save report…", ready);
        let copy = action_button(palette, "usage-report-copy", "Copy report", ready);
        let (save, copy) = if ready {
            (
                save.on_click(cx.listener(|this, _, window, cx| {
                    this.save_usage_report(window, cx);
                    cx.notify();
                })),
                copy.on_click(cx.listener(|this, _, _, cx| {
                    this.copy_usage_report(cx);
                    cx.notify();
                })),
            )
        } else {
            (save, copy)
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(save)
            .child(copy)
    }

    fn prompt_block(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let copy = action_button(palette, "usage-report-prompt", "Copy prompt", true).on_click(
            cx.listener(|this, _, _, cx| {
                this.copy_suggested_prompt(cx);
                cx.notify();
            }),
        );
        div()
            .pt(px(12.))
            .border_t_1()
            .border_color(rgba(palette.panel_divider))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Suggested prompt"),
            )
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(SUGGESTED_PROMPT),
            )
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child(
                        "Syrinx sends nothing. Paste the report and this prompt into a model chat \
                         yourself; whoever reads the report can tell it is yours.",
                    ),
            )
            .child(div().flex().child(copy))
    }
}

/// Runs on the save thread: the dialog, then the exact bytes. Logs nothing about the path.
fn choose_and_save(name: &str, bytes: &str, owner: isize) -> SaveOutcome {
    let path = match choose_new_file("Save usage report", name, owner) {
        Ok(Some(path)) => path,
        Ok(None) => return SaveOutcome::Cancelled,
        Err(error) => return SaveOutcome::Failed(format!("{error}.")),
    };
    match write_report(&path, bytes.as_bytes()) {
        Ok(()) => SaveOutcome::Saved,
        Err(_) => SaveOutcome::Failed("Syrinx couldn't write the report to that file.".to_owned()),
    }
}

/// Writes exactly `bytes` to the owner's chosen file and flushes it to disk.
fn write_report(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, |file| file.write_all(bytes))
}

/// Fills a sibling temporary file, flushes it and renames it over `path`, so a failure
/// leaves the chosen file as it was and no partial copy behind.
fn replace_with(path: &Path, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let temporary = temporary_path(path)?;
    // Opening fails if the name is taken; that file is not this save's, so it is left alone.
    let file = create_temporary(&temporary)?;
    let replaced = fill_and_flush(file, fill).and_then(|()| fs::rename(&temporary, path));
    let Err(error) = replaced else {
        return Ok(());
    };
    // Only the file this call created is removed; a failed removal replaces the error.
    remove_partial(&temporary)?;
    Err(error)
}

/// `<chosen name>.partial` beside the chosen file.
fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    let Some(name) = path.file_name() else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    let mut temporary: OsString = name.to_owned();
    temporary.push(".partial");
    Ok(path.with_file_name(temporary))
}

/// Creates the temporary file, never opening an existing one.
fn create_temporary(temporary: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
}

/// Writes and flushes the temporary file, closing it before it is renamed or removed.
fn fill_and_flush(
    mut file: File,
    fill: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    fill(&mut file)?;
    file.sync_all()
}

fn remove_partial(temporary: &Path) -> io::Result<()> {
    match fs::remove_file(temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The one place Syrinx writes the clipboard; reading it back shows whether Windows took it.
fn put_on_clipboard(cx: &mut App, text: &str) -> bool {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .is_some_and(|copied| copied == text)
}

fn note(palette: &Palette, message: &str) -> Div {
    div()
        .text_size(px(text::SMALL))
        .text_color(rgba(palette.text_secondary))
        .child(message.to_owned())
}

/// A period radio button; an unavailable one is drawn dimmed and takes no click or Tab stop.
fn period_button(
    palette: &Palette,
    id: &'static str,
    name: &'static str,
    selected: bool,
    available: bool,
) -> Stateful<Div> {
    let toggled = if selected {
        Toggled::True
    } else {
        Toggled::False
    };
    let element = if available {
        control(div().id(id), Role::RadioButton, name)
    } else {
        unavailable(div().id(id), Role::RadioButton, name)
    };
    let mut button = pill(palette, element).aria_toggled(toggled).child(name);
    if selected {
        button = button
            .bg(rgba(palette.accent.soft))
            .border_color(rgba(palette.accent.soft_line))
            .text_color(rgba(palette.text));
    }
    if !available {
        button = button.opacity(0.5);
    }
    button
}

/// A pill button; a disabled one is dimmed, says why, and takes no click or Tab stop.
fn action_button(
    palette: &Palette,
    id: &'static str,
    name: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    if enabled {
        return pill(palette, control(div().id(id), Role::Button, name)).child(name);
    }
    pill(palette, unavailable(div().id(id), Role::Button, name))
        .opacity(0.5)
        .child(name)
}

/// A control that cannot be used now: named for screen readers, but not focusable.
pub(super) fn unavailable(element: Stateful<Div>, role: Role, name: &'static str) -> Stateful<Div> {
    element
        .role(role)
        .aria_label(name)
        .aria_description("Unavailable: there is no report to export")
}

/// The exact text, line by line in a monospace face, as it would be saved or copied.
fn exact_text(palette: &Palette, report: &str) -> Div {
    let mut block = div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .bg(rgba(palette.field))
        .p(px(10.))
        .font_family(text::MONO_FONT_FAMILIES[0])
        .text_size(px(text::CAPTION))
        .text_color(rgba(palette.text));
    for line in report.lines() {
        block = block.child(div().child(line.to_owned()));
    }
    block
}

/// The report as a readable table: the envelope, then each group of counters.
fn report_table(palette: &Palette, table: &ReportTable) -> Div {
    let mut envelope = group_block(palette, "About this report");
    for (key, value) in &table.envelope {
        envelope = envelope.child(count_row(palette, key, value));
    }
    let mut blocks = div().flex().flex_col().gap(px(14.)).child(envelope);
    for group in &table.groups {
        blocks = blocks.child(group_rows(palette, group));
    }
    blocks
}

fn group_rows(palette: &Palette, group: &ReportGroup) -> Div {
    let title = match group.key.as_str() {
        "counts" => "Features",
        "commands" => "Commands",
        "timings" => "Timings (milliseconds)",
        "errors" => "Errors",
        other => other,
    };
    let mut block = group_block(palette, title);
    for row in &group.rows {
        block = block.child(table_row(palette, row));
    }
    block
}

fn group_block(palette: &Palette, title: &str) -> Div {
    div().flex().flex_col().gap(px(4.)).child(
        div()
            .text_size(px(text::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(palette.text_muted))
            .child(title.to_uppercase()),
    )
}

fn table_row(palette: &Palette, row: &ReportRow) -> Div {
    match &row.value {
        ReportValue::Count(count) => count_row(palette, &row.key, &count.to_string()),
        ReportValue::Buckets(buckets) => {
            let mut cells = div().flex().flex_wrap().gap(px(8.));
            for (bucket, count) in buckets {
                cells = cells.child(format!("{bucket}: {count}"));
            }
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .text_size(px(text::SMALL))
                .child(
                    div()
                        .text_color(rgba(palette.text_secondary))
                        .child(row.key.clone()),
                )
                .child(cells.text_color(rgba(palette.text)))
        }
    }
}

/// One field and its value as written in the file.
fn count_row(palette: &Palette, key: &str, value: &str) -> Div {
    div()
        .flex()
        .gap(px(12.))
        .text_size(px(text::SMALL))
        .child(
            div()
                .flex_none()
                .w(px(170.))
                .text_color(rgba(palette.text_secondary))
                .child(key.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(rgba(palette.text))
                .child(value.to_owned()),
        )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use syrinx_core::usage::{Feature, UsageDate, UsageFile, UsageRecorder};

    use super::*;
    use crate::session::UsageError;

    /// A synthetic report: a period from 2026-11-02 with a few counts, exported 2026-11-30.
    fn synthetic_report() -> String {
        let date = |day| UsageDate::from_ymd(2026, 11, day).expect("synthetic date");
        let recorder = UsageRecorder::new();
        recorder.switch_on();
        recorder.count(Feature::TabCreated);
        recorder.count(Feature::PaletteOpenedCtrlK);
        let mut file = UsageFile::begin(date(2));
        file.add(&recorder.take().expect("synthetic counts"));
        file.report_text(ReportPeriod::Current, date(30))
            .expect("the current period always reports")
    }

    fn shown(text: &str) -> ReportState {
        let mut state = ReportState::default();
        let request = state.request(ReportPeriod::Current);
        state.received(request, ReportPeriod::Current, Ok(text.to_owned()));
        state
    }

    /// A fresh synthetic folder for a saved report, removed by the test that made it.
    fn synthetic_folder() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "syrinx-an02-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let folder = std::env::temp_dir().join(name);
        fs::create_dir(&folder).expect("a fresh synthetic folder");
        folder
    }

    #[test]
    fn the_export_is_the_preview() {
        let text = synthetic_report();
        let state = shown(&text);
        let preview = state.exportable().expect("a shown report is exportable");

        // The exact-file view, the table, the saved file and the clipboard text are one text.
        let exported = preview.exported();
        assert_eq!(exported.as_bytes(), text.as_bytes());
        let shown_lines: String = exported.lines().map(|line| format!("{line}\n")).collect();
        assert_eq!(
            shown_lines, text,
            "the exact view's lines rebuild every byte"
        );
        assert_eq!(read_report(&exported), Ok(preview.table.clone()));
        let clipboard = ClipboardItem::new_string(exported.to_string());
        assert_eq!(clipboard.text().as_deref(), Some(text.as_str()));

        let folder = synthetic_folder();
        let path = folder.join(preview.suggested_name());
        write_report(&path, exported.as_bytes()).expect("the synthetic folder is writable");
        let saved = fs::read(&path).expect("the saved report reads back");
        fs::remove_dir_all(&folder).expect("the synthetic folder is removed");
        assert_eq!(
            saved,
            text.as_bytes(),
            "saved byte for byte, normalising nothing"
        );
    }

    #[test]
    fn the_suggested_name_carries_the_two_period_dates() {
        let state = shown(&synthetic_report());
        let preview = state.exportable().expect("a shown report is exportable");
        assert_eq!(
            preview.suggested_name(),
            "syrinx-usage-report-2026-11-02-to-2026-11-30.json"
        );
    }

    #[test]
    fn only_the_latest_request_is_shown() {
        let text = synthetic_report();
        let mut state = ReportState::default();
        let first = state.request(ReportPeriod::Current);
        let second = state.request(ReportPeriod::Previous);
        // The earlier request's answer, and an answer for another period, are ignored.
        state.received(first, ReportPeriod::Current, Ok(text.clone()));
        state.received(second, ReportPeriod::Current, Ok(text.clone()));
        assert!(state.exportable().is_none());
        state.received(second, ReportPeriod::Previous, Ok(text));
        assert!(state.exportable().is_some());
        assert_eq!(state.period(), Some(ReportPeriod::Previous));
    }

    #[test]
    fn nothing_is_exportable_while_off_failed_loading_or_saving() {
        let text = synthetic_report();

        let mut state = shown(&text);
        state.counting_changed(false);
        assert!(!state.is_open() && state.exportable().is_none());

        let mut state = ReportState::default();
        let request = state.request(ReportPeriod::Current);
        assert!(state.exportable().is_none(), "nothing while loading");
        let off = Err(SessionError::Usage(UsageError::Off));
        state.received(request, ReportPeriod::Current, off);
        assert!(state.is_open() && state.exportable().is_none());

        // A text the table cannot read is neither shown nor exported.
        let mut state = ReportState::default();
        let request = state.request(ReportPeriod::Current);
        let damaged = text.replace('\n', "\r\n");
        state.received(request, ReportPeriod::Current, Ok(damaged));
        assert!(state.exportable().is_none());

        let mut state = shown(&text);
        state.saving = true;
        assert!(state.exportable().is_none(), "one export at a time");
    }

    /// The folder's file names, sorted, to prove nothing partial was left behind.
    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .expect("the synthetic folder lists")
            .map(|entry| {
                let entry = entry.expect("a folder entry");
                entry.file_name().to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_failed_save_leaves_the_replaced_file_untouched_and_nothing_partial() {
        let folder = synthetic_folder();
        let path = folder.join("syrinx-usage-report-2026-11-02-to-2026-11-30.json");
        fs::write(&path, b"an earlier synthetic report\n").expect("a synthetic old file");

        // The disk fills half way through the write.
        let failed = replace_with(&path, |file| {
            file.write_all(b"{\n  \"schema_version\": 1,")?;
            Err(io::Error::from(io::ErrorKind::StorageFull))
        });
        assert_eq!(
            failed.map_err(|error| error.kind()),
            Err(io::ErrorKind::StorageFull)
        );
        let old = fs::read(&path).expect("the old file reads");
        let names = names_in(&folder);
        // A target that cannot be replaced, here a folder, is left as it was too.
        let blocked = folder.join("blocked.json");
        fs::create_dir(&blocked).expect("a synthetic folder in the way");
        let refused = write_report(&blocked, synthetic_report().as_bytes());
        let after_refusal = names_in(&folder);
        let blocked_empty = names_in(&blocked).is_empty();
        fs::remove_dir_all(&folder).expect("the synthetic folder is removed");

        assert_eq!(old, b"an earlier synthetic report\n");
        assert_eq!(names, ["syrinx-usage-report-2026-11-02-to-2026-11-30.json"]);
        assert!(refused.is_err());
        assert_eq!(
            after_refusal,
            [
                "blocked.json",
                "syrinx-usage-report-2026-11-02-to-2026-11-30.json"
            ]
        );
        assert!(blocked_empty);
    }

    #[test]
    fn a_save_never_touches_a_partial_file_it_did_not_create() {
        let folder = synthetic_folder();
        let path = folder.join("syrinx-usage-report-2026-11-02-to-2026-11-30.json");
        let partial = folder.join("syrinx-usage-report-2026-11-02-to-2026-11-30.json.partial");
        fs::write(&path, b"an earlier synthetic report\n").expect("a synthetic old file");
        fs::write(&partial, b"someone else's synthetic bytes\n").expect("a synthetic leftover");

        let refused = write_report(&path, synthetic_report().as_bytes());
        let target = fs::read(&path).expect("the old file reads");
        let leftover = fs::read(&partial).expect("the leftover is still there");
        fs::remove_dir_all(&folder).expect("the synthetic folder is removed");

        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(target, b"an earlier synthetic report\n");
        assert_eq!(leftover, b"someone else's synthetic bytes\n");
    }

    /// The approved suggested prompt, copied from the proposal rather than imported.
    const APPROVED_PROMPT: &str = "This is a usage report from Syrinx, a desktop browser I use. \
        It contains only aggregate counts and timing buckets for one period, described in its \
        \"about\" field. Suggest up to five changes that might make Syrinx faster or easier for \
        me, each tied to the numbers that support it. Say what each number cannot tell you, and \
        do not assume why I did something from how often I did it.";

    #[test]
    fn the_suggested_prompt_is_the_approved_text() {
        assert_eq!(SUGGESTED_PROMPT, APPROVED_PROMPT);
    }

    /// The source of this crate's non-test code, file by file.
    fn product_sources() -> Vec<(String, String)> {
        fn walk(folder: &Path, sources: &mut Vec<(String, String)>) {
            for entry in fs::read_dir(folder).expect("the source folder lists") {
                let path = entry.expect("a source entry").path();
                if path.is_dir() {
                    walk(&path, sources);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                // Files on disk may have CRLF endings (the line-ending trap).
                let text = fs::read_to_string(&path)
                    .expect("a source file reads")
                    .replace("\r\n", "\n");
                // A file's own test module comes last; everything before it is product code.
                let product = match text.find("\n#[cfg(test)]\nmod tests {") {
                    Some(end) => text[..end].to_owned(),
                    None => text,
                };
                let name = path.file_name().expect("a file name").to_string_lossy();
                sources.push((name.into_owned(), product));
            }
        }
        let mut sources = Vec::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut sources,
        );
        sources
    }

    /// How often `needle` appears in each file's product code, for files where it does.
    fn occurrences(needle: &str) -> Vec<(String, usize)> {
        product_sources()
            .into_iter()
            .map(|(name, text)| (name, text.matches(needle).count()))
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    #[test]
    fn only_the_export_buttons_reach_the_clipboard_or_a_chosen_file() {
        // Needles are split so this test's own text never matches them.
        let clipboard = concat!("write_to_", "clipboard(");
        let mut writers = occurrences(clipboard);
        writers.sort();
        // The usage report's one write, and the sidebar row menu's Copy link (#95), which
        // copies only the row's own address and never anything from the report.
        assert_eq!(
            writers,
            [
                ("row_menu.rs".to_owned(), 1),
                ("usage_preview.rs".to_owned(), 1)
            ]
        );
        // Defined once, called by Copy report and Copy prompt only.
        let helper = concat!("put_on_", "clipboard(");
        assert_eq!(occurrences(helper), [("usage_preview.rs".to_owned(), 3)]);
        // The save dialog and the write: defined or imported once, used by Save report… only.
        let dialog = concat!("choose_new_", "file");
        assert_eq!(occurrences(dialog), [("usage_preview.rs".to_owned(), 2)]);
        let write = concat!("write_", "report(");
        assert_eq!(occurrences(write), [("usage_preview.rs".to_owned(), 2)]);
        for button in [
            concat!("this.copy_usage_", "report(cx)"),
            concat!("this.copy_suggested_", "prompt(cx)"),
            concat!("this.save_usage_", "report(window, cx)"),
        ] {
            assert_eq!(occurrences(button), [("usage_preview.rs".to_owned(), 1)]);
        }
    }
}
