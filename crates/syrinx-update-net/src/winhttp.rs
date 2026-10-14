//! GET requests through WinHTTP, Windows' own HTTP client: Windows validates the certificate,
//! and nothing here follows a redirect, sends a cookie or credentials, or adds a header.

use std::ffi::c_void;
use std::time::Duration;

use syrinx_update::install::TransferProblem;
use windows::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_CANNOT_CONNECT, ERROR_WINHTTP_CLIENT_AUTH_CERT_NEEDED,
    ERROR_WINHTTP_CONNECTION_ERROR, ERROR_WINHTTP_NAME_NOT_RESOLVED,
    ERROR_WINHTTP_OPERATION_CANCELLED, ERROR_WINHTTP_REDIRECT_FAILED,
    ERROR_WINHTTP_SECURE_CERT_CN_INVALID, ERROR_WINHTTP_SECURE_CERT_DATE_INVALID,
    ERROR_WINHTTP_SECURE_CERT_REV_FAILED, ERROR_WINHTTP_SECURE_CERT_REVOKED,
    ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE, ERROR_WINHTTP_SECURE_CHANNEL_ERROR,
    ERROR_WINHTTP_SECURE_FAILURE, ERROR_WINHTTP_SECURE_INVALID_CA,
    ERROR_WINHTTP_SECURE_INVALID_CERT, ERROR_WINHTTP_TIMEOUT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
    WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_REDIRECTS,
    WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_AUTOLOGON_POLICY, WINHTTP_OPTION_DISABLE_FEATURE,
    WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_FLAG_NUMBER64,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

use crate::endpoint::{Endpoint, Timeouts};

/// The only agent string Syrinx sends (the proposal's "What the host learns").
pub(crate) const USER_AGENT: &str = "Syrinx-Update";

/// Fetches one file at a path under the endpoint's prefix.
pub(crate) trait Transport: Send + Sync {
    fn get(&self, path: &str) -> Result<Box<dyn Body + '_>, TransferProblem>;
}

/// A response body with status 200.
pub(crate) trait Body {
    /// The declared `Content-Length`, when the host sent one.
    fn length(&self) -> Option<u64>;
    /// Read the next bytes; 0 means the body has ended.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransferProblem>;
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

// WinHTTP handles may be used from any thread; Syrinx uses each from one thread at a time.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    /// Take ownership of a handle a WinHTTP call returned, or report why it returned none.
    fn new(raw: *mut c_void) -> Result<Self, TransferProblem> {
        if raw.is_null() {
            return Err(last_problem());
        }
        Ok(Self(raw))
    }

    fn set_u32(&self, option: u32, value: u32) -> Result<(), TransferProblem> {
        let bytes = value.to_ne_bytes();
        unsafe { WinHttpSetOption(Some(self.0), option, Some(&bytes)) }.map_err(problem)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // Drop cannot return the failure, which only an already invalid handle would cause.
        if let Err(error) = unsafe { WinHttpCloseHandle(self.0) } {
            eprintln!("update download: closing a WinHTTP handle failed: {error}");
        }
    }
}

/// One WinHTTP session for one check or download, bound to the endpoint.
pub(crate) struct WinHttp {
    session: Handle,
    endpoint: Endpoint,
}

impl WinHttp {
    /// Open a session; this makes no network call by itself.
    pub(crate) fn open(endpoint: Endpoint) -> Result<Self, TransferProblem> {
        // The automatic proxy follows Windows' own proxy settings, as the browser does.
        let access = if endpoint.system_proxy {
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
        } else {
            WINHTTP_ACCESS_TYPE_NO_PROXY
        };
        let agent = HSTRING::from(USER_AGENT);
        let session =
            Handle::new(unsafe { WinHttpOpen(&agent, access, PCWSTR::null(), PCWSTR::null(), 0) })?;
        session.set_u32(
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
        )?;
        session.set_u32(
            WINHTTP_OPTION_REDIRECT_POLICY,
            WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
        )?;
        set_timeouts(&session, endpoint.timeouts)?;
        Ok(Self { session, endpoint })
    }
}

/// Apply the connect and receive bounds to every request in the session.
fn set_timeouts(session: &Handle, timeouts: Timeouts) -> Result<(), TransferProblem> {
    let connect = milliseconds(timeouts.connect);
    let receive = milliseconds(timeouts.receive);
    unsafe { WinHttpSetTimeouts(session.0, connect, connect, receive, receive) }
        .map_err(problem)?;
    session.set_u32(
        WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT,
        receive.unsigned_abs(),
    )
}

fn milliseconds(duration: Duration) -> i32 {
    i32::try_from(duration.as_millis()).unwrap_or(i32::MAX)
}

impl Transport for WinHttp {
    fn get(&self, path: &str) -> Result<Box<dyn Body + '_>, TransferProblem> {
        let endpoint = &self.endpoint;
        let host = HSTRING::from(endpoint.host);
        let connection =
            Handle::new(unsafe { WinHttpConnect(self.session.0, &host, endpoint.port, 0) })?;
        let request = open_request(&connection, endpoint, path)?;
        send(&request)?;
        let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE)?;
        if let Some(problem) = status_problem(status) {
            return Err(problem);
        }
        let length = query_length(&request)?;
        Ok(Box::new(Response {
            request,
            _connection: connection,
            length,
        }))
    }
}

/// A GET request with cookies, credentials and redirects switched off.
fn open_request(
    connection: &Handle,
    endpoint: &Endpoint,
    path: &str,
) -> Result<Handle, TransferProblem> {
    let object = HSTRING::from(format!("{}{path}", endpoint.prefix));
    let verb = HSTRING::from("GET");
    let flags = if endpoint.secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let request = Handle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            &verb,
            &object,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    })?;
    request.set_u32(
        WINHTTP_OPTION_DISABLE_FEATURE,
        WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION | WINHTTP_DISABLE_REDIRECTS,
    )?;
    request.set_u32(
        WINHTTP_OPTION_AUTOLOGON_POLICY,
        WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
    )?;
    set_timeouts(&request, endpoint.timeouts)?;
    Ok(request)
}

/// Send the request with no extra headers or body and wait for the response headers.
fn send(request: &Handle) -> Result<(), TransferProblem> {
    unsafe { WinHttpSendRequest(request.0, None, None, 0, 0, 0) }.map_err(problem)?;
    unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }.map_err(problem)
}

/// 200 is the only answer accepted; a redirect is never followed.
fn status_problem(status: u32) -> Option<TransferProblem> {
    match status {
        200 => None,
        300..=399 => Some(TransferProblem::Redirected),
        other => Some(TransferProblem::Status(
            u16::try_from(other).unwrap_or(u16::MAX),
        )),
    }
}

fn query_number(request: &Handle, header: u32) -> Result<u32, TransferProblem> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            header | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut value).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .map_err(problem)?;
    Ok(value)
}

/// The declared body length, or `None` when the host sent no `Content-Length`.
fn query_length(request: &Handle) -> Result<Option<u64>, TransferProblem> {
    let mut value = 0u64;
    let mut size = size_of::<u64>() as u32;
    let queried = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
            PCWSTR::null(),
            Some((&raw mut value).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
    };
    match queried {
        Ok(()) => Ok(Some(value)),
        Err(error) if win32_code(&error) == HEADER_NOT_FOUND => Ok(None),
        Err(error) => Err(problem(error)),
    }
}

/// `ERROR_WINHTTP_HEADER_NOT_FOUND`: the response has no such header.
const HEADER_NOT_FOUND: u32 = 12150;

/// An open response; the connection lives as long as the request reading from it.
struct Response {
    request: Handle,
    _connection: Handle,
    length: Option<u64>,
}

impl Body for Response {
    fn length(&self) -> Option<u64> {
        self.length
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransferProblem> {
        let wanted = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let mut read = 0u32;
        unsafe {
            WinHttpReadData(
                self.request.0,
                buffer.as_mut_ptr().cast(),
                wanted,
                &mut read,
            )
        }
        .map_err(problem)?;
        Ok(read as usize)
    }
}

/// The problem behind the calling thread's last WinHTTP failure.
fn last_problem() -> TransferProblem {
    problem(windows::core::Error::from_thread())
}

/// The Windows error code inside a WinHTTP failure.
fn win32_code(error: &windows::core::Error) -> u32 {
    (error.code().0 as u32) & 0xFFFF
}

/// Sort a WinHTTP failure into the fixed categories the owner and the diagnostics see.
fn problem(error: windows::core::Error) -> TransferProblem {
    match win32_code(&error) {
        ERROR_WINHTTP_TIMEOUT => TransferProblem::TimedOut,
        ERROR_WINHTTP_NAME_NOT_RESOLVED
        | ERROR_WINHTTP_CANNOT_CONNECT
        | ERROR_WINHTTP_CONNECTION_ERROR => TransferProblem::Unreachable,
        ERROR_WINHTTP_SECURE_FAILURE
        | ERROR_WINHTTP_SECURE_CHANNEL_ERROR
        | ERROR_WINHTTP_SECURE_INVALID_CA
        | ERROR_WINHTTP_SECURE_INVALID_CERT
        | ERROR_WINHTTP_SECURE_CERT_CN_INVALID
        | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
        | ERROR_WINHTTP_SECURE_CERT_REVOKED
        | ERROR_WINHTTP_SECURE_CERT_REV_FAILED
        | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE
        | ERROR_WINHTTP_CLIENT_AUTH_CERT_NEEDED => TransferProblem::Certificate,
        ERROR_WINHTTP_REDIRECT_FAILED => TransferProblem::Redirected,
        ERROR_WINHTTP_OPERATION_CANCELLED => TransferProblem::Cancelled,
        other => TransferProblem::Failed(other),
    }
}
