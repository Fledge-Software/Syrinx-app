//! The update host, compiled in: no setting, environment variable or feature can redirect it.

use std::time::Duration;

/// Where every update file lives (owner decision, 2026-10-13).
pub const UPDATE_BASE_URL: &str = "https://syrinxapp.dev/updates/v1/";

/// The host, port and path prefix requests go to, and whether they use TLS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub(crate) host: &'static str,
    pub(crate) port: u16,
    pub(crate) prefix: &'static str,
    pub(crate) secure: bool,
    /// Whether Windows' proxy settings apply; only a test's loopback server skips them.
    pub(crate) system_proxy: bool,
    pub(crate) timeouts: Timeouts,
}

/// How long a request may wait before it is abandoned (the proposal's "Stalls" row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timeouts {
    /// Resolving the name and opening the connection, each.
    pub(crate) connect: Duration,
    /// Waiting for the response, or for the next bytes of its body.
    pub(crate) receive: Duration,
}

/// The proposal's bounds: 30 s to connect, 60 s with no bytes received.
const PRODUCTION_TIMEOUTS: Timeouts = Timeouts {
    connect: Duration::from_secs(30),
    receive: Duration::from_secs(60),
};

impl Endpoint {
    /// `UPDATE_BASE_URL`, the only endpoint a shipped Syrinx can build.
    pub const fn production() -> Self {
        Self {
            host: "syrinxapp.dev",
            port: 443,
            prefix: "/updates/v1/",
            secure: true,
            system_proxy: true,
            timeouts: PRODUCTION_TIMEOUTS,
        }
    }

    /// The host name, for the owner and for diagnostics.
    pub fn host(&self) -> &'static str {
        self.host
    }

    /// A loopback server for tests; plain HTTP is possible only here, in a test build. It skips
    /// proxy detection, which could otherwise send a WPAD query beyond this machine.
    #[cfg(test)]
    pub(crate) fn loopback(port: u16, secure: bool, timeouts: Timeouts) -> Self {
        Self {
            host: "127.0.0.1",
            port,
            prefix: "/updates/v1/",
            secure,
            system_proxy: false,
            timeouts,
        }
    }
}
