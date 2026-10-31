//! Opens a page in the desktop's default browser when the owner asks, for sign-in that breaks
//! in WebKitGTK (D36's route on Linux).

use std::{fmt, process::Command};

use syrinx_core::web_address;

/// Why a page could not be handed to the other browser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeError {
    /// Only `http` and `https` pages are handed over; nothing else is ever launched.
    NotWeb,
    /// The desktop did not start a browser, for example because `xdg-open` is missing.
    Launch,
}

impl fmt::Display for EdgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotWeb => "Only web pages can be opened in another browser.",
            Self::Launch => "The desktop could not open your default browser.",
        })
    }
}

impl std::error::Error for EdgeError {}

/// Hands one checked web address to `xdg-open` as a single argument, never through a shell.
/// Call this only from an explicit owner action; Syrinx never opens another browser on its own.
pub fn open_in_edge(address: &str) -> Result<(), EdgeError> {
    let address = web_address(address).map_err(|_| EdgeError::NotWeb)?;
    let mut child = Command::new("xdg-open")
        .arg(address.as_str())
        .spawn()
        .map_err(|_| EdgeError::Launch)?;
    // xdg-open hands off and exits; reap it so no zombie stays until Syrinx quits.
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            eprintln!("Syrinx could not collect xdg-open: {error}");
        }
    });
    Ok(())
}
