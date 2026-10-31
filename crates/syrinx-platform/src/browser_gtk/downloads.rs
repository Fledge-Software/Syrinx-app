//! Saves downloads into the owner's Downloads folder and reports their progress (#143).

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use syrinx_core::{DownloadControl, DownloadInterruption, DownloadState};
use webkit2gtk::{
    Download, DownloadError, DownloadExt, URIResponseExt, WebContext, WebContextExt, WebView, glib,
};

use super::{BrowserEvent, EventSink};

/// The context's downloads and which page each one came from.
#[derive(Clone)]
pub(super) struct Downloads(Rc<RefCell<Registry>>);

#[derive(Default)]
struct Registry {
    pages: HashMap<u64, WebView>,
    running: HashMap<u64, Download>,
    next: u64,
}

impl Downloads {
    pub(super) fn watch(context: &WebContext, events: EventSink) -> Self {
        let downloads = Self(Rc::default());
        let watcher = downloads.clone();
        context.connect_download_started(move |_, download| {
            watcher.started(download, &events);
        });
        downloads
    }

    pub(super) fn track(&self, tab: u64, view: &WebView) {
        self.0.borrow_mut().pages.insert(tab, view.clone());
    }

    /// The page closed; its downloads keep running and reporting, as on Windows.
    pub(super) fn forget(&self, tab: u64) {
        self.0.borrow_mut().pages.remove(&tab);
    }

    /// WebKitGTK can cancel a download but not pause it, so only Cancel acts.
    pub(super) fn control(&self, download: u64, control: DownloadControl) {
        let running = self.0.borrow().running.get(&download).cloned();
        let Some(running) = running else {
            return;
        };
        if control == DownloadControl::Cancel {
            running.cancel();
        }
    }

    fn started(&self, download: &Download, events: &EventSink) {
        let id = self.register(download);
        let tab = self.tab_of(download);
        let events = events.clone();
        let registry = self.clone();
        download.connect_decide_destination(move |download, suggested| {
            let Some(path) = destination(suggested) else {
                download.cancel();
                return true;
            };
            download.set_allow_overwrite(false);
            download.set_destination(&glib::filename_to_uri(&path, None).unwrap_or_default());
            events.emit(BrowserEvent::DownloadStarted {
                download: id,
                tab,
                file_name: file_name(&path),
                total: total_length(download),
            });
            registry.report_progress(id, download, &events);
            true
        });
    }

    fn report_progress(&self, id: u64, download: &Download, events: &EventSink) {
        let progress = events.clone();
        download.connect_received_data(move |download, _| {
            progress.emit(changed(id, download, DownloadState::InProgress));
        });
        let finished = events.clone();
        let registry = self.clone();
        download.connect_finished(move |download| {
            registry.0.borrow_mut().running.remove(&id);
            // A failure emits its own report first; `finished` follows every download.
            if download.estimated_progress() >= 1.0 {
                finished.emit(changed(id, download, DownloadState::Completed));
            }
        });
        let failed = events.clone();
        download.connect_failed(move |download, error| {
            failed.emit(changed(
                id,
                download,
                DownloadState::Interrupted(interruption(error)),
            ));
        });
    }

    fn register(&self, download: &Download) -> u64 {
        let mut registry = self.0.borrow_mut();
        registry.next += 1;
        let id = registry.next;
        registry.running.insert(id, download.clone());
        id
    }

    fn tab_of(&self, download: &Download) -> u64 {
        let Some(view) = download.web_view() else {
            return 0;
        };
        self.0
            .borrow()
            .pages
            .iter()
            .find(|(_, page)| **page == view)
            .map_or(0, |(tab, _)| *tab)
    }
}

fn changed(id: u64, download: &Download, state: DownloadState) -> BrowserEvent {
    BrowserEvent::DownloadChanged {
        download: id,
        state,
        received: download.received_data_length(),
        total: total_length(download),
        can_resume: false,
    }
}

fn total_length(download: &Download) -> Option<u64> {
    download
        .response()
        .map(|response| response.content_length())
        .filter(|length| *length > 0)
}

fn interruption(error: &glib::Error) -> DownloadInterruption {
    match error.kind::<DownloadError>() {
        Some(DownloadError::CancelledByUser) => DownloadInterruption::Cancelled,
        Some(DownloadError::Destination) => DownloadInterruption::File,
        Some(DownloadError::Network) => DownloadInterruption::Network,
        _ => DownloadInterruption::Other,
    }
}

/// A fresh path in the Downloads folder for the server's suggested name, never replacing a file.
fn destination(suggested: &str) -> Option<PathBuf> {
    let folder = glib::user_special_dir(glib::UserDirectory::Downloads)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads")))?;
    let name = safe_name(suggested);
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_owned(), format!(".{extension}")),
        _ => (name.clone(), String::new()),
    };
    (0..1000).find_map(|copy| {
        let candidate = if copy == 0 {
            folder.join(&name)
        } else {
            folder.join(format!("{stem} ({copy}){extension}"))
        };
        (!candidate.exists()).then_some(candidate)
    })
}

/// The suggested name without folders or hidden-file dots, so it lands where Syrinx chose.
fn safe_name(suggested: &str) -> String {
    let base = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let trimmed = base.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "download".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn a_suggested_name_cannot_leave_the_downloads_folder_or_hide() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("..\\secret.txt"), "secret.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("   "), "download");
        assert_eq!(safe_name("report.pdf"), "report.pdf");
    }
}
