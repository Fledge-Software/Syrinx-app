//! Downloads Syrinx's update files from the one compiled-in host over WinHTTP (P1-39c), and only
//! when the owner presses "Check for updates". The design is `docs/update-delivery.md`.

mod endpoint;
mod remote;
mod transport;
// WinHTTP is the only client; other platforms build no network client at all (D9).
#[cfg(windows)]
mod winhttp;

#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod tests_tls;

pub use endpoint::{Endpoint, UPDATE_BASE_URL};
pub use remote::{Cancel, RemotePackage};
