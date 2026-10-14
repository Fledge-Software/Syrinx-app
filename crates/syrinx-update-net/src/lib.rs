//! Downloads Syrinx's update files from the one compiled-in host over WinHTTP (P1-39c), and only
//! when the owner presses "Check for updates". The design is `docs/proposals/p1-39-update-delivery.md`.

mod endpoint;
mod remote;
mod winhttp;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_tls;

pub use endpoint::{Endpoint, UPDATE_BASE_URL};
pub use remote::{Cancel, RemotePackage};
