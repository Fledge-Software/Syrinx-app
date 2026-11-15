//! Serves synthetic attachments; only explicitly held transfers may end in client cancellation.

use super::super::fixture::LIMIT;
use std::{
    io::{self, Write},
    net::TcpStream,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Instant,
};

pub const SAME: &str = "synthetic-same.zip";
pub const POPUP: &str = "synthetic-popup.zip";
pub const CLOSING: &str = "synthetic-closing.zip";
pub const HELD: &str = "synthetic-held.zip";
pub const CANCEL: &str = "synthetic-cancel.zip";
const EMPTY_ZIP: &[u8] = &[
    0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[derive(Default)]
pub struct Transfers {
    held: AtomicBool,
    cancel: AtomicBool,
}

impl Transfers {
    pub fn release(&self, name: &str) -> Result<(), String> {
        self.gate(name)
            .ok_or("Only a held synthetic transfer can be released")?
            .store(true, Ordering::Release);
        Ok(())
    }

    fn gate(&self, name: &str) -> Option<&AtomicBool> {
        match name {
            HELD => Some(&self.held),
            CANCEL => Some(&self.cancel),
            _ => None,
        }
    }
}

pub fn body(name: &str) -> Vec<u8> {
    let mut body = if matches!(name, HELD | CANCEL) {
        vec![0; 128 * 1024]
    } else {
        Vec::new()
    };
    body.extend_from_slice(EMPTY_ZIP);
    body
}

pub fn name(route: &str) -> Option<&'static str> {
    let requested = route.strip_prefix("/download/")?;
    [SAME, POPUP, CLOSING, HELD, CANCEL]
        .into_iter()
        .find(|name| *name == requested)
}

pub fn send(
    stream: &mut TcpStream,
    name: &str,
    transfers: &Transfers,
    stop: &AtomicBool,
) -> Result<(), String> {
    let body = body(name);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let Some(gate) = transfers.gate(name) else {
        stream
            .write_all(header.as_bytes())
            .and_then(|()| stream.write_all(&body))
            .map_err(|error| error.to_string())?;
        return Ok(());
    };
    // The cancellation case alone may disconnect after its first half; ordinary files must succeed.
    let first = stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(&body[..64 * 1024]));
    if !write_result(first, name)? {
        return Ok(());
    }
    let deadline = Instant::now() + LIMIT;
    while !gate.load(Ordering::Acquire) {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Held synthetic transfer was not released before its deadline".into());
        }
        thread::sleep(std::time::Duration::from_millis(5));
    }
    write_result(stream.write_all(&body[64 * 1024..]), name)?;
    Ok(())
}

fn write_result(result: io::Result<()>, name: &str) -> Result<bool, String> {
    match result {
        Ok(()) => Ok(true),
        Err(error)
            if name == CANCEL
                && matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cancel_route_accepts_expected_disconnects() {
        for name in [SAME, POPUP, CLOSING, HELD, CANCEL] {
            let disconnected = write_result(Err(io::ErrorKind::BrokenPipe.into()), name);
            assert_eq!(disconnected.is_ok(), name == CANCEL);
            assert!(write_result(Err(io::ErrorKind::PermissionDenied.into()), name).is_err());
            assert!(write_result(Err(io::ErrorKind::TimedOut.into()), name).is_err());
        }
    }
}
