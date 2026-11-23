//! One plugin's way to its approved address: checks the request, adds the credential, sends it
//! through the transport and follows redirects only within that address.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bindings::syrinx::plugin::service::{Header, Method, Response};
use crate::limits;

use super::request::{check_headers, check_path, header_bytes, is_withheld_response_header};
use super::{
    Credential, IncomingResponse, OutgoingRequest, OwnerTyping, ServiceAddress, ServiceRefusal,
    Transport, TransportError,
};

/// The address and credential the owner approved for one plugin, and the transport to reach it.
#[derive(Clone)]
pub struct ServiceDoor {
    address: ServiceAddress,
    credential: Option<Credential>,
    transport: Arc<dyn Transport>,
    typing: OwnerTyping,
}

/// Where the next hop goes and what it carries.
struct Hop {
    method: Method,
    path: String,
    body: Vec<u8>,
}

impl ServiceDoor {
    /// `typing` must be the flag the palette sets while the owner types (D48).
    pub fn new(
        address: ServiceAddress,
        credential: Option<Credential>,
        transport: Arc<dyn Transport>,
        typing: OwnerTyping,
    ) -> Self {
        Self {
            address,
            credential,
            transport,
            typing,
        }
    }

    /// Check the request, then send it and any redirects within the address, ending by
    /// `budget` or `SERVICE_REQUEST_DEADLINE`, whichever is sooner.
    pub fn request(
        &self,
        method: Method,
        path: &str,
        headers: Vec<Header>,
        body: &[u8],
        budget: Duration,
    ) -> Result<Response, ServiceRefusal> {
        check_path(path)?;
        check_headers(&headers, self.credential.as_ref())?;
        if body.len() > limits::MAX_SERVICE_REQUEST_BYTES {
            return Err(ServiceRefusal::RequestTooLarge);
        }
        let deadline = Instant::now() + budget.min(limits::SERVICE_REQUEST_DEADLINE);
        let headers = self.with_credential(headers);
        let first = Hop {
            method,
            path: path.to_owned(),
            body: body.to_vec(),
        };
        let response = self.follow(first, &headers, deadline)?;
        Ok(self.for_plugin(response))
    }

    /// Send hops until one is not a redirect, at most `MAX_SERVICE_REDIRECTS` redirects later.
    /// Once a writing hop has been answered, the write is done: any later failure is `uncertain`.
    fn follow(
        &self,
        mut hop: Hop,
        headers: &[Header],
        deadline: Instant,
    ) -> Result<IncomingResponse, ServiceRefusal> {
        let mut wrote = false;
        for _ in 0..=limits::MAX_SERVICE_REDIRECTS {
            let response = self
                .send(&hop, headers, deadline)
                .map_err(|refusal| after_write(refusal, wrote))?;
            wrote |= !is_reading(hop.method);
            let Some(location) = redirect_location(&response) else {
                return Ok(response);
            };
            hop = self
                .redirected(hop, response.status, location)
                .map_err(|refusal| after_write(refusal, wrote))?;
        }
        Err(after_write(ServiceRefusal::TooManyRedirects, wrote))
    }

    /// Send one hop, unless the owner is typing or no time is left; every hop is a request.
    fn send(
        &self,
        hop: &Hop,
        headers: &[Header],
        deadline: Instant,
    ) -> Result<IncomingResponse, ServiceRefusal> {
        if self.typing.is_typing() {
            return Err(ServiceRefusal::OwnerTyping);
        }
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(ServiceRefusal::NoTime);
        }
        let outgoing = OutgoingRequest {
            address: &self.address,
            method: hop.method,
            path: &hop.path,
            headers,
            body: &hop.body,
            timeout,
            max_response_bytes: limits::MAX_SERVICE_RESPONSE_BYTES,
        };
        let answered = self
            .transport
            .send(&outgoing)
            .map_err(|error| refusal_for(error, hop.method))?;
        // An oversized answer to a write still means the write was carried out.
        check_response_size(&answered)
            .map_err(|refusal| after_write(refusal, !is_reading(hop.method)))?;
        Ok(answered)
    }

    /// The next hop after a redirect, if it stays on the approved address. It becomes a
    /// bodiless GET where Fetch says so (`becomes_get`); otherwise it repeats method and body.
    fn redirected(&self, hop: Hop, status: u16, location: &str) -> Result<Hop, ServiceRefusal> {
        let path = self
            .same_address_path(location)
            .ok_or(ServiceRefusal::RedirectOffHost)?;
        check_path(&path).map_err(|_| ServiceRefusal::InvalidRedirect)?;
        if becomes_get(status, hop.method) {
            return Ok(Hop {
                method: Method::Get,
                path,
                body: Vec::new(),
            });
        }
        Ok(Hop { path, ..hop })
    }

    /// The path a redirect target names on this address: an absolute path, or an absolute
    /// address with the same scheme, host and port. `None` when it names another address. A
    /// relative target stays on the address, and `check_path` refuses it as an invalid redirect.
    fn same_address_path(&self, location: &str) -> Option<String> {
        if location.starts_with("//") {
            return None;
        }
        let Some((scheme, rest)) = location.split_once("://") else {
            return Some(location.to_owned());
        };
        let (authority, path) = match rest.find(['/', '?']) {
            Some(index) => rest.split_at(index),
            None => (rest, "/"),
        };
        if !self.address.is_same_origin(scheme, authority) {
            return None;
        }
        if path.starts_with('?') {
            return Some(format!("/{path}"));
        }
        Some(path.to_owned())
    }

    /// The plugin's headers and then the credential, which only the host adds.
    fn with_credential(&self, mut headers: Vec<Header>) -> Vec<Header> {
        if let Some(credential) = &self.credential {
            headers.push(Header {
                name: credential.header().to_owned(),
                value: credential.value().to_owned(),
            });
        }
        headers
    }

    /// The response without the headers kept from the plugin: cookies, and any echo of the
    /// credential's header.
    fn for_plugin(&self, response: IncomingResponse) -> Response {
        let credential = self.credential.as_ref();
        let headers = response
            .headers
            .into_iter()
            .filter(|header| !is_withheld_response_header(&header.name, credential))
            .collect();
        Response {
            status: response.status,
            headers,
            body: response.body,
        }
    }
}

/// The Fetch standard's method rule: a POST answered 301 or 302, or any write answered 303,
/// was likely carried out (post, then redirect to the result), so repeating it would write twice.
fn becomes_get(status: u16, method: Method) -> bool {
    match status {
        301 | 302 => method == Method::Post,
        303 => !is_reading(method),
        _ => false,
    }
}

/// GET and HEAD read; every other method may change something on the server.
fn is_reading(method: Method) -> bool {
    matches!(method, Method::Get | Method::Head)
}

/// A refusal as the plugin must hear it once a write may have reached the server: `uncertain`,
/// naming the rule that failed, so it is never taken as safe to resend.
fn after_write(refusal: ServiceRefusal, wrote: bool) -> ServiceRefusal {
    if !wrote || matches!(refusal, ServiceRefusal::Uncertain(_)) {
        return refusal;
    }
    ServiceRefusal::Uncertain(refusal.name())
}

/// A transport failure as the plugin is told it. Only a read is safe to ask again once the
/// request may have reached the server; for a write every such failure is `uncertain`.
fn refusal_for(error: TransportError, method: Method) -> ServiceRefusal {
    let refusal = match error {
        TransportError::Unreachable => return ServiceRefusal::Unreachable,
        TransportError::ResponseTooLarge => ServiceRefusal::ResponseTooLarge,
        TransportError::TimedOut => ServiceRefusal::TimedOut,
        TransportError::BadResponse => ServiceRefusal::BadResponse,
    };
    after_write(refusal, !is_reading(method))
}

/// A transport that read past a bound is held to it here too, headers included.
fn check_response_size(response: &IncomingResponse) -> Result<(), ServiceRefusal> {
    let too_large = response.body.len() > limits::MAX_SERVICE_RESPONSE_BYTES
        || response.headers.len() > limits::MAX_SERVICE_RESPONSE_HEADERS
        || header_bytes(&response.headers) > limits::MAX_SERVICE_RESPONSE_HEADER_BYTES;
    if too_large {
        return Err(ServiceRefusal::ResponseTooLarge);
    }
    Ok(())
}

/// A redirect's target, for the statuses that redirect and carry one.
fn redirect_location(response: &IncomingResponse) -> Option<&str> {
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("location"))
        .map(|header| header.value.as_str())
}

#[cfg(test)]
mod tests;
