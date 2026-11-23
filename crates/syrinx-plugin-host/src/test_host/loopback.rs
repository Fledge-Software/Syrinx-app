//! The test host's `service` transport: plain HTTP/1.1 to a loopback address only, straight over
//! a socket with no proxy, so a contract test never leaves the computer (the WinHTTP trap in
//! `docs/dev/trapstoknow.md`). It is not Syrinx's network client.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::bindings::syrinx::plugin::service::Header;
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
        let mut stream = TcpStream::connect_timeout(&socket, request.timeout).map_err(io_error)?;
        set_timeouts(&stream, deadline)?;
        stream
            .write_all(&request_bytes(request))
            .map_err(io_error)?;
        let raw = read_bounded(&mut stream, deadline, request.max_response_bytes)?;
        parse_response(&raw, request.max_response_bytes)
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

fn set_timeouts(stream: &TcpStream, deadline: Instant) -> Result<(), TransportError> {
    let left = remaining(deadline)?;
    stream.set_read_timeout(Some(left)).map_err(io_error)?;
    stream.set_write_timeout(Some(left)).map_err(io_error)
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
            .map_err(io_error)?;
        let read = stream.read(&mut chunk).map_err(io_error)?;
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
fn parse_response(raw: &[u8], max_body: usize) -> Result<IncomingResponse, TransportError> {
    let malformed = TransportError::Unreachable;
    let head_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(malformed)?;
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| malformed)?;
    let mut lines = head.split("\r\n");
    let status = parse_status(lines.next().unwrap_or_default())?;
    let headers = lines.map(parse_header).collect::<Result<Vec<_>, _>>()?;
    let body = body(&raw[head_end + 4..], &headers)?;
    if body.len() > max_body {
        return Err(TransportError::ResponseTooLarge);
    }
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
        return Err(TransportError::Unreachable);
    }
    code.parse().map_err(|_| TransportError::Unreachable)
}

fn parse_header(line: &str) -> Result<Header, TransportError> {
    let (name, value) = line.split_once(':').ok_or(TransportError::Unreachable)?;
    Ok(Header {
        name: name.trim().to_owned(),
        value: value.trim().to_owned(),
    })
}

/// A chunked body is not supported by this test transport and reads as malformed.
fn body(rest: &[u8], headers: &[Header]) -> Result<Vec<u8>, TransportError> {
    let value = |name: &str| {
        headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    };
    if value("transfer-encoding").is_some() {
        return Err(TransportError::Unreachable);
    }
    let Some(length) = value("content-length") else {
        return Ok(rest.to_vec());
    };
    let length: usize = length.parse().map_err(|_| TransportError::Unreachable)?;
    rest.get(..length)
        .map(<[u8]>::to_vec)
        .ok_or(TransportError::Unreachable)
}

fn io_error(error: std::io::Error) -> TransportError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => TransportError::TimedOut,
        _ => TransportError::Unreachable,
    }
}
