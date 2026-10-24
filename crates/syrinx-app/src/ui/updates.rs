//! Settings' Updates section, "Install update from file…", "Check for updates" and the
//! handover on restart or quit (P1-39b, P1-39c). Nothing installs until Syrinx closes after a
//! D13 snapshot.

mod check;
mod reset;

use std::thread;

use futures::channel::oneshot;
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use syrinx_platform::file_dialog::choose_folder;
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;
use syrinx_update::Version;
use syrinx_update::install::{ApplyMode, InstallError, Staged};
use syrinx_update_net::{Cancel, RemotePackage};

use super::BrowserView;
use crate::controls::control;
use crate::session::{SessionCommand, SessionError};
use crate::theme::rgba;
use crate::updates::{StartupUpdates, UpdateContext, refusal_message, staged_message, update_host};

/// Where the update is between choosing a package and handing over to the helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    /// The folder dialog is open or the package is being verified and copied.
    Staging,
    /// "Check for updates" is reading the statement and manifest from the host (P1-39c).
    Checking,
    /// The host offers this version; nothing larger is downloaded until the owner says yes.
    Available {
        version: Version,
        bytes: u64,
    },
    /// The owner said yes: the package is downloading into staging.
    Downloading(Version),
    /// Verified beside the program, waiting for Restart now or Later.
    Ready(Version),
    /// The owner chose Later: it installs when they quit.
    InstallOnQuit(Version),
    /// Closing waits for the pre-update D13 snapshot.
    Snapshotting {
        version: Version,
        mode: ApplyMode,
    },
    /// The snapshot is taken; the helper starts once Syrinx has closed.
    HandedOver,
    /// "Reset update records" is setting an unreadable `Updates\state` aside (DW-66).
    ResettingRecords,
}

/// What the window does once the pre-update snapshot has finished.
#[derive(Debug, PartialEq, Eq)]
enum AfterSnapshot {
    /// Close; the helper starts if `request` is set.
    Close { request: Option<ApplyMode> },
    /// Stay open and tell the owner why nothing was installed.
    StayOpen,
    /// No snapshot was awaited.
    Nothing,
}

pub(super) struct UpdatePanel {
    context: UpdateContext,
    stage: Stage,
    /// The last thing to tell the owner, kept in Settings.
    status: Option<String>,
    /// Shown once in the status line when the session first loads.
    startup_notice: Option<String>,
    /// The checked host, kept between the owner's check and their yes (P1-39c).
    remote: Option<RemotePackage>,
    /// Cancels the check or download in progress.
    cancel: Option<Cancel>,
    /// Where a check returns to when it finds nothing or the owner declines.
    before_check: Stage,
    /// `Updates\state` cannot be read, so "Reset update records" is offered (DW-66).
    records_unreadable: bool,
    /// The owner pressed "Reset update records" and is reading what it does.
    confirming_reset: bool,
}

impl UpdatePanel {
    pub(super) fn new(context: UpdateContext, startup: StartupUpdates) -> Self {
        let stage = startup.staged.map_or(Stage::Idle, Stage::Ready);
        let status = startup.notice.clone().or_else(|| {
            startup
                .staged
                .map(|version| format!("Syrinx {version} is ready."))
        });
        Self {
            context,
            stage,
            status,
            startup_notice: startup.notice,
            remote: None,
            cancel: None,
            before_check: Stage::Idle,
            records_unreadable: startup.records_unreadable,
            confirming_reset: false,
        }
    }

    pub(super) fn take_startup_notice(&mut self) -> Option<String> {
        self.startup_notice.take()
    }

    /// The staged version waiting for Restart now, Later or a quit.
    fn waiting(&self) -> Option<Version> {
        match self.stage {
            Stage::Ready(version) | Stage::InstallOnQuit(version) => Some(version),
            _ => None,
        }
    }

    /// Whether a close must first take the snapshot; returns the mode to snapshot for.
    fn close_needs_snapshot(&mut self, session_alive: bool) -> Option<ApplyMode> {
        match self.stage {
            Stage::InstallOnQuit(version) if session_alive => {
                self.stage = Stage::Snapshotting {
                    version,
                    mode: ApplyMode::Quit,
                };
                Some(ApplyMode::Quit)
            }
            _ => None,
        }
    }

    /// Apply the snapshot's result: hand over, or keep the update staged.
    fn snapshot_finished(&mut self, taken: Result<(), String>) -> AfterSnapshot {
        let Stage::Snapshotting { version, mode } = self.stage else {
            return AfterSnapshot::Nothing;
        };
        match (taken, mode) {
            (Ok(()), _) => {
                self.context.request(mode);
                self.stage = Stage::HandedOver;
                AfterSnapshot::Close {
                    request: Some(mode),
                }
            }
            // Quitting continues without installing; the update stays staged for next time.
            (Err(error), ApplyMode::Quit) => {
                eprintln!("The pre-update snapshot failed, so the update waits: {error}");
                self.stage = Stage::Ready(version);
                AfterSnapshot::Close { request: None }
            }
            (Err(error), ApplyMode::Restart) => {
                self.stage = Stage::Ready(version);
                self.status = Some(format!(
                    "Syrinx couldn't back up your data before updating, so it didn't restart: \
                     {error}"
                ));
                AfterSnapshot::StayOpen
            }
        }
    }
}

impl BrowserView {
    /// Ask for a package folder on its own thread, then stage it there (P1-39b).
    pub(super) fn install_update_from_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if let Err(reason) = self.updates.context.availability() {
            self.show_update_status(reason);
            return;
        }
        if !matches!(self.updates.stage, Stage::Idle | Stage::Ready(_)) {
            self.show_update_status("An update is already being prepared or installed.".into());
            return;
        }
        let (sender, receiver) = oneshot::channel();
        let owner = self.parent_hwnd;
        let context = self.updates.context.clone();
        let spawned = thread::Builder::new()
            .name("syrinx-update-staging".to_owned())
            .spawn(move || {
                if sender.send(choose_and_stage(&context, owner)).is_err() {
                    eprintln!("The update was staged after its window closed.");
                }
            });
        if spawned.is_err() {
            self.show_update_status("Cannot open the folder dialog.".into());
            return;
        }
        let previous = self.updates.stage;
        self.updates.stage = Stage::Staging;
        self.message = "Choose the update package folder".to_owned();
        cx.spawn_in(window, async move |view, cx| {
            let staged = receiver.await.unwrap_or(StagingResult::Failed {
                reason: "Staging stopped unexpectedly.".into(),
                still_staged: None,
                records_unreadable: None,
            });
            if view
                .update(cx, |view, cx| {
                    view.update_staged(staged, previous);
                    cx.notify();
                })
                .is_err()
            {
                eprintln!("The staged update arrived after the browser window closed.");
            }
        })
        .detach();
    }

    fn update_staged(&mut self, result: StagingResult, previous: Stage) {
        match result {
            StagingResult::Cancelled => {
                self.updates.stage = previous;
                self.message = "Update cancelled".to_owned();
            }
            StagingResult::Failed {
                reason,
                still_staged,
                records_unreadable,
            } => {
                // A refusal after staging began has also removed the earlier staged copy.
                self.updates.stage = still_staged.map_or(Stage::Idle, Stage::Ready);
                if let Some(unreadable) = records_unreadable {
                    self.updates.records_unreadable = unreadable;
                }
                self.show_update_status(reason);
            }
            StagingResult::Staged(staged) => {
                self.updates.records_unreadable = false;
                self.updates.stage = match staged {
                    Staged::Ready { version, .. } => Stage::Ready(version),
                    Staged::UpToDate(_) => Stage::Idle,
                };
                self.show_update_status(staged_message(&staged));
            }
        }
    }

    fn show_update_status(&mut self, status: String) {
        self.message = status.clone();
        self.updates.status = Some(status);
    }

    /// Restart now: take the snapshot, then close as an ordinary quit does.
    fn restart_for_update(&mut self, cx: &mut Context<Self>) {
        let Some(version) = self.updates.waiting() else {
            return;
        };
        self.updates.stage = Stage::Snapshotting {
            version,
            mode: ApplyMode::Restart,
        };
        if !self.request_update_snapshot() {
            self.update_snapshot_taken(Err(SessionError::StorageUnavailable), cx);
        }
    }

    /// Later: install when the owner next quits Syrinx.
    fn install_update_later(&mut self) {
        let Some(version) = self.updates.waiting() else {
            return;
        };
        self.updates.stage = Stage::InstallOnQuit(version);
        self.show_update_status(format!(
            "Syrinx {version} will be installed when you quit Syrinx."
        ));
    }

    /// Called first by `request_close`: `true` while closing waits for the update snapshot.
    pub(super) fn hold_close_for_update_snapshot(&mut self) -> bool {
        // A check or download stops at its next read; staging removes what it wrote.
        self.cancel_update_transfer();
        if matches!(self.updates.stage, Stage::Snapshotting { .. }) {
            return true;
        }
        if self
            .updates
            .close_needs_snapshot(!self.session_stopped)
            .is_none()
        {
            return false;
        }
        if self.request_update_snapshot() {
            return true;
        }
        // The snapshot cannot be taken, so quitting goes ahead without installing.
        self.updates
            .snapshot_finished(Err("local storage is unavailable".into()));
        false
    }

    fn request_update_snapshot(&mut self) -> bool {
        self.message = "Backing up your data before the update…".to_owned();
        !self.session_stopped && self.try_command(SessionCommand::SnapshotBeforeUpdate)
    }

    /// The session's answer to `SnapshotBeforeUpdate`.
    pub(super) fn update_snapshot_taken(
        &mut self,
        result: Result<(), SessionError>,
        cx: &mut Context<Self>,
    ) {
        let taken = result.map_err(|error| error.to_string());
        match self.updates.snapshot_finished(taken) {
            AfterSnapshot::Close { .. } => self.request_close(cx),
            AfterSnapshot::StayOpen => {
                if let Some(status) = self.updates.status.clone() {
                    self.message = status;
                }
            }
            AfterSnapshot::Nothing => {}
        }
    }

    /// The session stopped while a snapshot was awaited: nothing installs.
    pub(super) fn update_snapshot_lost(&mut self) {
        if matches!(self.updates.stage, Stage::Snapshotting { .. }) {
            self.updates
                .snapshot_finished(Err("local storage stopped".into()));
        }
    }

    /// A Syrinx the helper relaunched records its clean start once its session has loaded.
    pub(super) fn record_clean_start(&self, cx: &mut Context<Self>) {
        let Some(job) = self.updates.context.clean_start_job() else {
            return;
        };
        // After this frame, so the record means the window drew its restored session.
        cx.spawn(async move |_, cx| {
            if let Err(error) = cx.background_executor().spawn(async move { job() }).await {
                eprintln!("The clean start after an update could not be recorded: {error}");
            }
        })
        .detach();
    }

    /// Settings' Updates section: the running version, the staged one and its choices.
    pub(super) fn updates_section(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let running = self.updates.context.running().map_or_else(
            || "Syrinx".to_owned(),
            |version| format!("Syrinx {version}"),
        );
        let mut section = div()
            .pt(px(14.))
            .border_t_1()
            .border_color(rgba(palette.panel_divider))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(section_text(palette, &running))
            // The two actions get their own row, so the explanation keeps the panel's width.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.check_button(palette, cx))
                    .child(self.install_from_file_button(palette, cx)),
            );
        if let Some(row) = self.transfer_row(palette, cx) {
            section = section.child(row);
        }
        if let Some(version) = self.updates.waiting() {
            section = section.child(self.ready_row(palette, version, cx));
        }
        if let Some(row) = self.reset_row(palette, cx) {
            section = section.child(row);
        }
        if let Some(status) = &self.updates.status {
            section = section.child(
                div()
                    .id("updates-status")
                    .role(Role::Label)
                    .aria_label(status.clone())
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(status.clone()),
            );
        }
        section
    }

    fn install_from_file_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(
            palette,
            control(
                div().id("updates-install-file"),
                Role::Button,
                "Install update from file",
            ),
        )
        .child("Install from file…")
        .on_click(cx.listener(|this, _, window, cx| {
            this.install_update_from_file(window, cx);
            cx.notify();
        }))
    }

    fn ready_row(&self, palette: &Palette, version: Version, cx: &mut Context<Self>) -> Div {
        let restart = pill(
            palette,
            control(div().id("updates-restart"), Role::Button, "Restart now"),
        )
        .child("Restart now")
        .on_click(cx.listener(|this, _, _, cx| {
            this.restart_for_update(cx);
            cx.notify();
        }));
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("Syrinx {version} is ready")),
            )
            .child(restart);
        if matches!(self.updates.stage, Stage::Ready(_)) {
            let later = pill(
                palette,
                control(div().id("updates-later"), Role::Button, "Later"),
            )
            .child("Later")
            .on_click(cx.listener(|this, _, _, cx| {
                this.install_update_later();
                cx.notify();
            }));
            row = row.child(later);
        }
        row
    }
}

/// What the staging thread found.
enum StagingResult {
    Cancelled,
    Failed {
        reason: String,
        /// The earlier staged version, if a refusal left it in place.
        still_staged: Option<Version>,
        /// Whether `Updates\state` was unreadable once the worker finished (DW-66); `None`
        /// when it was not looked at, which leaves the action as it was.
        records_unreadable: Option<bool>,
    },
    Staged(Staged),
}

impl StagingResult {
    /// A refusal from staging or a download, explained for the owner.
    fn refused(context: &UpdateContext, error: &InstallError) -> Self {
        Self::Failed {
            reason: refusal_message(error),
            still_staged: context.staged(),
            records_unreadable: Some(context.records_unreadable()),
        }
    }
}

/// Runs on the staging thread: the folder dialog, then verification and copying.
fn choose_and_stage(context: &UpdateContext, owner: isize) -> StagingResult {
    let folder = match choose_folder("Choose the update package folder", owner) {
        Ok(Some(folder)) => folder,
        Ok(None) => return StagingResult::Cancelled,
        Err(error) => {
            return StagingResult::Failed {
                reason: format!("{error}."),
                still_staged: context.staged(),
                records_unreadable: None,
            };
        }
    };
    match context.stage(&folder) {
        Ok(staged) => StagingResult::Staged(staged),
        Err(error) => {
            log_refusal(&error);
            StagingResult::refused(context, &error)
        }
    }
}

/// The diagnostic line: the fixed category and verifier reason, never a path.
fn log_refusal(error: &InstallError) {
    eprintln!("update staging refused: {error}");
}

fn section_text(palette: &Palette, running: &str) -> Div {
    div()
        .flex_1()
        // Without a zero minimum width the text cannot wrap and pushes the button out.
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().font_weight(FontWeight::MEDIUM).child("Updates"))
        .child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_muted))
                .child(format!(
                    "{running}. Check for updates contacts {} only when you press it, or \
                     install a signed package you built or copied. Nothing changes until you \
                     restart or quit.",
                    update_host()
                )),
        )
}

/// The small outlined button Settings and Customize use for its section actions.
pub(super) fn pill(palette: &Palette, element: Stateful<Div>) -> Stateful<Div> {
    element
        .h(px(28.))
        .px(px(12.))
        .flex_none()
        .rounded(px(14.))
        .border_1()
        .border_color(rgba(palette.pill_line))
        .text_color(rgba(palette.text_secondary))
        .text_size(px(text::SMALL))
        .flex()
        .items_center()
        .cursor_pointer()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGED: Version = Version::new(1, 4, 0);

    fn panel(stage: Stage) -> UpdatePanel {
        let mut panel = UpdatePanel::new(UpdateContext::disabled(), StartupUpdates::default());
        panel.stage = stage;
        panel
    }

    #[test]
    fn a_staged_update_found_at_start_waits_for_the_owner() {
        let startup = StartupUpdates {
            notice: None,
            staged: Some(STAGED),
            records_unreadable: false,
        };
        let panel = UpdatePanel::new(UpdateContext::disabled(), startup);
        assert_eq!(panel.stage, Stage::Ready(STAGED));
        assert_eq!(panel.status.as_deref(), Some("Syrinx 1.4.0 is ready."));
    }

    #[test]
    fn only_install_on_quit_makes_a_close_take_the_snapshot() {
        for stage in [
            Stage::Idle,
            Stage::Staging,
            Stage::Ready(STAGED),
            Stage::HandedOver,
        ] {
            assert_eq!(panel(stage).close_needs_snapshot(true), None, "{stage:?}");
        }
        let mut later = panel(Stage::InstallOnQuit(STAGED));
        assert_eq!(
            later.close_needs_snapshot(false),
            None,
            "no storage, no snapshot"
        );
        assert_eq!(later.close_needs_snapshot(true), Some(ApplyMode::Quit));
    }

    #[test]
    fn the_helper_is_requested_only_after_the_snapshot() {
        let mut restart = panel(Stage::Snapshotting {
            version: STAGED,
            mode: ApplyMode::Restart,
        });
        assert_eq!(restart.context.requested(), None);
        assert_eq!(
            restart.snapshot_finished(Ok(())),
            AfterSnapshot::Close {
                request: Some(ApplyMode::Restart)
            }
        );
        assert_eq!(restart.context.requested(), Some(ApplyMode::Restart));
        assert_eq!(restart.snapshot_finished(Ok(())), AfterSnapshot::Nothing);
    }

    #[test]
    fn a_failed_snapshot_installs_nothing() {
        let mut restart = panel(Stage::Snapshotting {
            version: STAGED,
            mode: ApplyMode::Restart,
        });
        assert_eq!(
            restart.snapshot_finished(Err("synthetic failure".into())),
            AfterSnapshot::StayOpen
        );
        assert_eq!(restart.stage, Stage::Ready(STAGED));
        assert_eq!(restart.context.requested(), None);

        let mut quit = panel(Stage::Snapshotting {
            version: STAGED,
            mode: ApplyMode::Quit,
        });
        assert_eq!(
            quit.snapshot_finished(Err("synthetic failure".into())),
            AfterSnapshot::Close { request: None }
        );
        assert_eq!(quit.context.requested(), None);
    }
}
