//! The test host's `service` transport: plain HTTP/1.1 to a loopback address only, straight over
//! a socket with no proxy, so a contract test never leaves the computer (the WinHTTP trap in
//! `docs/dev/trapstoknow.md`). It is not Syrinx's network client.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::bindings::syrinx::plugin::service::{Header, Method};
use crate::service::{
    IncomingResponse, OutgoingRequest, ServiceAddress, Transport, TransportError, method_name,
};

/// A response's status line and headers may take at most this much.
const MAX_HEAD_BYTES: usize = 64 * 1024;

/// Sends each request on a fresh connection to `127.0.0.1`, `[::1]` or `localhost`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoopbackTransport;

impl Transport for LoopbackTransport {
    fn send(&self, request: &OutgoingRequest<'_>) -> Result<IncomingResponse, TransportError> {
        let deadline = Instant::now() + request.timeout;
        let socket = loopback_socket(request.address)?;
        // Until the connection stands, nothing has been sent.
        let mut stream = TcpStream::connect_timeout(&socket, request.timeout)
            .map_err(|_| TransportError::Unreachable)?;
        set_timeouts(&stream, deadline)?;
        stream
            .write_all(&request_bytes(request))
            .map_err(after_sending)?;
        let raw = read_bounded(&mut stream, deadline, request.max_response_bytes)?;
        parse_response(&raw, request.method, request.max_response_bytes)
    }
}

/// The socket for a loopback address; anything else is unreachable from the test host.
fn loopback_socket(address: &ServiceAddress) -> Result<SocketAddr, TransportError> {
    if address.is_secure() || !address.is_loopback() {
        return Err(TransportError::Unreachable);
    }
    let host = address.host().trim_start_matches('[').trim_end_matches(']');
    let ip = match host {
        "localhost" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        literal => literal.parse().map_err(|_| TransportError::Unreachable)?,
    };
    Ok(SocketAddr::new(ip, address.port()))
}

/// Before anything is written: running out of time here means nothing was sent.
fn set_timeouts(stream: &TcpStream, deadline: Instant) -> Result<(), TransportError> {
    let left = remaining(deadline).map_err(|_| TransportError::Unreachable)?;
    let unset = |_| TransportError::Unreachable;
    stream.set_read_timeout(Some(left)).map_err(unset)?;
    stream.set_write_timeout(Some(left)).map_err(unset)
}

fn remaining(deadline: Instant) -> Result<Duration, TransportError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(TransportError::TimedOut);
    }
    Ok(left)
}

/// The request line, the host's own headers, the given headers and the body.
fn request_bytes(request: &OutgoingRequest<'_>) -> Vec<u8> {
    let address = request.address;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nhost: {}:{}\r\nconnection: close\r\ncontent-length: {}\r\n",
        method_name(request.method),
        request.path,
        address.host(),
        address.port(),
        request.body.len()
    );
    for header in request.headers {
        head.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(request.body);
    bytes
}

/// Read until the server closes, refusing to hold more than a head and the largest body.
fn read_bounded(
    stream: &mut TcpStream,
    deadline: Instant,
    max_body: usize,
) -> Result<Vec<u8>, TransportError> {
    let limit = MAX_HEAD_BYTES + max_body;
    let mut raw = Vec::new();
    let mut chunk = [0; 16 * 1024];
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(after_sending)?;
        let read = stream.read(&mut chunk).map_err(after_sending)?;
        if read == 0 {
            return Ok(raw);
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > limit {
            return Err(TransportError::ResponseTooLarge);
        }
    }
}

/// Status, headers and a body bounded by its `content-length` or the end of the stream.
fn parse_response(
    raw: &[u8],
    method: Method,
    max_body: usize,
) -> Result<IncomingResponse, TransportError> {
    let malformed = TransportError::BadResponse;
    let head_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(malformed)?;
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| malformed)?;
    let mut lines = head.split("\r\n");
    let status = parse_status(lines.next().unwrap_or_default())?;
    let headers = lines.map(parse_header).collect::<Result<Vec<_>, _>>()?;
    let body = if has_no_body(method, status) {
        Vec::new()
    } else {
        body(&raw[head_end + 4..], &headers, max_body)?
    };
    Ok(IncomingResponse {
        status,
        headers,
        body,
    })
}

fn parse_status(line: &str) -> Result<u16, TransportError> {
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let code = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(TransportError::BadResponse);
    }
    code.parse().map_err(|_| TransportError::BadResponse)
}

/// HTTP gives no body to a HEAD response, nor to 1xx, 204 and 304, whatever their headers say.
fn has_no_body(method: Method, status: u16) -> bool {
    method == Method::Head || (100..200).contains(&status) || status == 204 || status == 304
}

fn parse_header(line: &str) -> Result<Header, TransportError> {
    let (name, value) = line.split_once(':').ok_or(TransportError::BadResponse)?;
    Ok(Header {
        name: name.trim().to_owned(),
        value: value.trim().to_owned(),
    })
}

/// The body, refused as too large as soon as its declared length says so. A chunked body is
/// not supported by this test transport and reads as malformed.
fn body(rest: &[u8], headers: &[Header], max_body: usize) -> Result<Vec<u8>, TransportError> {
    let value = |name: &str| {
        headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    };
    if value("transfer-encoding").is_some() {
        return Err(TransportError::BadResponse);
    }
    let length = match value("content-length") {
        Some(length) => length.parse().map_err(|_| TransportError::BadResponse)?,
        None => rest.len(),
    };
    if length > max_body {
        return Err(TransportError::ResponseTooLarge);
    }
    rest.get(..length)
        .map(<[u8]>::to_vec)
        .ok_or(TransportError::BadResponse)
}

/// A failure once the request may have left: a timeout, or an answer lost on the way.
fn after_sending(error: std::io::Error) -> TransportError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => TransportError::TimedOut,
        _ => TransportError::BadResponse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str, method: Method) -> Result<IncomingResponse, TransportError> {
        parse_response(raw.as_bytes(), method, 16)
    }

    #[test]
    fn a_head_204_or_304_answer_has_no_body_whatever_its_length_says() {
        let head = "HTTP/1.1 200 OK\r\ncontent-length: 12\r\n\r\n";
        assert_eq!(
            parse(head, Method::Head).map(|response| response.body),
            Ok(Vec::new())
        );
        for status in ["204 No Content", "304 Not Modified"] {
            let raw = format!("HTTP/1.1 {status}\r\ncontent-length: 5\r\n\r\n");
            assert!(parse(&raw, Method::Get).is_ok_and(|response| response.body.is_empty()));
        }
    }

    #[test]
    fn a_declared_length_past_the_bound_is_refused_before_the_body() {
        let raw = "HTTP/1.1 200 OK\r\ncontent-length: 17\r\n\r\nshort";
        assert_eq!(
            parse(raw, Method::Get),
            Err(TransportError::ResponseTooLarge)
        );
    }

    #[test]
    fn a_cut_off_or_malformed_answer_is_a_bad_response() {
        for raw in [
            "HTTP/1.1 200 OK\r\ncontent-length: 9\r\n\r\nshort",
            "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n",
            "SPDY 200\r\n\r\n",
            "HTTP/1.1 200 OK\r\n",
        ] {
            assert_eq!(
                parse(raw, Method::Get),
                Err(TransportError::BadResponse),
                "{raw}"
            );
        }
    }
}
