//! Cancels certificate errors and reports observed connection state without asserting site safety.

use std::{cell::RefCell, rc::Rc};

use webview2_com::{
    CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, ServerCertificateErrorDetectedEventHandler,
};
use windows::{
    Win32::Foundation::{E_FAIL, E_POINTER},
    core::{BOOL, Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{
    BrowserError, BrowserEvent, ConnectionScheme, ConnectionState, ConnectionStatus, EventSink,
};

struct Security {
    navigation: Option<u64>,
    state: ConnectionState,
}

impl Security {
    fn new() -> Self {
        Self {
            navigation: None,
            state: ConnectionState {
                scheme: ConnectionScheme::Other,
                status: ConnectionStatus::Unavailable,
            },
        }
    }

    fn starting(&mut self, navigation: u64, uri: &str) {
        let rejected = self.navigation == Some(navigation)
            && self.state.status == ConnectionStatus::CertificateRejected;
        self.navigation = Some(navigation);
        self.state = ConnectionState {
            scheme: if uri.starts_with("https://") {
                ConnectionScheme::Https
            } else if uri.starts_with("http://") {
                ConnectionScheme::Http
            } else {
                ConnectionScheme::Other
            },
            status: if rejected {
                ConnectionStatus::CertificateRejected
            } else {
                ConnectionStatus::Loading
            },
        };
    }

    fn completed(&mut self, navigation: u64, success: bool) {
        if self.navigation != Some(navigation)
            || matches!(
                self.state.status,
                ConnectionStatus::CertificateRejected | ConnectionStatus::Unavailable
            )
        {
            return;
        }
        self.state.status = if success {
            ConnectionStatus::Loaded
        } else {
            ConnectionStatus::Failed
        };
    }

    fn emit(&self, id: u64, events: &EventSink) {
        events.emit(BrowserEvent::Connection {
            id,
            state: self.state,
        });
    }
}

pub(super) fn register(view: &WebView, id: u64, events: EventSink) -> Result<(), BrowserError> {
    let state = Rc::new(RefCell::new(Security::new()));
    let core = view.webview();
    register_certificate_errors(&core, id, state.clone(), events.clone())
        .map_err(|_| BrowserError::EventRegistration)?;
    register_starting(&core, id, state.clone(), events.clone())
        .map_err(|_| BrowserError::EventRegistration)?;
    register_completed(&core, id, state, events).map_err(|_| BrowserError::EventRegistration)
}

fn register_starting(
    core: &ICoreWebView2,
    id: u64,
    state: Rc<RefCell<Security>>,
    events: EventSink,
) -> windows::core::Result<()> {
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let result = (|| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let mut navigation = 0;
            let mut raw = PWSTR::null();
            unsafe {
                args.NavigationId(&mut navigation)?;
            }
            let uri_result = unsafe { args.Uri(&mut raw) };
            let uri = CoTaskMemPWSTR::from(raw);
            uri_result?;
            state.borrow_mut().starting(navigation, &uri.to_string());
            Ok(())
        })();
        report(result, id, &state, &events)
    }));
    let mut token = 0;
    unsafe { core.add_NavigationStarting(&handler, &mut token) }
}

fn register_completed(
    core: &ICoreWebView2,
    id: u64,
    state: Rc<RefCell<Security>>,
    events: EventSink,
) -> windows::core::Result<()> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let result = (|| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let mut navigation = 0;
            let mut success = BOOL::default();
            unsafe {
                args.NavigationId(&mut navigation)?;
                args.IsSuccess(&mut success)?;
            }
            state.borrow_mut().completed(navigation, success.as_bool());
            Ok(())
        })();
        report(result, id, &state, &events)
    }));
    let mut token = 0;
    unsafe { core.add_NavigationCompleted(&handler, &mut token) }
}

fn register_certificate_errors(
    core: &ICoreWebView2,
    id: u64,
    state: Rc<RefCell<Security>>,
    events: EventSink,
) -> windows::core::Result<()> {
    let handler =
        ServerCertificateErrorDetectedEventHandler::create(Box::new(move |core, args| {
            let result = (|| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                // Apply cancellation before inspecting or reporting anything. There is no bypass UI.
                unsafe {
                    args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL)?;
                }
                let mut action = COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_DEFAULT;
                unsafe {
                    args.Action(&mut action)?;
                }
                if action != COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL {
                    return Err(E_FAIL.into());
                }
                // This event also covers subresources; never describe it as proof about only the main document.
                state.borrow_mut().state.status = ConnectionStatus::CertificateRejected;
                Ok(())
            })();
            if result.is_err() {
                let Some(core) = core else {
                    return report(Err(E_POINTER.into()), id, &state, &events);
                };
                if let Err(error) = unsafe { core.Stop() } {
                    return report(Err(error), id, &state, &events);
                }
            }
            report(result, id, &state, &events)
        }));
    let mut token = 0;
    unsafe {
        core.cast::<ICoreWebView2_14>()?
            .add_ServerCertificateErrorDetected(&handler, &mut token)
    }
}

fn report(
    result: windows::core::Result<()>,
    id: u64,
    state: &RefCell<Security>,
    events: &EventSink,
) -> windows::core::Result<()> {
    if result.is_err() {
        state.borrow_mut().state.status = ConnectionStatus::Unavailable;
        events.emit(BrowserEvent::Error {
            tab: Some(id),
            error: BrowserError::Security,
        });
    }
    state.borrow().emit(id, events);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_identity_prevents_old_success_from_certifying_a_new_attempt() {
        let mut state = Security::new();
        state.starting(1, "https://first.invalid/");
        state.starting(2, "http://second.test/");
        state.completed(1, true);
        assert_eq!(
            state.state,
            ConnectionState {
                scheme: ConnectionScheme::Http,
                status: ConnectionStatus::Loading
            }
        );
        state.completed(2, false);
        assert_eq!(state.state.status, ConnectionStatus::Failed);
    }

    #[test]
    fn certificate_rejection_survives_completion_and_resets_only_for_navigation() {
        let mut state = Security::new();
        state.starting(1, "https://example.com/");
        state.state.status = ConnectionStatus::CertificateRejected;
        state.completed(1, true);
        assert_eq!(state.state.status, ConnectionStatus::CertificateRejected);
        state.starting(1, "https://redirect.invalid/");
        assert_eq!(state.state.status, ConnectionStatus::CertificateRejected);
        state.starting(2, "about:blank");
        assert_eq!(state.state.scheme, ConnectionScheme::Other);
        assert_eq!(state.state.status, ConnectionStatus::Loading);
    }

    #[test]
    fn a_native_observation_failure_stays_unknown_until_a_new_navigation() {
        let mut state = Security::new();
        state.starting(1, "https://example.com/");
        state.state.status = ConnectionStatus::Unavailable;
        state.completed(1, true);
        assert_eq!(state.state.status, ConnectionStatus::Unavailable);
        state.starting(2, "https://example.com/");
        assert_eq!(state.state.status, ConnectionStatus::Loading);
    }

    #[test]
    fn redirects_report_the_destination_scheme_without_an_early_success() {
        let mut state = Security::new();
        state.starting(1, "https://example.com/");
        state.starting(1, "http://example.com/");
        assert_eq!(state.state.scheme, ConnectionScheme::Http);
        assert_eq!(state.state.status, ConnectionStatus::Loading);
        state.completed(1, true);
        assert_eq!(state.state.status, ConnectionStatus::Loaded);
    }
}
