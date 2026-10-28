//! Reports each change to a page's back and forward list from host-side events only (#126).

use std::{cell::Cell, rc::Rc};

use syrinx_core::{HISTORY_ENTRY_LIMIT, HistoryChange, HistoryDirection, HistoryReport};
use webview2_com::{
    CoTaskMemPWSTR, HistoryChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_NAVIGATION_KIND, COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD,
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD, ICoreWebView2,
        ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
        ICoreWebView2NavigationStartingEventArgs3, ICoreWebView2SourceChangedEventArgs,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler, SourceChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

/// What the latest navigation said about itself when it started.
#[derive(Clone, Copy)]
enum Started {
    NewDocument { user_initiated: bool },
    BackOrForward,
    Reload,
}

/// Facts gathered between two `HistoryChanged` events.
#[derive(Clone, Copy, Default)]
struct Pending {
    /// The latest navigation's id and kind, until it commits or fails.
    started: Option<(u64, Started)>,
    /// Set by `SourceChanged`: whether the new address came with a new document.
    new_document: Option<bool>,
    /// The latest step back or forward failed and no navigation has started since, so the
    /// jump it was for is abandoned. Reported with the next history change: each middle step
    /// of a jump fails just before the next one starts (spike, cacheable pages).
    abandoned: bool,
}

impl Pending {
    /// The change the spike's event sequences describe. A reload, and any visit to the
    /// address already shown, raises no `SourceChanged` (trapstoknow, F14).
    fn change(self) -> HistoryChange {
        let started = self.started.map(|(_, started)| started);
        match (self.new_document, started) {
            (Some(false), started) => HistoryChange::SamePage {
                navigating: started.is_some(),
            },
            (Some(true), Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            (Some(true), Some(Started::NewDocument { user_initiated })) => {
                HistoryChange::NewDocument { user_initiated }
            }
            // A reload that redirects replaces its entry, so it adds no row.
            (Some(true), Some(Started::Reload)) => HistoryChange::Unchanged,
            // A new document with no start seen is Syrinx's own or the engine's: user-made.
            (Some(true), None) => HistoryChange::NewDocument {
                user_initiated: true,
            },
            // A step back or forward to an entry with the shown address still needs its
            // landing checked against the requested jump.
            (None, Some(Started::BackOrForward)) => HistoryChange::BackOrForward,
            // Chromium turns a new visit to the shown address into a replacement, so it adds
            // no row; a form posted back to the same address is the known exception.
            (None, Some(Started::NewDocument { .. } | Started::Reload) | None) => {
                HistoryChange::Unchanged
            }
        }
    }

    /// What is left once a `HistoryChanged` has reported `change`. A same-page change keeps
    /// a navigation still in flight, such as a Back during a slow load.
    fn after(self, change: HistoryChange) -> Self {
        match change {
            HistoryChange::SamePage { .. } => Self {
                new_document: None,
                ..self
            },
            HistoryChange::NewDocument { .. }
            | HistoryChange::BackOrForward
            | HistoryChange::Unchanged
            | HistoryChange::JumpAbandoned => Self::default(),
        }
    }

    /// Navigation `id` has started; any abandoned step before it was a middle step.
    fn start(self, id: u64, started: Started) -> Self {
        Self {
            started: Some((id, started)),
            abandoned: false,
            ..self
        }
    }

    /// Navigation `id` finished. It is forgotten either way, so a later `HistoryChanged` is
    /// never read as its own; a failed latest step back or forward abandons its jump. A commit's
    /// `HistoryChanged` always came before its `NavigationCompleted` in the spike.
    fn complete(self, id: u64, success: bool) -> Self {
        let latest_step =
            matches!(self.started, Some((started, Started::BackOrForward)) if started == id);
        Self {
            started: self.started.filter(|(started, _)| *started != id),
            abandoned: self.abandoned || (latest_step && !success),
            ..self
        }
    }
}

/// Registers four host-side events; none adds anything to the page.
pub(super) fn register(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let core = view.webview();
    let pending = Rc::new(Cell::new(Pending::default()));
    let watch = Watch {
        id,
        events,
        pending,
    };
    register_starting(&core, watch.clone())?;
    register_source(&core, watch.clone())?;
    register_failure(&core, watch.clone())?;
    register_changed(&core, watch)
}

/// Sends `steps` Back or Forward calls in a row; the engine commits only the last (spike).
pub(super) fn step(
    core: &ICoreWebView2,
    direction: HistoryDirection,
    steps: u32,
) -> Result<(), BrowserError> {
    let too_many = usize::try_from(steps).map_or(true, |steps| steps > HISTORY_ENTRY_LIMIT);
    if steps == 0 || too_many {
        return Err(BrowserError::Navigation);
    }
    let backward = direction == HistoryDirection::Back;
    let mut allowed = BOOL::default();
    unsafe {
        if backward {
            core.CanGoBack(&mut allowed)
        } else {
            core.CanGoForward(&mut allowed)
        }
    }
    .map_err(|_| BrowserError::Navigation)?;
    if !allowed.as_bool() {
        return Ok(());
    }
    for _ in 0..steps {
        unsafe {
            if backward {
                core.GoBack()
            } else {
                core.GoForward()
            }
        }
        .map_err(|_| BrowserError::Navigation)?;
    }
    Ok(())
}

/// What every handler shares: the page, its event sink and the facts gathered so far.
#[derive(Clone)]
struct Watch {
    id: u64,
    events: EventSink,
    pending: Rc<Cell<Pending>>,
}

impl Watch {
    /// Reports a handler that could not read its event, so a lost kind is never silent.
    fn reported(&self, result: windows::core::Result<()>) -> windows::core::Result<()> {
        if result.is_err() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.id),
                error: BrowserError::Metadata,
            });
        }
        result
    }

    /// Replaces the gathered facts with `change` applied to them.
    fn update(&self, change: impl FnOnce(Pending) -> Pending) {
        self.pending.set(change(self.pending.get()));
    }

    /// Records the kind and origin of a navigation that is starting.
    fn starting(
        &self,
        args: &ICoreWebView2NavigationStartingEventArgs,
    ) -> windows::core::Result<()> {
        let mut kind = COREWEBVIEW2_NAVIGATION_KIND::default();
        let (mut navigation, mut user) = (0, BOOL::default());
        unsafe {
            args.cast::<ICoreWebView2NavigationStartingEventArgs3>()?
                .NavigationKind(&mut kind)?;
            args.NavigationId(&mut navigation)?;
            args.IsUserInitiated(&mut user)?;
        }
        let started = match kind {
            COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD => Started::BackOrForward,
            COREWEBVIEW2_NAVIGATION_KIND_RELOAD => Started::Reload,
            _ => Started::NewDocument {
                user_initiated: user.as_bool(),
            },
        };
        self.update(|pending| pending.start(navigation, started));
        Ok(())
    }

    /// Records whether the new address came with a new document.
    fn source(&self, args: &ICoreWebView2SourceChangedEventArgs) -> windows::core::Result<()> {
        let mut new_document = BOOL::default();
        unsafe { args.IsNewDocument(&mut new_document) }?;
        self.update(|pending| Pending {
            new_document: Some(new_document.as_bool()),
            ..pending
        });
        Ok(())
    }

    /// Forgets a navigation that finished; a failed latest step marks its jump abandoned.
    fn completed(
        &self,
        args: &ICoreWebView2NavigationCompletedEventArgs,
    ) -> windows::core::Result<()> {
        let (mut navigation, mut success) = (0, BOOL::default());
        unsafe {
            args.NavigationId(&mut navigation)?;
            args.IsSuccess(&mut success)?;
        }
        self.update(|pending| pending.complete(navigation, success.as_bool()));
        Ok(())
    }

    /// Reports the change the gathered facts describe, with the engine's current state.
    fn changed(&self, core: &ICoreWebView2) {
        // An abandoned jump is dropped first, so this change is not checked against it.
        if self.pending.get().abandoned {
            self.report(core, HistoryChange::JumpAbandoned);
            self.update(|pending| Pending {
                abandoned: false,
                ..pending
            });
        }
        let change = self.pending.get().change();
        self.update(|pending| pending.after(change));
        self.report(core, change);
    }

    /// Emits `change` with the address shown and where the engine can go.
    fn report(&self, core: &ICoreWebView2, change: HistoryChange) {
        let event = match read_report(core, change) {
            Ok(report) => BrowserEvent::History {
                id: self.id,
                report,
            },
            Err(error) => BrowserEvent::Error {
                tab: Some(self.id),
                error,
            },
        };
        self.events.emit(event);
    }
}

/// The error a handler returns when WebView2 passed it no sender or arguments.
fn missing() -> windows::core::Error {
    windows::core::Error::from(E_POINTER)
}

/// Each `register_*` adds one host-side handler to the page's web view.
fn register_starting(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.starting(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_NavigationStarting(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_source(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = SourceChangedEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.source(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_SourceChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_failure(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        watch.reported(
            args.ok_or_else(missing)
                .and_then(|args| watch.completed(&args)),
        )
    }));
    let mut token = 0;
    unsafe { core.add_NavigationCompleted(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

fn register_changed(core: &ICoreWebView2, watch: Watch) -> Result<(), BrowserError> {
    let handler = HistoryChangedEventHandler::create(Box::new(move |core, _| {
        let Some(core) = core else {
            return watch.reported(Err(missing()));
        };
        watch.changed(&core);
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_HistoryChanged(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

/// Reads the address the engine shows now and where it can go from there.
fn read_report(core: &ICoreWebView2, change: HistoryChange) -> Result<HistoryReport, BrowserError> {
    let (mut back, mut forward) = (BOOL::default(), BOOL::default());
    unsafe {
        core.CanGoBack(&mut back)
            .and_then(|()| core.CanGoForward(&mut forward))
    }
    .map_err(|_| BrowserError::Metadata)?;
    let mut raw = PWSTR::null();
    let result = unsafe { core.Source(&mut raw) };
    let address = CoTaskMemPWSTR::from(raw).to_string();
    result.map_err(|_| BrowserError::Metadata)?;
    Ok(HistoryReport {
        change,
        address,
        can_go_back: back.as_bool(),
        can_go_forward: forward.as_bool(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(started: Option<Started>, new_document: Option<bool>) -> Pending {
        Pending {
            started: started.map(|started| (7, started)),
            new_document,
            abandoned: false,
        }
    }

    fn change(started: Option<Started>, new_document: Option<bool>) -> HistoryChange {
        pending(started, new_document).change()
    }

    const TYPED: Option<Started> = Some(Started::NewDocument {
        user_initiated: true,
    });

    #[test]
    fn a_same_document_address_change_is_a_same_page_change() {
        assert_eq!(
            change(None, Some(false)),
            HistoryChange::SamePage { navigating: false }
        );
    }

    #[test]
    fn a_same_page_change_during_another_navigation_is_the_page_s_own() {
        // A replaceState while a Back loads must not be read as the Back's landing.
        assert_eq!(
            change(Some(Started::BackOrForward), Some(false)),
            HistoryChange::SamePage { navigating: true }
        );
        assert_eq!(
            change(TYPED, Some(false)),
            HistoryChange::SamePage { navigating: true }
        );
    }

    #[test]
    fn the_middle_steps_of_a_jump_do_not_abandon_it() {
        // The spike: each cancelled middle step completes just before the next one starts.
        let jump = Pending::default()
            .start(33, Started::BackOrForward)
            .complete(33, false)
            .start(34, Started::BackOrForward)
            .complete(34, false)
            .start(35, Started::BackOrForward);
        assert!(!jump.abandoned);
    }

    #[test]
    fn a_middle_step_that_completes_after_the_next_start_does_not_abandon_it() {
        let jump = Pending::default()
            .start(33, Started::BackOrForward)
            .start(34, Started::BackOrForward)
            .complete(33, false);
        assert!(!jump.abandoned);
    }

    #[test]
    fn a_failed_last_step_abandons_its_jump() {
        let stayed = Pending::default()
            .start(40, Started::BackOrForward)
            .complete(40, false);
        assert!(stayed.abandoned);
        let typed = Pending::default()
            .start(
                41,
                Started::NewDocument {
                    user_initiated: true,
                },
            )
            .complete(41, false);
        assert!(
            !typed.abandoned,
            "only a step back or forward abandons a jump"
        );
    }

    #[test]
    fn a_finished_navigation_is_forgotten_either_way() {
        let done = Pending::default()
            .start(50, Started::BackOrForward)
            .complete(50, true);
        assert_eq!(done.change(), HistoryChange::Unchanged);
        assert!(!done.abandoned);
    }

    #[test]
    fn a_new_document_carries_its_navigation_kind() {
        let script = Some(Started::NewDocument {
            user_initiated: false,
        });
        assert_eq!(
            change(script, Some(true)),
            HistoryChange::NewDocument {
                user_initiated: false
            }
        );
        assert_eq!(
            change(Some(Started::BackOrForward), Some(true)),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_reload_or_a_change_without_a_new_address_is_unchanged() {
        assert_eq!(
            change(Some(Started::Reload), None),
            HistoryChange::Unchanged
        );
        assert_eq!(change(None, None), HistoryChange::Unchanged);
    }

    #[test]
    fn a_reload_that_redirects_replaces_rather_than_adds() {
        assert_eq!(
            change(Some(Started::Reload), Some(true)),
            HistoryChange::Unchanged
        );
    }

    #[test]
    fn a_step_to_an_entry_at_the_shown_address_is_still_a_step() {
        // No SourceChanged arrives, but the jump's landing must still be checked.
        assert_eq!(
            change(Some(Started::BackOrForward), None),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_new_visit_to_the_shown_address_replaces_rather_than_adds() {
        assert_eq!(change(TYPED, None), HistoryChange::Unchanged);
    }

    #[test]
    fn a_navigation_that_never_committed_is_forgotten() {
        let started = pending(Some(Started::BackOrForward), None);
        assert_eq!(
            started.complete(7, false).change(),
            HistoryChange::Unchanged
        );
        // A late failure of an older navigation leaves the newer one in place.
        assert_eq!(
            started.complete(6, false).change(),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_same_page_change_keeps_a_navigation_still_in_flight() {
        // A replaceState during a slow Back must not turn the Back's commit into a push.
        let during = pending(Some(Started::BackOrForward), Some(false));
        let left = during.after(during.change());
        assert_eq!(
            Pending {
                new_document: Some(true),
                ..left
            }
            .change(),
            HistoryChange::BackOrForward
        );
    }

    #[test]
    fn a_committed_change_clears_everything() {
        let committed = pending(TYPED, Some(true));
        let left = committed.after(committed.change());
        assert_eq!(left.change(), HistoryChange::Unchanged);
    }
}
