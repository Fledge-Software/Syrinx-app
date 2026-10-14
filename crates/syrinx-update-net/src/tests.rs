//! The WinHTTP adapter against loopback servers on 127.0.0.1, and the download bounds against an
//! in-process fake. Nothing here resolves a name or reaches another machine.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use syrinx_update::install::{InstallError, PackageSource, SignedFile, TransferProblem};
use syrinx_update::{FileEntry, PackagePath, Version};

use crate::endpoint::{Endpoint, Timeouts, UPDATE_BASE_URL};
use crate::remote::RemotePackage;
use crate::winhttp::{Body, Transport, USER_AGENT, WinHttp};

const VERSION: Version = Version::new(1, 4, 0);

/// Short bounds, so a stalled loopback server fails a test in about a second.
const QUICK: Timeouts = Timeouts {
    connect: Duration::from_secs(2),
    receive: Duration::from_secs(1),
};

/// How a loopback server answers one connection.
enum Reply {
    /// Send these bytes, then close.
    Raw(Vec<u8>),
    /// Send these bytes, then wait before closing.
    Hold(Vec<u8>, Duration),
    /// Send these headers, then body bytes until the client goes away.
    Endless(Vec<u8>),
}

/// A plain-HTTP server on 127.0.0.1 that answers each connection in turn and keeps every request.
struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
        let port = listener.local_addr().expect("loopback address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&requests);
        // Each connection gets its own thread, so one held reply never delays the next.
        let thread = thread::spawn(move || {
            let mut answering = Vec::new();
            for reply in replies {
                let Some(stream) = accept(&listener, Duration::from_secs(10)) else {
                    break;
                };
                let kept = Arc::clone(&kept);
                answering.push(thread::spawn(move || answer(stream, reply, &kept)));
            }
            for connection in answering {
                connection.join().expect("a loopback connection thread");
            }
        });
        Self {
            port,
            requests,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }

    fn package(&self) -> RemotePackage {
        package(Endpoint::loopback(self.port, false, QUICK))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
            && !thread::panicking()
        {
            panic!("the loopback server thread panicked");
        }
    }
}

fn package(endpoint: Endpoint) -> RemotePackage {
    let transport = WinHttp::open(endpoint).expect("WinHTTP session");
    RemotePackage::with_transport(Box::new(transport), endpoint.host())
}

/// The next connection within `patience`, in blocking mode, or `None`.
fn accept(listener: &TcpListener, patience: Duration) -> Option<TcpStream> {
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let deadline = Instant::now() + patience;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // An accepted socket inherits the listener's mode (docs/trapstoknow.md).
                stream.set_nonblocking(false).expect("blocking stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout");
                return Some(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("loopback accept failed: {error}"),
        }
    }
}

fn answer(mut stream: TcpStream, reply: Reply, requests: &Mutex<Vec<String>>) {
    let head = read_head(&mut stream);
    requests.lock().expect("requests").push(head);
    match reply {
        Reply::Raw(bytes) => {
            send(&mut stream, &bytes);
        }
        Reply::Hold(bytes, wait) => {
            send(&mut stream, &bytes);
            thread::sleep(wait);
        }
        Reply::Endless(headers) => {
            let chunk = [0x5a; 4096];
            let mut open = send(&mut stream, &headers);
            while open {
                open = send(&mut stream, &chunk);
            }
        }
    }
}

/// Write ytes; alse once the client has closed, which several tests make it do.
fn send(stream: &mut TcpStream, bytes: &[u8]) -> bool {
    match stream.write_all(bytes) {
        Ok(()) => true,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            ) =>
        {
            false
        }
        Err(error) => panic!("loopback write failed: {error}"),
    }
}

/// The request line and headers, up to the blank line.
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

fn ok(body: &[u8]) -> Reply {
    let mut bytes = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    Reply::Raw(bytes)
}

fn status(line: &str, extra: &str) -> Reply {
    Reply::Raw(
        format!("HTTP/1.1 {line}\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes(),
    )
}

fn entry(path: &str, size: u64) -> FileEntry {
    FileEntry::new(
        PackagePath::parse(path).expect("synthetic path"),
        size,
        [0; 32],
    )
}

fn transfer<T: std::fmt::Debug>(result: Result<T, InstallError>) -> TransferProblem {
    match result {
        Err(InstallError::Transfer(problem)) => problem,
        other => panic!("expected a transfer problem, got {other:?}"),
    }
}

/// Read a payload through the staging path's own bound: at most one byte past its size.
fn download(package: &RemotePackage, entry: &FileEntry) -> Result<Vec<u8>, InstallError> {
    let reader = package.open_file(VERSION, entry)?;
    let mut bytes = Vec::new();
    match reader.take(entry.size() + 1).read_to_end(&mut bytes) {
        Ok(_) => Ok(bytes),
        Err(error) => Err(InstallError::Transfer(
            TransferProblem::carried_by(&error).expect("a download's own problem"),
        )),
    }
}

#[test]
fn the_production_endpoint_is_the_compiled_in_https_base() {
    let endpoint = Endpoint::production();
    assert!(endpoint.secure);
    assert!(
        endpoint.system_proxy,
        "Windows' proxy settings apply, as in the browser"
    );
    assert_eq!(endpoint.port, 443);
    assert_eq!(
        format!("https://{}{}", endpoint.host, endpoint.prefix),
        UPDATE_BASE_URL
    );
    assert_eq!(endpoint.timeouts.connect, Duration::from_secs(30));
    assert_eq!(endpoint.timeouts.receive, Duration::from_secs(60));
    // Opening the session makes no request; nothing here asks the host for anything.
    let package = RemotePackage::production().expect("WinHTTP session");
    assert_eq!(package.host(), "syrinxapp.dev");
}

#[test]
fn no_shipped_build_can_redirect_the_base_url() {
    // The only other endpoint is a test build's loopback; no environment or feature reaches it.
    let sources = [
        include_str!("endpoint.rs"),
        include_str!("remote.rs"),
        include_str!("winhttp.rs"),
        include_str!("lib.rs"),
    ];
    for source in sources {
        assert!(!source.contains("env::var"), "no environment override");
        assert!(!source.contains("cfg(feature"), "no feature switch");
        assert!(!source.contains("std::env"), "no environment access");
    }
    let endpoint = include_str!("endpoint.rs").replace("\r\n", "\n");
    assert!(endpoint.contains("    #[cfg(test)]\n    pub(crate) fn loopback("));
    assert_eq!(endpoint.matches("fn loopback(").count(), 1);
    assert_eq!(endpoint.matches("-> Self").count(), 2, "two constructors");
    let manifest = include_str!("../Cargo.toml");
    let tables = manifest.lines().map(str::trim);
    assert!(
        tables.into_iter().all(|line| line != "[features]"),
        "the crate has no features"
    );
}

#[test]
fn a_request_carries_only_the_fixed_path_and_agent() {
    let server = Server::start(vec![ok(b"statement")]);
    let bytes = server
        .package()
        .read_signed(SignedFile::Statement, 4096)
        .expect("fetch");
    assert_eq!(bytes, b"statement");
    let requests = server.requests();
    let mut lines = requests[0].lines();
    assert_eq!(lines.next(), Some("GET /updates/v1/latest HTTP/1.1"));
    let mut headers: Vec<_> = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once(": ")
                .expect("header")
                .0
                .to_ascii_lowercase()
        })
        .collect();
    headers.sort();
    // No cookie, credentials, referrer, encoding or identifier: Host and the agent only.
    assert_eq!(headers, ["connection", "host", "user-agent"]);
    assert!(requests[0].contains(&format!("User-Agent: {USER_AGENT}\r\n")));
}

#[test]
fn manifests_and_payloads_live_under_their_checked_version() {
    let server = Server::start(vec![ok(b"manifest"), ok(b"exe!")]);
    let package = server.package();
    package
        .read_signed(SignedFile::Manifest(VERSION), 4096)
        .expect("manifest");
    let bytes = download(&package, &entry("resources/syrinx.exe", 4)).expect("payload");
    assert_eq!(bytes, b"exe!");
    let requests = server.requests();
    assert!(requests[0].starts_with("GET /updates/v1/1.4.0/manifest HTTP/1.1\r\n"));
    assert!(
        requests[1].starts_with("GET /updates/v1/1.4.0/files/resources/syrinx.exe HTTP/1.1\r\n")
    );
}

#[test]
fn the_signed_files_are_fetched_once_and_kept() {
    let server = Server::start(vec![ok(b"statement")]);
    let package = server.package();
    for _ in 0..3 {
        let bytes = package
            .read_signed(SignedFile::Statement, 4096)
            .expect("fetch");
        assert_eq!(bytes, b"statement");
    }
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_redirect_is_refused_and_never_followed() {
    let target = TcpListener::bind("127.0.0.1:0").expect("redirect target");
    let target_port = target.local_addr().expect("target address").port();
    let location = format!("Location: http://127.0.0.1:{target_port}/updates/v1/latest\r\n");
    for line in [
        "301 Moved Permanently",
        "302 Found",
        "307 Temporary Redirect",
    ] {
        let server = Server::start(vec![status(line, &location)]);
        let problem = transfer(server.package().read_signed(SignedFile::Statement, 4096));
        assert_eq!(problem, TransferProblem::Redirected, "{line}");
    }
    assert!(
        accept(&target, Duration::from_millis(200)).is_none(),
        "nothing reached the target"
    );
}

#[test]
fn any_status_but_200_is_refused() {
    for (line, code) in [("404 Not Found", 404), ("503 Service Unavailable", 503)] {
        let server = Server::start(vec![status(line, "")]);
        let problem = transfer(server.package().read_signed(SignedFile::Statement, 4096));
        assert_eq!(problem, TransferProblem::Status(code));
    }
}

#[test]
fn bodies_stop_at_their_bounds() {
    // A declared length over the limit is refused before the body is read.
    let server = Server::start(vec![ok(&[7; 100])]);
    let problem = transfer(
        server
            .package()
            .read_signed(SignedFile::StatementSignature, 76),
    );
    assert_eq!(problem, TransferProblem::TooLong);

    // A body with no length stops one read past the limit.
    let endless = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    let server = Server::start(vec![Reply::Endless(endless.clone())]);
    let problem = transfer(server.package().read_signed(SignedFile::Statement, 4096));
    assert_eq!(problem, TransferProblem::TooLong);

    // A payload must be exactly its listed size, declared or not.
    let server = Server::start(vec![
        ok(b"0123456789"),
        ok(b"0123"),
        Reply::Endless(endless),
    ]);
    let package = server.package();
    let listed = entry("syrinx.exe", 8);
    assert_eq!(
        transfer(download(&package, &listed)),
        TransferProblem::TooLong
    );
    assert_eq!(
        transfer(download(&package, &listed)),
        TransferProblem::Truncated
    );
    assert_eq!(
        transfer(download(&package, &listed)),
        TransferProblem::TooLong
    );
}

#[test]
fn a_body_that_ends_early_is_refused() {
    let short =
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nonly ten.".to_vec();
    let server = Server::start(vec![Reply::Raw(short.clone()), Reply::Raw(short)]);
    let package = server.package();
    let small = transfer(package.read_signed(SignedFile::Statement, 4096));
    let payload = transfer(download(&package, &entry("syrinx.exe", 100)));
    for problem in [small, payload] {
        assert!(
            matches!(
                problem,
                TransferProblem::Truncated | TransferProblem::Unreachable
            ),
            "{problem:?}"
        );
    }
}

#[test]
fn a_stalled_host_times_out() {
    let wait = Duration::from_secs(8);
    let headers =
        b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nConnection: close\r\n\r\nfirst".to_vec();
    let server = Server::start(vec![
        Reply::Hold(Vec::new(), wait),
        Reply::Hold(headers, wait),
    ]);
    let package = server.package();
    // WinHTTP reported each timeout about 3 s after its bound here, still well before wait.
    let started = Instant::now();
    let silent = transfer(package.read_signed(SignedFile::Statement, 4096));
    assert_eq!(silent, TransferProblem::TimedOut);
    assert!(
        started.elapsed() < wait,
        "no response: {:?}",
        started.elapsed()
    );
    let started = Instant::now();
    let stalled = transfer(download(&package, &entry("syrinx.exe", 50)));
    assert_eq!(stalled, TransferProblem::TimedOut);
    assert!(
        started.elapsed() < wait,
        "a stalled body: {:?}",
        started.elapsed()
    );
}

#[test]
fn a_secure_request_never_speaks_plain_http() {
    // The production endpoint is HTTPS; a host that only speaks HTTP never sees a request.
    let server = Server::start(vec![ok(b"plain")]);
    let secure = package(Endpoint::loopback(server.port, true, QUICK));
    let problem = transfer(secure.read_signed(SignedFile::Statement, 4096));
    assert!(
        matches!(
            problem,
            TransferProblem::Certificate | TransferProblem::Unreachable | TransferProblem::TimedOut
        ),
        "{problem:?}"
    );
    drop(secure);
    for request in server.requests() {
        assert!(!request.contains("GET "), "no plain request: {request:?}");
    }
}

#[test]
fn an_unreachable_host_is_reported_as_such() {
    let unused = TcpListener::bind("127.0.0.1:0").expect("free port");
    let port = unused.local_addr().expect("address").port();
    drop(unused);
    let package = package(Endpoint::loopback(port, false, QUICK));
    let problem = transfer(package.read_signed(SignedFile::Statement, 4096));
    assert_eq!(problem, TransferProblem::Unreachable);
}

/// A transport whose body hands out one chunk at a time and counts every request.
struct Chunks {
    requests: Arc<AtomicUsize>,
}

struct ChunkBody {
    left: usize,
}

impl Body for ChunkBody {
    fn length(&self) -> Option<u64> {
        None
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransferProblem> {
        let read = buffer.len().min(self.left).min(3);
        buffer[..read].fill(1);
        self.left -= read;
        Ok(read)
    }
}

impl Transport for Chunks {
    fn get(&self, _path: &str) -> Result<Box<dyn Body + '_>, TransferProblem> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(ChunkBody { left: 30 }))
    }
}

#[test]
fn cancelling_stops_at_the_next_read_and_prevents_new_requests() {
    let requests = Arc::new(AtomicUsize::new(0));
    let chunks = Chunks {
        requests: Arc::clone(&requests),
    };
    let package = RemotePackage::with_transport(Box::new(chunks), "synthetic.invalid");
    let listed = entry("syrinx.exe", 30);
    let mut reader = package.open_file(VERSION, &listed).expect("open");
    let mut first = [0u8; 3];
    reader.read_exact(&mut first).expect("first chunk");
    package.canceller().cancel();
    let error = reader.read(&mut first).expect_err("cancelled");
    assert_eq!(
        TransferProblem::carried_by(&error),
        Some(TransferProblem::Cancelled)
    );
    drop(reader);
    assert_eq!(
        transfer(package.read_signed(SignedFile::Statement, 4096)),
        TransferProblem::Cancelled
    );
    assert_eq!(
        transfer(package.open_file(VERSION, &listed).map(drop)),
        TransferProblem::Cancelled
    );
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "no request after cancelling"
    );
}
