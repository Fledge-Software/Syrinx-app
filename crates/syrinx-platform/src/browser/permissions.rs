//! Defers site permission requests to Syrinx and denies abandoned or obsolete requests.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use syrinx_store::PermissionKind;
use webview2_com::{
    CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::*, NavigationStartingEventHandler,
    PermissionRequestedEventHandler,
};
use windows::{
    Win32::Foundation::E_POINTER,
    core::{Interface, PWSTR},
};
use wry::{WebView, WebViewExtWindows};

use super::{BrowserError, BrowserEvent, EventSink};

#[derive(Default)]
struct Pending {
    requests: HashMap<u64, Request>,
}

#[derive(Clone, Default)]
pub(super) struct Permissions(Rc<RefCell<Pending>>);

struct Request {
    id: u64,
    request: u64,
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: Option<ICoreWebView2Deferral>,
    events: EventSink,
}

impl Request {
    fn complete(&mut self) -> Result<(), BrowserError> {
        if let Some(deferral) = self.deferral.take() {
            unsafe { deferral.Complete() }.map_err(|_| BrowserError::Permission)?;
        }
        Ok(())
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        if let Err(error) = self.complete() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.id),
                error,
            });
        }
        self.events.emit(BrowserEvent::PermissionFinished {
            id: self.id,
            request: self.request,
        });
    }
}

impl Permissions {
    pub(super) fn register(
        view: &WebView,
        id: u64,
        events: EventSink,
    ) -> Result<Self, BrowserError> {
        let permissions = Self::default();
        let pending = permissions.clone();
        let handler = PermissionRequestedEventHandler::create(Box::new(move |_core, args| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            // Deny first: failures and unknown future kinds never fall through to an engine grant.
            unsafe {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
            }
            let result = pending.enqueue(id, args, events.clone());
            if result.is_err() {
                events.emit(BrowserEvent::Error {
                    tab: Some(id),
                    error: BrowserError::Permission,
                });
            }
            result
        }));
        let mut token = 0;
        unsafe { view.webview().add_PermissionRequested(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)?;
        let pending = permissions.clone();
        let navigation = NavigationStartingEventHandler::create(Box::new(move |_, _| {
            pending.clear();
            Ok(())
        }));
        unsafe {
            view.webview()
                .add_NavigationStarting(&navigation, &mut token)
        }
        .map_err(|_| BrowserError::EventRegistration)?;
        Ok(permissions)
    }

    fn enqueue(
        &self,
        id: u64,
        args: ICoreWebView2PermissionRequestedEventArgs,
        events: EventSink,
    ) -> windows::core::Result<()> {
        let extended: ICoreWebView2PermissionRequestedEventArgs3 = args.cast()?;
        unsafe {
            extended.SetSavesInProfile(false)?;
        }
        if !events.accepts_callbacks() {
            return Ok(());
        }
        let mut native_kind = COREWEBVIEW2_PERMISSION_KIND::default();
        unsafe {
            args.PermissionKind(&mut native_kind)?;
        }
        let Some(kind) = permission_kind(native_kind) else {
            return Ok(());
        };
        let mut raw = PWSTR::null();
        let result = unsafe { args.Uri(&mut raw) };
        let uri = CoTaskMemPWSTR::from(raw);
        result?;
        let request = {
            let pending = self.0.borrow();
            let Some(next) = next_request() else {
                return Ok(());
            };
            if pending.requests.len() >= 16 {
                return Ok(());
            }
            next
        };
        let deferral = unsafe { args.GetDeferral()? };
        self.0.borrow_mut().requests.insert(
            request,
            Request {
                id,
                request,
                args,
                deferral: Some(deferral),
                events: events.clone(),
            },
        );
        events.emit(BrowserEvent::PermissionRequested {
            id,
            request,
            uri: uri.to_string(),
            kind,
        });
        Ok(())
    }

    pub(super) fn resolve(&self, request: u64, allowed: bool) -> Result<(), BrowserError> {
        let pending = self.0.borrow_mut().requests.remove(&request);
        let Some(mut pending) = pending else {
            return Ok(());
        };
        let state = if allowed {
            COREWEBVIEW2_PERMISSION_STATE_ALLOW
        } else {
            COREWEBVIEW2_PERMISSION_STATE_DENY
        };
        unsafe { pending.args.SetState(state) }.map_err(|_| BrowserError::Permission)?;
        pending.complete()
    }

    pub(super) fn clear(&self) {
        // Drop outside the borrow: completing a native deferral can reenter callbacks.
        let requests = std::mem::take(&mut self.0.borrow_mut().requests);
        drop(requests);
    }
}

fn permission_kind(kind: COREWEBVIEW2_PERMISSION_KIND) -> Option<PermissionKind> {
    Some(match kind {
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => PermissionKind::Microphone,
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => PermissionKind::Camera,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => PermissionKind::Location,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => PermissionKind::Notifications,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => PermissionKind::Clipboard,
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => PermissionKind::Sensors,
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => PermissionKind::Midi,
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => PermissionKind::Downloads,
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => PermissionKind::FileSystem,
        COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY => PermissionKind::Autoplay,
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => PermissionKind::Fonts,
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => PermissionKind::WindowManagement,
        _ => return None,
    })
}

// Request identities must survive controller replacement and delayed session-worker replies.
fn next_request() -> Option<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    match SEQUENCE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        current.checked_add(1)
    }) {
        Ok(previous) => Some(previous + 1),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_controllers_never_reuse_pending_permission_request_ids() {
        let original = super::next_request().expect("first synthetic permission identity");
        let replacement = super::next_request().expect("replacement synthetic permission identity");
        assert_ne!(original, replacement);
        assert!(replacement > original);
    }

    #[test]
    fn unknown_permissions_are_denied_and_clipboard_requires_consent() {
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND(999)), None);
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ),
            Some(PermissionKind::Clipboard)
        );
        assert_eq!(
            permission_kind(COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS),
            Some(PermissionKind::Notifications)
        );
    }
}
