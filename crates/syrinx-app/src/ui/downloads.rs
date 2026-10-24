//! Feeds every page's downloads into the session's in-memory list and its flyout (#143), and
//! keeps a popup that asked to close alive until the downloads it started have finished.

use syrinx_core::{DownloadControl, DownloadInterruption, DownloadList, DownloadState};

use crate::session::SessionCommand;

use super::*;

impl BrowserView {
    /// Page `tab` started a download.
    pub(super) fn download_started(
        &mut self,
        download: u64,
        tab: u64,
        file_name: &str,
        total: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.downloads.add(download, tab, file_name, total);
        self.message = format!("Downloading {}.", self.download_name(download));
        self.menu_downloads_changed(window, cx);
        cx.notify();
    }

    /// A download moved on; a popup waiting on it may now close.
    pub(super) fn download_changed(
        &mut self,
        download: u64,
        state: DownloadState,
        received: u64,
        progress: (Option<u64>, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (total, can_resume) = progress;
        self.downloads.progress(download, received, total);
        self.downloads.set_resumable(download, can_resume);
        if let Some(command) =
            released_close(&mut self.downloads, download, state, received, self.closing)
        {
            self.command(command);
        }
        if let Some(message) = finished_message(state, &self.download_name(download)) {
            self.message = message;
        }
        self.menu_downloads_changed(window, cx);
        cx.notify();
    }

    /// A page asked to close itself. One that is still downloading stays open until its
    /// downloads end, in case closing its web view would stop them.
    pub(super) fn page_requested_close(&mut self, id: u64) {
        if !self.closing && self.downloads.defer_close(id) {
            self.message = "The popup closes once its download finishes.".into();
            return;
        }
        self.close_requested_page(id);
    }

    /// Records the close a page asked for, so the session removes its tab.
    fn close_requested_page(&mut self, id: u64) {
        if let Some(command) = page_lifecycle::observed_close(id, self.closing) {
            self.command(command);
        }
    }

    /// Pauses, resumes or cancels a download the owner chose in the flyout; the result comes
    /// back as a `DownloadChanged` event.
    pub(super) fn control_download(&mut self, download: u64, control: DownloadControl) {
        self.page_command(BrowserCommand::ControlDownload { download, control });
    }

    fn download_name(&self, download: u64) -> String {
        self.downloads
            .downloads()
            .iter()
            .find(|listed| listed.id == download)
            .map_or_else(|| "a file".to_owned(), |listed| listed.file_name.clone())
    }
}

/// The close a download's change releases, as the session must observe it: `None` while the
/// page's other downloads run, when no close was deferred, or once the page is already gone
/// (`sync_pages` calls `forget_close` for a page the saved model dropped, as after Ctrl W).
fn released_close(
    downloads: &mut DownloadList,
    download: u64,
    state: DownloadState,
    received: u64,
    closing: bool,
) -> Option<SessionCommand> {
    let tab = downloads.update(download, state, received)?;
    page_lifecycle::observed_close(tab, closing)
}

/// The status line for a download that just finished or stopped; `None` while it runs.
fn finished_message(state: DownloadState, name: &str) -> Option<String> {
    match state {
        DownloadState::InProgress => None,
        DownloadState::Completed => Some(format!("Downloaded {name}.")),
        DownloadState::Interrupted(DownloadInterruption::Paused) => {
            Some(format!("{name} is paused."))
        }
        DownloadState::Interrupted(DownloadInterruption::Cancelled) => {
            Some(format!("{name} was cancelled."))
        }
        DownloadState::Interrupted(DownloadInterruption::Blocked(_)) => Some(format!(
            "{name} was blocked to keep this computer safe. Open Downloads (Ctrl J) for why."
        )),
        DownloadState::Interrupted(_) => Some(format!(
            "{name} did not download. Open Downloads (Ctrl J) for details."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_finished_or_stopped_download_changes_the_status_line() {
        assert_eq!(
            finished_message(DownloadState::InProgress, "synthetic.zip"),
            None
        );
        assert_eq!(
            finished_message(DownloadState::Completed, "synthetic.zip").as_deref(),
            Some("Downloaded synthetic.zip.")
        );
        let stopped = DownloadState::Interrupted(DownloadInterruption::Network);
        assert!(
            finished_message(stopped, "synthetic.zip")
                .is_some_and(|message| message.starts_with("synthetic.zip did not download"))
        );
    }

    fn downloading_popup() -> DownloadList {
        let mut downloads = DownloadList::default();
        downloads.add(1, 10, "synthetic.zip", None);
        assert!(
            downloads.defer_close(10),
            "the popup asked to close mid-download"
        );
        downloads
    }

    #[test]
    fn a_deferred_close_is_observed_once_the_download_ends() {
        let mut downloads = downloading_popup();
        assert!(
            released_close(&mut downloads, 1, DownloadState::InProgress, 5, false).is_none(),
            "still downloading"
        );
        assert!(matches!(
            released_close(&mut downloads, 1, DownloadState::Completed, 5, false),
            Some(SessionCommand::ObserveClosed(tab)) if tab.get() == 10
        ));
    }

    #[test]
    fn a_popup_the_owner_closed_sends_no_second_close_when_its_download_ends() {
        let mut downloads = downloading_popup();
        // Ctrl W: the saved model dropped the page and sync_pages forgot its deferred close.
        downloads.forget_close(10);
        assert!(released_close(&mut downloads, 1, DownloadState::Completed, 5, false).is_none());
    }

    #[test]
    fn a_closing_browser_observes_no_release() {
        let mut downloads = downloading_popup();
        assert!(released_close(&mut downloads, 1, DownloadState::Completed, 5, true).is_none());
    }
}
