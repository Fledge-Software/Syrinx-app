//! Opens links that later Atlas launches forward, once the session can accept them (P1-34).

use atlas_platform::instance::{DroppedLinks, InstanceError, LinkFeed, LinkRequest, StopSignal};
use futures::StreamExt;
use gpui::{Context, Window};

use super::BrowserView;
use crate::session::SessionCommand;

/// Forwarded addresses waiting for the session, the switch that stops the listener and
/// the record of links that arrived too late to open.
#[derive(Default)]
pub(super) struct IncomingLinks {
    pending: Vec<String>,
    stop: Option<StopSignal>,
    dropped: DroppedLinks,
}

impl IncomingLinks {
    /// Queues a forwarded request; returns whether the window should come forward.
    fn receive(&mut self, request: LinkRequest, closing: bool) -> bool {
        if closing {
            // Atlas tells the owner after it exits, so the link can be opened again.
            if matches!(request, LinkRequest::Open(_)) {
                self.dropped.record();
            }
            return false;
        }
        if let LinkRequest::Open(address) = request {
            self.pending.push(address.into_string());
        }
        true
    }

    /// The addresses to open now, in arrival order; none until the session has started.
    fn take_ready(&mut self, session_ready: bool, closing: bool) -> Vec<String> {
        if closing || !session_ready {
            return Vec::new();
        }
        std::mem::take(&mut self.pending)
    }

    /// Stops accepting forwarded links, once; later launches then wait for Atlas to exit.
    fn stop_listening(&mut self) -> Result<(), InstanceError> {
        // Links still waiting for a session that never started will not open either.
        if !self.pending.is_empty() {
            self.pending.clear();
            self.dropped.record();
        }
        match self.stop.take() {
            Some(stop) => stop.raise(),
            None => Ok(()),
        }
    }
}

impl BrowserView {
    /// Receives the requests later launches forward, starting with this launch's own link.
    pub(super) fn start_link_feed(
        &mut self,
        feed: LinkFeed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let LinkFeed {
            mut requests,
            stop,
            dropped,
        } = feed;
        self.incoming.stop = Some(stop);
        self.incoming.dropped = dropped;
        cx.spawn_in(window, async move |view, cx| {
            while let Some(request) = requests.next().await {
                let delivered = view.update_in(cx, |view, window, cx| {
                    view.link_request(request, window, cx);
                });
                if delivered.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn link_request(&mut self, request: LinkRequest, window: &mut Window, cx: &mut Context<Self>) {
        if !self.incoming.receive(request, self.closing) {
            return;
        }
        // The forwarding launch granted foreground rights, so this can come to the front.
        window.activate_window();
        self.open_forwarded_links();
        cx.notify();
    }

    /// Opens each waiting link in its own new tab once the session is ready.
    pub(super) fn open_forwarded_links(&mut self) {
        let ready = self.session_revision.is_some();
        for address in self.incoming.take_ready(ready, self.closing) {
            self.command(SessionCommand::NewTab { address });
        }
    }

    /// Called as closing starts, so a link sent now opens in the next Atlas instead.
    pub(super) fn stop_link_feed(&mut self) {
        if let Err(error) = self.incoming.stop_listening() {
            self.message = error.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(text: &str) -> LinkRequest {
        LinkRequest::open(text).expect("synthetic web address")
    }

    #[test]
    fn links_wait_for_the_session_and_open_in_arrival_order() {
        let mut links = IncomingLinks::default();
        assert!(links.receive(link("https://first.example.test/"), false));
        assert!(links.receive(LinkRequest::Activate, false));
        assert!(links.receive(link("https://second.example.test/"), false));
        assert!(
            links.take_ready(false, false).is_empty(),
            "session still starting"
        );
        assert_eq!(
            links.take_ready(true, false),
            [
                "https://first.example.test/",
                "https://second.example.test/"
            ]
        );
        assert!(
            links.take_ready(true, false).is_empty(),
            "each link opens once"
        );
    }

    #[test]
    fn a_closing_window_neither_opens_nor_comes_forward() {
        let mut links = IncomingLinks::default();
        assert!(!links.receive(LinkRequest::Activate, true));
        assert!(!links.dropped.any(), "a late activation loses nothing");
        assert!(!links.receive(link("https://late.example.test/"), true));
        assert!(links.dropped.any(), "a late link is reported");
        assert!(links.take_ready(true, true).is_empty());
        assert!(
            links.stop_listening().is_ok(),
            "no listener to stop is fine"
        );
    }

    #[test]
    fn links_waiting_for_a_session_that_never_started_are_reported() {
        let mut links = IncomingLinks::default();
        assert!(links.receive(link("https://waiting.example.test/"), false));
        assert!(!links.dropped.any());
        assert!(links.stop_listening().is_ok());
        assert!(links.dropped.any());
        assert!(links.take_ready(true, false).is_empty());
    }
}
