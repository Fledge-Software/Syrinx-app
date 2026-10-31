//! Verifies a persistent cookie survives Syrinx's shutdown, even when the engine is then killed (#96).
//!
//! Options: `--trace` prints timestamped harness commands, page events and loopback requests;
//! `--abrupt` also reports, without asserting, whether the cookie survives an engine killed
//! before any shutdown, which is what a Windows session end did before #96's quit hook.

#[cfg(windows)]
#[path = "cookie_persistence/windows.rs"]
mod windows;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows::main()
}

/// The fixture drives WebView2, which exists only on Windows.
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("This fixture drives WebView2 and runs only on Windows.");
    std::process::ExitCode::FAILURE
}
