//! Verifies a synthetic page close stays deferred until the host releases its native view.
//!
//! Options: `--suspension-only` runs only the suspension suite; `--audio-only` only the audio
//! suite (#130); `--downloads-only` only the download suite (#143); `--stall-first-close`
//! holds the first close page's body past the hidden-page suspension grace; `--trace` prints
//! timestamped harness commands, page events and loopback requests to stderr.

#[cfg(windows)]
#[path = "page_close/windows.rs"]
mod windows;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows::main()
}

/// The fixture drives WebView2, which exists only on Windows.
#[cfg(not(any(windows, target_os = "linux")))]
fn main() -> std::process::ExitCode {
    eprintln!("This fixture drives WebView2 and runs only on Windows.");
    std::process::ExitCode::FAILURE
}

// WebView2's COM-specific suites stay on Windows; Linux's host checks and limits
// are recorded in docs/dev/linux-native-checks.md and implemented below.
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::main("page_close")
}
