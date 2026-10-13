//! Reads a response's status, its request's method and three named headers; never a body, a
//! filter or another header.

use atlas_core::ObservedResponse;
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2HttpResponseHeaders, ICoreWebView2WebResourceResponseReceivedEventArgs,
    },
    take_pwstr,
};
use windows::{
    Win32::Foundation::E_UNEXPECTED,
    core::{BOOL, PCWSTR, PWSTR, w},
};

/// The most values read for one header; more makes the response unreadable (fails closed).
const HEADER_VALUES: usize = 64;

/// Reads one response if `wanted` accepts its address; otherwise reads nothing more.
pub(super) fn observed_response(
    args: &ICoreWebView2WebResourceResponseReceivedEventArgs,
    wanted: impl Fn(&str) -> bool,
) -> windows::core::Result<Option<ObservedResponse>> {
    let request = unsafe { args.Request() }?;
    let mut raw = PWSTR::null();
    let read = unsafe { request.Uri(&mut raw) };
    let address = take_pwstr(raw);
    read?;
    if !wanted(&address) {
        return Ok(None);
    }
    let mut raw = PWSTR::null();
    let read = unsafe { request.Method(&mut raw) };
    let method = take_pwstr(raw);
    read?;
    let request_headers = unsafe { request.Headers() }?;
    let destination = named_header(
        |present| unsafe { request_headers.Contains(w!("Sec-Fetch-Dest"), present) },
        |value| unsafe { request_headers.GetHeader(w!("Sec-Fetch-Dest"), value) },
    )?;
    let response = unsafe { args.Response() }?;
    let mut status = 0;
    unsafe { response.StatusCode(&mut status) }?;
    let status = u16::try_from(status).map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
    let headers = unsafe { response.Headers() }?;
    let content_type = named_header(
        |present| unsafe { headers.Contains(w!("Content-Type"), present) },
        |value| unsafe { headers.GetHeader(w!("Content-Type"), value) },
    )?;
    Ok(Some(ObservedResponse {
        address,
        status,
        content_type,
        // WebView2 splits one line's directives but drops a repeated header's later lines
        // (F2); see `docs/p1-25-page-text.md`.
        cache_control: header_values(&headers, w!("Cache-Control"))?,
        destination,
        method,
    }))
}

/// One header's first value from either header interface, or `None` when it is absent.
fn named_header(
    contains: impl FnOnce(&mut BOOL) -> windows::core::Result<()>,
    get: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<Option<String>> {
    let mut present = BOOL::default();
    contains(&mut present)?;
    if !present.as_bool() {
        return Ok(None);
    }
    let mut raw = PWSTR::null();
    let read = get(&mut raw);
    let value = take_pwstr(raw);
    read.map(|()| Some(value))
}

/// Every value WebView2 reports for a response header, in order.
fn header_values(
    headers: &ICoreWebView2HttpResponseHeaders,
    name: PCWSTR,
) -> windows::core::Result<Vec<String>> {
    let iterator = unsafe { headers.GetHeaders(name) }?;
    let mut values = Vec::new();
    loop {
        let mut current = BOOL::default();
        unsafe { iterator.HasCurrentHeader(&mut current) }?;
        if !current.as_bool() {
            return Ok(values);
        }
        let (mut raw_name, mut raw_value) = (PWSTR::null(), PWSTR::null());
        let read = unsafe { iterator.GetCurrentHeader(&mut raw_name, &mut raw_value) };
        let (_name, value) = (take_pwstr(raw_name), take_pwstr(raw_value));
        read?;
        values.push(value);
        if values.len() > HEADER_VALUES {
            return Err(windows::core::Error::from(E_UNEXPECTED));
        }
        // As in Microsoft's sample, HasCurrentHeader alone decides when the values end.
        let mut _has_next = BOOL::default();
        unsafe { iterator.MoveNext(&mut _has_next) }?;
    }
}
