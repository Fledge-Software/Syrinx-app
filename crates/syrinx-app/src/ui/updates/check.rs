//! "Check for updates" (P1-39c): ask the host on a thread, show the version and size, and
//! download into staging only after the owner's yes. Nothing here runs unless the owner asks.

use std::thread;

use futures::channel::oneshot;
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use syrinx_ui::Palette;
use syrinx_update::Version;
use syrinx_update::install::{Checked, InstallError};
use syrinx_update_net::RemotePackage;

use super::{BrowserView, Stage, StagingResult, UpdatePanel, pill};
use crate::controls::control;
use crate::updates::{available_message, refusal_message, size_text, update_host};

/// What the check thread found, with the host kept for the owner's yes.
struct CheckResult {
    checked: Result<Checked, InstallError>,
    remote: RemotePackage,
    /// Whether `Updates\state` was unreadable once the check finished (DW-66).
    records_unreadable: bool,
}

impl UpdatePanel {
    /// Whether a check may start now: not while another update step is under way.
    pub(super) fn can_check(&self) -> bool {
        matches!(self.stage, Stage::Idle | Stage::Ready(_))
    }

    fn begin_check(&mut self, remote: &RemotePackage) {
        self.cancel = Some(remote.canceller());
        self.remote = None;
        self.before_check = self.stage;
        self.stage = Stage::Checking;
    }

    /// Apply a check's result; returns what to tell the owner.
    fn finish_check(
        &mut self,
        checked: Option<Result<Checked, InstallError>>,
        remote: Option<RemotePackage>,
    ) -> String {
        let cancelled = self
            .cancel
            .take()
            .is_some_and(|cancel| cancel.is_cancelled());
        let resume = self.before_check;
        self.stage = resume;
        // A Cancel pressed after the last read still means no offer.
        if cancelled && matches!(checked, Some(Ok(Checked::Available { .. }))) {
            return "Update check cancelled; nothing was downloaded.".into();
        }
        match checked {
            None => "The update check stopped unexpectedly.".into(),
            Some(Ok(Checked::UpToDate(version))) => format!("Syrinx {version} is up to date."),
            Some(Ok(Checked::Available { version, .. })) if resume == Stage::Ready(version) => {
                format!("Syrinx {version} is already downloaded and ready.")
            }
            Some(Ok(Checked::Available { version, bytes })) => {
                let Some(remote) = remote else {
                    return "The update check stopped unexpectedly.".into();
                };
                self.remote = Some(remote);
                self.stage = Stage::Available { version, bytes };
                available_message(version, bytes)
            }
            Some(Err(error)) => {
                eprintln!("update check refused: {error}");
                refusal_message(&error)
            }
        }
    }

    /// The owner said yes: hand over the checked host, or `None` if nothing is offered.
    fn accept_download(&mut self) -> Option<(Version, RemotePackage)> {
        let Stage::Available { version, .. } = self.stage else {
            return None;
        };
        let Some(mut remote) = self.remote.take() else {
            self.stage = self.before_check;
            return None;
        };
        self.cancel = Some(remote.renew_canceller());
        self.stage = Stage::Downloading(version);
        Some((version, remote))
    }

    /// The owner said not now: forget the offer; nothing was downloaded.
    fn decline_download(&mut self) -> Option<Version> {
        let Stage::Available { version, .. } = self.stage else {
            return None;
        };
        self.remote = None;
        self.stage = self.before_check;
        Some(version)
    }

    /// Stop a check or download at its next read; `true` if one was running.
    fn cancel_transfer(&mut self) -> bool {
        let Some(cancel) = &self.cancel else {
            return false;
        };
        cancel.cancel();
        matches!(self.stage, Stage::Checking | Stage::Downloading(_))
    }
}

impl BrowserView {
    /// Ask the host what the newest release is, on its own thread (P1-39c).
    pub(in crate::ui) fn check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        // A copy that cannot install, such as a development build, never contacts the host.
        if let Err(reason) = self.updates.context.availability() {
            self.show_update_status(reason);
            return;
        }
        if !self.updates.can_check() {
            self.show_update_status("An update is already being prepared or installed.".into());
            return;
        }
        // Opening the session makes no request; the thread makes the first one.
        let remote = match RemotePackage::production() {
            Ok(remote) => remote,
            Err(error) => {
                self.show_update_status(refusal_message(&error));
                return;
            }
        };
        self.updates.begin_check(&remote);
        let (sender, receiver) = oneshot::channel();
        let context = self.updates.context.clone();
        let spawned = thread::Builder::new()
            .name("syrinx-update-check".to_owned())
            .spawn(move || {
                let checked = context.check(&remote);
                let records_unreadable = context.records_unreadable();
                let result = CheckResult {
                    checked,
                    remote,
                    records_unreadable,
                };
                if sender.send(result).is_err() {
                    eprintln!("The update check finished after its window closed.");
                }
            });
        if spawned.is_err() {
            self.updates.finish_check(None, None);
            self.show_update_status("Cannot start the update check.".into());
            return;
        }
        self.show_update_status(format!("Checking {}…", update_host()));
        cx.spawn_in(window, async move |view, cx| {
            let result = receiver.await.ok();
            if view
                .update(cx, |view, cx| {
                    view.update_checked(result);
                    cx.notify();
                })
                .is_err()
            {
                eprintln!("The update check arrived after the browser window closed.");
            }
        })
        .detach();
    }

    fn update_checked(&mut self, result: Option<CheckResult>) {
        let (checked, remote) = match result {
            Some(CheckResult {
                checked,
                remote,
                records_unreadable,
            }) => {
                // The worker reread the record, so the reset follows it whatever the result.
                self.updates.records_unreadable = records_unreadable;
                (Some(checked), Some(remote))
            }
            None => (None, None),
        };
        let status = self.updates.finish_check(checked, remote);
        self.show_update_status(status);
        // The question is answered in Settings, which the palette may not be showing.
        if let Stage::Available { version, bytes } = self.updates.stage {
            self.message = format!(
                "Syrinx {version} is available ({}). Download it from Settings.",
                size_text(bytes)
            );
        }
    }

    /// Download: fetch the package into staging on its own thread, then offer Restart now.
    fn download_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let Some((version, remote)) = self.updates.accept_download() else {
            return;
        };
        let (sender, receiver) = oneshot::channel();
        let context = self.updates.context.clone();
        let spawned = thread::Builder::new()
            .name("syrinx-update-download".to_owned())
            .spawn(move || {
                let result = match context.stage_from(&remote) {
                    Ok(staged) => StagingResult::Staged(staged),
                    Err(error) => {
                        eprintln!("update download refused: {error}");
                        StagingResult::refused(&context, &error)
                    }
                };
                if sender.send(result).is_err() {
                    eprintln!("The update download finished after its window closed.");
                }
            });
        if spawned.is_err() {
            self.updates.cancel = None;
            self.updates.stage = self.updates.before_check;
            self.show_update_status("Cannot start the download.".into());
            return;
        }
        self.show_update_status(format!("Downloading and verifying Syrinx {version}…"));
        cx.spawn_in(window, async move |view, cx| {
            let staged = receiver.await.unwrap_or(StagingResult::Failed {
                reason: "The download stopped unexpectedly.".into(),
                still_staged: None,
                records_unreadable: None,
            });
            if view
                .update(cx, |view, cx| {
                    view.updates.cancel = None;
                    view.update_staged(staged, Stage::Idle);
                    cx.notify();
                })
                .is_err()
            {
                eprintln!("The downloaded update arrived after the browser window closed.");
            }
        })
        .detach();
    }

    fn decline_update(&mut self) {
        if let Some(version) = self.updates.decline_download() {
            self.show_update_status(format!("Syrinx {version} was not downloaded."));
        }
    }

    /// Cancel a check or download; its thread reports when it has stopped.
    pub(in crate::ui) fn cancel_update_transfer(&mut self) {
        if self.updates.cancel_transfer() {
            self.message = "Cancelling the update…".to_owned();
        }
    }

    pub(super) fn check_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(
            palette,
            control(div().id("updates-check"), Role::Button, "Check for updates"),
        )
        .child("Check for updates")
        .on_click(cx.listener(|this, _, window, cx| {
            this.check_for_updates(window, cx);
            cx.notify();
        }))
    }

    /// The row for a check in progress, an offer awaiting the owner, or a download.
    pub(super) fn transfer_row(&self, palette: &Palette, cx: &mut Context<Self>) -> Option<Div> {
        let (text, buttons) = match self.updates.stage {
            Stage::Checking => (
                format!("Checking {}…", update_host()),
                vec![self.cancel_button(palette, cx)],
            ),
            Stage::Available { version, bytes } => (
                format!("Syrinx {version} is available ({})", size_text(bytes)),
                vec![
                    self.download_button(palette, cx),
                    self.decline_button(palette, cx),
                ],
            ),
            Stage::Downloading(version) => (
                format!("Downloading Syrinx {version}…"),
                vec![self.cancel_button(palette, cx)],
            ),
            _ => return None,
        };
        let row = div().flex().items_center().gap(px(8.)).child(
            div()
                .flex_1()
                .min_w_0()
                .font_weight(FontWeight::MEDIUM)
                .child(text),
        );
        Some(row.children(buttons))
    }

    fn download_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(
            palette,
            control(div().id("updates-download"), Role::Button, "Download"),
        )
        .child("Download")
        .on_click(cx.listener(|this, _, window, cx| {
            this.download_update(window, cx);
            cx.notify();
        }))
    }

    fn decline_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(
            palette,
            control(div().id("updates-decline"), Role::Button, "Not now"),
        )
        .child("Not now")
        .on_click(cx.listener(|this, _, _, cx| {
            this.decline_update();
            cx.notify();
        }))
    }

    fn cancel_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(
            palette,
            control(div().id("updates-cancel"), Role::Button, "Cancel"),
        )
        .child("Cancel")
        .on_click(cx.listener(|this, _, _, cx| {
            this.cancel_update_transfer();
            cx.notify();
        }))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use syrinx_update::install::TransferProblem;

    use super::*;
    use crate::updates::{StartupUpdates, UpdateContext};

    const OFFERED: Version = Version::new(1, 4, 0);
    #[cfg(windows)]
    const BYTES: u64 = 38 * 1024 * 1024;

    fn panel() -> UpdatePanel {
        UpdatePanel::new(UpdateContext::disabled(), StartupUpdates::default())
    }

    /// The production host, opened but never asked anything: no test here makes a request.
    #[cfg(windows)]
    fn unused_host() -> RemotePackage {
        RemotePackage::production().expect("WinHTTP session")
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn an_offer_waits_for_the_owner_with_its_host_kept() {
        let mut panel = panel();
        panel.begin_check(&unused_host());
        assert_eq!(panel.stage, Stage::Checking);
        let available = Checked::Available {
            version: OFFERED,
            bytes: BYTES,
        };
        let status = panel.finish_check(Some(Ok(available)), Some(unused_host()));
        assert_eq!(
            status,
            "Syrinx 1.4.0 is available (38 MB). Download and verify it?"
        );
        assert_eq!(
            panel.stage,
            Stage::Available {
                version: OFFERED,
                bytes: BYTES
            }
        );
        assert!(panel.remote.is_some() && panel.cancel.is_none());

        let (version, _remote) = panel.accept_download().expect("download");
        assert_eq!(version, OFFERED);
        assert_eq!(panel.stage, Stage::Downloading(OFFERED));
        assert!(panel.remote.is_none(), "the host moved to the download");
        assert!(panel.cancel_transfer(), "the download can be cancelled");
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn a_cancel_after_the_last_read_offers_nothing_and_keeps_the_staged_copy() {
        let staged = Version::new(1, 3, 5);
        let mut panel = panel();
        panel.stage = Stage::Ready(staged);
        let remote = unused_host();
        panel.begin_check(&remote);
        assert!(panel.cancel_transfer());
        let available = Checked::Available {
            version: OFFERED,
            bytes: BYTES,
        };
        let status = panel.finish_check(Some(Ok(available)), Some(remote));
        assert_eq!(status, "Update check cancelled; nothing was downloaded.");
        assert_eq!(panel.stage, Stage::Ready(staged));
        assert!(panel.remote.is_none() && panel.cancel.is_none());
        assert!(panel.accept_download().is_none(), "no download can start");
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn a_download_starts_with_a_fresh_cancel() {
        let mut panel = panel();
        let remote = unused_host();
        let earlier = remote.canceller();
        panel.begin_check(&remote);
        let available = Checked::Available {
            version: OFFERED,
            bytes: BYTES,
        };
        panel.finish_check(Some(Ok(available)), Some(remote));
        // A stale handle from the check cannot reach the download.
        earlier.cancel();
        let (_, remote) = panel.accept_download().expect("download");
        assert!(!remote.canceller().is_cancelled());
        assert!(
            panel
                .cancel
                .as_ref()
                .is_some_and(|cancel| !cancel.is_cancelled())
        );
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn declining_forgets_the_offer_and_downloads_nothing() {
        let mut panel = panel();
        panel.stage = Stage::Ready(Version::new(1, 3, 5));
        panel.begin_check(&unused_host());
        let available = Checked::Available {
            version: OFFERED,
            bytes: BYTES,
        };
        panel.finish_check(Some(Ok(available)), Some(unused_host()));
        assert_eq!(panel.decline_download(), Some(OFFERED));
        assert_eq!(panel.stage, Stage::Ready(Version::new(1, 3, 5)));
        assert!(panel.remote.is_none());
        assert!(
            panel.accept_download().is_none(),
            "nothing left to download"
        );
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn a_check_that_finds_nothing_returns_to_where_it_started() {
        let cases = [
            (
                Some(Ok(Checked::UpToDate(Version::new(1, 3, 0)))),
                "Syrinx 1.3.0 is up to date.",
            ),
            (
                Some(Err(InstallError::Transfer(TransferProblem::Unreachable))),
                "Syrinx couldn't reach syrinxapp.dev. Check your connection and try again.",
            ),
            (
                Some(Err(InstallError::Transfer(TransferProblem::Cancelled))),
                "Update cancelled; nothing was installed.",
            ),
            (None, "The update check stopped unexpectedly."),
        ];
        for (checked, expected) in cases {
            let mut panel = panel();
            panel.begin_check(&unused_host());
            assert_eq!(panel.finish_check(checked, None), expected);
            assert_eq!(panel.stage, Stage::Idle);
            assert!(panel.remote.is_none() && panel.cancel.is_none());
        }
    }

    // Only Windows builds a download host; elsewhere the panel never gets one.
    #[cfg(windows)]
    #[test]
    fn an_offer_of_the_staged_version_is_not_downloaded_again() {
        let mut panel = panel();
        panel.stage = Stage::Ready(OFFERED);
        panel.begin_check(&unused_host());
        let available = Checked::Available {
            version: OFFERED,
            bytes: BYTES,
        };
        let status = panel.finish_check(Some(Ok(available)), Some(unused_host()));
        assert_eq!(status, "Syrinx 1.4.0 is already downloaded and ready.");
        assert_eq!(panel.stage, Stage::Ready(OFFERED));
        assert!(panel.remote.is_none());
    }

    #[test]
    fn checks_wait_for_other_update_steps() {
        for stage in [
            Stage::Staging,
            Stage::Checking,
            Stage::Downloading(OFFERED),
            Stage::InstallOnQuit(OFFERED),
            Stage::HandedOver,
        ] {
            let mut panel = panel();
            panel.stage = stage;
            assert!(!panel.can_check(), "{stage:?}");
        }
        assert!(panel().can_check());
    }
}
