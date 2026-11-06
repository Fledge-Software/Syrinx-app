//! Requires both native TLS rejection reports before allowing a safe recovery.

use super::{
    fixture::{Fixture, reserve_profile},
    server::Server,
};
use std::{fs, path::Path, time::Duration};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent, ConnectionScheme, ConnectionStatus};

mod tls_server;
use tls_server::CertificateServer;

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut tls = CertificateServer::start(reserve_profile()?)?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    let first = server.url("/security-recovery?baseline");
    fixture.create(4000, first.clone())?;
    recovery(&mut fixture, &first)?;
    for attempt in 1..=2 {
        fixture.barrier_checked(reject_content)?;
        fixture.send(BrowserCommand::Navigate {
            id: 4000,
            url: tls.url(),
        })?;
        let mut rejection = Rejection::default();
        fixture.wait_checked(
            "failed TLS completion and certificate rejection",
            |event, _| rejection.observe(event),
        )?;
        fixture.barrier_checked(reject_content)?;
        let url = server.url(&format!("/security-recovery?attempt={attempt}"));
        fixture.send(BrowserCommand::Navigate {
            id: 4000,
            url: url.clone(),
        })?;
        recovery(&mut fixture, &url)?;
        tls.require_no_delivery()?;
    }
    fixture.barrier_checked(reject_content)?;
    fixture.stop(Duration::from_secs(5))?;
    // Inspect while the server is alive, then again after reaping it to exclude a write race.
    tls.require_no_delivery()?;
    tls.stop()?;
    tls.require_no_delivery()?;
    server.finish()?;
    println!(
        "Linux TLS: two untrusted rejections, both native reports, distinct HTTP recovery and no request delivery passed."
    );
    Ok(())
}

fn recovery(fixture: &mut Fixture, expected: &str) -> Result<(), String> {
    let (mut loaded, mut completed, mut source) = (false, false, false);
    fixture.wait_checked("distinct safe HTTP recovery", |event, _| {
        reject_content(event)?;
        match event {
            BrowserEvent::Connection { id: 4000, state } => {
                loaded |= state.scheme == ConnectionScheme::Http
                    && state.status == ConnectionStatus::Loaded
            }
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success,
                cancelled,
            } => {
                if !success || *cancelled {
                    return Err("Safe HTTP recovery failed".into());
                }
                completed = true;
            }
            BrowserEvent::Url { id: 4000, url } => source |= url == expected,
            _ => {}
        }
        Ok(loaded && completed && source)
    })
}

fn reject_content(event: &BrowserEvent) -> Result<(), String> {
    if matches!(event, BrowserEvent::Title { id: 4000, title } if title == "Invalid certificate content reached")
        || matches!(event, BrowserEvent::Connection { id: 4000, state } if state.scheme == ConnectionScheme::Https && state.status == ConnectionStatus::Loaded)
    {
        return Err("Untrusted TLS content reached or was labelled loaded".into());
    }
    Ok(())
}

fn no_delivery(directory: &Path) -> Result<(), String> {
    match fs::metadata(directory.join("delivered")) {
        Ok(_) => Err("Untrusted TLS received HTTP application bytes".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Default)]
struct Rejection {
    failed: bool,
    rejected: bool,
}

impl Rejection {
    fn observe(&mut self, event: &BrowserEvent) -> Result<bool, String> {
        reject_content(event)?;
        if matches!(
            event,
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: true,
                ..
            }
        ) {
            return Err("Untrusted TLS navigation succeeded".into());
        }
        if let BrowserEvent::Connection { id: 4000, state } = event
            && state.scheme != ConnectionScheme::Https
        {
            return Err("Rejected TLS attempt lost its HTTPS scheme".into());
        }
        if matches!(
            event,
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: false,
                ..
            }
        ) {
            self.failed = true;
        }
        if matches!(event, BrowserEvent::Connection { id: 4000, state } if state.status == ConnectionStatus::CertificateRejected)
        {
            self.rejected = true;
        }
        Ok(self.failed && self.rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrinx_platform::browser::{ConnectionScheme, ConnectionState};

    fn failed() -> BrowserEvent {
        BrowserEvent::NavigationCompleted {
            id: 4000,
            success: false,
            cancelled: false,
        }
    }

    fn certificate() -> BrowserEvent {
        BrowserEvent::Connection {
            id: 4000,
            state: ConnectionState {
                scheme: ConnectionScheme::Https,
                status: ConnectionStatus::CertificateRejected,
            },
        }
    }

    #[test]
    fn neither_callback_alone_proves_certificate_rejection() {
        for events in [[failed(), certificate()], [certificate(), failed()]] {
            let mut rejection = Rejection::default();
            assert!(
                !rejection.observe(&events[0]).unwrap(),
                "Both independent native reports are required"
            );
            assert!(rejection.observe(&events[1]).unwrap());
        }
    }

    #[test]
    fn success_loaded_and_sentinel_content_are_rejected() {
        let events = [
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: true,
                cancelled: false,
            },
            BrowserEvent::Connection {
                id: 4000,
                state: ConnectionState {
                    scheme: ConnectionScheme::Https,
                    status: ConnectionStatus::Loaded,
                },
            },
            BrowserEvent::Title {
                id: 4000,
                title: "Invalid certificate content reached".into(),
            },
        ];
        for event in events {
            assert!(
                Rejection::default().observe(&event).is_err(),
                "Unsafe TLS outcome must fail"
            );
        }
    }
}
