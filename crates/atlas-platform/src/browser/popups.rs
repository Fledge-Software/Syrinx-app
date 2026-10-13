//! Retains native popup deferrals on their owning thread until the logical tab is committed.

use super::{BrowserError, BrowserEvent, EventSink, page::validate_url};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Deferral, ICoreWebView2NewWindowRequestedEventArgs,
    },
    NewWindowRequestedEventHandler,
};
use windows::{Win32::Foundation::E_POINTER, core::PWSTR};
use wry::{WebView, WebViewExtWindows};

const MAX_PENDING: usize = 32;

#[derive(Default)]
struct Pending {
    sequence: u64,
    requests: HashMap<u64, PopupRequest>,
}

#[derive(Clone, Default)]
pub(super) struct PopupRequests(Rc<RefCell<Pending>>);

pub(super) struct PopupRequest {
    pub(super) opener: u64,
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: Option<ICoreWebView2Deferral>,
    events: EventSink,
}

impl PopupRequest {
    pub(super) fn adopt(mut self, target: &ICoreWebView2) -> Result<(), BrowserError> {
        unsafe { self.args.SetNewWindow(target) }.map_err(|_| BrowserError::Popup)?;
        self.complete()
    }

    fn complete(&mut self) -> Result<(), BrowserError> {
        if let Some(deferral) = self.deferral.take() {
            unsafe { deferral.Complete() }.map_err(|_| BrowserError::Popup)?;
        }
        Ok(())
    }
}

impl Drop for PopupRequest {
    fn drop(&mut self) {
        if let Err(error) = self.complete() {
            self.events.emit(BrowserEvent::Error {
                tab: Some(self.opener),
                error,
            });
        }
    }
}

impl PopupRequests {
    pub(super) fn register(
        &self,
        view: &WebView,
        opener: u64,
        events: EventSink,
    ) -> Result<(), BrowserError> {
        let requests = self.clone();
        let handler = NewWindowRequestedEventHandler::create(Box::new(move |_core, args| {
            let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            // wry's no-callback handler sets Handled too; neither handler opens an external window.
            unsafe {
                args.SetHandled(true)?;
            }
            if !events.accepts_callbacks() {
                return Ok(());
            }
            let result = requests.enqueue(opener, args, events.clone());
            if let Err(error) = &result {
                events.emit(BrowserEvent::Error {
                    tab: Some(opener),
                    error: BrowserError::Popup,
                });
                return Err(error.clone());
            }
            Ok(())
        }));
        let mut token = 0;
        unsafe { view.webview().add_NewWindowRequested(&handler, &mut token) }
            .map_err(|_| BrowserError::EventRegistration)
    }

    fn enqueue(
        &self,
        opener: u64,
        args: ICoreWebView2NewWindowRequestedEventArgs,
        events: EventSink,
    ) -> windows::core::Result<()> {
        let mut raw = PWSTR::null();
        let result = unsafe { args.Uri(&mut raw) };
        let owned = CoTaskMemPWSTR::from(raw);
        result?;
        let url = owned.to_string();
        if validate_url(&url).is_err() {
            events.emit(BrowserEvent::PopupBlocked { id: opener });
            return Ok(());
        }
        let request = {
            let mut pending = self.0.borrow_mut();
            let Some(sequence) = pending.sequence.checked_add(1) else {
                events.emit(BrowserEvent::PopupBlocked { id: opener });
                return Ok(());
            };
            if pending.requests.len() >= MAX_PENDING {
                events.emit(BrowserEvent::PopupBlocked { id: opener });
                return Ok(());
            }
            pending.sequence = sequence;
            sequence
        };
        let deferral = unsafe { args.GetDeferral()? };
        self.0.borrow_mut().requests.insert(
            request,
            PopupRequest {
                opener,
                args,
                deferral: Some(deferral),
                events: events.clone(),
            },
        );
        events.emit(BrowserEvent::PopupRequested {
            id: opener,
            request,
            url,
        });
        Ok(())
    }

    pub(super) fn take(&self, request: u64) -> Option<PopupRequest> {
        self.0.borrow_mut().requests.remove(&request)
    }

    pub(super) fn cancel_opener(&self, opener: u64) {
        let ids: Vec<_> = self
            .0
            .borrow()
            .requests
            .iter()
            .filter_map(|(id, request)| (request.opener == opener).then_some(*id))
            .collect();
        for id in ids {
            // Complete outside the RefCell borrow because native completion may deliver callbacks.
            drop(self.take(id));
        }
    }

    pub(super) fn clear(&self) {
        let requests = std::mem::take(&mut self.0.borrow_mut().requests);
        drop(requests);
    }
}
