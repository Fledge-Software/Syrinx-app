//! Opens a page in Microsoft Edge when the owner asks, for sign-in that breaks in WebView2 (D36).

use std::fmt;

use syrinx_core::web_address;
use windows::{
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    core::{HSTRING, PCWSTR, w},
};

/// Why a page could not be handed to Edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeError {
    /// Only `http` and `https` pages are handed over; nothing else is ever launched.
    NotWeb,
    /// Windows did not start Edge, for example because it is not installed.
    Launch,
}

impl fmt::Display for EdgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotWeb => "Only web pages can be opened in Edge.",
            Self::Launch => "Windows could not open Microsoft Edge.",
        })
    }
}

impl std::error::Error for EdgeError {}

/// Hands one checked web address to Edge through its registered `microsoft-edge:` protocol.
/// Call this only from an explicit owner action; Syrinx never opens Edge on its own.
pub fn open_in_edge(address: &str) -> Result<(), EdgeError> {
    let target = HSTRING::from(edge_target(address)?);
    let instance = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value greater than 32.
    if instance.0 as isize <= 32 {
        return Err(EdgeError::Launch);
    }
    Ok(())
}

/// The protocol target for an address, refusing anything that is not an http(s) page.
fn edge_target(address: &str) -> Result<String, EdgeError> {
    let address = web_address(address).map_err(|_| EdgeError::NotWeb)?;
    Ok(format!("microsoft-edge:{address}"))
}

#[cfg(test)]
mod tests {
    use super::{EdgeError, edge_target};

    #[test]
    fn web_pages_become_edge_protocol_targets() {
        assert_eq!(
            edge_target("https://accounts.example.test/signin?step=2"),
            Ok("microsoft-edge:https://accounts.example.test/signin?step=2".to_owned())
        );
        assert_eq!(
            edge_target("HTTP://Example.TEST"),
            Ok("microsoft-edge:http://example.test/".to_owned())
        );
    }

    #[test]
    fn nothing_but_a_web_page_is_handed_over() {
        for text in [
            "about:blank",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "microsoft-edge:https://example.test/",
            "https://user:secret@example.test/",
            "\"https://example.test/\" --flag",
            "",
        ] {
            assert_eq!(edge_target(text), Err(EdgeError::NotWeb), "{text}");
        }
    }
}
