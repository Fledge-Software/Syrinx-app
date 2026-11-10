//! Joins native download events with the same shared close lifecycle used by the application.

use super::{CHILD, OPENER, files, require};
use syrinx_core::{Download, DownloadInterruption, DownloadList, DownloadState};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

pub(super) struct Watch {
    name: String,
    popup: Option<String>,
    requested: bool,
    created: bool,
    list: DownloadList,
    pub deferred: bool,
    pub closed: bool,
}

impl Watch {
    pub fn new(name: &str, popup: Option<String>) -> Self {
        Self {
            name: name.into(),
            popup,
            requested: false,
            created: false,
            list: DownloadList::default(),
            deferred: false,
            closed: false,
        }
    }

    pub fn seen(&self) -> Result<&Download, String> {
        self.list
            .downloads()
            .first()
            .ok_or("Synthetic download was never announced".into())
    }

    pub fn receiving(&self) -> bool {
        self.seen().is_ok_and(|download| {
            download.state == DownloadState::InProgress
                && download.received > 0
                && Some(download.received) < download.total
        })
    }

    pub fn completed(&self) -> bool {
        self.seen()
            .is_ok_and(|download| download.state == DownloadState::Completed)
            && (self.popup.is_none() || self.created)
    }

    pub fn inspect_without_command(&mut self, event: &BrowserEvent) -> Result<(), String> {
        require(
            self.observe(event)?.is_none(),
            "Unexpected late popup or close command",
        )
    }

    pub fn observe(&mut self, event: &BrowserEvent) -> Result<Option<BrowserCommand>, String> {
        match event {
            BrowserEvent::PopupRequested { id, request, url } => {
                require(
                    *id == OPENER && !self.requested && self.popup.as_deref() == Some(url),
                    "Unexpected download popup target or opener",
                )?;
                self.requested = true;
                return Ok(Some(BrowserCommand::AdoptPopup {
                    request: *request,
                    id: CHILD,
                }));
            }
            BrowserEvent::Created { id: CHILD } => {
                require(
                    self.requested && !self.created,
                    "Popup creation did not match adoption",
                )?;
                self.created = true;
                return Ok(Some(BrowserCommand::ShowActive(Some(CHILD))));
            }
            BrowserEvent::DownloadStarted {
                download,
                tab,
                file_name,
                total,
            } => {
                self.started(*download, *tab, file_name, *total)?;
            }
            BrowserEvent::DownloadChanged {
                download,
                state,
                received,
                total,
                can_resume,
            } => {
                return self.changed(*download, *state, *received, *total, *can_resume);
            }
            BrowserEvent::CloseRequested { id } => {
                require(
                    *id == CHILD && self.created && !self.closed,
                    "Unexpected download page close",
                )?;
                if self.list.defer_close(*id) {
                    self.deferred = true;
                } else {
                    require(
                        self.completed(),
                        "Popup requested close before its download was announced",
                    )?;
                    self.closed = true;
                    return Ok(Some(BrowserCommand::Close { id: *id }));
                }
            }
            BrowserEvent::PopupBlocked { .. } => {
                return Err("Gesture download popup was blocked".into());
            }
            _ => {}
        }
        Ok(None)
    }
    fn started(
        &mut self,
        download: u64,
        tab: u64,
        file_name: &str,
        total: Option<u64>,
    ) -> Result<(), String> {
        let expected = if self.popup.is_some() { CHILD } else { OPENER };
        require(
            self.list.downloads().is_empty(),
            "Duplicate or unexpected download announcement",
        )?;
        require(
            tab == expected && file_name == self.name,
            "Download attribution or destination name differs from the page that started it",
        )?;
        require(
            self.popup.is_none() || self.requested,
            "Popup download was not requested for adoption",
        )?;
        require(
            total == Some(files::body(&self.name).len() as u64),
            "Attachment length differs from native download announcement",
        )?;
        self.list.add(download, tab, file_name, total);
        Ok(())
    }

    fn changed(
        &mut self,
        download: u64,
        state: DownloadState,
        received: u64,
        total: Option<u64>,
        can_resume: bool,
    ) -> Result<Option<BrowserCommand>, String> {
        let current = self.seen()?;
        require(current.id == download, "Download update was not announced")?;
        require(
            !can_resume
                && total == current.total
                && received >= current.received
                && received <= total.unwrap_or(0),
            "Download progress or resumability is invalid",
        )?;
        require(
            current.state == DownloadState::InProgress || current.state == state,
            "Terminal download changed state again",
        )?;
        if let DownloadState::Interrupted(reason) = state {
            require(
                self.name == files::CANCEL && reason == DownloadInterruption::Cancelled,
                "Synthetic download interrupted unexpectedly",
            )?;
        }
        if state == DownloadState::Completed {
            require(
                self.name != files::CANCEL && Some(received) == total,
                "Download completed with missing bytes or after Cancel",
            )?;
        }
        self.list.progress(download, received, total);
        self.list.set_resumable(download, can_resume);
        if let Some(tab) = self.list.update(download, state, received) {
            require(
                tab == CHILD && self.deferred && !self.closed,
                "Shared close release named the wrong popup",
            )?;
            self.closed = true;
            return Ok(Some(BrowserCommand::Close { id: tab }));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(state: DownloadState, received: u64) -> BrowserEvent {
        BrowserEvent::DownloadChanged {
            download: 91,
            state,
            received,
            total: Some(131094),
            can_resume: false,
        }
    }

    fn adopted() -> Watch {
        let mut watch = Watch::new(
            files::HELD,
            Some("http://127.0.0.1:1234/download-held-child".into()),
        );
        assert!(matches!(
            watch
                .observe(&BrowserEvent::PopupRequested {
                    id: 5000,
                    request: 6,
                    url: "http://127.0.0.1:1234/download-held-child".into()
                })
                .unwrap(),
            Some(BrowserCommand::AdoptPopup {
                request: 6,
                id: 5001
            })
        ));
        assert!(matches!(
            watch.observe(&BrowserEvent::Created { id: 5001 }).unwrap(),
            Some(BrowserCommand::ShowActive(Some(5001)))
        ));
        watch
            .observe(&BrowserEvent::DownloadStarted {
                download: 91,
                tab: 5001,
                file_name: "synthetic-held.zip".into(),
                total: Some(131094),
            })
            .unwrap();
        watch
    }

    #[test]
    fn held_popup_reports_may_arrive_before_adoption_acknowledgement() {
        let mut watch = Watch::new(
            files::POPUP,
            Some("http://127.0.0.1:1234/download/synthetic-popup.zip".into()),
        );
        watch
            .observe(&BrowserEvent::PopupRequested {
                id: 5000,
                request: 6,
                url: "http://127.0.0.1:1234/download/synthetic-popup.zip".into(),
            })
            .unwrap();
        watch
            .observe(&BrowserEvent::DownloadStarted {
                download: 91,
                tab: 5001,
                file_name: "synthetic-popup.zip".into(),
                total: Some(22),
            })
            .unwrap();
        watch
            .observe(&BrowserEvent::DownloadChanged {
                download: 91,
                state: DownloadState::Completed,
                received: 22,
                total: Some(22),
                can_resume: false,
            })
            .unwrap();
        assert!(
            !watch.completed(),
            "Native adoption acknowledgement is still required"
        );
        watch.observe(&BrowserEvent::Created { id: 5001 }).unwrap();
        assert!(watch.completed());
    }

    #[test]
    fn shared_close_deferral_releases_only_after_real_completion() {
        let mut watch = adopted();
        watch
            .observe(&changed(DownloadState::InProgress, 65536))
            .unwrap();
        assert!(watch.receiving());
        assert!(
            watch
                .observe(&BrowserEvent::CloseRequested { id: 5001 })
                .unwrap()
                .is_none()
        );
        assert!(watch.deferred && !watch.closed);
        assert!(
            watch
                .observe(&changed(DownloadState::InProgress, 90000))
                .unwrap()
                .is_none()
        );
        assert!(!watch.closed);
        assert!(matches!(
            watch
                .observe(&changed(DownloadState::Completed, 131094))
                .unwrap(),
            Some(BrowserCommand::Close { id: 5001 })
        ));
        assert!(watch.closed && watch.completed());
    }

    #[test]
    fn wrong_attribution_and_short_completion_cannot_pass() {
        let mut watch = Watch::new(files::SAME, None);
        assert!(
            watch
                .observe(&BrowserEvent::DownloadStarted {
                    download: 91,
                    tab: 5001,
                    file_name: "synthetic-same.zip".into(),
                    total: Some(22)
                })
                .is_err()
        );
        assert!(
            adopted()
                .observe(&changed(DownloadState::Completed, 65536))
                .is_err()
        );
    }

    #[test]
    fn cancelled_download_cannot_later_complete_or_resume() {
        let mut watch = Watch::new(files::CANCEL, None);
        watch
            .observe(&BrowserEvent::DownloadStarted {
                download: 91,
                tab: 5000,
                file_name: "synthetic-cancel.zip".into(),
                total: Some(131094),
            })
            .unwrap();
        watch
            .observe(&changed(DownloadState::InProgress, 65536))
            .unwrap();
        watch
            .observe(&changed(
                DownloadState::Interrupted(DownloadInterruption::Cancelled),
                65536,
            ))
            .unwrap();
        assert!(watch.seen().unwrap().offered_controls(false).is_empty());
        assert!(
            watch
                .observe(&changed(DownloadState::Completed, 131094))
                .is_err()
        );
        assert!(
            watch
                .observe(&changed(DownloadState::InProgress, 70000))
                .is_err()
        );
    }
}
