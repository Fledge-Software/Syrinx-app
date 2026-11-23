//! A synthetic HTTP server on `127.0.0.1` for the `service` contract tests: it records each
//! request's head and answers by path, one connection at a time.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use syrinx_plugin_host::limits;

/// The body `/notes` answers with.
pub const NOTES: &str = "synthetic note titles";

pub struct SyntheticServer {
    socket: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SyntheticServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let socket = listener.local_addr().expect("the bound address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread = {
            let (requests, stopping) = (requests.clone(), stopping.clone());
            std::thread::spawn(move || serve(&listener, &requests, &stopping))
        };
        Self {
            socket,
            requests,
            stopping,
            thread: Some(thread),
        }
    }

    /// The address the owner would approve, such as `http://127.0.0.1:49152`.
    pub fn address(&self) -> String {
        format!("http://{}", self.socket)
    }

    /// The head of every request received, oldest first.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for SyntheticServer {
    /// Wake the accept loop with one last connection, then wait for it to stop.
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if TcpStream::connect(self.socket).is_ok()
            && let Some(thread) = self.thread.take()
        {
            thread.join().expect("the server thread stops cleanly");
        }
    }
}

fn serve(listener: &TcpListener, requests: &Mutex<Vec<String>>, stopping: &AtomicBool) {
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        answer(stream.expect("the server accepts a connection"), requests);
    }
}

/// Read one request's head, record it, and answer by its path.
fn answer(mut stream: TcpStream, requests: &Mutex<Vec<String>>) {
    // An accepted socket can inherit a nonblocking listener's mode on Windows (trapstoknow.md).
    stream.set_nonblocking(false).expect("a blocking socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout");
    let Some(head) = read_head(&mut stream) else {
        return;
    };
    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
    requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(head);
    let response = response_for(&path);
    // The client may stop reading an oversized body early: that is what the test checks, so
    // a failed write is expected there and the connection simply ends.
    if let Err(error) = stream.write_all(&response) {
        assert_eq!(path, "/big", "the client closed early on {path}: {error}");
    }
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
    }
    String::from_utf8(head).ok()
}

fn response_for(path: &str) -> Vec<u8> {
    match path {
        "/notes" => with_body("200 OK", &[], NOTES.as_bytes()),
        "/redirect-home" => with_body("302 Found", &[("location", "/notes")], b""),
        "/redirect-away" => with_body(
            "302 Found",
            &[("location", "http://other.example.test/notes")],
            b"",
        ),
        "/loop" => with_body("302 Found", &[("location", "/loop")], b""),
        "/big" => with_body(
            "200 OK",
            &[],
            &vec![b'x'; limits::MAX_SERVICE_RESPONSE_BYTES + 1],
        ),
        _ => with_body("404 Not Found", &[], b""),
    }
}

fn with_body(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
