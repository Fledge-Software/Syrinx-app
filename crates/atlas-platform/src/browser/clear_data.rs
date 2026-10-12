//! Clears WebView2's own browsing data for the shared profile without touching any page.

use std::sync::mpsc;

use webview2_com::{
    ClearBrowsingDataCompletedHandler, CreateCoreWebView2ControllerCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_BROWSING_DATA_KINDS, COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY,
        COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, ICoreWebView2_13, ICoreWebView2Controller,
        ICoreWebView2Environment, ICoreWebView2Profile2,
    },
};
use windows::{
    Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND},
    core::Interface,
};

use super::BrowserError;

/// The owner's choices, mapped onto WebView2's data kinds; Atlas's own history is separate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowsingDataKinds {
    /// WebView2's own visited-page record, beside Atlas's history database.
    pub history: bool,
    /// Cookies plus every kind of site storage, which signs the owner out of sites.
    pub cookies_and_site_data: bool,
    /// The HTTP disk cache.
    pub cached_files: bool,
}

impl BrowsingDataKinds {
    pub fn is_empty(self) -> bool {
        !(self.history || self.cookies_and_site_data || self.cached_files)
    }

    fn webview2(self) -> COREWEBVIEW2_BROWSING_DATA_KINDS {
        let mut kinds = 0;
        if self.history {
            kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY.0;
        }
        if self.cookies_and_site_data {
            kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE.0;
        }
        if self.cached_files {
            kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0;
        }
        COREWEBVIEW2_BROWSING_DATA_KINDS(kinds)
    }
}

/// A half-open span of Unix seconds; `None` in a command means all time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClearSpan {
    pub start: i64,
    pub end: i64,
}

/// Whether the data was cleared, and separately whether the hidden view closed afterwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClearOutcome {
    pub cleared: Result<(), BrowserError>,
    pub closed: Result<(), BrowserError>,
}

/// Clears the chosen kinds within `span`, or from all time.
///
/// A hidden controller on the shared environment reaches the profile every page uses; it
/// never navigates, and no script, handler or user agent is added to any page.
pub(super) fn clear(
    environment: &ICoreWebView2Environment,
    parent: isize,
    kinds: BrowsingDataKinds,
    span: Option<ClearSpan>,
) -> ClearOutcome {
    if kinds.is_empty() {
        return ClearOutcome {
            cleared: Ok(()),
            closed: Ok(()),
        };
    }
    let controller = match hidden_controller(environment, parent) {
        Ok(controller) => controller,
        Err(error) => {
            return ClearOutcome {
                cleared: Err(error),
                closed: Ok(()),
            };
        }
    };
    let cleared = clear_profile(&controller, kinds, span);
    // Closing releases the hidden view whether or not clearing succeeded.
    let closed = unsafe { controller.Close() }.map_err(clear_error);
    ClearOutcome { cleared, closed }
}

/// Keeps WebView2's HRESULT so a failure can be named; it carries no page content.
fn clear_error(error: windows::core::Error) -> BrowserError {
    BrowserError::ClearData {
        code: error.code().0,
    }
}

/// A wait that ends without WebView2's answer keeps its code, or reports an unexpected stop.
fn pump_error(error: webview2_com::Error) -> BrowserError {
    match error {
        webview2_com::Error::WindowsError(error) => clear_error(error),
        _ => BrowserError::ClearData {
            code: E_UNEXPECTED.0,
        },
    }
}

fn hidden_controller(
    environment: &ICoreWebView2Environment,
    parent: isize,
) -> Result<ICoreWebView2Controller, BrowserError> {
    let (send, receive) = mpsc::channel();
    let completion = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            let result = status
                .map_err(clear_error)
                .and_then(|()| controller.ok_or(BrowserError::ClearData { code: E_POINTER.0 }));
            send.send(result)
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
        },
    ));
    unsafe { environment.CreateCoreWebView2Controller(HWND(parent as *mut _), &completion) }
        .map_err(clear_error)?;
    let controller = webview2_com::wait_with_pump(receive).map_err(pump_error)??;
    unsafe { controller.SetIsVisible(false) }.map_err(clear_error)?;
    Ok(controller)
}

fn clear_profile(
    controller: &ICoreWebView2Controller,
    kinds: BrowsingDataKinds,
    span: Option<ClearSpan>,
) -> Result<(), BrowserError> {
    let profile: ICoreWebView2Profile2 = unsafe {
        controller
            .CoreWebView2()
            .and_then(|webview| webview.cast::<ICoreWebView2_13>())
            .and_then(|webview| webview.Profile())
            .and_then(|profile| profile.cast())
    }
    .map_err(clear_error)?;
    let (send, receive) = mpsc::channel();
    let completion = ClearBrowsingDataCompletedHandler::create(Box::new(move |status| {
        send.send(status.map_err(clear_error))
            .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
    }));
    let started = match span {
        // WebView2 counts seconds since the Unix epoch, as Atlas's history does.
        Some(span) => unsafe {
            profile.ClearBrowsingDataInTimeRange(
                kinds.webview2(),
                span.start as f64,
                span.end as f64,
                &completion,
            )
        },
        None => unsafe { profile.ClearBrowsingData(kinds.webview2(), &completion) },
    };
    started.map_err(clear_error)?;
    webview2_com::wait_with_pump(receive).map_err(pump_error)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_choice_maps_to_its_documented_webview2_kinds() {
        assert!(BrowsingDataKinds::default().is_empty());
        let history = BrowsingDataKinds {
            history: true,
            ..BrowsingDataKinds::default()
        };
        assert_eq!(
            history.webview2(),
            COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY
        );
        let all = BrowsingDataKinds {
            history: true,
            cookies_and_site_data: true,
            cached_files: true,
        };
        assert_eq!(
            all.webview2().0,
            COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY.0
                | COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE.0
                | COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0
        );
    }
}
