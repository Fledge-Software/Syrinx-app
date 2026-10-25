//! Reports every page's downloads to Syrinx's own list from WebView2's download events, and
//! pauses, resumes or cancels them on the owner's request (#143); it attaches nothing to the
//! page and never changes where a file is saved.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use syrinx_core::{BlockReason, DownloadControl, DownloadInterruption, DownloadState};
use webview2_com::{
    BytesReceivedChangedEventHandler, CoTaskMemPWSTR, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

/// Download numbers are unique across every page and controller of this run.
static NEXT_DOWNLOAD: AtomicU64 = AtomicU64::new(1);
/// Progress is reported at most this often; state changes are always reported.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The running downloads Syrinx can still pause, resume or cancel, with the tab that started
/// each. They live on the browser thread and outlive the page that started them.
#[derive(Clone, Default)]
pub(super) struct DownloadOperations(
    Rc<RefCell<HashMap<u64, (u64, ICoreWebView2DownloadOperation)>>>,
);

impl DownloadOperations {
    /// Applies the owner's `control` to `download`. A download that has already finished is
    /// no longer held, so a late click does nothing; a refused call is reported for its tab.
    pub(super) fn control(&self, download: u64, control: DownloadControl, events: &EventSink) {
        // Clone out first: the call can raise StateChanged at once, which updates this map.
        let Some((tab, operation)) = self.0.borrow().get(&download).cloned() else {
            return;
        };
        let result = unsafe {
            match control {
                DownloadControl::Pause => operation.Pause(),
                DownloadControl::Resume => operation.Resume(),
                DownloadControl::Cancel => operation.Cancel(),
            }
        };
        if result.is_err() {
            events.emit(BrowserEvent::Error {
                tab: Some(tab),
                error: BrowserError::PageControl,
            });
        }
    }

    pub(super) fn clear(&self) {
        let operations = std::mem::take(&mut *self.0.borrow_mut());
        drop(operations);
    }

    fn hold(&self, download: u64, tab: u64, operation: ICoreWebView2DownloadOperation) {
        self.0.borrow_mut().insert(download, (tab, operation));
    }

    fn release(&self, download: u64) {
        let released = self.0.borrow_mut().remove(&download);
        drop(released);
    }
}

/// Watches page `tab`'s downloads. `events` must not be tied to the page's lifetime, because a
/// download keeps running after a popup that started it has closed.
pub(super) fn register(
    view: &WebView,
    tab: u64,
    events: EventSink,
    operations: DownloadOperations,
) -> Result<(), BrowserError> {
    let handler = DownloadStartingEventHandler::create(Box::new(move |_core, args| {
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let result = watch(&args, tab, &events, &operations);
        if result.is_err() {
            events.emit(BrowserEvent::Error {
                tab: Some(tab),
                error: BrowserError::PageControl,
            });
        }
        result
    }));
    let mut token = 0;
    let core = view
        .webview()
        .cast::<ICoreWebView2_4>()
        .map_err(|_| BrowserError::EventRegistration)?;
    unsafe { core.add_DownloadStarting(&handler, &mut token) }
        .map_err(|_| BrowserError::EventRegistration)
}

/// Announces a starting download and follows its bytes and state.
fn watch(
    args: &ICoreWebView2DownloadStartingEventArgs,
    tab: u64,
    events: &EventSink,
    operations: &DownloadOperations,
) -> windows::core::Result<()> {
    // Syrinx's flyout replaces WebView2's dialog (owner, 2026-10-24); the download, its path
    // and SmartScreen's check are unchanged, since Handled only hides that dialog.
    unsafe { args.SetHandled(true)? };
    let operation = unsafe { args.DownloadOperation()? };
    let download = NEXT_DOWNLOAD.fetch_add(1, Ordering::Relaxed);
    let path = read_string(|raw| unsafe { args.ResultFilePath(raw) })?;
    let total = total_bytes(&operation)?;
    // Follow the download before announcing it: a row whose state could not be followed would
    // read "in progress" for good, with buttons that reach nothing.
    follow_state(
        &operation,
        download,
        tab,
        events.clone(),
        operations.clone(),
    )?;
    follow_bytes(&operation, download, tab, events.clone())?;
    operations.hold(download, tab, operation);
    events.emit(BrowserEvent::DownloadStarted {
        download,
        tab,
        file_name: file_name(&path),
        total,
    });
    Ok(())
}

/// Reports every state change: finished, interrupted, paused or resumed.
/// A download that can no longer change is released from `operations`.
fn follow_state(
    operation: &ICoreWebView2DownloadOperation,
    download: u64,
    tab: u64,
    events: EventSink,
    operations: DownloadOperations,
) -> windows::core::Result<()> {
    let handler = StateChangedEventHandler::create(Box::new(move |operation, _| {
        let operation = operation.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let finished = report(&operation, download, tab, &events)?;
        if finished {
            operations.release(download);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { operation.add_StateChanged(&handler, &mut token) }
}

/// Reports progress, at most every `PROGRESS_INTERVAL`.
fn follow_bytes(
    operation: &ICoreWebView2DownloadOperation,
    download: u64,
    tab: u64,
    events: EventSink,
) -> windows::core::Result<()> {
    let last = Cell::new(None::<Instant>);
    let handler = BytesReceivedChangedEventHandler::create(Box::new(move |operation, _| {
        let now = Instant::now();
        if last
            .get()
            .is_some_and(|last| now.duration_since(last) < PROGRESS_INTERVAL)
        {
            return Ok(());
        }
        last.set(Some(now));
        let operation = operation.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        report(&operation, download, tab, &events).map(drop)
    }));
    let mut token = 0;
    unsafe { operation.add_BytesReceivedChanged(&handler, &mut token) }
}

/// Sends the operation's current state and byte counts; a failed read is reported. Returns
/// whether the download can no longer change.
fn report(
    operation: &ICoreWebView2DownloadOperation,
    download: u64,
    tab: u64,
    events: &EventSink,
) -> windows::core::Result<bool> {
    match read_change(operation, download) {
        Ok((change, finished)) => {
            events.emit(change);
            Ok(finished)
        }
        Err(error) => {
            events.emit(BrowserEvent::Error {
                tab: Some(tab),
                error: BrowserError::PageControl,
            });
            Err(error)
        }
    }
}

/// The download's change event, and whether it has finished for good: completed, or
/// interrupted with no way to resume.
fn read_change(
    operation: &ICoreWebView2DownloadOperation,
    download: u64,
) -> windows::core::Result<(BrowserEvent, bool)> {
    let mut native = COREWEBVIEW2_DOWNLOAD_STATE::default();
    let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
    let mut received = 0;
    let mut can_resume = BOOL::default();
    unsafe {
        operation.State(&mut native)?;
        operation.InterruptReason(&mut reason)?;
        operation.BytesReceived(&mut received)?;
        operation.CanResume(&mut can_resume)?;
    }
    let state = state(native, reason);
    let can_resume = can_resume.as_bool();
    let finished = match state {
        DownloadState::InProgress => false,
        DownloadState::Completed => true,
        DownloadState::Interrupted(DownloadInterruption::Paused) => false,
        DownloadState::Interrupted(_) => !can_resume,
    };
    let change = BrowserEvent::DownloadChanged {
        download,
        state,
        received: received.max(0).unsigned_abs(),
        total: total_bytes(operation)?,
        can_resume,
    };
    Ok((change, finished))
}

/// The expected size, or `None` when the server sent no length.
fn total_bytes(operation: &ICoreWebView2DownloadOperation) -> windows::core::Result<Option<u64>> {
    let mut total = -1;
    unsafe { operation.TotalBytesToReceive(&mut total)? };
    Ok(known_total(total))
}

/// WebView2 reports -1 for a size the server did not send: a sentinel, not an error.
fn known_total(total: i64) -> Option<u64> {
    (total >= 0).then_some(total.unsigned_abs())
}

fn state(
    native: COREWEBVIEW2_DOWNLOAD_STATE,
    reason: COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON,
) -> DownloadState {
    match native {
        COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS => DownloadState::InProgress,
        COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => DownloadState::Completed,
        _ => DownloadState::Interrupted(interruption(reason)),
    }
}

/// Groups WebView2's interrupt reasons as the flyout shows them.
fn interruption(reason: COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON) -> DownloadInterruption {
    match reason {
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED => DownloadInterruption::Cancelled,
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED => DownloadInterruption::Paused,
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_SHUTDOWN => DownloadInterruption::Shutdown,
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS => {
            DownloadInterruption::Blocked(BlockReason::Unsafe)
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED => {
            DownloadInterruption::Blocked(BlockReason::SecurityCheck)
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY => {
            DownloadInterruption::Blocked(BlockReason::Policy)
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_SERVER_DOWN
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_INVALID_REQUEST => {
            DownloadInterruption::Network
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FAILED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_NO_RANGE
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_BAD_CONTENT
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNAUTHORIZED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CERTIFICATE_PROBLEM
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FORBIDDEN
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNEXPECTED_RESPONSE
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CONTENT_LENGTH_MISMATCH
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CROSS_ORIGIN_REDIRECT => {
            DownloadInterruption::Server
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_FAILED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NAME_TOO_LONG
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_LARGE
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TRANSIENT_ERROR
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_SHORT
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_HASH_MISMATCH => DownloadInterruption::File,
        _ => DownloadInterruption::Other,
    }
}

/// The name of the file being saved, without its folder.
fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_string(
    read: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut raw = PWSTR::null();
    let result = read(&mut raw);
    let owned = CoTaskMemPWSTR::from(raw);
    result?;
    Ok(owned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_reasons_group_as_the_flyout_shows_them() {
        let group = |code| interruption(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON(code));
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED.0),
            DownloadInterruption::Cancelled
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED.0),
            DownloadInterruption::Paused
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_SHUTDOWN.0),
            DownloadInterruption::Shutdown
        );
        for (code, reason) in [
            (
                COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS,
                BlockReason::Unsafe,
            ),
            (
                COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY,
                BlockReason::Policy,
            ),
            (
                COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED,
                BlockReason::SecurityCheck,
            ),
        ] {
            assert_eq!(group(code.0), DownloadInterruption::Blocked(reason));
        }
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_HASH_MISMATCH.0),
            DownloadInterruption::File,
            "WebView2 may restart it by itself"
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT.0),
            DownloadInterruption::Network
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FORBIDDEN.0),
            DownloadInterruption::Server
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE.0),
            DownloadInterruption::File
        );
        assert_eq!(
            group(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_DOWNLOAD_PROCESS_CRASHED.0),
            DownloadInterruption::Other
        );
    }

    #[test]
    fn an_unknown_total_is_the_minus_one_sentinel_not_an_error() {
        assert_eq!(known_total(-1), None);
        assert_eq!(known_total(0), Some(0));
        assert_eq!(known_total(4 << 20), Some(4 << 20));
    }

    #[test]
    fn only_the_file_name_leaves_the_platform() {
        assert_eq!(
            file_name(r"C:\Users\Synthetic\Downloads\synthetic.zip"),
            "synthetic.zip"
        );
        assert_eq!(file_name(""), "");
    }
}
