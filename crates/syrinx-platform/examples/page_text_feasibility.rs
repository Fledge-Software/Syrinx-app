//! Runs P1-25's synthetic page-text feasibility cases against local loopback sites only;
//! see `https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md` for the results and `--help` for the options.

#[cfg(windows)]
#[path = "page_text_feasibility/windows.rs"]
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
