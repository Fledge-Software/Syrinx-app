//! The `service` import (#280): a plugin's HTTP requests to the one address the owner approved,
//! with the credential, the bounds and the redirect rule kept here rather than in the plugin.
//!
//! The host performs a request only when a granted plugin asks for one, never on its own. The
//! network itself is the `Transport` the application passes in; the test host's is loopback-only.

mod address;
mod door;
mod request;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub use address::{AddressError, Credential, ServiceAddress};
pub use door::ServiceDoor;

use crate::bindings::syrinx::plugin::service::{Header, Method, Response};
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::limits;

/// One request as the transport sends it. `headers` include the credential: a transport sends
/// them and never logs, stores or reports them.
pub struct OutgoingRequest<'a> {
    pub address: &'a ServiceAddress,
    pub method: Method,
    pub path: &'a str,
    pub headers: &'a [Header],
    pub body: &'a [u8],
    /// The transport gives up after this long, connection included.
    pub timeout: Duration,
    /// A body longer than this is `TransportError::ResponseTooLarge`, not read to the end.
    pub max_response_bytes: usize,
}

/// What came back, before the host's own checks.
#[derive(Debug, Clone, PartialEq)]
pub struct IncomingResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

/// Why a transport could not deliver a response. It carries no text, so no address, header or
/// credential can leak through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    TimedOut,
    ResponseTooLarge,
    /// No connection, a failed TLS handshake, or a malformed response.
    Unreachable,
}

/// The network under the `service` import. It must follow no redirect itself (the host decides
/// which to follow), use no cookie store, and send no user agent that imitates a browser.
pub trait Transport: Send + Sync {
    fn send(&self, request: &OutgoingRequest<'_>) -> Result<IncomingResponse, TransportError>;
}

/// The method's name on the wire.
pub fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
    }
}

/// Set by the application while the owner types in the palette: no request starts then, so
/// the palette keeps its search budget (D48).
#[derive(Clone, Default)]
pub struct OwnerTyping(Arc<AtomicBool>);

impl OwnerTyping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, typing: bool) {
        self.0.store(typing, Ordering::Relaxed);
    }

    pub fn is_typing(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Why the host refused or ended a `service` request. Each is named in the plugin's error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRefusal {
    /// The owner has approved no address for this plugin.
    NoAddress,
    OwnerTyping,
    /// The path names a scheme or host: a plugin gives only a path.
    OtherHost,
    InvalidPath(&'static str),
    InvalidHeader(&'static str),
    /// A header only the host sets, such as `host` or the credential's; holds the name.
    HostHeader(String),
    RequestTooLarge,
    /// The server redirected to another scheme, host or port.
    RedirectOffHost,
    TooManyRedirects,
    ResponseTooLarge,
    /// The deadline passed; a write may still have reached the server.
    TimedOut,
    Unreachable,
}

impl ServiceRefusal {
    /// The rule's stable name, which starts the plugin's error text.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NoAddress => "no-address",
            Self::OwnerTyping => "owner-typing",
            Self::OtherHost => "other-host",
            Self::InvalidPath(_) => "invalid-path",
            Self::InvalidHeader(_) => "invalid-header",
            Self::HostHeader(_) => "host-header",
            Self::RequestTooLarge => "request-too-large",
            Self::RedirectOffHost => "redirect-off-host",
            Self::TooManyRedirects => "too-many-redirects",
            Self::ResponseTooLarge => "response-too-large",
            Self::TimedOut => "timed-out",
            Self::Unreachable => "unreachable",
        }
    }

    fn explanation(&self) -> String {
        match self {
            Self::NoAddress => "the owner has approved no address for this plugin".into(),
            Self::OwnerTyping => "the owner is typing in the palette; ask again shortly".into(),
            Self::OtherHost => "a plugin gives only a path; the owner chose the address".into(),
            Self::InvalidPath(why) => format!("the path was refused: {why}"),
            Self::InvalidHeader(why) => format!("a header was refused: {why}"),
            Self::HostHeader(name) => format!("only the host sets the {name} header"),
            Self::RequestTooLarge => format!(
                "the body is larger than {} bytes",
                limits::MAX_SERVICE_REQUEST_BYTES
            ),
            Self::RedirectOffHost => "the server redirected away from the approved address".into(),
            Self::TooManyRedirects => format!(
                "the server redirected more than {} times",
                limits::MAX_SERVICE_REDIRECTS
            ),
            Self::ResponseTooLarge => format!(
                "the response is larger than {} bytes",
                limits::MAX_SERVICE_RESPONSE_BYTES
            ),
            Self::TimedOut => {
                "no answer in time; the request may still have reached the server".into()
            }
            Self::Unreachable => "the approved address could not be reached".into(),
        }
    }
}

impl fmt::Display for ServiceRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.name(), self.explanation())
    }
}

impl std::error::Error for ServiceRefusal {}

impl From<ServiceRefusal> for PluginError {
    /// Policy refusals are `denied`, a malformed request `invalid-input`, and a refusal that may
    /// pass on a later try `unavailable`.
    fn from(refusal: ServiceRefusal) -> Self {
        let text = refusal.to_string();
        match refusal {
            ServiceRefusal::NoAddress
            | ServiceRefusal::OtherHost
            | ServiceRefusal::HostHeader(_)
            | ServiceRefusal::RedirectOffHost
            | ServiceRefusal::TooManyRedirects
            | ServiceRefusal::ResponseTooLarge => Self::Denied(text),
            ServiceRefusal::InvalidPath(_)
            | ServiceRefusal::InvalidHeader(_)
            | ServiceRefusal::RequestTooLarge => Self::InvalidInput(text),
            ServiceRefusal::OwnerTyping
            | ServiceRefusal::TimedOut
            | ServiceRefusal::Unreachable => Self::Unavailable(text),
        }
    }
}

/// Answer a plugin's request through its door, or refuse it when the owner approved none.
/// `budget` is how long the current export lets a host service wait.
pub fn answer(
    door: Option<&ServiceDoor>,
    budget: Duration,
    method: Method,
    path: &str,
    headers: Vec<Header>,
    body: Vec<u8>,
) -> Result<Response, PluginError> {
    let door = door.ok_or(ServiceRefusal::NoAddress)?;
    Ok(door.request(method, path, headers, &body, budget)?)
}
