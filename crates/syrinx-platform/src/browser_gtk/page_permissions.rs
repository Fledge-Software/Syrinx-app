//! Denies pending consent on a new document without letting an obsolete reply affect it.

use super::super::{BrowserEvent, EventSink};
use std::{cell::RefCell, collections::HashMap};

/// Drain before calling the engine: denial may synchronously call back into page signals.
pub(super) fn cancel_pending<T>(
    prompts: &RefCell<HashMap<u64, T>>,
    id: u64,
    events: &EventSink,
    mut deny: impl FnMut(T),
) {
    let pending = std::mem::take(&mut *prompts.borrow_mut());
    for (request, prompt) in pending {
        deny(prompt);
        events.emit(BrowserEvent::PermissionFinished { id, request });
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::CallbackLife;
    use super::*;
    use futures::channel::mpsc::{TryRecvError, unbounded};

    #[test]
    fn navigation_denies_and_finishes_every_pending_request_once() {
        let prompts = RefCell::new(HashMap::from([(41, "first"), (42, "second")]));
        let (send, mut events) = unbounded();
        let sink = EventSink(send, None);
        let mut denied = Vec::new();
        cancel_pending(&prompts, 7, &sink, |prompt| {
            assert!(
                prompts.borrow().is_empty(),
                "engine callbacks see no stale prompts"
            );
            denied.push(prompt);
        });
        denied.sort();
        assert_eq!(denied, ["first", "second"]);
        let mut finished = Vec::new();
        for _ in 0..2 {
            let event = events
                .try_recv()
                .expect("navigation must finish pending consent");
            let BrowserEvent::PermissionFinished { id: 7, request } = event else {
                panic!("wrong cancellation event")
            };
            finished.push(request);
        }
        finished.sort();
        assert_eq!(finished, [41, 42]);
        assert!(
            prompts.borrow_mut().remove(&41).is_none(),
            "stale reply cannot resolve the old request"
        );
        cancel_pending(&prompts, 7, &sink, |_| panic!("request denied twice"));
        assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn queued_cancellation_completions_retire_with_the_page() {
        let prompts = RefCell::new(HashMap::from([(41, ())]));
        let (send, mut events) = unbounded();
        let life = CallbackLife::new();
        let sink = EventSink(send, None).for_page(life.clone());
        let mut denied = false;
        cancel_pending(&prompts, 7, &sink, |()| denied = true);
        life.retire();
        assert!(denied);
        assert!(
            events
                .try_recv()
                .expect("queued completion")
                .into_current()
                .is_none()
        );
    }
    #[test]
    fn closed_pages_deny_pending_requests_without_emitting_new_callbacks() {
        let prompts = RefCell::new(HashMap::from([(41, ())]));
        let (send, mut events) = unbounded();
        let life = CallbackLife::new();
        let sink = EventSink(send, None).for_page(life.clone());
        life.retire();
        let mut denied = false;
        cancel_pending(&prompts, 7, &sink, |()| denied = true);
        assert!(denied);
        assert!(prompts.borrow().is_empty());
        assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));
    }
}
