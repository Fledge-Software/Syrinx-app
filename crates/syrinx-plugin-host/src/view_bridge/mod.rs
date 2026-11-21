//! The gatekeeper between a plugin view's interface and its plugin (#138): every message a view
//! sends is checked here, in research note 10's order, before any WebAssembly call.

mod csp;
mod envelope;
mod files;
mod origin;

use std::collections::BTreeMap;

pub use csp::{Nonce, RandomFill, content_security_policy};
pub use envelope::EnvelopeError;
pub use files::{FileResolver, InvalidUiFolder, ListedFile, Missing, Served, Sha256};
pub use origin::ViewOrigin;

use crate::bindings::syrinx::plugin::types::PluginError;
use crate::limits::{MAX_PENDING_VIEW_REQUESTS, MAX_VIEW_MESSAGE_BYTES};
use crate::manifest::TabType;
use envelope::Envelope;

/// A message as the platform hands it over from `WebMessageReceived`.
#[derive(Debug, Clone, Copy)]
pub struct Inbound<'a> {
    /// WebView2's `Source()`: the sending document's full URI.
    pub source: &'a str,
    /// The generation of the view the platform received it on.
    pub generation: u64,
    pub json: &'a str,
}

/// Why a message never reached the plugin, one variant per check, in the order they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    WrongOrigin,
    StaleGeneration,
    TooLarge,
    Malformed(EnvelopeError),
    UndeclaredMethod(String),
    DuplicateId(u64),
    QueueFull,
}

/// A message that was not relayed: refused by a check, or refused by the plugin's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayError<E> {
    Refused(Refusal),
    Delivery(E),
}

/// What a message that passed the checks led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relayed {
    /// The request was delivered and is pending under the host's request id.
    Queued { host_request: u64 },
    /// The view gave up on a pending request; the caller cancels `host_request`
    /// (`Session::cancel_request`).
    Cancelled { host_request: u64 },
    /// A cancel for an id that is not pending, perhaps answered already.
    CancelIgnored,
}

/// One open view's bridge: its origin, its declared methods, its generation and the
/// requests its interface is waiting on.
#[derive(Debug)]
pub struct ViewBridge {
    origin: ViewOrigin,
    methods: Vec<String>,
    generation: u64,
    /// Pending requests: the interface's id to the host's request id.
    pending: BTreeMap<u64, u64>,
}

impl ViewBridge {
    pub fn new(origin: ViewOrigin, tab_type: &TabType, generation: u64) -> Self {
        Self {
            origin,
            methods: tab_type.ui_methods.clone(),
            generation,
            pending: BTreeMap::new(),
        }
    }

    pub fn origin(&self) -> &ViewOrigin {
        &self.origin
    }

    /// The generation a message or reply must carry to be current.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Check a message and pass a request to `deliver`, the only route to the plugin (the
    /// application passes a closure over `Session::queue_view_message` for this view's id);
    /// `deliver` returns the host's request id. Nothing is delivered for a refused message or a cancel.
    pub fn relay<E>(
        &mut self,
        inbound: Inbound<'_>,
        deliver: impl FnOnce(&str, &str) -> Result<u64, E>,
    ) -> Result<Relayed, RelayError<E>> {
        let envelope = self
            .checked_envelope(inbound)
            .map_err(RelayError::Refused)?;
        match envelope {
            Envelope::Cancel { id } => Ok(self.cancel(id)),
            Envelope::Request { id, method, body } => {
                self.check_request(id, &method)
                    .map_err(RelayError::Refused)?;
                let host_request = deliver(&method, &body).map_err(RelayError::Delivery)?;
                self.pending.insert(id, host_request);
                Ok(Relayed::Queued { host_request })
            }
        }
    }

    /// The response envelope for the plugin's answer to `host_request`, or `None` (drop it)
    /// when the view was recreated since or the request is no longer pending.
    pub fn reply(
        &mut self,
        generation: u64,
        host_request: u64,
        result: Result<String, PluginError>,
    ) -> Option<String> {
        // Defence in depth: `renew` already empties `pending`, and host ids are never reused.
        if generation != self.generation {
            return None;
        }
        let id = self.interface_id(host_request)?;
        self.pending.remove(&id);
        Some(envelope::response(id, result))
    }

    /// The view was reloaded or recreated: a new generation, and the host requests to cancel.
    pub fn renew(&mut self) -> Vec<u64> {
        self.generation += 1;
        self.drain_pending()
    }

    /// The view closed: the host requests to cancel.
    pub fn close(mut self) -> Vec<u64> {
        self.drain_pending()
    }

    /// Checks 1–4: origin, generation, size, then the envelope itself.
    fn checked_envelope(&self, inbound: Inbound<'_>) -> Result<Envelope, Refusal> {
        if !self.origin.matches_source(inbound.source) {
            return Err(Refusal::WrongOrigin);
        }
        if inbound.generation != self.generation {
            return Err(Refusal::StaleGeneration);
        }
        if inbound.json.len() > MAX_VIEW_MESSAGE_BYTES {
            return Err(Refusal::TooLarge);
        }
        envelope::parse(inbound.json).map_err(Refusal::Malformed)
    }

    /// Checks 5–7 for a request: a declared method, a fresh id and room in the queue.
    fn check_request(&self, id: u64, method: &str) -> Result<(), Refusal> {
        if !self.methods.iter().any(|declared| declared == method) {
            return Err(Refusal::UndeclaredMethod(method.into()));
        }
        if self.pending.contains_key(&id) {
            return Err(Refusal::DuplicateId(id));
        }
        if self.pending.len() >= MAX_PENDING_VIEW_REQUESTS {
            return Err(Refusal::QueueFull);
        }
        Ok(())
    }

    fn cancel(&mut self, id: u64) -> Relayed {
        match self.pending.remove(&id) {
            Some(host_request) => Relayed::Cancelled { host_request },
            None => Relayed::CancelIgnored,
        }
    }

    fn interface_id(&self, host_request: u64) -> Option<u64> {
        self.pending
            .iter()
            .find(|(_, pending)| **pending == host_request)
            .map(|(id, _)| *id)
    }

    fn drain_pending(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.pending).into_values().collect()
    }
}

#[cfg(test)]
mod tests;
