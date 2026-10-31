//! Saves downloads into the owner's Downloads folder and reports their progress (#143).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use syrinx_core::{DownloadControl, DownloadInterruption, DownloadState};
use webkit2gtk::{
    Download, DownloadError, DownloadExt, URIResponseExt, WebContext, WebContextExt, WebView, glib,
};

use super::{BrowserError, BrowserEvent, EventSink, engine::Popups};

/// Progress is reported at most this often, as on Windows; state changes are always reported.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The context's downloads and which page each one came from.
#[derive(Clone)]
pub(super) struct Downloads(Rc<RefCell<Registry>>);

struct Registry {
    events: EventSink,
    pages: HashMap<u64, WebView>,
    /// Popups the UI has not adopted yet; a download can start in one before it is a tab.
    popups: Popups,
    running: HashMap<u64, (Owner, Download)>,
    /// Reports of downloads started in a popup not yet adopted, released once it is.
    held: HashMap<u64, Held>,
    next: u64,
}

/// Who started a download: a tab, or a popup still waiting on the UI (its request number).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner {
    Tab(u64),
    Popup(u64),
}

/// A popup's download announcement and its latest change, held until the popup has a tab.
struct Held {
    request: u64,
    started: BrowserEvent,
    latest: Option<BrowserEvent>,
}

impl Downloads {
    pub(super) fn watch(context: &WebContext, events: EventSink, popups: Popups) -> Self {
        let downloads = Self(Rc::new(RefCell::new(Registry {
            events,
            pages: HashMap::new(),
            popups,
            running: HashMap::new(),
            held: HashMap::new(),
            next: 0,
        })));
        let watcher = downloads.clone();
        context.connect_download_started(move |_, download| watcher.started(download));
        downloads
    }

    pub(super) fn track(&self, tab: u64, view: &WebView) {
        self.0.borrow_mut().pages.insert(tab, view.clone());
    }

    /// The page closed; its downloads keep running and reporting, as on Windows.
    pub(super) fn forget(&self, tab: u64) {
        self.0.borrow_mut().pages.remove(&tab);
    }

    /// Popup `request` became tab `tab`: its downloads are reported as that tab's.
    pub(super) fn adopted(&self, request: u64, tab: u64) {
        self.release(request, tab);
    }

    /// Popup `request` was refused or its opener closed: its downloads are the opener's.
    pub(super) fn released(&self, request: u64, opener: u64) {
        self.release(request, opener);
    }

    /// WebKitGTK can cancel a download but not pause or resume one; the flyout never offers
    /// those here (`PAUSES_DOWNLOADS`), and a stray request is reported, not ignored.
    pub(super) fn control(&self, download: u64, control: DownloadControl) {
        let registry = self.0.borrow();
        let Some((owner, running)) = registry.running.get(&download).cloned() else {
            return;
        };
        if control == DownloadControl::Cancel {
            drop(registry);
            running.cancel();
            return;
        }
        // A held popup download is not listed yet, so the flyout cannot have asked for it.
        let Owner::Tab(tab) = owner else {
            return;
        };
        registry.events.emit(BrowserEvent::Error {
            tab: Some(tab),
            error: BrowserError::PageControl,
        });
    }

    fn started(&self, download: &Download) {
        let id = self.register(download);
        let registry = self.clone();
        download.connect_decide_destination(move |download, suggested| {
            let Some(path) = destination(suggested) else {
                download.cancel();
                return true;
            };
            download.set_allow_overwrite(false);
            download.set_destination(&glib::filename_to_uri(&path, None).unwrap_or_default());
            registry.announce(id, download, &path);
            registry.report_progress(id, download);
            true
        });
    }

    /// Reports the download's start, now or once its popup has a tab.
    fn announce(&self, id: u64, download: &Download, path: &std::path::Path) {
        let owner = self.owner_of(download);
        let tab = match owner {
            Owner::Tab(tab) => tab,
            Owner::Popup(_) => 0,
        };
        let started = BrowserEvent::DownloadStarted {
            download: id,
            tab,
            file_name: file_name(path),
            total: total_length(download),
        };
        let mut registry = self.0.borrow_mut();
        if let Some((recorded, _)) = registry.running.get_mut(&id) {
            *recorded = owner;
        }
        match owner {
            Owner::Tab(_) => registry.events.emit(started),
            Owner::Popup(request) => {
                registry.held.insert(
                    id,
                    Held {
                        request,
                        started,
                        latest: None,
                    },
                );
            }
        }
    }

    fn report_progress(&self, id: u64, download: &Download) {
        let progress = self.clone();
        let last = Cell::new(None::<Instant>);
        download.connect_received_data(move |download, _| {
            let now = Instant::now();
            if last
                .get()
                .is_some_and(|last| now.duration_since(last) < PROGRESS_INTERVAL)
            {
                return;
            }
            last.set(Some(now));
            progress.report(changed(id, download, DownloadState::InProgress));
        });
        let finished = self.clone();
        download.connect_finished(move |download| {
            finished.0.borrow_mut().running.remove(&id);
            // A failure emits its own report first; `finished` follows every download.
            if download.estimated_progress() >= 1.0 {
                finished.report(changed(id, download, DownloadState::Completed));
            }
        });
        let failed = self.clone();
        download.connect_failed(move |download, error| {
            failed.report(changed(
                id,
                download,
                DownloadState::Interrupted(interruption(error)),
            ));
        });
    }

    /// Sends a change, or keeps it as the latest while the download's popup waits.
    fn report(&self, event: BrowserEvent) {
        let BrowserEvent::DownloadChanged { download, .. } = &event else {
            return;
        };
        let mut registry = self.0.borrow_mut();
        match registry.held.get_mut(download) {
            Some(held) => held.latest = Some(event),
            None => registry.events.emit(event),
        }
    }

    /// Sends what was held for popup `request`'s downloads, as tab `tab`'s.
    fn release(&self, request: u64, tab: u64) {
        let mut registry = self.0.borrow_mut();
        let ids: Vec<u64> = registry
            .held
            .iter()
            .filter(|(_, held)| held.request == request)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(held) = registry.held.remove(&id) else {
                continue;
            };
            if let Some((owner, _)) = registry.running.get_mut(&id) {
                *owner = Owner::Tab(tab);
            }
            registry.events.emit(retarget(held.started, tab));
            if let Some(latest) = held.latest {
                registry.events.emit(latest);
            }
        }
    }

    fn register(&self, download: &Download) -> u64 {
        let mut registry = self.0.borrow_mut();
        registry.next += 1;
        let id = registry.next;
        registry
            .running
            .insert(id, (Owner::Tab(0), download.clone()));
        id
    }

    fn owner_of(&self, download: &Download) -> Owner {
        let Some(view) = download.web_view() else {
            return Owner::Tab(0);
        };
        let registry = self.0.borrow();
        if let Some((tab, _)) = registry.pages.iter().find(|(_, page)| **page == view) {
            return Owner::Tab(*tab);
        }
        let popups = registry.popups.borrow();
        popups
            .iter()
            .find(|(_, pending)| pending.view == view)
            .map_or(Owner::Tab(0), |(request, _)| Owner::Popup(*request))
    }
}

/// A held announcement, now naming the tab its popup became.
fn retarget(started: BrowserEvent, tab: u64) -> BrowserEvent {
    match started {
        BrowserEvent::DownloadStarted {
            download,
            file_name,
            total,
            ..
        } => BrowserEvent::DownloadStarted {
            download,
            tab,
            file_name,
            total,
        },
        other => other,
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
    use super::{BrowserEvent, retarget, safe_name};

    #[test]
    fn a_suggested_name_cannot_leave_the_downloads_folder_or_hide() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("..\\secret.txt"), "secret.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("   "), "download");
        assert_eq!(safe_name("report.pdf"), "report.pdf");
    }

    #[test]
    fn a_held_popup_download_is_announced_as_the_tab_it_became() {
        let held = BrowserEvent::DownloadStarted {
            download: 4,
            tab: 0,
            file_name: "synthetic.zip".to_owned(),
            total: Some(10),
        };
        let BrowserEvent::DownloadStarted {
            download,
            tab,
            file_name,
            total,
        } = retarget(held, 7)
        else {
            panic!("still a start");
        };
        assert_eq!((download, tab, total), (4, 7, Some(10)));
        assert_eq!(file_name, "synthetic.zip");
    }
}
