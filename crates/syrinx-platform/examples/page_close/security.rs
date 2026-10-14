//! Proves untrusted TLS is cancelled with no page content or cached bypass, using only loopback.

use super::*;
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
};
use syrinx_platform::browser::{ConnectionScheme, ConnectionState, ConnectionStatus};

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    (request.starts_with(b"GET /security-recovery ") || request.starts_with(b"GET /security-recovery?"))
        .then_some("<!doctype html><title>Synthetic certificate recovery</title><link rel=icon href=data:,><p>Harmless local HTTP recovery document.</p>")
}

/// How long an attempt whose state is already `Failed` may wait for its certificate event.
/// WebView2 can report the failed load first, so failing at `Failed` itself would be wrong.
const CERTIFICATE_EVENT_GRACE: Duration = Duration::from_secs(5);

pub(super) fn run(http_url: &str) -> Result<(), String> {
    let base = http_url
        .strip_suffix("/close")
        .ok_or("Synthetic HTTP URL shape changed.")?;
    let directory = reserve_profile()?;
    let mut server = CertificateServer::start(directory.clone())?;
    let log = directory.join("server.log");
    let result = server.url().and_then(|url| {
        run_browser(
            directory.join("WebView2"),
            &format!("{url}\n{base}/security-recovery\n{}", log.display()),
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
            .env("SYRINX_SYNTHETIC_CERT_DIRECTORY", &directory)
            .env(
                "SYRINX_SYNTHETIC_CERT_GENERATOR",
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
    let mut lines = url.split('\n');
    let (Some(url), Some(recovery), Some(log), None) =
        (lines.next(), lines.next(), lines.next(), lines.next())
    else {
        return Err("Synthetic security URL and log lines changed.".into());
    };
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
        wait_rejection(events, deadline, attempt + 1, log)?;
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

/// Where the rejection wait stands: what it has seen and when it must stop waiting.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RejectionWait {
    /// Both the failed completion and the certificate event have arrived.
    Complete,
    /// Keep waiting until `until`; `after_failed` means a `Failed` state lacks its certificate event.
    Waiting { until: Instant, after_failed: bool },
}

/// Decides how long to keep waiting, given when the attempt first reported `Failed`.
fn rejection_wait(
    failed_state_at: Option<Instant>,
    rejected: bool,
    failed: bool,
    suite_deadline: Instant,
) -> RejectionWait {
    if rejected && failed {
        return RejectionWait::Complete;
    }
    let Some(failed_at) = failed_state_at.filter(|_| !rejected) else {
        return RejectionWait::Waiting {
            until: suite_deadline,
            after_failed: false,
        };
    };
    RejectionWait::Waiting {
        until: (failed_at + CERTIFICATE_EVENT_GRACE).min(suite_deadline),
        after_failed: true,
    }
}

/// Names the attempt, its last connection state, the server log and the wait's own error.
fn missing_certificate_event(
    attempt: usize,
    state: Option<ConnectionState>,
    log: &str,
    error: &str,
) -> String {
    format!(
        "Synthetic invalid certificate attempt {attempt} failed without a certificate event \
         within {}s of its Failed state or before the suite deadline (last connection state \
         {state:?}); inspect the synthetic TLS server log at {log}. Wait error: {error}",
        CERTIFICATE_EVENT_GRACE.as_secs()
    )
}

fn wait_rejection(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
    attempt: usize,
    log: &str,
) -> Result<(), String> {
    let mut rejected = false;
    let mut failed = false;
    let mut failed_state_at = None;
    let mut last_state = None;
    loop {
        let (until, after_failed) =
            match rejection_wait(failed_state_at, rejected, failed, deadline) {
                RejectionWait::Complete => break,
                RejectionWait::Waiting {
                    until,
                    after_failed,
                } => (until, after_failed),
            };
        let event = match next_event(events, until) {
            Ok(event) => event,
            // The wait expired after `Failed`: name what was seen and keep the original error.
            Err(error) if after_failed && Instant::now() >= until => {
                return Err(missing_certificate_event(attempt, last_state, log, &error));
            }
            Err(error) => return Err(error),
        };
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
                if state.status == ConnectionStatus::Failed && failed_state_at.is_none() {
                    failed_state_at = Some(Instant::now());
                }
                last_state = Some(state);
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

#[cfg(test)]
mod tests {
    use super::{
        CERTIFICATE_EVENT_GRACE, ConnectionScheme, ConnectionState, ConnectionStatus, Duration,
        Instant, RejectionWait, missing_certificate_event, rejection_wait,
    };

    #[test]
    fn waits_for_the_suite_deadline_until_a_failed_state_arrives() {
        let suite = Instant::now() + Duration::from_secs(45);
        let waiting = RejectionWait::Waiting {
            until: suite,
            after_failed: false,
        };
        assert_eq!(rejection_wait(None, false, false, suite), waiting);
        // A failed completion alone, or a certificate event alone, keeps the old wait.
        assert_eq!(rejection_wait(None, false, true, suite), waiting);
        assert_eq!(rejection_wait(None, true, false, suite), waiting);
    }

    #[test]
    fn a_failed_state_without_a_certificate_event_starts_a_short_grace() {
        let failed_at = Instant::now();
        let suite = failed_at + Duration::from_secs(45);
        // The grace is a bounded wait, not an immediate failure.
        assert_eq!(
            rejection_wait(Some(failed_at), false, true, suite),
            RejectionWait::Waiting {
                until: failed_at + CERTIFICATE_EVENT_GRACE,
                after_failed: true,
            }
        );
        assert!(CERTIFICATE_EVENT_GRACE >= Duration::from_secs(5));
    }

    #[test]
    fn a_late_certificate_event_ends_the_grace() {
        let failed_at = Instant::now();
        let suite = failed_at + Duration::from_secs(45);
        assert_eq!(
            rejection_wait(Some(failed_at), true, false, suite),
            RejectionWait::Waiting {
                until: suite,
                after_failed: false,
            }
        );
        assert_eq!(
            rejection_wait(Some(failed_at), true, true, suite),
            RejectionWait::Complete
        );
    }

    #[test]
    fn the_grace_never_extends_the_suite_deadline() {
        let failed_at = Instant::now();
        let suite = failed_at + Duration::from_secs(2);
        // Expiring at the suite deadline still reports the missing certificate event.
        assert_eq!(
            rejection_wait(Some(failed_at), false, true, suite),
            RejectionWait::Waiting {
                until: suite,
                after_failed: true,
            }
        );
    }

    #[test]
    fn the_failure_names_the_attempt_state_and_log() {
        let state = ConnectionState {
            scheme: ConnectionScheme::Https,
            status: ConnectionStatus::Failed,
        };
        let message = missing_certificate_event(
            2,
            Some(state),
            r"C:\synthetic\server.log",
            "Synthetic browser event channel closed.",
        );
        assert!(message.contains("attempt 2"), "{message}");
        assert!(message.contains("Https"), "{message}");
        assert!(message.contains("Failed"), "{message}");
        assert!(message.contains(r"C:\synthetic\server.log"), "{message}");
        // The original wait error survives, so a closed channel is not mislabelled.
        assert!(message.contains("event channel closed"), "{message}");
    }
}
