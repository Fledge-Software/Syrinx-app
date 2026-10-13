//! Serves the synthetic P1-25 pages from distinct loopback addresses, one per site role.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::pages;

/// The synthetic sites. Each listens on its own loopback address, so each is its own host.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    Plain,
    Bank,
    Pay,
    Owner,
    Probe,
    Plain2,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Plain,
        Role::Bank,
        Role::Pay,
        Role::Owner,
        Role::Probe,
        Role::Plain2,
    ];

    fn address(self) -> Ipv4Addr {
        match self {
            Role::Plain => Ipv4Addr::new(127, 0, 0, 1),
            Role::Bank => Ipv4Addr::new(127, 0, 0, 2),
            Role::Pay => Ipv4Addr::new(127, 0, 0, 3),
            Role::Owner => Ipv4Addr::new(127, 0, 0, 4),
            Role::Probe => Ipv4Addr::new(127, 0, 0, 5),
            Role::Plain2 => Ipv4Addr::new(127, 0, 0, 6),
        }
    }

    /// The host name the eligibility lists use for this role.
    pub fn host(self) -> String {
        self.address().to_string()
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Plain => "plain",
            Role::Bank => "bank",
            Role::Pay => "pay",
            Role::Owner => "owner",
            Role::Probe => "probe",
            Role::Plain2 => "plain2",
        }
    }

    fn named(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.name() == name)
    }
}

/// One synthetic response.
pub struct Reply {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Self {
        Self::typed("text/html; charset=utf-8", body.into().into_bytes())
    }

    pub fn typed(content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            headers: vec![("Content-Type".into(), content_type.into())],
            body,
        }
    }

    pub fn status(mut self, status: &'static str) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// The base address of every role, so pages and cases can link across sites.
#[derive(Clone)]
pub struct Sites(Arc<HashMap<Role, String>>);

impl Sites {
    pub fn base(&self, role: Role) -> &str {
        &self.0[&role]
    }

    pub fn url(&self, role: Role, path: &str) -> String {
        format!("{}{path}", self.base(role))
    }

    fn role_for(&self, name: &str) -> Option<&str> {
        Role::named(name).map(|role| self.base(role))
    }

    /// Resolves a `role/path` pair used in redirect routes.
    pub fn resolve(&self, role: &str, path: &str) -> Option<String> {
        self.role_for(role).map(|base| format!("{base}/{path}"))
    }
}

pub struct Server {
    pub sites: Sites,
    hits: Arc<Mutex<HashMap<String, u32>>>,
    stopping: Arc<AtomicBool>,
    worker: JoinHandle<Result<(), String>>,
}

impl Server {
    pub fn start() -> Result<Self, String> {
        let mut listeners = Vec::new();
        let mut bases = HashMap::new();
        for role in Role::ALL {
            let listener = TcpListener::bind((role.address(), 0))
                .map_err(|error| io_error("bind synthetic loopback site", error))?;
            let port = listener
                .local_addr()
                .map_err(|error| io_error("read synthetic port", error))?
                .port();
            listener
                .set_nonblocking(true)
                .map_err(|error| io_error("poll synthetic accepts", error))?;
            bases.insert(role, format!("http://{}:{port}", role.address()));
            listeners.push((role, listener));
        }
        let sites = Sites(Arc::new(bases));
        let hits = Arc::new(Mutex::new(HashMap::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = {
            let (sites, hits, stopping) = (sites.clone(), hits.clone(), stopping.clone());
            thread::Builder::new()
                .name("atlas-page-text-sites".into())
                .spawn(move || serve(listeners, sites, hits, stopping))
                .map_err(|error| io_error("start synthetic sites", error))?
        };
        Ok(Self {
            sites,
            hits,
            stopping,
            worker,
        })
    }

    /// How many requests reached `role` for `path` (query included).
    pub fn hits(&self, role: Role, path: &str) -> Result<u32, String> {
        let hits = self
            .hits
            .lock()
            .map_err(|_| "Synthetic hit counter poisoned.".to_owned())?;
        Ok(hits
            .get(&format!("{}{path}", role.name()))
            .copied()
            .unwrap_or(0))
    }

    pub fn stop(self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        self.worker
            .join()
            .map_err(|_| "Synthetic site worker panicked.".to_owned())?
    }
}

fn serve(
    listeners: Vec<(Role, TcpListener)>,
    sites: Sites,
    hits: Arc<Mutex<HashMap<String, u32>>>,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    while !stopping.load(Ordering::Acquire) {
        let mut accepted = false;
        for (role, listener) in &listeners {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted = true;
                    respond(*role, stream, &sites, &hits)?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(io_error("accept synthetic request", error)),
            }
        }
        if !accepted {
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

fn respond(
    role: Role,
    mut stream: TcpStream,
    sites: &Sites,
    hits: &Mutex<HashMap<String, u32>>,
) -> Result<(), String> {
    configure_stream(&stream)?;
    let Some(path) = read_request(&mut stream)? else {
        return Ok(());
    };
    *hits
        .lock()
        .map_err(|_| "Synthetic hit counter poisoned.".to_owned())?
        .entry(format!("{}{path}", role.name()))
        .or_default() += 1;
    let reply = pages::route(role, &path, sites);
    write_reply(&mut stream, reply)
}

/// Reads one request, including any form body, and returns its path, or `None` for an idle
/// connection.
fn read_request(stream: &mut TcpStream) -> Result<Option<String>, String> {
    let mut request = [0u8; 8_192];
    let mut length = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !request[..length]
        .windows(4)
        .any(|bytes| bytes == b"\r\n\r\n")
    {
        if Instant::now() >= deadline || length == request.len() {
            return Err("Synthetic request headers exceeded their bound.".into());
        }
        match stream.read(&mut request[length..]) {
            Ok(0) => return Ok(None),
            Ok(read) => length += read,
            Err(error)
                if length == 0
                    && matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(io_error("read synthetic request", error)),
        }
    }
    let head = String::from_utf8_lossy(&request[..length]).into_owned();
    let path = head
        .lines()
        .next()
        .and_then(|first| first.split(' ').nth(1))
        .ok_or("Synthetic request line is malformed.")?;
    drain_body(stream, &head)?;
    Ok(Some(path.to_owned()))
}

/// Reads and discards a POST body, so closing the connection does not reset it before the
/// browser reads the reply.
fn drain_body(stream: &mut TcpStream, head: &str) -> Result<(), String> {
    let Some((headers, received)) = head.split_once("\r\n\r\n") else {
        return Ok(());
    };
    let declared = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(|_| "Synthetic request body length is malformed.".to_owned())?
        .unwrap_or(0);
    if declared > 65_536 {
        return Err("Synthetic request body exceeded its bound.".into());
    }
    let mut remaining = declared.saturating_sub(received.len());
    let mut buffer = [0u8; 4_096];
    while remaining > 0 {
        let read = stream
            .read(&mut buffer[..remaining.min(4_096)])
            .map_err(|error| io_error("read synthetic request body", error))?;
        if read == 0 {
            return Err("Synthetic request body ended early.".into());
        }
        remaining -= read;
    }
    Ok(())
}

fn write_reply(stream: &mut TcpStream, reply: Reply) -> Result<(), String> {
    let mut head = format!("HTTP/1.1 {}\r\n", reply.status);
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        reply.body.len()
    ));
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&reply.body))
        .map_err(|error| io_error("send synthetic response", error))
}

fn configure_stream(stream: &TcpStream) -> Result<(), String> {
    // Windows accepts inherit nonblocking mode; the timeouts below require blocking I/O.
    stream
        .set_nonblocking(false)
        .map_err(|error| io_error("configure synthetic blocking reads", error))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| io_error("bound synthetic request read", error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| io_error("bound synthetic response write", error))
}

pub fn io_error(operation: &str, error: io::Error) -> String {
    format!(
        "{operation} failed ({:?}, code {:?}).",
        error.kind(),
        error.raw_os_error()
    )
}
