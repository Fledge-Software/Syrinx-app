//! One plugin's way to its approved address: checks the request, adds the credential, sends it
//! through the transport and follows redirects only within that address.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bindings::syrinx::plugin::service::{Header, Method, Response};
use crate::limits;

use super::request::{check_headers, check_path, is_host_header};
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

    pub fn address(&self) -> &ServiceAddress {
        &self.address
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
        if self.typing.is_typing() {
            return Err(ServiceRefusal::OwnerTyping);
        }
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
    fn follow(
        &self,
        mut hop: Hop,
        headers: &[Header],
        deadline: Instant,
    ) -> Result<IncomingResponse, ServiceRefusal> {
        for _ in 0..=limits::MAX_SERVICE_REDIRECTS {
            let response = self.send(&hop, headers, deadline)?;
            let Some(location) = redirect_location(&response) else {
                return Ok(response);
            };
            hop = self.redirected(hop, response.status, location)?;
        }
        Err(ServiceRefusal::TooManyRedirects)
    }

    fn send(
        &self,
        hop: &Hop,
        headers: &[Header],
        deadline: Instant,
    ) -> Result<IncomingResponse, ServiceRefusal> {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(ServiceRefusal::TimedOut);
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
        let response = self
            .transport
            .send(&outgoing)
            .map_err(|error| match error {
                TransportError::TimedOut => ServiceRefusal::TimedOut,
                TransportError::ResponseTooLarge => ServiceRefusal::ResponseTooLarge,
                TransportError::Unreachable => ServiceRefusal::Unreachable,
            })?;
        // A transport that read past the bound is held to it here too.
        if response.body.len() > limits::MAX_SERVICE_RESPONSE_BYTES {
            return Err(ServiceRefusal::ResponseTooLarge);
        }
        Ok(response)
    }

    /// The next hop after a redirect, if it stays on the approved address. A 303 becomes a
    /// bodiless GET, as HTTP says; every other redirect repeats the method and body.
    fn redirected(&self, hop: Hop, status: u16, location: &str) -> Result<Hop, ServiceRefusal> {
        let path = self
            .same_address_path(location)
            .ok_or(ServiceRefusal::RedirectOffHost)?;
        check_path(&path).map_err(|_| ServiceRefusal::RedirectOffHost)?;
        if status == 303 {
            return Ok(Hop {
                method: Method::Get,
                path,
                body: Vec::new(),
            });
        }
        Ok(Hop { path, ..hop })
    }

    /// The path a redirect target names on this address: an absolute path, or an absolute
    /// address with the same scheme, host and port. Anything else is off the address.
    fn same_address_path(&self, location: &str) -> Option<String> {
        if location.starts_with('/') && !location.starts_with("//") {
            return Some(location.to_owned());
        }
        let (scheme, rest) = location.split_once("://")?;
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

    /// The response without any header the host sets, so a server echoing the credential's
    /// header, or setting a cookie, does not hand either to the plugin.
    fn for_plugin(&self, response: IncomingResponse) -> Response {
        let headers = response
            .headers
            .into_iter()
            .filter(|header| {
                let name = header.name.to_ascii_lowercase();
                name != "set-cookie" && !is_host_header(&name, self.credential.as_ref())
            })
            .collect();
        Response {
            status: response.status,
            headers,
            body: response.body,
        }
    }
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
