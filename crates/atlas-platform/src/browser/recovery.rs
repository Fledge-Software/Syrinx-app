//! Rebuilds failed native controllers while retaining their live identities and destinations.

use super::*;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
};

const ATTEMPT_LIMIT: usize = 3;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct RecoveryBudget {
    attempts: VecDeque<Instant>,
    pub(super) due: Option<Instant>,
    stopped: bool,
}

impl RecoveryBudget {
    fn schedule(&mut self, now: Instant) -> bool {
        while self
            .attempts
            .front()
            .is_some_and(|attempt| now.saturating_duration_since(*attempt) >= ATTEMPT_WINDOW)
        {
            self.attempts.pop_front();
        }
        if self.stopped || self.attempts.len() >= ATTEMPT_LIMIT {
            self.stopped = true;
            self.due = None;
            return false;
        }
        self.due = Some(now + Duration::from_secs(1 << self.attempts.len()));
        true
    }

    fn take_due(&mut self, now: Instant) -> bool {
        if !self.due.is_some_and(|due| due <= now) {
            return false;
        }
        self.due = None;
        self.attempts.push_back(now);
        true
    }

    fn owner_retry(&mut self, now: Instant) {
        self.attempts.clear();
        self.attempts.push_back(now);
        self.due = None;
        self.stopped = false;
    }
}

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
        // Preserve the smoke failure signal before retirement invalidates native callback tickets.
        self.events.emit(BrowserEvent::ProcessFailed { id, kind });
        match failure_action(kind) {
            FailureAction::Browser => self.schedule_recovery(Instant::now()),
            FailureAction::Renderer => self.renderer_failed(id),
            FailureAction::Informational => {}
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
        if self.recovery.stopped || self.recovery.due.is_some() {
            return Err(BrowserError::Recovery);
        }
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

    fn schedule_recovery(&mut self, now: Instant) {
        if !self.begin_recovery() {
            return;
        }
        if !self.recovery.schedule(now) {
            self.finish_recovery(Vec::new());
        }
    }

    pub(super) fn recovery_tick(&mut self, now: Instant) -> bool {
        if !self.recovery.take_due(now) {
            return false;
        }
        self.rebuild_environment(environment::create);
        true
    }

    pub(super) fn retry_unavailable(&mut self, id: u64) -> Result<(), BrowserError> {
        // An already scheduled attempt must finish before an explicit retry can replace it.
        if self.recovery.due.is_some() {
            return Err(BrowserError::Recovery);
        }
        if self.environment.is_some() {
            return self.restore_page(id);
        }
        self.recovery.owner_retry(Instant::now());
        if self.begin_recovery() {
            self.rebuild_environment(environment::create);
        }
        Ok(())
    }

    fn begin_recovery(&mut self) -> bool {
        let Some(sequence) = self.recovery_sequence.checked_add(1) else {
            self.events.emit(BrowserEvent::Error {
                tab: None,
                error: BrowserError::Recovery,
            });
            return false;
        };
        self.recovery_sequence = sequence;
        for (id, page) in &self.pages {
            self.unavailable.insert(*id, page.destination());
        }
        self.events.emit(BrowserEvent::RecoveryStarted {
            sequence,
            tabs: self.unavailable_tabs(),
        });

        // No old controller or environment may survive into the replacement generation.
        self.popups.clear();
        self.pages.clear();
        drop(self.environment.take());
        true
    }

    fn unavailable_tabs(&self) -> Vec<u64> {
        let mut tabs: Vec<_> = self.unavailable.keys().copied().collect();
        tabs.sort_unstable();
        tabs
    }

    fn rebuild_environment(
        &mut self,
        create: impl FnOnce(&std::path::Path) -> Result<ICoreWebView2Environment, BrowserError>,
    ) {
        let tabs = self.unavailable_tabs();
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
        self.finish_recovery(restored);
    }

    fn finish_recovery(&self, restored: Vec<u64>) {
        self.events.emit(BrowserEvent::RecoveryFinished {
            sequence: self.recovery_sequence,
            restored,
            failed: self.unavailable_tabs(),
            stopped: self.recovery.stopped,
        });
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
