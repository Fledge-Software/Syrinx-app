//! Creates the shared Evergreen environment without changing page identity or Windows policy.

use std::{os::windows::ffi::OsStrExt, path::Path, sync::mpsc};

use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_RELEASE_CHANNELS_STABLE, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
    },
};
use windows::{
    Win32::{
        Foundation::E_UNEXPECTED,
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

use super::BrowserError;

pub(super) struct Apartment;

impl Apartment {
    pub(super) fn new() -> Result<Self, BrowserError> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|_| BrowserError::Apartment)?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) fn validate_profile(profile: &Path) -> Result<(), BrowserError> {
    if !profile.is_absolute()
        || profile.as_os_str().is_empty()
        || profile.as_os_str().encode_wide().any(|unit| unit == 0)
    {
        return Err(BrowserError::InvalidProfile);
    }
    // Runtime overrides can silently replace the profile, browser channel or safe arguments.
    for variable in [
        "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
        "WEBVIEW2_USER_DATA_FOLDER",
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
        "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
    ] {
        if std::env::var_os(variable).is_some() {
            return Err(BrowserError::EnvironmentOverride);
        }
    }
    Ok(())
}

pub(super) fn create(profile: &Path) -> Result<ICoreWebView2Environment, BrowserError> {
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        // Supplying our own environment bypasses wry's SmartScreen-disabling defaults.
        options
            .set_additional_browser_arguments("--enable-features=msSmartScreenProtection".into());
        options.set_is_custom_crash_reporting_enabled(true);
        options.set_are_browser_extensions_enabled(false);
        options.set_release_channels(COREWEBVIEW2_RELEASE_CHANNELS_STABLE);
    }
    let options = ICoreWebView2EnvironmentOptions::from(options);
    let profile = HSTRING::from(profile);
    let (send, receive) = mpsc::channel();
    let completion = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            let result = status
                .map_err(|_| BrowserError::Environment)
                .and_then(|()| environment.ok_or(BrowserError::Environment));
            send.send(result)
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
        },
    ));
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &profile, &options, &completion)
    }
    .map_err(|_| BrowserError::Environment)?;
    webview2_com::wait_with_pump(receive).map_err(|_| BrowserError::Environment)?
}

/// The Evergreen runtime version this environment runs, for D36's change notice.
pub(super) fn runtime_version(environment: &ICoreWebView2Environment) -> Option<String> {
    let mut version = PWSTR::null();
    if unsafe { environment.BrowserVersionString(&mut version) }.is_err() {
        return None;
    }
    let version = webview2_com::take_pwstr(version);
    (!version.is_empty()).then_some(version)
}
