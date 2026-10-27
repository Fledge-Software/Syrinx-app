//! Receives a snapshot, drops an oversized one unparsed, and parses on one bounded worker.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::{self, SyncSender, TrySendError},
    time::{Duration, Instant, SystemTime},
};

use syrinx_core::EligibleDocument;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
};

use super::{
    super::{BrowserEvent, CapturedText, DiscardReason, EventSink, NativeSink, PageTextOutcome},
    snapshot,
};

/// What the DevTools call returned.
pub(super) enum Arrival {
    Failed,
    /// Larger than the parse limit; dropped without parsing.
    TooLarge,
    Json(String),
}

/// The completion handler. webview2-com converts the UTF-16 response before Syrinx sees it;
/// measuring it first needs a direct `windows-core` dependency, which is not approved.
pub(super) fn handler(
    finish: impl FnOnce(Arrival) + 'static,
) -> ICoreWebView2CallDevToolsProtocolMethodCompletedHandler {
    CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
        finish(arrival(result, json));
        Ok(())
    }))
}

fn arrival(result: windows::core::Result<()>, json: String) -> Arrival {
    if result.is_err() {
        return Arrival::Failed;
    }
    if json.len() > snapshot::RESPONSE_LIMIT_BYTES {
        return Arrival::TooLarge;
    }
    Arrival::Json(json)
}

/// One snapshot waiting to be parsed.
pub(super) struct Job {
    pub(super) id: u64,
    pub(super) json: String,
    pub(super) document: EligibleDocument,
    pub(super) call_time: Duration,
    /// When the visit behind this capture happened, which dates its text.
    pub(super) visited_at: Option<SystemTime>,
    pub(super) events: EventSink,
    pub(super) native: NativeSink,
}

/// Why a snapshot could not be queued.
pub(super) enum Refused {
    /// One snapshot is parsing and one waiting; this one is skipped to bound memory.
    Busy,
    /// The worker could not be started.
    Unavailable,
}

/// The page thread's handle to the one parse worker, started on first use. At most one
/// snapshot parses and one waits, so at most two responses are held at once.
#[derive(Clone, Default)]
pub(super) struct Reader(Rc<RefCell<Option<SyncSender<Job>>>>);

impl Reader {
    pub(super) fn read(&self, job: Job) -> Result<(), Refused> {
        let mut sender = self.0.borrow_mut();
        if sender.is_none() {
            *sender = Some(spawn_worker().map_err(|_| Refused::Unavailable)?);
        }
        let Some(queue) = sender.as_ref() else {
            return Err(Refused::Unavailable);
        };
        match queue.try_send(job) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Refused::Busy),
            // The worker ended; the next capture starts a new one.
            Err(TrySendError::Disconnected(_)) => {
                *sender = None;
                Err(Refused::Unavailable)
            }
        }
    }
}

/// The worker ends when the page thread drops its last handle.
fn spawn_worker() -> std::io::Result<SyncSender<Job>> {
    let (send, receive) = mpsc::sync_channel::<Job>(1);
    std::thread::Builder::new()
        .name("syrinx-page-text".into())
        .spawn(move || {
            for job in receive {
                parse(job);
            }
        })?;
    Ok(send)
}

/// Parses one snapshot and posts the outcome back to the page thread.
fn parse(job: Job) {
    let started = Instant::now();
    let response_bytes = job.json.len();
    let outcome = match snapshot::read(&job.json, &job.document) {
        Ok(read) => PageTextOutcome::Captured(CapturedText {
            address: job.document.address().to_owned(),
            text: read.text,
            documents: read.documents,
            truncated: read.truncated,
            response_bytes,
            call_time: job.call_time,
            parse_time: started.elapsed(),
            visited_at: job.visited_at,
        }),
        Err(reason) => PageTextOutcome::Discarded(reason),
    };
    drop(job.json);
    let event = BrowserEvent::PageText {
        id: job.id,
        outcome,
    };
    if let Err(error) = job.native.post(event, &job.events) {
        // The page thread has stopped, so nothing remains to receive this result.
        eprintln!("{error}");
    }
}

/// The outcome reported when a snapshot cannot be queued.
pub(super) fn refused_outcome(refused: Refused) -> PageTextOutcome {
    match refused {
        Refused::Busy => PageTextOutcome::Discarded(DiscardReason::Busy),
        Refused::Unavailable => PageTextOutcome::Failed,
    }
}
