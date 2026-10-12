//! Rebuilds failed native controllers while retaining their live identities and destinations.

use super::*;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
};

#[derive(Debug, Eq, PartialEq)]
enum FailureAction {
    Browser,
    Renderer,
    Informational,
}

fn failure_action(kind: i32) -> FailureAction {
    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED.0 {
        return FailureAction::Browser;
    }
    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED.0 {
        return FailureAction::Renderer;
    }
    FailureAction::Informational
}

impl BrowserState {
    pub(super) fn native_event(&mut self, event: BrowserEvent) {
        let failure = match &event {
            BrowserEvent::Callback { ticket, event } if ticket.current() => match event.as_ref() {
                BrowserEvent::ProcessFailed { id, kind } => Some((*id, *kind)),
                _ => None,
            },
            _ => None,
        };
        let Some((id, kind)) = failure else {
            self.events.emit(event);
            return;
        };
        match failure_action(kind) {
            FailureAction::Browser => self.recover_browser(),
            FailureAction::Renderer => self.renderer_failed(id),
            FailureAction::Informational => self.events.emit(event),
        }
    }

    fn renderer_failed(&mut self, id: u64) {
        let Some(page) = self.pages.remove(&id) else {
            return;
        };
        self.unavailable.insert(id, page.destination());
        self.popups.cancel_opener(id);
        // Retiring this controller invalidates repeated failure events already queued for it.
        drop(page);
        self.events.emit(BrowserEvent::PageCrashed { id });
    }

    pub(super) fn ensure_environment(&mut self) -> Result<(), BrowserError> {
        if self.environment.is_none() {
            self.environment = Some(environment::create(&self.profile)?);
        }
        Ok(())
    }

    pub(super) fn restore_page(&mut self, id: u64) -> Result<(), BrowserError> {
        let url = self
            .unavailable
            .get(&id)
            .ok_or(BrowserError::UnknownTab)?
            .clone();
        let page = self.build_page(id)?;
        page.navigate(&url)?;
        page.show(self.active == Some(id))?;
        self.pages.insert(id, page);
        self.unavailable.remove(&id);
        self.events.emit(BrowserEvent::PageRestored { id });
        Ok(())
    }

    pub(super) fn recover_browser(&mut self) {
        self.recover_browser_with(environment::create);
    }

    fn recover_browser_with(
        &mut self,
        create: impl FnOnce(&std::path::Path) -> Result<ICoreWebView2Environment, BrowserError>,
    ) {
        let Some(sequence) = self.recovery_sequence.checked_add(1) else {
            self.events.emit(BrowserEvent::Error {
                tab: None,
                error: BrowserError::Recovery,
            });
            return;
        };
        self.recovery_sequence = sequence;
        for (id, page) in &self.pages {
            self.unavailable.insert(*id, page.destination());
        }
        let mut tabs: Vec<_> = self.unavailable.keys().copied().collect();
        tabs.sort_unstable();
        self.events.emit(BrowserEvent::RecoveryStarted {
            sequence,
            tabs: tabs.clone(),
        });

        // No old controller or environment may survive into the replacement generation.
        self.popups.clear();
        self.pages.clear();
        drop(self.environment.take());
        let mut restored = Vec::new();
        match create(&self.profile) {
            Ok(environment) => {
                self.environment = Some(environment);
                for id in &tabs {
                    match self.restore_page(*id) {
                        Ok(()) => restored.push(*id),
                        Err(error) => self.events.emit(BrowserEvent::Error {
                            tab: Some(*id),
                            error,
                        }),
                    }
                }
            }
            Err(error) => {
                for id in &tabs {
                    self.events.emit(BrowserEvent::Error {
                        tab: Some(*id),
                        error,
                    });
                }
            }
        }
        let failed = tabs
            .into_iter()
            .filter(|id| self.unavailable.contains_key(id))
            .collect();
        self.events.emit(BrowserEvent::RecoveryFinished {
            sequence,
            restored,
            failed,
        });
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
