//! Proves untrusted TLS is cancelled with no page content or cached bypass, using only loopback.

use super::*;
use atlas_platform::browser::{ConnectionScheme, ConnectionStatus};
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
};

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    (request.starts_with(b"GET /security-recovery ") || request.starts_with(b"GET /security-recovery?"))
        .then_some("<!doctype html><title>Synthetic certificate recovery</title><link rel=icon href=data:,><p>Harmless local HTTP recovery document.</p>")
}

pub(super) fn run(http_url: &str) -> Result<(), String> {
    let base = http_url
        .strip_suffix("/close")
        .ok_or("Synthetic HTTP URL shape changed.")?;
    let directory = reserve_profile()?;
    let mut server = CertificateServer::start(directory.clone())?;
    let result = server.url().and_then(|url| {
        run_browser(
            directory.join("WebView2"),
            &format!("{url}\n{base}/security-recovery"),
            exercise,
        )
    });
    let stopped = server.stop();
    let delivered = match fs::metadata(directory.join("delivered")) {
        Ok(_) => Err("Synthetic untrusted TLS delivered an HTTP request.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect synthetic TLS delivery", error)),
    };
    combine(combine(result, stopped), delivered)
}

struct CertificateServer {
    child: Child,
    directory: PathBuf,
}

impl CertificateServer {
    fn start(directory: PathBuf) -> Result<Self, String> {
        let output = fs::File::create_new(directory.join("server.log"))
            .map_err(|error| io_error("create synthetic TLS log", error))?;
        let errors = output
            .try_clone()
            .map_err(|error| io_error("clone synthetic TLS log", error))?;
        let child = Command::new("python.exe")
            .args(["-c", include_str!("certificate_server.py")])
            .env("ATLAS_SYNTHETIC_CERT_DIRECTORY", &directory)
            .env(
                "ATLAS_SYNTHETIC_CERT_GENERATOR",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/examples/page_close/certificate_server.ps1"
                ),
            )
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors)
            .spawn()
            .map_err(|error| io_error("start synthetic TLS server", error))?;
        Ok(Self { child, directory })
    }

    fn url(&mut self) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self
                .child
                .try_wait()
                .map_err(|error| io_error("poll synthetic TLS server", error))?
                .is_some()
            {
                return Err("Synthetic TLS server stopped before publishing its port; inspect its synthetic server.log.".into());
            }
            match fs::read_to_string(self.directory.join("port")) {
                Ok(port) if !port.is_empty() => {
                    let port: u16 = port.parse().map_err(|_| "Synthetic TLS port is invalid.")?;
                    if port == 0 {
                        return Err("Synthetic TLS port was not allocated.".into());
                    }
                    return Ok(format!("https://127.0.0.1:{port}/invalid"));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error("read synthetic TLS port", error)),
            }
            if Instant::now() >= deadline {
                return Err("Synthetic TLS server startup timed out.".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|error| io_error("poll synthetic TLS server shutdown", error))?
            .is_none()
        {
            self.child
                .kill()
                .map_err(|error| io_error("stop owned synthetic TLS server", error))?;
        }
        self.child
            .wait()
            .map_err(|error| io_error("join owned synthetic TLS server", error))?;
        Ok(())
    }
}

fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    _parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    let (url, recovery) = url
        .split_once('\n')
        .ok_or("Synthetic security URL pair changed.")?;
    send(
        host,
        BrowserCommand::Create {
            id: 4000,
            url: recovery.into(),
        },
    )?;
    wait_recovery(events, recovery, deadline)?;
    println!("Synthetic security HTTP baseline loaded.");
    for attempt in 0..2 {
        send(
            host,
            BrowserCommand::Navigate {
                id: 4000,
                url: url.into(),
            },
        )?;
        wait_rejection(events, deadline)?;
        // A cancelled attempt can retain the previous document; use a distinct source for recovery.
        let next = format!("{recovery}?attempt={attempt}");
        send(
            host,
            BrowserCommand::Navigate {
                id: 4000,
                url: next.clone(),
            },
        )?;
        wait_recovery(events, &next, deadline)?;
    }
    send(host, BrowserCommand::Close { id: 4000 })?;
    Ok(())
}

fn wait_recovery(
    events: &mut UnboundedReceiver<BrowserEvent>,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    let mut recovered = false;
    let mut completed = false;
    let mut source = false;
    while !(recovered && completed && source) {
        let event = checked_event(events, deadline)?;
        reject_content(&event)?;
        match event {
            BrowserEvent::Connection { id: 4000, state } => {
                recovered |= state.scheme == ConnectionScheme::Http
                    && state.status == ConnectionStatus::Loaded
            }
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: true,
            } => completed = true,
            BrowserEvent::Url {
                id: 4000,
                url: observed,
            } => source |= observed == url,
            _ => {}
        }
    }
    Ok(())
}

fn wait_rejection(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    let mut rejected = false;
    let mut failed = false;
    while !(rejected && failed) {
        let event = next_event(events, deadline)?;
        if !matches!(
            event,
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: false
            }
        ) {
            check_event(&event)?;
        }
        reject_content(&event)?;
        match event {
            BrowserEvent::NavigationCompleted { id: 4000, success } => {
                if success {
                    return Err("Synthetic invalid certificate navigation succeeded.".into());
                }
                failed = true;
            }
            BrowserEvent::Connection { id: 4000, state } => {
                if state.scheme != ConnectionScheme::Https {
                    return Err("Synthetic invalid certificate lost its HTTPS scheme.".into());
                }
                if state.status == ConnectionStatus::Loaded {
                    return Err("Synthetic invalid certificate was labelled loaded.".into());
                }
                rejected |= state.status == ConnectionStatus::CertificateRejected;
            }
            _ => {}
        }
    }
    println!("Synthetic self-signed certificate cancelled; failed navigation observed.");
    Ok(())
}

fn reject_content(event: &BrowserEvent) -> Result<(), String> {
    if matches!(event, BrowserEvent::Title { id: 4000, title } if title == "Invalid certificate content reached")
    {
        return Err("Synthetic untrusted page content was displayed.".into());
    }
    Ok(())
}
