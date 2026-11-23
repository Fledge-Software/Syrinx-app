//! A synthetic HTTP server on `127.0.0.1` for the `service` contract tests: it records each
//! request's head and answers by path, each connection on its own thread.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use syrinx_plugin_host::limits;

/// The body `/notes` answers with.
pub const NOTES: &str = "synthetic note titles";

/// How long `/stall` holds its connection without answering: well past any request's deadline.
const STALL: Duration = Duration::from_secs(3);

/// What the server saw, shared with its threads.
#[derive(Default)]
struct Seen {
    requests: Mutex<Vec<String>>,
    /// Read and write failures, which a test checks rather than the server thread panicking.
    problems: Mutex<Vec<String>>,
}

impl Seen {
    fn note(list: &Mutex<Vec<String>>, entry: String) {
        list.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
    }

    fn list(list: &Mutex<Vec<String>>) -> Vec<String> {
        list.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

pub struct SyntheticServer {
    socket: SocketAddr,
    seen: Arc<Seen>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SyntheticServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let socket = listener.local_addr().expect("the bound address");
        let seen = Arc::new(Seen::default());
        let stopping = Arc::new(AtomicBool::new(false));
        let thread = {
            let (seen, stopping) = (seen.clone(), stopping.clone());
            std::thread::spawn(move || serve(&listener, &seen, &stopping))
        };
        Self {
            socket,
            seen,
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
        Seen::list(&self.seen.requests)
    }

    /// Every read or write that failed on the server's side, oldest first.
    pub fn problems(&self) -> Vec<String> {
        Seen::list(&self.seen.problems)
    }
}

impl Drop for SyntheticServer {
    /// Wake the accept loop with one last connection, then wait for it to stop.
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let woken = TcpStream::connect(self.socket);
        let Some(thread) = self.thread.take() else {
            return;
        };
        if std::thread::panicking() {
            return;
        }
        assert!(woken.is_ok(), "the server could not be woken to stop");
        thread
            .join()
            .expect("the server's accept loop stops cleanly");
    }
}

/// Accept until stopped; each connection is answered on its own thread, so `/stall` holds up
/// no other request (the WinHTTP stall advice in `docs/dev/trapstoknow.md`).
fn serve(listener: &TcpListener, seen: &Arc<Seen>, stopping: &Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                Seen::note(&seen.problems, format!("accepting a connection: {error}"));
                continue;
            }
        };
        let (seen, stopping) = (seen.clone(), stopping.clone());
        std::thread::spawn(move || answer(stream, &seen, &stopping));
    }
}

/// Read one request's head, record it, and answer by its path.
fn answer(mut stream: TcpStream, seen: &Seen, stopping: &AtomicBool) {
    // An accepted socket can inherit a nonblocking listener's mode on Windows (trapstoknow.md).
    let configured = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(5))));
    if let Err(error) = configured {
        return Seen::note(&seen.problems, format!("configuring a connection: {error}"));
    }
    let head = match read_head(&mut stream) {
        Ok(head) => head,
        Err(problem) => return Seen::note(&seen.problems, problem),
    };
    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
    let credential = header_value(&head, "x-api-key");
    Seen::note(&seen.requests, head);
    if path == "/stall" {
        return stall(stopping);
    }
    if let Err(error) = stream.write_all(&response_for(&path, &credential)) {
        Seen::note(&seen.problems, format!("answering {path}: {error}"));
    }
}

/// Hold the connection open without answering until `STALL` passes or the server stops.
fn stall(stopping: &AtomicBool) {
    let until = Instant::now() + STALL;
    while Instant::now() < until && !stopping.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The request line and headers, up to the blank line; the wake-up connection sends none.
fn read_head(stream: &mut TcpStream) -> Result<String, String> {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) if head.is_empty() => return Err("a connection sent nothing".into()),
            Ok(0) => return Err("a request ended inside its head".into()),
            Ok(_) => head.push(byte[0]),
            Err(error) => return Err(format!("reading a request: {error}")),
        }
    }
    String::from_utf8(head).map_err(|error| format!("a request head was not UTF-8: {error}"))
}

fn header_value(head: &str, name: &str) -> String {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(header, _)| header.trim().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default()
}

/// `/notes` echoes the credential it was sent and sets a cookie, so a test can show the plugin
/// sees neither.
fn response_for(path: &str, credential: &str) -> Vec<u8> {
    match path {
        "/notes" => with_body(
            "200 OK",
            &[
                ("content-type", "text/plain"),
                ("x-api-key", credential),
                ("set-cookie", "session=synthetic"),
            ],
            NOTES.as_bytes(),
        ),
        "/redirect-home" => with_body("302 Found", &[("location", "/notes")], b""),
        "/redirect-away" => with_body(
            "302 Found",
            &[("location", "http://other.example.test/notes")],
            b"",
        ),
        "/loop" => with_body("302 Found", &[("location", "/loop")], b""),
        "/big" => oversized(),
        _ => with_body("404 Not Found", &[], b""),
    }
}

/// A response that declares one byte more than the host takes, and sends only a short prefix.
fn oversized() -> Vec<u8> {
    let length = limits::MAX_SERVICE_RESPONSE_BYTES + 1;
    format!("HTTP/1.1 200 OK\r\ncontent-length: {length}\r\nconnection: close\r\n\r\nprefix")
        .into_bytes()
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
