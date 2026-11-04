//! What a download needs from an HTTP client, so the bounds in `remote` work on any platform.

use syrinx_update::install::TransferProblem;

/// Fetches one file at a path under the endpoint's prefix.
pub(crate) trait Transport: Send + Sync {
    fn get(&self, path: &str) -> Result<Box<dyn Body + '_>, TransferProblem>;
}

/// A response body with status 200.
pub(crate) trait Body {
    /// The declared `Content-Length`, when the host sent one.
    fn length(&self) -> Option<u64>;
    /// Read the next bytes; 0 means the body has ended.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransferProblem>;
}
