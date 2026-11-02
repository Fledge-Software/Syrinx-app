//! Serves only authored loopback fixture pages and retains errors from its worker threads.

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct Server {
    base: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
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
        let worker = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => workers.push(thread::spawn(move || answer(stream))),
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
        })
    }

    pub fn url(&self, route: &str) -> String {
        format!("{}{route}", self.base)
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

fn answer(mut stream: TcpStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut line = String::new();
    match BufReader::new(&mut stream).read_line(&mut line) {
        Ok(0) => return Ok(()),
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(());
        } // Idle speculative preconnect.
        Err(error) => return Err(error.to_string()),
    }
    let route = line
        .split_whitespace()
        .nth(1)
        .ok_or("Malformed synthetic request")?;
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
