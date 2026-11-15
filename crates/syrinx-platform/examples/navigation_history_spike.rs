//! Records which host-side WebView2 events a tab's back and forward list can be rebuilt from (#126),
//! against synthetic loopback pages only, then checks Syrinx's own list through `BrowserHost`; run with `cargo run -p syrinx-platform --example navigation_history_spike`.

#[cfg(windows)]
#[path = "navigation_history_spike/windows.rs"]
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
    linux::main("navigation_history_spike")
}
