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
    let mut observation = Observation::recovery(first.clone());
    fixture.create(4000, first)?;
    observation.wait(&mut fixture, "safe HTTP baseline")?;
    for attempt in 1..=2 {
        fixture.barrier_checked(|event| observation.inspect(event))?;
        observation = Observation::Rejection(Rejection::default());
        fixture.send(BrowserCommand::Navigate {
            id: 4000,
            url: tls.url(),
        })?;
        observation.wait(
            &mut fixture,
            "failed TLS completion and certificate rejection",
        )?;
        fixture.barrier_checked(|event| observation.inspect(event))?;
        let url = server.url(&format!("/security-recovery?attempt={attempt}"));
        observation = Observation::recovery(url.clone());
        fixture.send(BrowserCommand::Navigate { id: 4000, url })?;
        observation.wait(&mut fixture, "distinct safe HTTP recovery")?;
        tls.require_no_delivery()?;
    }
    fixture.barrier_checked(|event| observation.inspect(event))?;
    fixture.stop_checked(Duration::from_secs(5), |event| observation.inspect(event))?;
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

/// Successful completions are authorized only while a specific HTTP recovery is pending.
enum Observation {
    Rejection(Rejection),
    Recovery(Recovery),
    Settled,
}

impl Observation {
    fn recovery(expected: String) -> Self {
        Self::Recovery(Recovery {
            expected,
            source: false,
            loaded: false,
        })
    }

    fn wait(&mut self, fixture: &mut Fixture, reason: &str) -> Result<(), String> {
        fixture.wait_checked(reason, |event, _| self.observe(event))
    }

    fn inspect(&mut self, event: &BrowserEvent) -> Result<(), String> {
        self.observe(event).map(|_| ())
    }

    fn observe(&mut self, event: &BrowserEvent) -> Result<bool, String> {
        reject_content(event)?;
        match self {
            Self::Rejection(rejection) => rejection.observe(event),
            Self::Recovery(recovery) => {
                if !recovery.observe(event)? {
                    return Ok(false);
                }
                *self = Self::Settled;
                Ok(true)
            }
            Self::Settled => {
                reject_success(event)?;
                Ok(false)
            }
        }
    }
}

struct Recovery {
    expected: String,
    source: bool,
    loaded: bool,
}

impl Recovery {
    fn observe(&mut self, event: &BrowserEvent) -> Result<bool, String> {
        match event {
            BrowserEvent::Connection { id: 4000, state } => {
                self.loaded |= state.scheme == ConnectionScheme::Http
                    && state.status == ConnectionStatus::Loaded;
            }
            BrowserEvent::Url { id: 4000, url } => {
                if *url != self.expected {
                    return Err("Recovery reported an unexpected source".into());
                }
                self.source = true;
            }
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success,
                cancelled,
            } => {
                if !success || *cancelled {
                    return Err("Safe HTTP recovery failed".into());
                }
                // Page::finished emits Connection(Loaded) before its completion, after commit's Url.
                if !self.source || !self.loaded {
                    return Err(
                        "Success arrived before the exact HTTP recovery source and loaded state"
                            .into(),
                    );
                }
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }
}

fn reject_success(event: &BrowserEvent) -> Result<(), String> {
    if matches!(
        event,
        BrowserEvent::NavigationCompleted {
            id: 4000,
            success: true,
            ..
        }
    ) {
        return Err("Successful completion arrived without an authorized HTTP recovery".into());
    }
    Ok(())
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
        reject_success(event)?;
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
    fn late_success_after_both_rejection_reports_fails_the_barrier_drain() {
        for events in [[failed(), certificate()], [certificate(), failed()]] {
            let mut rejection = Observation::Rejection(Rejection::default());
            assert!(!rejection.observe(&events[0]).unwrap());
            assert!(rejection.observe(&events[1]).unwrap());
            let success = BrowserEvent::NavigationCompleted {
                id: 4000,
                success: true,
                cancelled: false,
            };
            assert!(
                rejection.inspect(&success).is_err(),
                "A successful TLS completion before any HTTP recovery command must fail the drain"
            );
        }
    }

    fn http_source() -> BrowserEvent {
        BrowserEvent::Url {
            id: 4000,
            url: "http://127.0.0.1:1234/security-recovery?attempt=2".into(),
        }
    }

    fn http_loaded() -> BrowserEvent {
        BrowserEvent::Connection {
            id: 4000,
            state: ConnectionState {
                scheme: ConnectionScheme::Http,
                status: ConnectionStatus::Loaded,
            },
        }
    }

    fn success() -> BrowserEvent {
        BrowserEvent::NavigationCompleted {
            id: 4000,
            success: true,
            cancelled: false,
        }
    }

    fn recovering() -> Observation {
        Observation::recovery("http://127.0.0.1:1234/security-recovery?attempt=2".into())
    }

    #[test]
    fn authorized_http_recovery_accepts_its_correlated_success_then_retires_it() {
        for events in [
            [http_source(), http_loaded()],
            [http_loaded(), http_source()],
        ] {
            let mut observation = recovering();
            assert!(!observation.observe(&events[0]).unwrap());
            assert!(!observation.observe(&events[1]).unwrap());
            assert!(observation.observe(&success()).unwrap());
            assert!(
                observation.inspect(&success()).is_err(),
                "Completed recovery cannot authorize a later success"
            );
        }
    }

    #[test]
    fn recovery_cannot_accept_success_without_its_exact_http_source_and_loaded_state() {
        assert!(recovering().observe(&success()).is_err());
        for event in [http_source(), http_loaded()] {
            let mut observation = recovering();
            observation.inspect(&event).unwrap();
            assert!(observation.inspect(&success()).is_err());
        }
        let wrong_source = BrowserEvent::Url {
            id: 4000,
            url: "http://127.0.0.1:1234/security-recovery?attempt=1".into(),
        };
        assert!(recovering().inspect(&wrong_source).is_err());
        for completion in [
            failed(),
            BrowserEvent::NavigationCompleted {
                id: 4000,
                success: true,
                cancelled: true,
            },
        ] {
            let mut observation = recovering();
            observation.inspect(&http_source()).unwrap();
            observation.inspect(&http_loaded()).unwrap();
            assert!(observation.inspect(&completion).is_err());
        }
    }

    #[test]
    fn final_shutdown_drain_keeps_content_and_completion_checks_active() {
        let mut observation = recovering();
        observation.inspect(&http_source()).unwrap();
        observation.inspect(&http_loaded()).unwrap();
        assert!(observation.observe(&success()).unwrap());
        let unsafe_events = [
            success(),
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
        for event in unsafe_events {
            assert!(observation.inspect(&event).is_err());
        }
        observation.inspect(&BrowserEvent::Stopped).unwrap();
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
