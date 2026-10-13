//! Labels the observed transport and certificate failures without promising that a site is safe.

use super::*;
use atlas_platform::browser::{ConnectionScheme, ConnectionState, ConnectionStatus};

impl BrowserView {
    pub(super) fn observe_connection(&mut self, id: u64, state: ConnectionState) {
        if self.closing || !self.model.tabs.iter().any(|tab| tab.id.get() == id) {
            return;
        }
        self.connections.insert(id, state);
    }

    pub(super) fn connection_display(&self) -> Div {
        let state = self
            .shown
            .and_then(|id| self.connections.get(&id.get()))
            .copied();
        let (scheme, status) = label(state);
        div()
            .flex()
            .flex_col()
            .text_xs()
            .max_w(px(180.))
            .flex_shrink_0()
            .text_color(rgb(
                if state.is_some_and(|state| state.status == ConnectionStatus::CertificateRejected)
                {
                    0xffb4ab
                } else {
                    0xb4b7bf
                },
            ))
            .child(scheme)
            .child(status)
    }
}

fn label(state: Option<ConnectionState>) -> (&'static str, &'static str) {
    let Some(state) = state else {
        return ("Connection", "Status unavailable");
    };
    let scheme = match state.scheme {
        ConnectionScheme::Http if state.status != ConnectionStatus::Loaded => {
            "HTTP attempt · unencrypted"
        }
        ConnectionScheme::Http => "HTTP · unencrypted",
        ConnectionScheme::Https if state.status != ConnectionStatus::Loaded => "HTTPS attempt",
        ConnectionScheme::Https => "HTTPS",
        ConnectionScheme::Other => "Local or other scheme",
    };
    let status = match state.status {
        ConnectionStatus::Loading => "Loading; status pending",
        ConnectionStatus::CertificateRejected => "Certificate request blocked",
        ConnectionStatus::Failed => "Load failed; status unknown",
        ConnectionStatus::Unavailable => "Certificate status unavailable",
        ConnectionStatus::Loaded => match state.scheme {
            ConnectionScheme::Https => "No certificate error reported",
            ConnectionScheme::Http => "No TLS certificate",
            ConnectionScheme::Other => "No HTTPS assurance",
        },
    };
    (scheme, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_never_presented_as_a_site_safety_guarantee() {
        for status in [
            ConnectionStatus::Loading,
            ConnectionStatus::Loaded,
            ConnectionStatus::Failed,
            ConnectionStatus::CertificateRejected,
            ConnectionStatus::Unavailable,
        ] {
            let (scheme, text) = label(Some(ConnectionState {
                scheme: ConnectionScheme::Https,
                status,
            }));
            assert_eq!(
                scheme,
                if status == ConnectionStatus::Loaded {
                    "HTTPS"
                } else {
                    "HTTPS attempt"
                }
            );
            assert!(!text.to_lowercase().contains("secure"));
            assert!(!text.to_lowercase().contains("safe"));
        }
        assert_eq!(
            label(Some(ConnectionState {
                scheme: ConnectionScheme::Https,
                status: ConnectionStatus::Loaded
            }))
            .1,
            "No certificate error reported"
        );
        assert_eq!(
            label(Some(ConnectionState {
                scheme: ConnectionScheme::Https,
                status: ConnectionStatus::CertificateRejected
            }))
            .1,
            "Certificate request blocked"
        );
    }

    #[test]
    fn unknown_or_non_tls_documents_cannot_borrow_an_https_label() {
        assert_eq!(label(None), ("Connection", "Status unavailable"));
        assert_eq!(
            label(Some(ConnectionState {
                scheme: ConnectionScheme::Http,
                status: ConnectionStatus::Failed
            }))
            .0,
            "HTTP attempt · unencrypted"
        );
        assert_eq!(
            label(Some(ConnectionState {
                scheme: ConnectionScheme::Http,
                status: ConnectionStatus::Loaded
            })),
            ("HTTP · unencrypted", "No TLS certificate")
        );
        assert_eq!(
            label(Some(ConnectionState {
                scheme: ConnectionScheme::Other,
                status: ConnectionStatus::Loaded
            })),
            ("Local or other scheme", "No HTTPS assurance")
        );
    }
}
