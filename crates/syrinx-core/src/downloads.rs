//! Keeps this session's downloads in memory only, whichever page started them, so the
//! Downloads flyout outlives a closed popup; nothing here is ever written down (owner, 2026-10-24).

/// The most downloads the list holds; beyond it the oldest finished one is forgotten.
pub const MAX_DOWNLOADS: usize = 100;

/// Why a download stopped before it finished, grouped as the owner needs to read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadInterruption {
    /// The owner, or the page, cancelled it.
    Cancelled,
    /// Paused in WebView2's own list; it can resume there.
    Paused,
    /// SmartScreen, a virus scan or a policy refused the file.
    Blocked,
    Network,
    Server,
    /// The disk refused it: no space, no access or a name that is too long.
    File,
    /// The browser engine shut down while it ran.
    Shutdown,
    Other,
}

/// Where a download stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadState {
    InProgress,
    Completed,
    Interrupted(DownloadInterruption),
}

/// One download as the flyout lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Download {
    /// Unique for this run of Syrinx.
    pub id: u64,
    /// The page (tab) that started it; it may have closed since.
    pub tab: u64,
    /// The saved file's name only, never its folder or source address.
    pub file_name: String,
    pub received: u64,
    /// `None` while the server has not said how large the file is.
    pub total: Option<u64>,
    pub state: DownloadState,
}

impl Download {
    /// Whether the download may still write to its file, including a pause it can resume from.
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            DownloadState::InProgress | DownloadState::Interrupted(DownloadInterruption::Paused)
        )
    }

    /// The short status the flyout shows beside the name, such as "1.5 of 4.0 MB".
    pub fn detail(&self) -> String {
        match self.state {
            DownloadState::InProgress => match self.total {
                Some(total) => progress_of(self.received, total),
                None => format_size(self.received),
            },
            DownloadState::Completed => format!("Done, {}", format_size(self.received)),
            DownloadState::Interrupted(reason) => interruption_label(reason).to_owned(),
        }
    }
}

/// The session's downloads, newest first, and the page closes that wait on them.
#[derive(Debug, Default)]
pub struct DownloadList {
    downloads: Vec<Download>,
    /// Pages that asked to close while one of their downloads was still active.
    deferred_closes: Vec<u64>,
}

impl DownloadList {
    /// Every download, newest first.
    pub fn downloads(&self) -> &[Download] {
        &self.downloads
    }

    /// Records a download page `tab` started; a repeated `id` is ignored.
    pub fn add(&mut self, id: u64, tab: u64, file_name: &str, total: Option<u64>) {
        if self.find(id).is_some() {
            return;
        }
        let file_name = match file_name.trim() {
            "" => "Unnamed download".to_owned(),
            name => name.to_owned(),
        };
        self.downloads.insert(
            0,
            Download {
                id,
                tab,
                file_name,
                received: 0,
                total,
                state: DownloadState::InProgress,
            },
        );
        self.forget_oldest_finished();
    }

    /// More bytes arrived. Returns whether the list changed.
    pub fn progress(&mut self, id: u64, received: u64, total: Option<u64>) -> bool {
        let Some(download) = self.find_mut(id) else {
            return false;
        };
        let total = total.or(download.total);
        let changed = download.received != received || download.total != total;
        download.received = received;
        download.total = total;
        changed
    }

    /// The download finished. Returns a page whose deferred close may now go ahead.
    pub fn complete(&mut self, id: u64, received: u64) -> Option<u64> {
        self.change_state(id, DownloadState::Completed, received)
    }

    /// The download stopped for `reason`. Returns a page whose deferred close may now go ahead.
    pub fn fail(&mut self, id: u64, reason: DownloadInterruption, received: u64) -> Option<u64> {
        self.change_state(id, DownloadState::Interrupted(reason), received)
    }

    /// The download was cancelled. Returns a page whose deferred close may now go ahead.
    pub fn cancel(&mut self, id: u64, received: u64) -> Option<u64> {
        self.fail(id, DownloadInterruption::Cancelled, received)
    }

    /// A paused download running again.
    pub fn resume(&mut self, id: u64, received: u64) {
        self.change_state(id, DownloadState::InProgress, received);
    }

    /// Applies a state reported by the engine; the same as the named calls above.
    pub fn update(&mut self, id: u64, state: DownloadState, received: u64) -> Option<u64> {
        self.change_state(id, state, received)
    }

    /// Forgets every download that can no longer change; active ones stay listed.
    pub fn clear_finished(&mut self) {
        self.downloads.retain(Download::is_active);
    }

    /// Forgets everything, as when Syrinx exits.
    pub fn clear(&mut self) {
        self.downloads.clear();
        self.deferred_closes.clear();
    }

    /// Whether any listed download can be cleared.
    pub fn has_finished(&self) -> bool {
        self.downloads.iter().any(|download| !download.is_active())
    }

    /// Page `tab` asked to close. Returns `true` when the close must wait for its active
    /// downloads, which `complete`, `fail` and `cancel` later release.
    pub fn defer_close(&mut self, tab: u64) -> bool {
        if !self.tab_is_downloading(tab) {
            return false;
        }
        if !self.deferred_closes.contains(&tab) {
            self.deferred_closes.push(tab);
        }
        true
    }

    /// Drops a deferred close that no longer applies, as when the owner closed the tab.
    pub fn forget_close(&mut self, tab: u64) {
        self.deferred_closes.retain(|deferred| *deferred != tab);
    }

    fn change_state(&mut self, id: u64, state: DownloadState, received: u64) -> Option<u64> {
        let download = self.find_mut(id)?;
        download.state = state;
        download.received = received;
        let tab = download.tab;
        self.release_close(tab)
    }

    /// The deferred close of `tab`, once none of its downloads is active.
    fn release_close(&mut self, tab: u64) -> Option<u64> {
        if self.tab_is_downloading(tab) || !self.deferred_closes.contains(&tab) {
            return None;
        }
        self.forget_close(tab);
        Some(tab)
    }

    fn tab_is_downloading(&self, tab: u64) -> bool {
        self.downloads
            .iter()
            .any(|download| download.tab == tab && download.is_active())
    }

    fn forget_oldest_finished(&mut self) {
        if self.downloads.len() <= MAX_DOWNLOADS {
            return;
        }
        if let Some(oldest) = self
            .downloads
            .iter()
            .rposition(|download| !download.is_active())
        {
            self.downloads.remove(oldest);
        }
    }

    fn find(&self, id: u64) -> Option<&Download> {
        self.downloads.iter().find(|download| download.id == id)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Download> {
        self.downloads.iter_mut().find(|download| download.id == id)
    }
}

/// What the flyout says for each interruption.
fn interruption_label(reason: DownloadInterruption) -> &'static str {
    match reason {
        DownloadInterruption::Cancelled => "Cancelled",
        DownloadInterruption::Paused => "Paused",
        DownloadInterruption::Blocked => "Blocked",
        DownloadInterruption::Network => "Failed: network",
        DownloadInterruption::Server => "Failed: server",
        DownloadInterruption::File => "Failed: disk",
        DownloadInterruption::Shutdown => "Stopped",
        DownloadInterruption::Other => "Failed",
    }
}

const UNITS: [(&str, u64); 3] = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)];

/// A byte count as "512 B", "3.4 KB", "1.5 MB" or "2.0 GB".
pub fn format_size(bytes: u64) -> String {
    match unit_for(bytes) {
        Some((unit, scale)) => format!("{} {unit}", scaled(bytes, scale)),
        None => format!("{bytes} B"),
    }
}

/// Progress in the total's unit, such as "1.5 of 4.0 MB".
fn progress_of(received: u64, total: u64) -> String {
    match unit_for(total) {
        Some((unit, scale)) => format!(
            "{} of {} {unit}",
            scaled(received, scale),
            scaled(total, scale)
        ),
        None => format!("{received} of {total} B"),
    }
}

fn unit_for(bytes: u64) -> Option<(&'static str, u64)> {
    UNITS.into_iter().find(|(_, scale)| bytes >= *scale)
}

/// `bytes` divided by `scale`, to one decimal place.
fn scaled(bytes: u64, scale: u64) -> String {
    format!("{:.1}", bytes as f64 / scale as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    fn list_with(id: u64, tab: u64) -> DownloadList {
        let mut list = DownloadList::default();
        list.add(id, tab, "synthetic.zip", Some(4 * MB));
        list
    }

    #[test]
    fn a_download_is_listed_newest_first_and_only_once() {
        let mut list = list_with(1, 10);
        list.add(2, 11, "second-synthetic.zip", None);
        list.add(1, 10, "duplicate.zip", None);
        let names: Vec<_> = list
            .downloads()
            .iter()
            .map(|d| d.file_name.as_str())
            .collect();
        assert_eq!(names, ["second-synthetic.zip", "synthetic.zip"]);
        assert_eq!(list.downloads()[1].state, DownloadState::InProgress);
    }

    #[test]
    fn an_empty_name_still_reads_as_a_download() {
        let mut list = DownloadList::default();
        list.add(1, 10, "  ", None);
        assert_eq!(list.downloads()[0].file_name, "Unnamed download");
    }

    #[test]
    fn progress_reports_changes_and_keeps_a_known_total() {
        let mut list = list_with(1, 10);
        assert!(list.progress(1, MB + MB / 2, None));
        assert!(
            !list.progress(1, MB + MB / 2, None),
            "same bytes, no change"
        );
        assert!(!list.progress(9, 5, None), "unknown download");
        let download = &list.downloads()[0];
        assert_eq!(download.total, Some(4 * MB));
        assert_eq!(download.detail(), "1.5 of 4.0 MB");
    }

    #[test]
    fn completion_failure_and_cancellation_are_shown() {
        let mut list = list_with(1, 10);
        list.add(2, 10, "b.zip", None);
        list.add(3, 10, "c.zip", None);
        list.complete(1, 4 * MB);
        list.fail(2, DownloadInterruption::Network, 10);
        list.cancel(3, 0);
        let details: Vec<_> = list.downloads().iter().map(Download::detail).collect();
        assert_eq!(details, ["Cancelled", "Failed: network", "Done, 4.0 MB"]);
    }

    #[test]
    fn a_pause_stays_active_and_can_resume() {
        let mut list = list_with(1, 10);
        list.fail(1, DownloadInterruption::Paused, MB);
        assert!(list.downloads()[0].is_active());
        list.resume(1, MB);
        assert_eq!(list.downloads()[0].state, DownloadState::InProgress);
    }

    #[test]
    fn clearing_finished_keeps_active_downloads() {
        let mut list = list_with(1, 10);
        list.add(2, 10, "b.zip", None);
        list.complete(1, 4 * MB);
        assert!(list.has_finished());
        list.clear_finished();
        assert_eq!(list.downloads().len(), 1);
        assert_eq!(list.downloads()[0].id, 2);
        assert!(!list.has_finished());
        list.clear();
        assert!(list.downloads().is_empty());
    }

    #[test]
    fn a_page_close_waits_for_its_active_downloads_only() {
        let mut list = list_with(1, 10);
        list.add(2, 10, "b.zip", None);
        list.add(3, 11, "c.zip", None);
        assert!(list.defer_close(10));
        assert!(
            !list.defer_close(12),
            "a page with no downloads closes at once"
        );
        assert_eq!(list.complete(1, 1), None, "download 2 still runs");
        assert_eq!(list.complete(3, 1), None, "another page's download");
        assert_eq!(list.fail(2, DownloadInterruption::Server, 1), Some(10));
        assert_eq!(list.complete(2, 1), None, "the close is released once");
        assert!(!list.defer_close(10), "nothing left to wait for");
    }

    #[test]
    fn a_forgotten_close_is_never_released() {
        let mut list = list_with(1, 10);
        assert!(list.defer_close(10));
        list.forget_close(10);
        assert_eq!(list.complete(1, 1), None);
    }

    #[test]
    fn the_list_forgets_its_oldest_finished_download_beyond_the_bound() {
        let mut list = DownloadList::default();
        for id in 0..MAX_DOWNLOADS as u64 {
            list.add(id, 1, "synthetic.zip", None);
        }
        list.complete(5, 0);
        list.complete(7, 0);
        list.add(1_000, 1, "newest.zip", None);
        assert_eq!(list.downloads().len(), MAX_DOWNLOADS);
        assert!(list.downloads().iter().all(|download| download.id != 5));
        assert!(list.downloads().iter().any(|download| download.id == 7));
    }

    #[test]
    fn sizes_read_in_the_largest_whole_unit() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(3 * 1024 + 512), "3.5 KB");
        assert_eq!(format_size(2 << 30), "2.0 GB");
        let mut list = DownloadList::default();
        list.add(1, 1, "small.zip", Some(900));
        list.progress(1, 300, None);
        assert_eq!(list.downloads()[0].detail(), "300 of 900 B");
        list.add(2, 1, "unknown.zip", None);
        list.progress(2, MB, None);
        assert_eq!(list.downloads()[0].detail(), "1.0 MB");
    }
}
