//! Serves only authored loopback fixture pages and retains errors from its worker threads.

use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

mod request;

pub struct Server {
    base: String,
    stop: Arc<AtomicBool>,
    cookie: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
    transfers: Arc<super::downloads::files::Transfers>,
}

impl Server {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let base = format!(
            "http://{}",
            listener.local_addr().map_err(|error| error.to_string())?
        );
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let transfers = Arc::new(super::downloads::files::Transfers::default());
        let serving = transfers.clone();
        let cookie = Arc::new(AtomicBool::new(false));
        let evidence = cookie.clone();
        let worker = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let transfers = serving.clone();
                        let stop = stopping.clone();
                        let cookie = evidence.clone();
                        workers.push(thread::spawn(move || {
                            answer(stream, &transfers, &stop, &cookie)
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            for worker in workers {
                worker.join().map_err(|_| "Server worker panicked")??;
            }
            Ok(())
        });
        Ok(Self {
            base,
            stop,
            worker: Some(worker),
            transfers,
            cookie,
        })
    }

    pub fn url(&self, route: &str) -> String {
        format!("{}{route}", self.base)
    }

    pub fn cookie_received(&self) -> bool {
        self.cookie.load(Ordering::Acquire)
    }

    pub fn release_download(&self, name: &str) -> Result<(), String> {
        self.transfers.release(name)
    }

    pub fn finish(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .ok_or("Server already stopped")?
            .join()
            .map_err(|_| "Server panicked")?
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.worker.is_some()
            && let Err(error) = self.finish()
        {
            eprintln!("Synthetic server cleanup: {error}");
        }
    }
}

fn answer(
    mut stream: TcpStream,
    transfers: &super::downloads::files::Transfers,
    stop: &AtomicBool,
    cookie: &AtomicBool,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let request = match request::read(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.to_string()),
    };
    let route = request.route.as_str();
    if route == "/cookie/process/read" && request.cookie {
        cookie.store(true, Ordering::Release);
    }
    if let Some(name) = super::downloads::files::name(route) {
        return super::downloads::files::send(&mut stream, name, transfers, stop);
    }
    let (status, extra, body) = page(route);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn page(route: &str) -> (&'static str, &'static str, String) {
    if let Some(body) = super::history::response(route) {
        let headers = if route.starts_with("/c/") {
            "Cache-Control: public, max-age=3600\r\n"
        } else {
            "Cache-Control: no-store\r\n"
        };
        return ("200 OK", headers, body);
    }
    if let Some(body) = super::controls::response(route)
        .or_else(|| super::popups::response(route))
        .or_else(|| super::permissions::response(route))
        .or_else(|| super::media::response(route))
        .or_else(|| super::downloads::response(route))
    {
        return ("200 OK", "Cache-Control: no-store\r\n", body.into());
    }
    if route == "/cookie/process/set" {
        return (
            "200 OK",
            "Cache-Control: no-store\r\nSet-Cookie: syrinx_process=synthetic; Path=/; Max-Age=3600; HttpOnly; SameSite=Lax\r\n",
            "<!doctype html><title>synthetic-cookie-set</title><link rel=icon href=data:,>".into(),
        );
    }
    let script = match route {
        "/close" => "addEventListener('load',()=>setTimeout(()=>window.close(),150))",
        "/cookie/set" => {
            "document.cookie='syrinx_fixture=synthetic; Max-Age=3600; SameSite=Lax';document.title='cookie-written'"
        }
        "/cookie/read" => {
            "document.title=document.cookie.includes('syrinx_fixture=synthetic')?'cookie-present':'cookie-missing'"
        }
        "/p/45?replace" => "history.replaceState({},'', '/p/45r');document.title='Synthetic 45r'",
        "/p/push" => "history.pushState({},'', '/p/pushed');document.title='Synthetic pushed'",
        "/p/script-redirect" => "location.replace('/p/redirected')",
        _ => "",
    };
    if route == "/p/302" {
        return ("302 Found", "Location: /p/redirected\r\n", String::new());
    }
    let title = route.trim_start_matches('/');
    (
        "200 OK",
        "",
        format!(
            "<!doctype html><meta charset=utf-8><title>Synthetic {title}</title><link rel=icon href=data:,><p>Synthetic Linux fixture {title}</p><script>{script}</script>"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn cookie_evidence_requires_the_received_request_header() {
        let mut server = Server::start().unwrap();
        assert!(!server.cookie_received());
        let address = server.base.strip_prefix("http://").unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(b"GET /cookie/process/read HTTP/1.1\r\nHost: localhost\r\ncOoKiE: unrelated=synthetic; syrinx_process=synthetic\r\n\r\n").unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(
            server.cookie_received(),
            "The server must inspect the received Cookie header"
        );
        server.finish().unwrap();
    }
}
