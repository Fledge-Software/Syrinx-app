//! Feeds every page's downloads into the session's in-memory list and its flyout (#143), and
//! keeps a popup that asked to close alive until the downloads it started have finished.

use syrinx_core::{DownloadControl, DownloadInterruption, DownloadState};

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
        if let Some(tab) = self.downloads.update(download, state, received) {
            self.close_requested_page(tab);
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
}
